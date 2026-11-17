// SPDX-License-Identifier: AGPL-3.0-or-later
//
//! Text / title clips.
//!
//! The engine's flattener deliberately leaves text clips out: titles are
//! rasterised separately (normally by concat-host, which is parked here) and
//! rejoin the render as stills. So this module does that step itself — render
//! each title to a PNG with `concat_text`, cache it on disk, and inject it as
//! an Image `ExportClip` — so the composed monitor and the export both show the
//! text with no other change. [`clips`] is flatten + titles, used wherever the
//! app builds the clip list.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;

use concat_export::{ClipKind, ExportClip};
use concat_project::commands::ClipPatch;
use concat_project::model::{ClipKind as ModelClipKind, TextAlign, TextStyle};
use concat_project::{Command, Editor};
use concat_text::{Align, Fonts, PlateShape, TitleStyle};

thread_local! {
    /// System fonts, loaded once per thread (the UI thread rasterises titles).
    static FONTS: Fonts = Fonts::new();
}

/// The full flattened clip list for a timeline: the engine's non-text clips
/// plus every title rasterised to a still, at `width` × `height`.
pub fn clips(editor: &Editor, width: u32, height: u32) -> Vec<ExportClip> {
    // flatten_timeline_in with the project folder so cutout clips get their
    // mask directory named (background removal); without a folder a cutout
    // leaves the picture whole.
    let mut list = concat_export::flatten::flatten_timeline_in(
        editor.project(),
        None,
        Some(&crate::cutout::project_dir()),
    );
    let timeline = editor.project().active();
    for clip in &timeline.clips {
        if clip.kind != ModelClipKind::Text {
            continue;
        }
        let Some(style) = clip.text.as_ref() else {
            continue;
        };
        let Some(track) = timeline.tracks.iter().position(|t| t.id == clip.track_id) else {
            continue;
        };
        if let Some(ec) = rasterise(style, clip.start, clip.duration, track, width, height) {
            list.push(ec);
        }
    }
    list
}

/// Render one title to a still and wrap it as an Image clip.
fn rasterise(
    style: &TextStyle,
    start: f64,
    duration: f64,
    track: usize,
    width: u32,
    height: u32,
) -> Option<ExportClip> {
    let title = title_style(style);
    let rendered = FONTS
        .with(|fonts| concat_text::render(fonts, &title, width, height))
        .ok()?;
    let path = write_png(&rendered.png, &title, width, height)?;

    let mut clip = ExportClip::blank(ClipKind::Image, start, duration, track);
    clip.path = path.to_string_lossy().into_owned();
    clip.overscan = rendered.overscan;
    clip.media_width = Some(rendered.width);
    clip.media_height = Some(rendered.height);
    Some(clip)
}

/// Cache the title PNG on disk, keyed by its look and size, and return the
/// path. An identical title reuses the file so the reader pool caches it.
fn write_png(png: &[u8], title: &TitleStyle, width: u32, height: u32) -> Option<PathBuf> {
    let mut hasher = DefaultHasher::new();
    format!("{title:?}").hash(&mut hasher);
    width.hash(&mut hasher);
    height.hash(&mut hasher);
    let key = hasher.finish();

    let dir = std::env::temp_dir().join("coupcat-titles");
    std::fs::create_dir_all(&dir).ok()?;
    let path = dir.join(format!("title-{key:016x}.png"));
    if !path.exists() {
        std::fs::write(&path, png).ok()?;
    }
    Some(path)
}

/// The document's text style in the painter's terms. Per-word runs are not
/// carried yet, so the whole title takes its base look.
fn title_style(style: &TextStyle) -> TitleStyle {
    TitleStyle {
        content: style.display_content(),
        font_family: style.font_family.clone(),
        font_size: style.font_size,
        font_weight: style.font_weight,
        italic: style.italic,
        underline: style.underline,
        strikethrough: style.strikethrough,
        color: style.color.clone(),
        align: match style.align {
            TextAlign::Left => Align::Left,
            TextAlign::Center => Align::Center,
            TextAlign::Right => Align::Right,
        },
        stroke_width: style.stroke_width,
        stroke_color: style.stroke_color.clone(),
        shadow: style.shadow,
        background: style.background.clone(),
        background_radius: style.background_radius,
        background_padding_x: style.background_padding_x,
        background_padding_y: style.background_padding_y,
        line_height: style.line_height,
        max_width: style.max_width,
        max_height: style.max_height,
        tracking: style.tracking,
        vertical: style.vertical,
        background_shape: PlateShape::from_name(&style.background_shape),
        background_outline: style.background_outline.clone(),
        gradient: style.gradient.clone(),
        runs: Vec::new(),
    }
}

/// Add a title at `start`, on a new lane above the picture. Returns its id.
pub fn add_text(editor: &mut Editor, start: f64) -> Option<String> {
    editor
        .apply(Command::AddTextClip {
            track_id: None,
            above: true,
            start,
            style: Some(TextStyle {
                content: "Texto".to_string(),
                ..TextStyle::default()
            }),
            duration: Some(3.0),
            offset_y: None,
        })
        .ok()?
        .created_id
}

/// The text style of a clip, if it is a title.
pub fn get_style(editor: &Editor, clip_id: &str) -> Option<TextStyle> {
    editor
        .project()
        .active()
        .clip(clip_id)
        .and_then(|c| c.text.clone())
}

/// Replace a title clip's whole text style.
pub fn set_style(editor: &mut Editor, clip_id: String, style: TextStyle) -> bool {
    editor
        .apply(Command::UpdateClip {
            clip_id,
            patch: ClipPatch {
                text: Some(Some(style)),
                ..ClipPatch::default()
            },
        })
        .map(|o| o.applied)
        .unwrap_or(false)
}
