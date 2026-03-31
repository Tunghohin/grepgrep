//! Project persistence for directory-based save and load.

use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::audio::AudioChannelMode;
use crate::state::{AppState, LoopRegion, TimelineTag};

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
            zoom: state.zoom,
            scroll_offset: state.scroll_offset,
            last_position: Some(state.position),
        }
    }
}

#[derive(Debug)]
pub struct LoadedProject {
    pub project_dir: PathBuf,
    pub audio_path: PathBuf,
    pub data: ProjectData,
}

pub fn default_project_directory_name(audio_path: &Path) -> String {
    let stem = audio_path
        .file_stem()
        .and_then(OsStr::to_str)
        .filter(|stem| !stem.trim().is_empty())
        .unwrap_or("project");

    format!("{stem}.ggproj")
}

pub fn save_to_directory(state: &AppState, project_dir: &Path) -> Result<PathBuf> {
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

    if project_dir.exists() && !project_dir.is_dir() {
        bail!(
            "Project destination must be a directory: {}",
            project_dir.display()
        );
    }

    fs::create_dir_all(project_dir).with_context(|| {
        format!(
            "Failed to create project directory: {}",
            project_dir.display()
        )
    })?;

    let audio_file_name = source_audio_path
        .file_name()
        .and_then(OsStr::to_str)
        .ok_or_else(|| anyhow::anyhow!("Audio file name is not valid UTF-8"))?
        .to_string();
    let target_audio_path = project_dir.join(&audio_file_name);

    copy_file_atomically(&source_audio_path, &target_audio_path)?;

    let project_data = ProjectData::from_state(state, audio_file_name);
    let project_json_path = project_dir.join(PROJECT_FILE_NAME);
    let json = serde_json::to_vec_pretty(&project_data).context("Failed to serialize project")?;
    write_file_atomically(&project_json_path, &json)?;

    Ok(match project_dir.canonicalize() {
        Ok(path) => path,
        Err(_) => project_dir.to_path_buf(),
    })
}

pub fn load_from_directory(project_dir: &Path) -> Result<LoadedProject> {
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

    let audio_path = project_dir.join(&data.audio_file_name);
    if !audio_path.is_file() {
        bail!("Project audio file is missing: {}", audio_path.display());
    }

    Ok(LoadedProject {
        project_dir: match project_dir.canonicalize() {
            Ok(path) => path,
            Err(_) => project_dir.to_path_buf(),
        },
        audio_path,
        data,
    })
}

fn copy_file_atomically(source: &Path, destination: &Path) -> Result<()> {
    if paths_refer_to_same_file(source, destination)? {
        return Ok(());
    }

    let temp_path = temp_path_for(destination);
    fs::copy(source, &temp_path).with_context(|| {
        format!(
            "Failed to copy audio from {} to {}",
            source.display(),
            temp_path.display()
        )
    })?;

    if destination.exists() {
        fs::remove_file(destination).with_context(|| {
            format!(
                "Failed to replace existing audio file: {}",
                destination.display()
            )
        })?;
    }

    fs::rename(&temp_path, destination).with_context(|| {
        format!(
            "Failed to finalize copied audio file: {}",
            destination.display()
        )
    })?;

    Ok(())
}

fn write_file_atomically(path: &Path, bytes: &[u8]) -> Result<()> {
    let temp_path = temp_path_for(path);
    fs::write(&temp_path, bytes)
        .with_context(|| format!("Failed to write {}", temp_path.display()))?;

    if path.exists() {
        fs::remove_file(path).with_context(|| format!("Failed to replace {}", path.display()))?;
    }

    fs::rename(&temp_path, path).with_context(|| format!("Failed to write {}", path.display()))?;
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

fn paths_refer_to_same_file(left: &Path, right: &Path) -> Result<bool> {
    if left == right {
        return Ok(true);
    }

    if !left.exists() || !right.exists() {
        return Ok(false);
    }

    let left = left.canonicalize()?;
    let right = right.canonicalize()?;
    Ok(left == right)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    use tempfile::tempdir;

    use crate::state::AppState;

    #[test]
    fn default_project_directory_name_uses_audio_stem() {
        let name = default_project_directory_name(Path::new("/tmp/demo-song.mp3"));
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

        let project_dir = temp.path().join("example.ggproj");
        save_to_directory(&state, &project_dir).expect("project should save");

        let loaded = load_from_directory(&project_dir).expect("project should load");

        assert_eq!(loaded.project_dir, project_dir.canonicalize().unwrap());
        assert_eq!(loaded.audio_path, project_dir.join("example.wav"));
        assert_eq!(loaded.data.timeline_tags.len(), 1);
        assert_eq!(loaded.data.timeline_tags[0].name, "Verse");
        assert_eq!(loaded.data.loop_region.unwrap().start, 10.0);
        assert!((loaded.data.speed - 0.75).abs() < 0.001);
        assert!((loaded.data.zoom - 2.5).abs() < 0.001);
        assert!((loaded.data.scroll_offset - 8.0).abs() < 0.001);
        assert_eq!(loaded.data.last_position, Some(12.5));
    }
}
