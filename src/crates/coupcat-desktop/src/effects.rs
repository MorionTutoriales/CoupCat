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

/// One effect the user can apply, from the catalogue.
#[derive(Clone, PartialEq)]
pub struct EffectInfo {
    pub id: String,
    pub name: String,
}

/// The visual effects and filters the built-in catalogue offers, by name.
pub fn catalogue() -> Vec<EffectInfo> {
    let mut list: Vec<EffectInfo> = Catalogue::builtin()
        .packages()
        .filter(|p| matches!(p.kind(), Kind::Effect | Kind::Filter))
        .map(|p| EffectInfo {
            id: p.id().to_string(),
            name: p.manifest.effect.name.clone(),
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
pub fn set_param(
    editor: &mut Editor,
    clip_id: &str,
    index: usize,
    key: &str,
    value: f64,
) -> bool {
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
