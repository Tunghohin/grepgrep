//! Project persistence for single-file save and load.

use std::ffi::OsStr;
use std::fs;
use std::fs::File;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use tempfile::TempDir;
use zip::write::FileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::audio::AudioChannelMode;
use crate::state::{AppState, LoopRegion, TimelineTag, VisualizationMode};

pub const PROJECT_FILE_NAME: &str = "project.json";
const PROJECT_VERSION: u32 = 1;

/// Serializable project data stored alongside the copied audio file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectData {
    pub version: u32,
    pub audio_file_name: String,
    pub timeline_tags: Vec<TimelineTag>,
    pub loop_region: Option<LoopRegion>,
    pub speed: f32,
    pub channel_mode: AudioChannelMode,
    #[serde(default)]
    pub visualization_mode: VisualizationMode,
    pub zoom: f32,
    pub scroll_offset: f64,
    pub last_position: Option<f64>,
}

impl ProjectData {
    pub fn from_state(state: &AppState, audio_file_name: String) -> Self {
        Self {
            version: PROJECT_VERSION,
            audio_file_name,
            timeline_tags: state.timeline_tags.clone(),
            loop_region: state.loop_region,
            speed: state.speed,
            channel_mode: state.channel_mode,
            visualization_mode: state.visualization_mode,
            zoom: state.zoom,
            scroll_offset: state.scroll_offset,
            last_position: Some(state.position),
        }
    }
}

#[derive(Debug)]
pub struct LoadedProject {
    pub project_path: Option<PathBuf>,
    pub audio_path: PathBuf,
    pub data: ProjectData,
    pub extracted_dir: Option<TempDir>,
}

pub fn default_project_file_name(audio_path: &Path) -> String {
    let stem = audio_path
        .file_stem()
        .and_then(OsStr::to_str)
        .filter(|stem| !stem.trim().is_empty())
        .unwrap_or("project");

    format!("{stem}.ggproj")
}

pub fn save_to_file(state: &AppState, project_file: &Path) -> Result<PathBuf> {
    let source_audio_path = state
        .file_path
        .as_deref()
        .map(PathBuf::from)
        .ok_or_else(|| anyhow::anyhow!("No audio file is currently loaded"))?;

    if !source_audio_path.is_file() {
        bail!(
            "Cannot save project because the source audio file does not exist: {}",
            source_audio_path.display()
        );
    }

    if project_file.exists() && project_file.is_dir() {
        bail!(
            "Project destination must be a file: {}",
            project_file.display()
        );
    }

    if let Some(parent) = project_file.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "Failed to create project parent directory: {}",
                parent.display()
            )
        })?;
    }

    let audio_file_name = source_audio_path
        .file_name()
        .and_then(OsStr::to_str)
        .ok_or_else(|| anyhow::anyhow!("Audio file name is not valid UTF-8"))?
        .to_string();

    let project_data = ProjectData::from_state(state, audio_file_name);
    let json = serde_json::to_vec_pretty(&project_data).context("Failed to serialize project")?;
    let temp_path = temp_path_for(project_file);
    write_project_archive(&temp_path, &source_audio_path, &project_data, &json)?;

    replace_file_atomically(&temp_path, project_file)?;

    Ok(match project_file.canonicalize() {
        Ok(path) => path,
        Err(_) => project_file.to_path_buf(),
    })
}

pub fn load_from_path(project_path: &Path) -> Result<LoadedProject> {
    if project_path.is_dir() {
        return load_from_directory(project_path);
    }

    if !project_path.is_file() {
        bail!("Project path must be a file: {}", project_path.display());
    }

    load_from_archive(project_path)
}

fn load_from_archive(project_path: &Path) -> Result<LoadedProject> {
    let archive_file = File::open(project_path)
        .with_context(|| format!("Failed to open project archive: {}", project_path.display()))?;
    let mut archive =
        ZipArchive::new(archive_file).context("Failed to read project archive contents")?;
    let extracted_dir = tempfile::tempdir().context("Failed to create temp project directory")?;

    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .with_context(|| format!("Failed to read archive entry #{index}"))?;

        if entry.is_dir() {
            continue;
        }

        let enclosed_name = entry
            .enclosed_name()
            .ok_or_else(|| anyhow::anyhow!("Project archive contains an unsafe path"))?
            .to_path_buf();
        if enclosed_name.components().count() != 1 {
            bail!("Project archive entries must be files at the archive root");
        }

        let output_path = extracted_dir.path().join(&enclosed_name);
        let mut output_file = File::create(&output_path)
            .with_context(|| format!("Failed to create {}", output_path.display()))?;
        std::io::copy(&mut entry, &mut output_file)
            .with_context(|| format!("Failed to extract {}", output_path.display()))?;
    }

    let mut loaded = load_from_directory_contents(extracted_dir.path())?;
    loaded.project_path = Some(canonicalized_or_original(project_path));
    loaded.extracted_dir = Some(extracted_dir);
    Ok(loaded)
}

fn load_from_directory(project_dir: &Path) -> Result<LoadedProject> {
    let mut loaded = load_from_directory_contents(project_dir)?;
    loaded.project_path = None;
    Ok(loaded)
}

fn load_from_directory_contents(project_dir: &Path) -> Result<LoadedProject> {
    if !project_dir.is_dir() {
        bail!(
            "Project path must be a directory: {}",
            project_dir.display()
        );
    }

    let project_json_path = project_dir.join(PROJECT_FILE_NAME);
    let raw = fs::read(&project_json_path)
        .with_context(|| format!("Failed to read {}", project_json_path.display()))?;
    let data: ProjectData =
        serde_json::from_slice(&raw).context("Failed to parse project metadata")?;

    if data.version != PROJECT_VERSION {
        bail!(
            "Unsupported project version {} (expected {})",
            data.version,
            PROJECT_VERSION
        );
    }

    let audio_path = project_audio_path(project_dir, &data.audio_file_name)?;
    if !audio_path.is_file() {
        bail!("Project audio file is missing: {}", audio_path.display());
    }

    Ok(LoadedProject {
        project_path: None,
        audio_path,
        data,
        extracted_dir: None,
    })
}

fn write_project_archive(
    archive_path: &Path,
    source_audio_path: &Path,
    project_data: &ProjectData,
    project_json: &[u8],
) -> Result<()> {
    let archive_file = File::create(archive_path)
        .with_context(|| format!("Failed to create {}", archive_path.display()))?;
    let mut writer = ZipWriter::new(archive_file);
    let options = FileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .unix_permissions(0o644);

    writer
        .start_file(PROJECT_FILE_NAME, options)
        .context("Failed to write project metadata into archive")?;
    writer
        .write_all(project_json)
        .context("Failed to serialize project metadata into archive")?;

    writer
        .start_file(&project_data.audio_file_name, options)
        .context("Failed to add project audio into archive")?;
    let mut audio_file = File::open(source_audio_path)
        .with_context(|| format!("Failed to open {}", source_audio_path.display()))?;
    std::io::copy(&mut audio_file, &mut writer).with_context(|| {
        format!(
            "Failed to write {} into archive",
            source_audio_path.display()
        )
    })?;

    writer
        .finish()
        .context("Failed to finalize project archive")?;
    Ok(())
}

fn replace_file_atomically(temp_path: &Path, destination: &Path) -> Result<()> {
    if destination.exists() {
        fs::remove_file(destination)
            .with_context(|| format!("Failed to replace {}", destination.display()))?;
    }

    fs::rename(temp_path, destination)
        .with_context(|| format!("Failed to write {}", destination.display()))?;
    Ok(())
}

fn temp_path_for(path: &Path) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();

    let file_name = path
        .file_name()
        .and_then(OsStr::to_str)
        .unwrap_or("tempfile");
    path.with_file_name(format!(".{file_name}.{unique}.tmp"))
}

fn project_audio_path(project_dir: &Path, audio_file_name: &str) -> Result<PathBuf> {
    let mut components = Path::new(audio_file_name).components();
    let is_plain_file_name =
        matches!(components.next(), Some(Component::Normal(_))) && components.next().is_none();

    if !is_plain_file_name {
        bail!("Project audio file name must be a plain file name inside the project directory");
    }

    Ok(project_dir.join(audio_file_name))
}

fn canonicalized_or_original(path: &Path) -> PathBuf {
    match path.canonicalize() {
        Ok(path) => path,
        Err(_) => path.to_path_buf(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use tempfile::tempdir;

    use crate::state::AppState;

    #[test]
    fn default_project_file_name_uses_audio_stem() {
        let name = default_project_file_name(Path::new("/tmp/demo-song.mp3"));
        assert_eq!(name, "demo-song.ggproj");
    }

    #[test]
    fn save_and_load_round_trip_preserves_project_data() {
        let temp = tempdir().expect("tempdir should be created");
        let audio_path = temp.path().join("example.wav");
        fs::write(&audio_path, b"audio").expect("audio fixture should be written");

        let mut state = AppState::new();
        state.file_path = Some(audio_path.to_string_lossy().to_string());
        state.duration = 120.0;
        state.position = 12.5;
        state.speed = 0.75;
        state.visualization_mode = VisualizationMode::Spectrogram;
        state.zoom = 2.5;
        state.scroll_offset = 8.0;
        state.timeline_tags = vec![TimelineTag {
            id: 7,
            time: 15.0,
            name: "Verse".to_string(),
        }];
        state.loop_region = Some(LoopRegion {
            start: 10.0,
            end: 20.0,
            enabled: true,
        });

        let project_file = temp.path().join("example.ggproj");
        save_to_file(&state, &project_file).expect("project should save");

        let loaded = load_from_path(&project_file).expect("project should load");

        assert_eq!(
            loaded.project_path,
            Some(project_file.canonicalize().unwrap())
        );
        assert_eq!(
            loaded.audio_path.file_name().and_then(OsStr::to_str),
            Some("example.wav")
        );
        assert_eq!(loaded.data.timeline_tags.len(), 1);
        assert_eq!(loaded.data.timeline_tags[0].name, "Verse");
        assert_eq!(loaded.data.loop_region.unwrap().start, 10.0);
        assert!((loaded.data.speed - 0.75).abs() < 0.001);
        assert_eq!(
            loaded.data.visualization_mode,
            VisualizationMode::Spectrogram
        );
        assert!((loaded.data.zoom - 2.5).abs() < 0.001);
        assert!((loaded.data.scroll_offset - 8.0).abs() < 0.001);
        assert_eq!(loaded.data.last_position, Some(12.5));
        assert!(loaded.extracted_dir.is_some());
    }

    #[test]
    fn load_rejects_audio_paths_outside_the_project_archive() {
        let temp = tempdir().expect("tempdir should be created");
        let source_audio_path = temp.path().join("example.wav");
        fs::write(&source_audio_path, b"audio").expect("audio fixture should be written");
        let project_file = temp.path().join("bad.ggproj");

        let project_data = ProjectData {
            version: PROJECT_VERSION,
            audio_file_name: "../outside.wav".to_string(),
            timeline_tags: Vec::new(),
            loop_region: None,
            speed: 1.0,
            channel_mode: AudioChannelMode::Stereo,
            visualization_mode: VisualizationMode::Waveform,
            zoom: 1.0,
            scroll_offset: 0.0,
            last_position: None,
        };
        let project_json =
            serde_json::to_vec_pretty(&project_data).expect("project json should serialize");
        write_project_archive(
            &project_file,
            &source_audio_path,
            &project_data,
            &project_json,
        )
        .expect("archive should be written");

        let error = load_from_path(&project_file).expect_err("invalid path should fail");
        assert!(
            error.to_string().contains("unsafe path"),
            "unexpected error: {error}"
        );
    }
}
