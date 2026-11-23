// SPDX-License-Identifier: AGPL-3.0-or-later
//
//! Applying the engine's effect catalogue to clips.
//!
//! A clip's look is a list of `AppliedFilter`s in `Clip::video_effects`; the
//! flattener hands that list to the compositor, so adding one here shows up in
//! the monitor and the export with no extra rendering code. The catalogue of
//! available effects is `concat_effects::Catalogue::builtin()`.

use concat_effects::{Catalogue, Kind, ParamType};
use concat_project::commands::ClipPatch;
use concat_project::model::AppliedFilter;
use concat_project::{Command, Editor};

/// The categories the effects panel lists, after "Todo": a label in Spanish and
/// the engine categories ("shelves") it gathers. The catalogue declares 34
/// categories, several with one or two effects, so related ones share a group;
/// the combined shelves ("Retro & Film", "Optical & Lens"…) are the engine's own.
pub const CATEGORIES: &[(&str, &[&str])] = &[
    ("Destacados", &["Featured"]),
    ("Retro y cine", &["Retro & Film", "Movies"]),
    ("Óptica y lentes", &["Optical & Lens"]),
    ("Distorsión y glitch", &["Distortion & Glitch"]),
    ("Fiesta y club", &["Party & Club", "Celebrate", "Vibe"]),
    (
        "Luz y sombra",
        &["Light & Shadow", "Night", "Neon", "Sparkle"],
    ),
    ("Color", &["Color", "Black & White", "Adjust", "Basic"]),
    ("Intro y cierre", &["Intro & Outro"]),
    ("Cotidiano", &["Everyday", "Food", "Nature", "Scenery"]),
    ("Retrato", &["Portrait", "Love", "Dreamy"]),
    ("Estilizado", &["Stylize", "Stylised", "Comics", "Texture"]),
    (
        "Recortes y texto",
        &["Cutout", "Text", "Split", "Screen", "Motion"],
    ),
];

/// One effect the user can apply, from the catalogue.
#[derive(Clone, PartialEq)]
pub struct EffectInfo {
    pub id: String,
    pub name: String,
    /// Indices into [`CATEGORIES`] this effect is listed under (may be empty:
    /// it is then only under "Todo").
    pub cats: Vec<usize>,
}

/// The visual effects and filters the built-in catalogue offers, by name.
pub fn catalogue() -> Vec<EffectInfo> {
    let mut list: Vec<EffectInfo> = Catalogue::builtin()
        .packages()
        .filter(|p| matches!(p.kind(), Kind::Effect | Kind::Filter))
        .map(|p| EffectInfo {
            id: p.id().to_string(),
            name: p.manifest.effect.name.clone(),
            cats: CATEGORIES
                .iter()
                .enumerate()
                .filter(|(_, (_, shelves))| shelves.iter().any(|s| p.matches_category(s)))
                .map(|(i, _)| i)
                .collect(),
        })
        .collect();
    list.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    list
}

/// A catalogue id's display name, or the id itself if unknown.
pub fn name_of(id: &str) -> String {
    Catalogue::builtin()
        .get(id)
        .map(|p| p.manifest.effect.name.clone())
        .unwrap_or_else(|| id.to_string())
}

/// An effect applied to a clip.
#[derive(Clone, PartialEq)]
pub struct Applied {
    pub index: usize,
    pub id: String,
    pub name: String,
}

/// The effects applied to a clip, in order.
pub fn applied(editor: &Editor, clip_id: &str) -> Vec<Applied> {
    editor
        .project()
        .active()
        .clip(clip_id)
        .map(|c| {
            c.video_effects
                .iter()
                .enumerate()
                .map(|(i, f)| Applied {
                    index: i,
                    id: f.id.clone(),
                    name: name_of(&f.id),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// One adjustable knob of an effect (the scalar kinds only).
#[derive(Clone, PartialEq)]
pub struct ParamInfo {
    pub key: String,
    pub label: String,
    /// The subhead the engine files it under ("Color", "Lightness"…); may be empty.
    pub group: String,
    pub kind: ParamType,
    pub min: f64,
    pub max: f64,
    pub default: f64,
    pub step: f64,
    pub unit: String,
    pub values: Vec<f64>,
    pub labels: Vec<String>,
}

/// The scalar parameters of an effect (Float, Int, Bool, Enum). Compound
/// kinds (Color, Point, Wheel, Curve) are left out for now.
pub fn params_of(effect_id: &str) -> Vec<ParamInfo> {
    Catalogue::builtin()
        .get(effect_id)
        .map(|p| {
            p.manifest
                .params
                .iter()
                .filter(|pr| {
                    matches!(
                        pr.kind,
                        ParamType::Float | ParamType::Int | ParamType::Bool | ParamType::Enum
                    )
                })
                .map(|pr| ParamInfo {
                    key: pr.key.clone(),
                    label: pr.label.clone(),
                    group: pr.group.clone(),
                    kind: pr.kind,
                    min: pr.min,
                    max: pr.max,
                    default: pr.default,
                    step: pr.step,
                    unit: pr.unit.clone(),
                    values: pr.values.clone(),
                    labels: pr.labels.clone(),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The current value of an effect's parameter on a clip, or its default.
pub fn param_value(editor: &Editor, clip_id: &str, index: usize, key: &str, default: f64) -> f64 {
    editor
        .project()
        .active()
        .clip(clip_id)
        .and_then(|c| c.video_effects.get(index))
        .and_then(|f| f.params.get(key).copied())
        .unwrap_or(default)
}

/// A parameter with its current value on a clip.
#[derive(Clone, PartialEq)]
pub struct ParamValue {
    pub info: ParamInfo,
    pub current: f64,
}

/// An applied effect with its parameters and their current values: everything
/// the panel needs for one effect, gathered in a single borrow.
#[derive(Clone, PartialEq)]
pub struct EffectCard {
    pub index: usize,
    pub id: String,
    pub name: String,
    pub params: Vec<ParamValue>,
}

/// The applied effects of a clip, each with its adjustable parameters.
pub fn effect_cards(editor: &Editor, clip_id: &str) -> Vec<EffectCard> {
    applied(editor, clip_id)
        .into_iter()
        // The colour panel has its own tab; it is not listed twice.
        .filter(|a| a.id != ADJUST_ID)
        .map(|a| {
            let params = params_of(&a.id)
                .into_iter()
                .map(|info| {
                    let current = param_value(editor, clip_id, a.index, &info.key, info.default);
                    ParamValue { info, current }
                })
                .collect();
            EffectCard {
                index: a.index,
                id: a.id,
                name: a.name,
                params,
            }
        })
        .collect()
}

/// Set an effect's parameter on a clip.
pub fn set_param(editor: &mut Editor, clip_id: &str, index: usize, key: &str, value: f64) -> bool {
    let mut list = current(editor, clip_id);
    let Some(filter) = list.get_mut(index) else {
        return false;
    };
    filter.params.insert(key.to_string(), value);
    set(editor, clip_id.to_string(), list)
}

/// Replace a clip's effect list.
fn set(editor: &mut Editor, clip_id: String, effects: Vec<AppliedFilter>) -> bool {
    editor
        .apply(Command::UpdateClip {
            clip_id,
            patch: ClipPatch {
                video_effects: Some(effects),
                ..ClipPatch::default()
            },
        })
        .map(|o| o.applied)
        .unwrap_or(false)
}

/// Current effect list of a clip (cloned), or empty.
fn current(editor: &Editor, clip_id: &str) -> Vec<AppliedFilter> {
    editor
        .project()
        .active()
        .clip(clip_id)
        .map(|c| c.video_effects.clone())
        .unwrap_or_default()
}

/// Append an effect to a clip.
pub fn add(editor: &mut Editor, clip_id: &str, effect_id: &str) -> bool {
    let mut list = current(editor, clip_id);
    list.push(AppliedFilter::new(effect_id));
    set(editor, clip_id.to_string(), list)
}

/// Remove the effect at `index` from a clip.
pub fn remove(editor: &mut Editor, clip_id: &str, index: usize) -> bool {
    let mut list = current(editor, clip_id);
    if index >= list.len() {
        return false;
    }
    list.remove(index);
    set(editor, clip_id.to_string(), list)
}

/// The engine's colour panel, shown in the inspector's Adjust tab.
pub const ADJUST_ID: &str = "concat.adjust";

/// The groups of the colour panel, as the engine names them, with a label.
pub const ADJUST_GROUPS: &[(&str, &str)] = &[
    ("Color", "Color"),
    ("Lightness", "Luminosidad"),
    ("Effects", "Efectos"),
];

/// Spanish names for the colour panel's knobs, by key.
pub fn adjust_label(key: &str, fallback: &str) -> String {
    match key {
        "temperature" => "Temperatura",
        "tint" => "Tinte",
        "saturation" => "Saturación",
        "exposure" => "Exposición",
        "brightness" => "Brillo",
        "contrast" => "Contraste",
        "highlights" => "Luces",
        "shadows" => "Sombras",
        "whites" => "Blancos",
        "blacks" => "Negros",
        "sharpen" => "Nitidez",
        "fade" => "Desvanecido",
        "vignette" => "Viñeta",
        _ => fallback,
    }
    .to_string()
}

/// The colour panel's grouped knobs with their values on a clip.
pub fn adjust_values(editor: &Editor, clip_id: &str) -> Vec<ParamValue> {
    let idx = current(editor, clip_id)
        .iter()
        .position(|f| f.id == ADJUST_ID);
    params_of(ADJUST_ID)
        .into_iter()
        .filter(|p| ADJUST_GROUPS.iter().any(|(g, _)| *g == p.group))
        .map(|info| {
            let current = match idx {
                Some(i) => param_value(editor, clip_id, i, &info.key, info.default),
                None => info.default,
            };
            ParamValue { info, current }
        })
        .collect()
}

/// Set one knob of the colour panel, adding the effect to the clip if needed.
pub fn set_adjust(editor: &mut Editor, clip_id: &str, key: &str, value: f64) -> bool {
    let mut list = current(editor, clip_id);
    let i = match list.iter().position(|f| f.id == ADJUST_ID) {
        Some(i) => i,
        None => {
            list.push(AppliedFilter::new(ADJUST_ID));
            list.len() - 1
        }
    };
    list[i].params.insert(key.to_string(), value);
    set(editor, clip_id.to_string(), list)
}

/// Take the colour panel off a clip.
pub fn reset_adjust(editor: &mut Editor, clip_id: &str) -> bool {
    let mut list = current(editor, clip_id);
    let before = list.len();
    list.retain(|f| f.id != ADJUST_ID);
    if list.len() == before {
        return false;
    }
    set(editor, clip_id.to_string(), list)
}
