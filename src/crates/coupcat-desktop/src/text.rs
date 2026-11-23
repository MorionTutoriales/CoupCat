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
use concat_project::model::{ClipKind as ModelClipKind, TextAlign, TextCase, TextStyle};
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
    add_styled(editor, start, base("Texto"))
}

/// Add a title with this look at `start`, above the picture.
fn add_styled(editor: &mut Editor, start: f64, style: TextStyle) -> Option<String> {
    editor
        .apply(Command::AddTextClip {
            track_id: None,
            above: true,
            start,
            style: Some(style),
            duration: Some(3.0),
            offset_y: None,
        })
        .ok()?
        .created_id
}

/// The default look with these words.
fn base(content: &str) -> TextStyle {
    TextStyle {
        content: content.to_string(),
        ..TextStyle::default()
    }
}

/// A ready-made title look, offered as a card in the Text tab.
pub struct Preset {
    pub id: &'static str,
    pub name: &'static str,
    /// The group (category) it is listed under; one of [`PRESET_GROUPS`].
    pub group: &'static str,
    /// The words typed into the new title and shown on the card.
    pub sample: &'static str,
    /// An approximate look for the card, as CSS (the real look is `style`).
    pub css: &'static str,
    style: fn() -> TextStyle,
}

/// The groups the presets are listed under, after "Todo".
pub const PRESET_GROUPS: &[&str] = &["Títulos", "Destacados"];

/// The title looks offered in the Text tab. Each is a real `TextStyle`, using
/// what the engine's title painter already draws (size, weight, outline,
/// gradient, shadow and the bubble-shaped plates).
pub const PRESETS: &[Preset] = &[
    Preset {
        id: "basic",
        name: "Texto básico",
        group: "Títulos",
        sample: "Texto",
        css: "color:#fff;font-weight:700;text-shadow:0 2px 6px rgba(0,0,0,.6)",
        style: || base("Texto"),
    },
    Preset {
        id: "big",
        name: "Título grande",
        group: "Títulos",
        sample: "Título",
        css: "color:#fff;font-weight:900;font-size:26px;text-shadow:0 2px 6px rgba(0,0,0,.6)",
        style: || TextStyle {
            font_size: 0.14,
            font_weight: 900.0,
            ..base("Título")
        },
    },
    Preset {
        id: "subtitle",
        name: "Subtítulo",
        group: "Títulos",
        sample: "Subtítulo",
        css: "color:#fff;font-weight:500;font-size:13px;background:rgba(0,0,0,.7);padding:2px 8px;border-radius:4px",
        style: || TextStyle {
            font_size: 0.05,
            font_weight: 500.0,
            shadow: false,
            background: "#000000b3".to_string(),
            ..base("Subtítulo")
        },
    },
    Preset {
        id: "outline",
        name: "Con contorno",
        group: "Títulos",
        sample: "Contorno",
        css: "color:#fff;font-weight:900;-webkit-text-stroke:1.5px #000",
        style: || TextStyle {
            font_weight: 900.0,
            stroke_width: 0.008,
            stroke_color: "#000000".to_string(),
            ..base("Contorno")
        },
    },
    Preset {
        id: "caps",
        name: "Mayúsculas",
        group: "Títulos",
        sample: "MAYÚSCULAS",
        css: "color:#fff;font-weight:800;letter-spacing:2px;font-size:12px",
        style: || TextStyle {
            text_case: TextCase::Upper,
            tracking: 0.01,
            font_weight: 800.0,
            ..base("Mayúsculas")
        },
    },
    Preset {
        id: "neon",
        name: "Neón",
        group: "Destacados",
        sample: "Neón",
        css: "background:linear-gradient(#19c4cf,#a24dff);-webkit-background-clip:text;color:transparent;font-weight:800;font-size:22px",
        style: || TextStyle {
            color: "#19c4cf".to_string(),
            gradient: "#a24dff".to_string(),
            font_weight: 800.0,
            tracking: 0.004,
            ..base("Neón")
        },
    },
    Preset {
        id: "comic",
        name: "Cómic",
        group: "Destacados",
        sample: "¡Zas!",
        css: "color:#ffd400;font-weight:900;font-style:italic;font-size:22px;-webkit-text-stroke:1.5px #111",
        style: || TextStyle {
            color: "#ffd400".to_string(),
            font_size: 0.11,
            font_weight: 900.0,
            italic: true,
            stroke_width: 0.012,
            stroke_color: "#111111".to_string(),
            ..base("¡Zas!")
        },
    },
    Preset {
        id: "speech",
        name: "Globo de diálogo",
        group: "Destacados",
        sample: "Hola",
        css: "color:#111;background:#fff;border:2px solid #111;border-radius:14px;padding:2px 12px;font-weight:700",
        style: || TextStyle {
            color: "#111111".to_string(),
            font_size: 0.07,
            shadow: false,
            background: "#ffffff".to_string(),
            background_shape: "speech".to_string(),
            background_outline: "#111111".to_string(),
            ..base("Hola")
        },
    },
    Preset {
        id: "tag",
        name: "Etiqueta",
        group: "Destacados",
        sample: "Etiqueta",
        css: "color:#04262b;background:#19c4cf;border-radius:4px 12px 12px 4px;padding:2px 12px;font-weight:700",
        style: || TextStyle {
            color: "#04262b".to_string(),
            font_size: 0.06,
            shadow: false,
            background: "#19c4cf".to_string(),
            background_shape: "tag".to_string(),
            ..base("Etiqueta")
        },
    },
    Preset {
        id: "burst",
        name: "Estallido",
        group: "Destacados",
        sample: "¡Oferta!",
        css: "color:#111;background:#ffd400;border-radius:50%;padding:6px 10px;font-weight:900;font-size:13px",
        style: || TextStyle {
            color: "#111111".to_string(),
            font_size: 0.07,
            font_weight: 900.0,
            shadow: false,
            background: "#ffd400".to_string(),
            background_shape: "burst".to_string(),
            ..base("¡Oferta!")
        },
    },
    Preset {
        id: "banner",
        name: "Banner",
        group: "Destacados",
        sample: "Banner",
        css: "color:#fff;background:#e5384f;padding:2px 14px;font-weight:700",
        style: || TextStyle {
            font_size: 0.06,
            shadow: false,
            background: "#e5384f".to_string(),
            background_shape: "banner".to_string(),
            ..base("Banner")
        },
    },
];

/// Add the title look `preset_id` at `start`. Returns the new clip's id.
pub fn add_preset(editor: &mut Editor, start: f64, preset_id: &str) -> Option<String> {
    let preset = PRESETS.iter().find(|p| p.id == preset_id)?;
    add_styled(editor, start, (preset.style)())
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
