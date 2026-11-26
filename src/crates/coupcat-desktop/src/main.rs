// SPDX-License-Identifier: AGPL-3.0-or-later
//
//! CoupCat desktop shell (M1 → M2).
//!
//! A Dioxus window over the Concat engine. The edit is a
//! `concat_project::Editor` ([`editor`]); preview and playback run on the
//! [`monitor`] thread. The UI sends editing commands and playback commands and
//! renders the timeline and the current frame.

#![windows_subsystem = "windows"]

mod app;
mod captions;
mod cutout;
mod editor;
mod effects;
mod export;
mod monitor;
mod peaks;
/// The engine's mixing playback, used for the preview's sound. Only this module
/// of `concat-host` is needed, so it is compiled here rather than pulling in the
/// whole host layer.
#[path = "../../concat-host/src/playback.rs"]
#[allow(dead_code)]
mod playback;
mod project_file;
mod recorder;
mod system_audio;
mod text;
mod thumbs;
mod ui;

/// The window/taskbar icon, decoded from the embedded PNG.
fn load_icon() -> Option<dioxus::desktop::tao::window::Icon> {
    let data = include_bytes!("../assets/icon-64.png");
    let decoder = png::Decoder::new(std::io::Cursor::new(&data[..]));
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf[..info.buffer_size()].to_vec(),
        png::ColorType::Rgb => buf[..info.buffer_size()]
            .chunks_exact(3)
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        _ => return None,
    };
    dioxus::desktop::tao::window::Icon::from_rgba(rgba, info.width, info.height).ok()
}

fn main() {
    concat_media::init();
    // Point ONNX Runtime (ort load-dynamic) at the onnxruntime.dll beside the
    // exe, for background removal.
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dll) = exe.parent().map(|d| d.join("onnxruntime.dll")) {
            if dll.exists() {
                unsafe { std::env::set_var("ORT_DYLIB_PATH", dll) };
            }
        }
    }
    use dioxus::desktop::tao::dpi::LogicalSize;
    use dioxus::desktop::{Config, WindowBuilder};
    let window = WindowBuilder::new()
        .with_title("CoupCat")
        .with_window_icon(load_icon())
        .with_inner_size(LogicalSize::new(1280.0, 800.0))
        .with_min_inner_size(LogicalSize::new(900.0, 600.0))
        // Dioxus' default desktop window comes up always-on-top, which makes it
        // sit over every other window — even the file-open dialog. Turn it off.
        .with_always_on_top(false);
    dioxus::LaunchBuilder::desktop()
        .with_cfg(Config::new().with_window(window))
        .launch(app::app);
}
