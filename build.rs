use std::env;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=assets/logo.svg");

    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    embed_windows_resources().unwrap_or_else(|error| {
        panic!("failed to embed Windows executable resources: {error}");
    });
}

fn embed_windows_resources() -> Result<(), Box<dyn Error>> {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR")?);
    let out_dir = PathBuf::from(env::var("OUT_DIR")?);
    let target = env::var("TARGET")?;

    let svg_path = manifest_dir.join("assets").join("logo.svg");
    let icon_path = out_dir.join("grepgrep-icon.ico");
    let rc_path = out_dir.join("grepgrep-icon.rc");
    let res_path = out_dir.join("grepgrep-icon.res");

    generate_icon_from_svg(&svg_path, &icon_path)?;
    fs::write(&rc_path, windows_resource_script(&icon_path)?)?;

    let mut compiler = resource_compiler(&target)?;

    let status = if target.ends_with("-msvc") {
        compiler.arg("/fo").arg(&res_path).arg(&rc_path).status()?
    } else {
        compiler
            .arg(&rc_path)
            .arg("-O")
            .arg("coff")
            .arg("-o")
            .arg(&res_path)
            .status()?
    };

    if !status.success() {
        return Err(format!("resource compiler exited with status {status}").into());
    }

    println!(
        "cargo:rustc-link-arg-bin=grepgrep={}",
        res_path.to_string_lossy()
    );

    Ok(())
}

fn generate_icon_from_svg(svg_path: &Path, icon_path: &Path) -> Result<(), Box<dyn Error>> {
    let svg = fs::read_to_string(svg_path)?;
    let spec = LogoSpec::parse(&svg)?;
    let sizes = [16_u32, 32, 48, 64, 128, 256];
    let mut images = Vec::with_capacity(sizes.len());

    for size in sizes {
        let rgba = spec.render(size);
        images.push(IcoImage::from_rgba(size, rgba)?);
    }

    let icon_data = encode_ico(&images)?;
    fs::write(icon_path, icon_data)?;
    Ok(())
}

fn encode_ico(images: &[IcoImage]) -> Result<Vec<u8>, Box<dyn Error>> {
    let image_count = u16::try_from(images.len())?;
    let dir_size = 6 + images.len() * 16;
    let mut offset = u32::try_from(dir_size)?;
    let mut entries = Vec::with_capacity(images.len() * 16);
    let mut payload = Vec::new();

    for image in images {
        let width = if image.size >= 256 {
            0
        } else {
            u8::try_from(image.size)?
        };
        let height = if image.size >= 256 {
            0
        } else {
            u8::try_from(image.size)?
        };
        let bytes_in_res = u32::try_from(image.data.len())?;

        entries.push(width);
        entries.push(height);
        entries.push(0);
        entries.push(0);
        entries.extend_from_slice(&1_u16.to_le_bytes());
        entries.extend_from_slice(&32_u16.to_le_bytes());
        entries.extend_from_slice(&bytes_in_res.to_le_bytes());
        entries.extend_from_slice(&offset.to_le_bytes());

        payload.extend_from_slice(&image.data);
        offset = offset
            .checked_add(bytes_in_res)
            .ok_or("icon payload size overflowed")?;
    }

    let mut icon = Vec::with_capacity(dir_size + payload.len());
    icon.extend_from_slice(&0_u16.to_le_bytes());
    icon.extend_from_slice(&1_u16.to_le_bytes());
    icon.extend_from_slice(&image_count.to_le_bytes());
    icon.extend_from_slice(&entries);
    icon.extend_from_slice(&payload);
    Ok(icon)
}

fn resource_compiler(target: &str) -> Result<Command, Box<dyn Error>> {
    if let Ok(path) = env::var("WINDRES") {
        return Ok(Command::new(path));
    }

    let candidates: Vec<String> = if target.ends_with("-msvc") {
        vec!["llvm-rc".into(), "rc".into()]
    } else {
        vec![
            format!("{target}-windres"),
            "x86_64-w64-mingw32-windres".into(),
            "windres".into(),
        ]
    };

    for candidate in candidates {
        if command_exists(&candidate) {
            return Ok(Command::new(candidate));
        }
    }

    Err(format!("no Windows resource compiler found for target {target}").into())
}

fn command_exists(command: &str) -> bool {
    env::var_os("PATH")
        .map(|paths| env::split_paths(&paths).any(|dir| is_executable(&dir.join(command))))
        .unwrap_or(false)
}

fn is_executable(path: &Path) -> bool {
    path.is_file() || cfg!(windows) && path.with_extension("exe").is_file()
}

fn windows_resource_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn windows_resource_script(icon_path: &Path) -> Result<String, Box<dyn Error>> {
    let version = env::var("CARGO_PKG_VERSION")?;
    let version_tuple = windows_version_tuple(&version);
    let product_name = env::var("CARGO_PKG_NAME")?;
    let file_description = env::var("CARGO_PKG_DESCRIPTION")?;
    let company_name = env::var("CARGO_PKG_AUTHORS")
        .ok()
        .and_then(|authors| authors.split(':').next().map(str::trim).map(str::to_owned))
        .filter(|author| !author.is_empty())
        .unwrap_or_else(|| "Tunghohin".to_string());
    let original_filename = format!("{product_name}.exe");
    let copyright = format!("Copyright (C) {company_name}");
    let icon_path = windows_resource_path(icon_path);

    Ok(format!(
        r#"1 ICON "{icon_path}"

1 VERSIONINFO
FILEVERSION {version_tuple}
PRODUCTVERSION {version_tuple}
FILEFLAGSMASK 0x3fL
FILEFLAGS 0x0L
FILEOS 0x40004L
FILETYPE 0x1L
FILESUBTYPE 0x0L
BEGIN
    BLOCK "StringFileInfo"
    BEGIN
        BLOCK "040904B0"
        BEGIN
            VALUE "CompanyName", "{company_name}\0"
            VALUE "FileDescription", "{file_description}\0"
            VALUE "FileVersion", "{version}\0"
            VALUE "InternalName", "{product_name}\0"
            VALUE "OriginalFilename", "{original_filename}\0"
            VALUE "ProductName", "{product_name}\0"
            VALUE "ProductVersion", "{version}\0"
            VALUE "LegalCopyright", "{copyright}\0"
        END
    END
    BLOCK "VarFileInfo"
    BEGIN
        VALUE "Translation", 0x0409, 1200
    END
END
"#
    ))
}

fn windows_version_tuple(version: &str) -> String {
    let mut parts = version
        .split('.')
        .take(4)
        .map(|part| part.parse::<u16>().unwrap_or(0))
        .collect::<Vec<_>>();

    while parts.len() < 4 {
        parts.push(0);
    }

    parts
        .into_iter()
        .map(|part| part.to_string())
        .collect::<Vec<_>>()
        .join(",")
}

struct LogoSpec {
    min_x: f32,
    min_y: f32,
    view_width: f32,
    view_height: f32,
    cx: f32,
    cy: f32,
    radius: f32,
    color: [u8; 3],
}

impl LogoSpec {
    fn parse(svg: &str) -> Result<Self, Box<dyn Error>> {
        let root_tag = extract_tag(svg, "svg").ok_or("missing <svg> tag")?;
        let circle_tag = extract_tag(svg, "circle").ok_or("missing <circle> tag")?;

        let view_box = extract_attr(root_tag, "viewBox").ok_or("missing svg viewBox")?;
        let [min_x, min_y, view_width, view_height] = parse_view_box(view_box)?;

        let cx = extract_attr(circle_tag, "cx")
            .ok_or("missing circle cx")?
            .parse::<f32>()?;
        let cy = extract_attr(circle_tag, "cy")
            .ok_or("missing circle cy")?
            .parse::<f32>()?;
        let radius = extract_attr(circle_tag, "r")
            .ok_or("missing circle r")?
            .parse::<f32>()?;
        let fill = extract_attr(circle_tag, "fill").ok_or("missing circle fill")?;
        let color = parse_hex_color(fill)?;

        Ok(Self {
            min_x,
            min_y,
            view_width,
            view_height,
            cx,
            cy,
            radius,
            color,
        })
    }

    fn render(&self, size: u32) -> Vec<u8> {
        let mut rgba = vec![0_u8; (size * size * 4) as usize];
        let pixel_size = (self.view_width / size as f32).max(self.view_height / size as f32);
        let edge_softness = pixel_size.max(1.0);

        for y in 0..size {
            for x in 0..size {
                let idx = ((y * size + x) * 4) as usize;
                let svg_x = self.min_x + ((x as f32 + 0.5) / size as f32) * self.view_width;
                let svg_y = self.min_y + ((y as f32 + 0.5) / size as f32) * self.view_height;
                let distance = ((svg_x - self.cx).powi(2) + (svg_y - self.cy).powi(2)).sqrt();
                let coverage = ((self.radius - distance) / edge_softness + 0.5).clamp(0.0, 1.0);

                rgba[idx] = self.color[0];
                rgba[idx + 1] = self.color[1];
                rgba[idx + 2] = self.color[2];
                rgba[idx + 3] = (coverage * 255.0).round() as u8;
            }
        }

        rgba
    }
}

struct IcoImage {
    size: u32,
    data: Vec<u8>,
}

impl IcoImage {
    fn from_rgba(size: u32, rgba: Vec<u8>) -> Result<Self, Box<dyn Error>> {
        let size_i32 = i32::try_from(size)?;
        let xor_mask_size = usize::try_from(size)?
            .checked_mul(usize::try_from(size)?)
            .and_then(|px| px.checked_mul(4))
            .ok_or("icon pixel buffer overflowed")?;
        let and_row_bytes = usize::try_from(((size + 31) / 32) * 4)?;
        let and_mask_size = and_row_bytes
            .checked_mul(usize::try_from(size)?)
            .ok_or("icon alpha mask overflowed")?;

        let mut data = Vec::with_capacity(40 + xor_mask_size + and_mask_size);
        data.extend_from_slice(&40_u32.to_le_bytes());
        data.extend_from_slice(&size_i32.to_le_bytes());
        data.extend_from_slice(&(size_i32 * 2).to_le_bytes());
        data.extend_from_slice(&1_u16.to_le_bytes());
        data.extend_from_slice(&32_u16.to_le_bytes());
        data.extend_from_slice(&0_u32.to_le_bytes());
        data.extend_from_slice(&u32::try_from(xor_mask_size + and_mask_size)?.to_le_bytes());
        data.extend_from_slice(&0_i32.to_le_bytes());
        data.extend_from_slice(&0_i32.to_le_bytes());
        data.extend_from_slice(&0_u32.to_le_bytes());
        data.extend_from_slice(&0_u32.to_le_bytes());

        for y in (0..size).rev() {
            for x in 0..size {
                let idx = ((y * size + x) * 4) as usize;
                data.push(rgba[idx + 2]);
                data.push(rgba[idx + 1]);
                data.push(rgba[idx]);
                data.push(rgba[idx + 3]);
            }
        }

        for y in (0..size).rev() {
            let mut row = vec![0_u8; and_row_bytes];

            for x in 0..size {
                let alpha = rgba[((y * size + x) * 4 + 3) as usize];
                if alpha < 128 {
                    let byte = usize::try_from(x / 8)?;
                    let bit = 7 - (x % 8);
                    row[byte] |= 1_u8 << bit;
                }
            }

            data.extend_from_slice(&row);
        }

        Ok(Self { size, data })
    }
}

fn parse_view_box(view_box: &str) -> Result<[f32; 4], Box<dyn Error>> {
    let values = view_box
        .split_whitespace()
        .map(str::parse::<f32>)
        .collect::<Result<Vec<_>, _>>()?;

    let [min_x, min_y, width, height]: [f32; 4] = values
        .try_into()
        .map_err(|_| "viewBox must have 4 numbers")?;

    Ok([min_x, min_y, width, height])
}

fn parse_hex_color(color: &str) -> Result<[u8; 3], Box<dyn Error>> {
    let color = color.trim();
    let hex = color
        .strip_prefix('#')
        .ok_or("unsupported fill color format")?;

    if hex.len() != 6 {
        return Err("expected 6-digit hex color".into());
    }

    let red = u8::from_str_radix(&hex[0..2], 16)?;
    let green = u8::from_str_radix(&hex[2..4], 16)?;
    let blue = u8::from_str_radix(&hex[4..6], 16)?;

    Ok([red, green, blue])
}

fn extract_tag<'a>(svg: &'a str, tag_name: &str) -> Option<&'a str> {
    let start = svg.find(&format!("<{tag_name}"))?;
    let tail = &svg[start..];
    let end = tail.find('>')?;
    Some(&tail[..=end])
}

fn extract_attr<'a>(tag: &'a str, attr_name: &str) -> Option<&'a str> {
    let attr_start = tag.find(&format!("{attr_name}=\""))?;
    let value_start = attr_start + attr_name.len() + 2;
    let value = &tag[value_start..];
    let value_end = value.find('"')?;
    Some(&value[..value_end])
}
