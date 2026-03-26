#![cfg_attr(all(target_os = "windows", not(debug_assertions)), windows_subsystem = "windows")]

//! grepgrep - A modern audio transcription tool
//!
//! This application provides:
//! - Waveform visualization
//! - Loop region selection for practice

use anyhow::Result;
use clap::Parser;

mod analysis;
mod audio;
mod state;
mod ui;

use ui::MainWindow;

const APP_ICON_SVG: &str = include_str!("../assets/logo.svg");
const APP_ICON_SIZE: u32 = 256;

/// grepgrep - A tool for transcribing audio
#[derive(Parser, Debug)]
#[command(name = "grepgrep")]
#[command(author)]
#[command(version)]
#[command(about = "A modern audio transcription tool", long_about = None)]
struct Args {
    /// Audio file to open on startup
    #[arg(short, long, value_name = "FILE")]
    file: Option<String>,

    /// Initial volume (0.0 - 1.0)
    #[arg(short, long, default_value = "0.8", value_name = "VOLUME")]
    volume: f32,

    /// Enable debug logging
    #[arg(short, long)]
    debug: bool,

    /// Target platform for display purposes (informational only)
    #[arg(long, value_name = "PLATFORM", value_parser = ["linux", "windows", "macos"])]
    platform: Option<String>,
}

fn main() -> Result<()> {
    let args = Args::parse();

    // Initialize logging based on debug flag
    if args.debug {
        tracing_subscriber::fmt::init();
    } else {
        // Only log errors and warnings in release mode
        let _ = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::WARN)
            .try_init();
    }

    tracing::info!("Starting grepgrep...");
    tracing::debug!("CLI arguments: {:?}", args);

    // Log platform information
    let current_platform = get_platform_name();
    tracing::info!("Running on platform: {}", current_platform);
    if let Some(target) = args.platform {
        tracing::info!("Target platform specified: {}", target);
    }

    // Configure the window
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1200.0, 700.0])
            .with_min_inner_size([800.0, 500.0])
            .with_title("grepgrep")
            .with_icon(load_icon()),
        ..Default::default()
    };

    // Run the application
    let result = eframe::run_native(
        "grepgrep",
        options,
        Box::new(|cc| {
            // Set up fonts if needed
            let _ = cc.egui_ctx.set_fonts(egui::FontDefinitions::default());

            let mut main_window = MainWindow::new();

            // Apply initial settings from CLI
            main_window.set_initial_volume(args.volume);

            // Load initial file if provided
            if let Some(file_path) = args.file {
                tracing::info!("Loading file from CLI: {}", file_path);
                main_window.load_file_from_path(&file_path);
            }

            Ok(Box::new(main_window))
        }),
    );

    match result {
        Ok(_) => Ok(()),
        Err(e) => Err(anyhow::anyhow!("Application error: {}", e)),
    }
}

/// Get the current platform name
fn get_platform_name() -> &'static str {
    #[cfg(target_os = "linux")]
    {
        "linux"
    }

    #[cfg(target_os = "windows")]
    {
        "windows"
    }

    #[cfg(target_os = "macos")]
    {
        "macos"
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    {
        "unknown"
    }
}

/// Load application icon
fn load_icon() -> egui::IconData {
    match render_logo_icon(APP_ICON_SVG, APP_ICON_SIZE) {
        Ok(icon) => icon,
        Err(error) => {
            tracing::warn!("Failed to render icon from assets/logo.svg: {error}");
            fallback_icon(APP_ICON_SIZE, [0x00, 0xB4, 0xB4])
        }
    }
}

fn render_logo_icon(svg: &str, size: u32) -> Result<egui::IconData> {
    let root_tag = extract_tag(svg, "svg").ok_or_else(|| anyhow::anyhow!("missing <svg> tag"))?;
    let circle_tag =
        extract_tag(svg, "circle").ok_or_else(|| anyhow::anyhow!("missing <circle> tag"))?;

    let view_box = extract_attr(root_tag, "viewBox")
        .ok_or_else(|| anyhow::anyhow!("missing svg viewBox"))?;
    let [min_x, min_y, view_width, view_height] = parse_view_box(view_box)?;

    let cx = extract_attr(circle_tag, "cx")
        .ok_or_else(|| anyhow::anyhow!("missing circle cx"))?
        .parse::<f32>()?;
    let cy = extract_attr(circle_tag, "cy")
        .ok_or_else(|| anyhow::anyhow!("missing circle cy"))?
        .parse::<f32>()?;
    let radius = extract_attr(circle_tag, "r")
        .ok_or_else(|| anyhow::anyhow!("missing circle r"))?
        .parse::<f32>()?;
    let fill = extract_attr(circle_tag, "fill")
        .ok_or_else(|| anyhow::anyhow!("missing circle fill"))?;
    let color = parse_hex_color(fill)?;

    Ok(fallback_icon_with_circle(
        size,
        color,
        min_x,
        min_y,
        view_width,
        view_height,
        cx,
        cy,
        radius,
    ))
}

fn parse_view_box(view_box: &str) -> Result<[f32; 4]> {
    let values = view_box
        .split_whitespace()
        .map(str::parse::<f32>)
        .collect::<std::result::Result<Vec<_>, _>>()?;

    let [min_x, min_y, width, height]: [f32; 4] = values
        .try_into()
        .map_err(|_| anyhow::anyhow!("viewBox must have 4 numbers"))?;

    Ok([min_x, min_y, width, height])
}

fn parse_hex_color(color: &str) -> Result<[u8; 3]> {
    let color = color.trim();
    let hex = color
        .strip_prefix('#')
        .ok_or_else(|| anyhow::anyhow!("unsupported fill color format"))?;

    if hex.len() != 6 {
        return Err(anyhow::anyhow!("expected 6-digit hex color"));
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

fn fallback_icon(size: u32, color: [u8; 3]) -> egui::IconData {
    fallback_icon_with_circle(size, color, 0.0, 0.0, 512.0, 512.0, 256.0, 256.0, 230.0)
}

fn fallback_icon_with_circle(
    size: u32,
    color: [u8; 3],
    min_x: f32,
    min_y: f32,
    view_width: f32,
    view_height: f32,
    cx: f32,
    cy: f32,
    radius: f32,
) -> egui::IconData {
    let width = size;
    let height = size;
    let mut rgba = vec![0u8; (width * height * 4) as usize];
    let pixel_size = (view_width / width as f32).max(view_height / height as f32);
    let edge_softness = pixel_size.max(1.0);

    for y in 0..height {
        for x in 0..width {
            let idx = ((y * width + x) * 4) as usize;
            let svg_x = min_x + ((x as f32 + 0.5) / width as f32) * view_width;
            let svg_y = min_y + ((y as f32 + 0.5) / height as f32) * view_height;
            let distance = ((svg_x - cx).powi(2) + (svg_y - cy).powi(2)).sqrt();
            let coverage = ((radius - distance) / edge_softness + 0.5).clamp(0.0, 1.0);

            rgba[idx] = color[0];
            rgba[idx + 1] = color[1];
            rgba[idx + 2] = color[2];
            rgba[idx + 3] = (coverage * 255.0).round() as u8;
        }
    }

    egui::IconData { rgba, width, height }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_logo_svg_can_be_rendered_into_icon_data() {
        let icon = render_logo_icon(APP_ICON_SVG, 64).expect("logo.svg should render");
        assert_eq!(icon.width, 64);
        assert_eq!(icon.height, 64);
        assert_eq!(icon.rgba.len(), (64 * 64 * 4) as usize);
        assert!(icon.rgba.iter().any(|channel| *channel != 0));
    }
}
