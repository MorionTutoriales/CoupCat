// SPDX-License-Identifier: AGPL-3.0-or-later
//
//! Small helpers shared by the interface modules.

use crate::editor;
use crate::thumbs;

/// The local time as `hh:mm:ss`. Rust's standard library has no local time
/// zone, so the webview (which does) is asked for it.
pub async fn now_hms() -> Option<String> {
    let mut ev = dioxus::document::eval(
        "dioxus.send(new Date().toLocaleTimeString('es-ES', { hour12: false }))",
    );
    ev.recv::<String>().await.ok()
}

/// The library thumbnail for a bin item, once the background worker has made
/// it. Audio has no picture and keeps its icon; so does anything still
/// queued or that failed to decode.
pub fn library_thumb(m: &editor::MediaView) -> Option<String> {
    if m.kind == "audio" {
        return None;
    }
    thumbs::get(&m.path, m.size, m.duration)
}

/// A badge glyph for a media kind.
pub fn kind_icon(kind: &str) -> &'static str {
    match kind {
        "audio" => "♪",
        "image" => "▣",
        _ => "▶",
    }
}

/// `m:ss` for an optional duration in seconds.
pub fn dur_label(secs: Option<f64>) -> String {
    match secs {
        Some(s) if s > 0.0 => {
            let total = s.round() as u64;
            format!("{}:{:02}", total / 60, total % 60)
        }
        _ => String::new(),
    }
}

/// A `#rrggbb` string for an `<input type=color>`, from the document's colour
/// (which may carry alpha or be malformed).
pub fn color6(s: &str) -> String {
    let t = s.trim();
    if t.starts_with('#') && t.len() >= 7 {
        t[..7].to_string()
    } else {
        "#ffffff".to_string()
    }
}
