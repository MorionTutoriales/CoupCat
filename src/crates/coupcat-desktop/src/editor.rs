// SPDX-License-Identifier: AGPL-3.0-or-later
//
//! The edit, owned by the engine.
//!
//! CoupCat's project is a `concat_project::Editor`: the single source of truth
//! for the timeline, with the engine's own undo stack and editing commands
//! (trim, split, move, ripple-delete…). This module wraps it with the few
//! operations the M2 UI drives and a cheap [`TimelineView`] snapshot the UI
//! renders from.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use concat_media::MediaInfo;
use concat_project::commands::{ClipMove, ClipPatch, NewMedia, TrackFlag, TrimEdge};
use concat_project::model::{ClipKind, MediaKind, Transition};
use concat_project::{Command, Editor};

/// A clip as the UI draws it on the timeline.
#[derive(Clone, PartialEq)]
pub struct ClipView {
    pub id: String,
    /// Index into [`TimelineView::tracks`], top lane first.
    pub track: usize,
    /// The engine track id this clip sits on (for move commands).
    pub track_id: String,
    pub start: f64,
    pub duration: f64,
    /// CSS class suffix for the clip's colour, by kind.
    pub kind: &'static str,
    pub name: String,
    /// True when the clip has an incoming transition on its cut.
    pub has_transition: bool,
    /// Seconds into the source where the clip starts (for the waveform).
    pub source_start: f64,
    /// The clip's media file, for the waveform.
    pub media_path: Option<String>,
}

/// A derived, render-ready snapshot of the active timeline.
#[derive(Clone, PartialEq, Default)]
pub struct TimelineView {
    /// Lane count, top to bottom.
    pub track_count: usize,
    /// Track ids, top to bottom (for dropping a clip onto a lane).
    pub track_ids: Vec<String>,
    /// Per lane: whether its audio is silenced (same order as `track_ids`).
    pub track_muted: Vec<bool>,
    /// Per lane: whether its video reaches the composite (same order).
    pub track_visible: Vec<bool>,
    pub clips: Vec<ClipView>,
    /// The timeline's end, in seconds.
    pub duration: f64,
    pub can_undo: bool,
    pub can_redo: bool,
    pub clip_count: usize,
}

/// Map a clip kind to a short CSS class suffix and label colour.
fn kind_class(kind: ClipKind) -> &'static str {
    match kind {
        ClipKind::Video => "video",
        ClipKind::Image => "image",
        ClipKind::Audio | ClipKind::SoundLayer => "audio",
        ClipKind::Text => "text",
        ClipKind::Layer => "layer",
        ClipKind::Shape => "shape",
    }
}

/// Build a [`NewMedia`] from a probed file.
fn new_media(info: &MediaInfo, path: &Path) -> NewMedia {
    let video = info.video.as_ref();
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "clip".to_string());
    let fraction = video.map(|v| {
        let fps = v.frame_rate.fps();
        format!("{}/{}", fps.numerator(), fps.denominator())
    });
    NewMedia {
        path: path.to_string_lossy().into_owned(),
        name,
        duration: info.duration.map(|r| r.as_f64()),
        kind: if video.is_some() {
            MediaKind::Video
        } else {
            MediaKind::Audio
        },
        width: video.map(|v| v.width),
        height: video.map(|v| v.height),
        frame_rate: video.map(|v| v.frame_rate.fps().as_f64()),
        frame_rate_fraction: fraction,
        video_codec: video.map(|v| v.codec.clone()),
        audio_codec: info.audio.as_ref().map(|a| a.codec.clone()),
        has_audio: info.audio.is_some(),
        audio_tracks: Vec::new(),
        origin: None,
        color_space: Default::default(),
    }
}

/// Add a probed file to the bin and drop a clip of it at the current end of
/// the timeline. Returns the new clip's id, if one was placed.
pub fn import(editor: &mut Editor, info: &MediaInfo, path: &Path) -> Option<String> {
    let media_id = editor
        .apply(Command::AddMedia {
            item: new_media(info, path),
        })
        .ok()?
        .created_id?;
    let start = view(editor).duration;
    editor
        .apply(Command::AddClipAtFirstFree { media_id, start })
        .ok()?
        .created_id
}

/// A media item in the project's bin (library).
#[derive(Clone, PartialEq)]
pub struct MediaView {
    pub id: String,
    pub name: String,
    pub kind: &'static str,
    pub duration: Option<f64>,
    /// The source file, for the thumbnail.
    pub path: String,
    /// Displayed `(width, height)` when the file has a picture.
    pub size: Option<(u32, u32)>,
}

/// The project's imported media, for the library panel.
pub fn media_list(editor: &Editor) -> Vec<MediaView> {
    editor
        .project()
        .media
        .iter()
        .map(|m| MediaView {
            id: m.id.clone(),
            name: m.name.clone(),
            kind: match m.kind {
                MediaKind::Video => "video",
                MediaKind::Audio => "audio",
                MediaKind::Image => "image",
            },
            duration: m.duration,
            path: m.path.clone(),
            size: m.width.zip(m.height),
        })
        .collect()
}

/// Add a probed file to the bin only (no clip). Returns the media id.
pub fn import_media(editor: &mut Editor, info: &MediaInfo, path: &Path) -> Option<String> {
    editor
        .apply(Command::AddMedia {
            item: new_media(info, path),
        })
        .ok()?
        .created_id
}

/// Drop a clip of a bin item onto a specific track at `start` seconds (the
/// engine finds the nearest free gap on that lane).
pub fn add_clip_on(
    editor: &mut Editor,
    media_id: &str,
    track_id: &str,
    start: f64,
) -> Option<String> {
    editor
        .apply(Command::AddClip {
            media_id: media_id.to_string(),
            track_id: track_id.to_string(),
            start: start.max(0.0),
            ripple: false,
        })
        .ok()?
        .created_id
}

/// Clip ids whose span contains `t` seconds, leaving out the clips of any lane
/// in `locked` (a locked lane's clips are not edited).
pub fn clips_at(editor: &Editor, t: f64, locked: &HashSet<String>) -> Vec<String> {
    editor
        .project()
        .active()
        .clips
        .iter()
        .filter(|c| t >= c.start && t < c.start + c.duration && !locked.contains(&c.track_id))
        .map(|c| c.id.clone())
        .collect()
}

/// Split every clip under `t` at `t` (except those on locked lanes).
pub fn split_at(editor: &mut Editor, t: f64, locked: &HashSet<String>) -> bool {
    let clip_ids = clips_at(editor, t, locked);
    if clip_ids.is_empty() {
        return false;
    }
    editor
        .apply(Command::SplitClips { clip_ids, time: t })
        .map(|o| o.applied)
        .unwrap_or(false)
}

/// Ripple-delete every clip under `t` (except those on locked lanes).
pub fn delete_at(editor: &mut Editor, t: f64, locked: &HashSet<String>) -> bool {
    let clip_ids = clips_at(editor, t, locked);
    if clip_ids.is_empty() {
        return false;
    }
    editor
        .apply(Command::RemoveClips {
            clip_ids,
            ripple: true,
        })
        .map(|o| o.applied)
        .unwrap_or(false)
}

/// Move a clip to `start` seconds on `track_id`. The engine places it
/// magnetically, avoiding overlaps.
pub fn move_clip(editor: &mut Editor, clip_id: String, start: f64, track_id: String) -> bool {
    editor
        .apply(Command::MoveClips {
            moves: vec![ClipMove {
                clip_id,
                start: start.max(0.0),
                track_id,
            }],
        })
        .map(|o| o.applied)
        .unwrap_or(false)
}

/// Trim a clip edge by `delta` seconds. `at_start` trims the head (moves the
/// in-point), otherwise the tail.
pub fn trim(editor: &mut Editor, clip_id: String, at_start: bool, delta: f64) -> bool {
    let edge = if at_start {
        TrimEdge::Start
    } else {
        TrimEdge::End
    };
    editor
        .apply(Command::TrimClip {
            clip_id,
            edge,
            delta,
            ripple: false,
        })
        .map(|o| o.applied)
        .unwrap_or(false)
}

/// A clip's audio settings, for the audio panel.
#[derive(Clone, PartialEq)]
pub struct AudioInfo {
    pub volume: f64,
    pub fade_in: f64,
    pub fade_out: f64,
    pub muted: bool,
}

/// A clip's audio settings, if it can carry sound (video or audio clip).
pub fn audio_info(editor: &Editor, clip_id: &str) -> Option<AudioInfo> {
    let c = editor.project().active().clip(clip_id)?;
    if !matches!(c.kind, ClipKind::Audio | ClipKind::Video) {
        return None;
    }
    Some(AudioInfo {
        volume: c.volume,
        fade_in: c.fade_in,
        fade_out: c.fade_out,
        muted: c.muted.unwrap_or(false),
    })
}

fn patch_clip(editor: &mut Editor, clip_id: String, patch: ClipPatch) -> bool {
    editor
        .apply(Command::UpdateClip { clip_id, patch })
        .map(|o| o.applied)
        .unwrap_or(false)
}

/// Set a clip's linear volume (1.0 = unity).
pub fn set_volume(editor: &mut Editor, clip_id: String, volume: f64) -> bool {
    patch_clip(
        editor,
        clip_id,
        ClipPatch {
            volume: Some(volume.max(0.0)),
            ..ClipPatch::default()
        },
    )
}

/// Set a clip's audio fade-in seconds.
pub fn set_fade_in(editor: &mut Editor, clip_id: String, secs: f64) -> bool {
    patch_clip(
        editor,
        clip_id,
        ClipPatch {
            fade_in: Some(secs.max(0.0)),
            ..ClipPatch::default()
        },
    )
}

/// Set a clip's audio fade-out seconds.
pub fn set_fade_out(editor: &mut Editor, clip_id: String, secs: f64) -> bool {
    patch_clip(
        editor,
        clip_id,
        ClipPatch {
            fade_out: Some(secs.max(0.0)),
            ..ClipPatch::default()
        },
    )
}

/// Mute or unmute a clip.
pub fn set_muted(editor: &mut Editor, clip_id: String, muted: bool) -> bool {
    patch_clip(
        editor,
        clip_id,
        ClipPatch {
            muted: Some(muted),
            ..ClipPatch::default()
        },
    )
}

/// The source file path of a clip's media, if it has one.
pub fn media_path(editor: &Editor, clip_id: &str) -> Option<String> {
    let project = editor.project();
    let clip = project.active().clip(clip_id)?;
    project
        .media
        .iter()
        .find(|m| m.id == clip.media_id)
        .map(|m| m.path.clone())
}

/// A clip's `(source_start, duration, timeline start)`, for captioning.
pub fn clip_timing(editor: &Editor, clip_id: &str) -> Option<(f64, f64, f64)> {
    editor
        .project()
        .active()
        .clip(clip_id)
        .map(|c| (c.source_start, c.duration, c.start))
}

/// Add many captions as text clips in one undo step, near the bottom.
pub fn add_captions(editor: &mut Editor, captions: &[(f64, f64, String)]) -> usize {
    use concat_project::model::{TextAlign, TextStyle};
    let mut commands = Vec::new();
    for (start, end, text) in captions {
        commands.push(Command::AddTextClip {
            track_id: None,
            above: true,
            start: *start,
            style: Some(TextStyle {
                content: text.clone(),
                font_size: 0.055,
                align: TextAlign::Center,
                background: "#000000a0".to_string(),
                background_radius: 0.01,
                background_padding_x: 0.015,
                background_padding_y: 0.008,
                ..TextStyle::default()
            }),
            duration: Some((end - start).max(0.3)),
            offset_y: Some(0.38),
        });
    }
    let n = commands.len();
    if n == 0 {
        return 0;
    }
    match editor.apply(Command::Batch { commands }) {
        Ok(o) if o.applied => n,
        _ => 0,
    }
}

/// Whether a clip has a background-removal cutout.
pub fn has_cutout(editor: &Editor, clip_id: &str) -> bool {
    editor
        .project()
        .active()
        .clip(clip_id)
        .is_some_and(|c| c.cutout.is_some())
}

/// Turn a clip into a background cutout (the masks must already be computed),
/// or remove it when `on` is false.
pub fn set_cutout(editor: &mut Editor, clip_id: String, on: bool) -> bool {
    let cutout = on.then(|| concat_project::model::Cutout {
        mode: concat_project::model::CutoutMode::Auto,
        subject: concat_project::model::Subject::Auto,
        feather: 0.01,
        strokes: Vec::new(),
    });
    editor
        .apply(Command::SetClipCutout { clip_id, cutout })
        .map(|o| o.applied)
        .unwrap_or(false)
}

/// Whether a clip directly follows another on its track (so a transition on
/// its cut has something to blend from). Without this, a transition is a
/// silent no-op — the engine orphans it.
pub fn has_incoming_cut(editor: &Editor, clip_id: &str) -> bool {
    let timeline = editor.project().active();
    let Some(clip) = timeline.clip(clip_id) else {
        return false;
    };
    timeline.clips.iter().any(|o| {
        o.id != clip.id
            && o.track_id == clip.track_id
            && (o.start + o.duration - clip.start).abs() < 0.03
    })
}

/// The incoming transition on a clip's cut, as `(kind id, duration)`.
pub fn get_transition(editor: &Editor, clip_id: &str) -> Option<(String, f64)> {
    editor
        .project()
        .active()
        .clip(clip_id)
        .and_then(|c| c.transition_in.as_ref().map(|t| (t.id.clone(), t.duration)))
}

/// The shortest a clip may be left after giving footage to an overlap.
const MIN_CLIP: f64 = 0.15;

/// The clip directly before `clip_id` on its track (the outgoing side of the
/// cut), and that clip's and `clip_id`'s current durations.
fn cut_neighbours(editor: &Editor, clip_id: &str) -> Option<(String, f64, f64)> {
    let timeline = editor.project().active();
    let b = timeline.clip(clip_id)?;
    let a = timeline.clips.iter().find(|o| {
        o.id != b.id && o.track_id == b.track_id && (o.start + o.duration - b.start).abs() < 0.03
    })?;
    Some((a.id.clone(), a.duration, b.duration))
}

/// Apply, change, or clear the transition on the cut into clip `clip_b`, the
/// CapCut way: the two clips really overlap.
///
/// A transition of `duration` needs `duration/2` of handle footage on each
/// side of the cut, so both clips are trimmed inward to create it (and
/// restored when the transition shrinks or is removed), shortening the
/// timeline — then the engine centres the overlap on the cut. `kind` of `None`
/// removes the transition and gives the footage back. One atomic undo step.
pub fn apply_transition(
    editor: &mut Editor,
    clip_b: &str,
    kind: Option<&str>,
    duration: f64,
) -> bool {
    let Some((a_id, a_dur, b_dur)) = cut_neighbours(editor, clip_b) else {
        return false;
    };

    // Handle currently carved out (we always carve duration/2 per side), and
    // the target for the requested duration.
    let h_cur = get_transition(editor, clip_b)
        .map(|(_, d)| d / 2.0)
        .unwrap_or(0.0);
    let mut h_target = match kind {
        Some(_) => (duration / 2.0).max(0.05),
        None => 0.0,
    };
    // Growing the handle consumes visible footage; never shrink a clip past
    // the minimum.
    if h_target > h_cur {
        let room = (a_dur.min(b_dur) - MIN_CLIP).max(0.0);
        h_target = h_cur + (h_target - h_cur).min(room);
    }
    let delta = h_target - h_cur;

    let mut commands = Vec::new();
    if delta.abs() > 1e-4 {
        // Carve (delta > 0) or give back (delta < 0) `delta` of handle on each
        // side; ripple keeps the lane closed up.
        commands.push(Command::TrimClip {
            clip_id: a_id,
            edge: TrimEdge::End,
            delta: -delta,
            ripple: true,
        });
        commands.push(Command::TrimClip {
            clip_id: clip_b.to_string(),
            edge: TrimEdge::Start,
            delta,
            ripple: true,
        });
    }
    let patch = match kind {
        Some(k) => ClipPatch {
            transition_in: Some(Some(Transition {
                id: k.to_string(),
                duration: (h_target * 2.0).max(0.1),
                params: Default::default(),
            })),
            ..ClipPatch::default()
        },
        None => ClipPatch {
            transition_in: Some(None),
            ..ClipPatch::default()
        },
    };
    commands.push(Command::UpdateClip {
        clip_id: clip_b.to_string(),
        patch,
    });

    editor
        .apply(Command::Batch { commands })
        .map(|o| o.applied)
        .unwrap_or(false)
}

/// Add an empty lane at the end of the lane list. The engine stores the stack
/// bottom first and draws the last lane on top of the others, so this is the
/// lane at the bottom of the list on screen and in front in the composition.
pub fn add_track(editor: &mut Editor) -> bool {
    editor
        .apply(Command::AddTrack { bottom: false })
        .map(|o| o.applied)
        .unwrap_or(false)
}

/// Delete a lane. The engine removes its clips too and refuses to go below
/// one lane, so callers only offer this for an empty lane.
pub fn remove_track(editor: &mut Editor, track_id: &str) -> bool {
    editor
        .apply(Command::RemoveTrack {
            track_id: track_id.to_string(),
        })
        .map(|o| o.applied)
        .unwrap_or(false)
}

/// Silence or un-silence a whole lane (`muted`), or hide or show its video in
/// the composite (`visible`). Undoable like any other edit.
pub fn set_track_flag(editor: &mut Editor, track_id: &str, flag: TrackFlag, value: bool) -> bool {
    editor
        .apply(Command::SetTrackFlag {
            track_id: track_id.to_string(),
            flag,
            value,
        })
        .map(|o| o.applied)
        .unwrap_or(false)
}

/// A render-ready snapshot of the active timeline.
pub fn view(editor: &Editor) -> TimelineView {
    let timeline = editor.project().active();
    let index: HashMap<&str, usize> = timeline
        .tracks
        .iter()
        .enumerate()
        .map(|(i, t)| (t.id.as_str(), i))
        .collect();

    let media: HashMap<&str, &str> = editor
        .project()
        .media
        .iter()
        .map(|m| (m.id.as_str(), m.path.as_str()))
        .collect();

    let mut clips = Vec::with_capacity(timeline.clips.len());
    let mut duration: f64 = 0.0;
    for clip in &timeline.clips {
        let track = index.get(clip.track_id.as_str()).copied().unwrap_or(0);
        duration = duration.max(clip.start + clip.duration);
        clips.push(ClipView {
            id: clip.id.clone(),
            track,
            track_id: clip.track_id.clone(),
            start: clip.start,
            duration: clip.duration,
            kind: kind_class(clip.kind),
            name: clip.name.clone(),
            has_transition: clip.transition_in.is_some(),
            source_start: clip.source_start,
            media_path: media.get(clip.media_id.as_str()).map(|s| s.to_string()),
        });
    }

    TimelineView {
        track_count: timeline.tracks.len(),
        track_ids: timeline.tracks.iter().map(|t| t.id.clone()).collect(),
        track_muted: timeline.tracks.iter().map(|t| t.muted).collect(),
        track_visible: timeline.tracks.iter().map(|t| t.visible).collect(),
        clip_count: clips.len(),
        clips,
        duration,
        can_undo: editor.can_undo(),
        can_redo: editor.can_redo(),
    }
}

// ---------------------------------------------------------------------------
// Inspector: placement, speed and animation of a clip
// ---------------------------------------------------------------------------

/// The kind of a clip as the inspector groups them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InspKind {
    Video,
    Image,
    Audio,
    Text,
    Other,
}

/// What kind of clip `clip_id` is, for choosing the inspector's tabs.
pub fn insp_kind(editor: &Editor, clip_id: &str) -> Option<InspKind> {
    editor
        .project()
        .active()
        .clip(clip_id)
        .map(|c| match c.kind {
            ClipKind::Video => InspKind::Video,
            ClipKind::Image => InspKind::Image,
            ClipKind::Audio => InspKind::Audio,
            ClipKind::Text => InspKind::Text,
            _ => InspKind::Other,
        })
}

/// A clip's picture placement and playback rate, as the inspector edits them.
#[derive(Clone, PartialEq)]
pub struct Props {
    pub scale: f64,
    pub offset_x: f64,
    pub offset_y: f64,
    pub rotation: f64,
    pub opacity: f64,
    pub flip_h: bool,
    pub flip_v: bool,
    pub blend: String,
    pub speed: f64,
    pub preserve_pitch: bool,
    pub duration: f64,
}

pub fn props(editor: &Editor, clip_id: &str) -> Option<Props> {
    editor.project().active().clip(clip_id).map(|c| Props {
        scale: c.scale,
        offset_x: c.offset_x,
        offset_y: c.offset_y,
        rotation: c.rotation,
        opacity: c.opacity,
        flip_h: c.flip_h,
        flip_v: c.flip_v,
        blend: c.blend.clone(),
        speed: c.speed,
        preserve_pitch: c.preserve_pitch,
        duration: c.duration,
    })
}

/// Change any of scale / offsets / rotation (each optional).
pub fn set_transform(
    editor: &mut Editor,
    clip_id: &str,
    scale: Option<f64>,
    offset_x: Option<f64>,
    offset_y: Option<f64>,
    rotation: Option<f64>,
) -> bool {
    editor
        .apply(Command::SetClipTransform {
            clip_id: clip_id.to_string(),
            scale,
            offset_x,
            offset_y,
            rotation,
            stretch_x: None,
            stretch_y: None,
        })
        .map(|o| o.applied)
        .unwrap_or(false)
}

/// Back to centred, unrotated, full size, opaque, unflipped, normal blend.
pub fn reset_transform(editor: &mut Editor, clip_id: &str) -> bool {
    let id = clip_id.to_string();
    let cmds = vec![
        Command::SetClipTransform {
            clip_id: id.clone(),
            scale: Some(1.0),
            offset_x: Some(0.0),
            offset_y: Some(0.0),
            rotation: Some(0.0),
            stretch_x: None,
            stretch_y: None,
        },
        Command::UpdateClip {
            clip_id: id,
            patch: ClipPatch {
                opacity: Some(1.0),
                flip_h: Some(false),
                flip_v: Some(false),
                blend: Some(String::new()),
                ..ClipPatch::default()
            },
        },
    ];
    batch(editor, cmds)
}

pub fn set_opacity(editor: &mut Editor, clip_id: &str, v: f64) -> bool {
    patch_clip(
        editor,
        clip_id.to_string(),
        ClipPatch {
            opacity: Some(v.clamp(0.0, 1.0)),
            ..ClipPatch::default()
        },
    )
}

pub fn set_flip(editor: &mut Editor, clip_id: &str, horizontal: bool, on: bool) -> bool {
    let mut patch = ClipPatch::default();
    if horizontal {
        patch.flip_h = Some(on);
    } else {
        patch.flip_v = Some(on);
    }
    patch_clip(editor, clip_id.to_string(), patch)
}

pub fn set_blend(editor: &mut Editor, clip_id: &str, blend: &str) -> bool {
    patch_clip(
        editor,
        clip_id.to_string(),
        ClipPatch {
            blend: Some(if blend == "normal" {
                String::new()
            } else {
                blend.to_string()
            }),
            ..ClipPatch::default()
        },
    )
}

/// Set the playback rate; the clip's duration follows (source held constant).
pub fn set_speed(editor: &mut Editor, clip_id: &str, speed: f64) -> bool {
    editor
        .apply(Command::SetClipSpeed {
            clip_id: clip_id.to_string(),
            speed,
            ripple: false,
        })
        .map(|o| o.applied)
        .unwrap_or(false)
}

pub fn set_preserve_pitch(editor: &mut Editor, clip_id: &str, on: bool) -> bool {
    patch_clip(
        editor,
        clip_id.to_string(),
        ClipPatch {
            preserve_pitch: Some(on),
            ..ClipPatch::default()
        },
    )
}

/// Where an animation sits in the clip.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AnimSlot {
    In,
    Out,
    Loop,
}

/// Ready-made animations: `(id, label)`.
pub const ANIM_IN: &[(&str, &str)] = &[
    ("fade", "Fundido"),
    ("zoom-in", "Acercar"),
    ("zoom-out", "Alejar"),
    ("slide-left", "Desde la izquierda"),
    ("slide-right", "Desde la derecha"),
    ("slide-up", "Desde arriba"),
    ("slide-down", "Desde abajo"),
    ("spin", "Giro"),
];
pub const ANIM_LOOP: &[(&str, &str)] = &[
    ("pulse", "Latido"),
    ("sway", "Balanceo"),
    ("slow-zoom-in", "Zoom lento +"),
    ("slow-zoom-out", "Zoom lento -"),
    ("blink", "Parpadeo"),
];

pub fn anim_list(slot: AnimSlot) -> &'static [(&'static str, &'static str)] {
    match slot {
        AnimSlot::In | AnimSlot::Out => ANIM_IN,
        AnimSlot::Loop => ANIM_LOOP,
    }
}

use concat_project::model::{KeyEase, KeyProperty};

/// The keys a preset puts on a clip, `(property, at, value)`, given the clip's
/// own values. The ramp of an in/out animation lasts about half a second.
fn anim_keys(slot: AnimSlot, id: &str, p: &Props) -> Vec<(KeyProperty, f64, f64)> {
    let mut k = Vec::new();
    if slot == AnimSlot::Loop {
        match id {
            "pulse" => {
                for (at, f) in [(0.0, 1.0), (0.5, 1.12), (1.0, 1.0)] {
                    k.push((KeyProperty::Scale, at, p.scale * f));
                }
            }
            "sway" => {
                for (at, d) in [(0.0, -5.0), (0.5, 5.0), (1.0, -5.0)] {
                    k.push((KeyProperty::Rotation, at, p.rotation + d));
                }
            }
            "slow-zoom-in" => {
                k.push((KeyProperty::Scale, 0.0, p.scale));
                k.push((KeyProperty::Scale, 1.0, p.scale * 1.2));
            }
            "slow-zoom-out" => {
                k.push((KeyProperty::Scale, 0.0, p.scale * 1.2));
                k.push((KeyProperty::Scale, 1.0, p.scale));
            }
            "blink" => {
                for (at, f) in [
                    (0.0, 1.0),
                    (0.25, 0.35),
                    (0.5, 1.0),
                    (0.75, 0.35),
                    (1.0, 1.0),
                ] {
                    k.push((KeyProperty::Opacity, at, p.opacity * f));
                }
            }
            _ => {}
        }
        return k;
    }
    let e = (0.5 / p.duration.max(0.2)).min(0.4);
    // The ramp runs between the resting value and the "away" value.
    let (rest_at, away_at) = match slot {
        AnimSlot::In => (e, 0.0),
        _ => (1.0 - e, 1.0),
    };
    let mut away = |prop: KeyProperty, rest: f64, gone: f64| {
        k.push((prop, rest_at, rest));
        k.push((prop, away_at, gone));
    };
    match id {
        "fade" => away(KeyProperty::Opacity, p.opacity, 0.0),
        "zoom-in" => {
            away(KeyProperty::Scale, p.scale, p.scale * 0.5);
            away(KeyProperty::Opacity, p.opacity, 0.0);
        }
        "zoom-out" => {
            away(KeyProperty::Scale, p.scale, p.scale * 1.6);
            away(KeyProperty::Opacity, p.opacity, 0.0);
        }
        "slide-left" => away(KeyProperty::OffsetX, p.offset_x, p.offset_x - 0.6),
        "slide-right" => away(KeyProperty::OffsetX, p.offset_x, p.offset_x + 0.6),
        "slide-up" => away(KeyProperty::OffsetY, p.offset_y, p.offset_y - 0.6),
        "slide-down" => away(KeyProperty::OffsetY, p.offset_y, p.offset_y + 0.6),
        "spin" => {
            away(KeyProperty::Rotation, p.rotation, p.rotation - 90.0);
            away(KeyProperty::Opacity, p.opacity, 0.0);
        }
        _ => {}
    }
    k
}

fn slot_has(slot: AnimSlot, at: f64) -> bool {
    match slot {
        AnimSlot::In => at <= 0.45,
        AnimSlot::Out => at >= 0.55,
        AnimSlot::Loop => true,
    }
}

fn animatable(p: KeyProperty) -> bool {
    matches!(
        p,
        KeyProperty::Scale
            | KeyProperty::OffsetX
            | KeyProperty::OffsetY
            | KeyProperty::Rotation
            | KeyProperty::Opacity
    )
}

fn key_order(a: &(KeyProperty, f64, f64), b: &(KeyProperty, f64, f64)) -> std::cmp::Ordering {
    (a.0 as u8, a.1)
        .partial_cmp(&(b.0 as u8, b.1))
        .unwrap_or(std::cmp::Ordering::Equal)
}

/// Which preset of `slot` is on the clip, if the keys match one.
pub fn current_anim(editor: &Editor, clip_id: &str, slot: AnimSlot) -> Option<&'static str> {
    let clip = editor.project().active().clip(clip_id)?;
    let props = props(editor, clip_id)?;
    let mut have: Vec<(KeyProperty, f64, f64)> = clip
        .keys
        .iter()
        .filter(|k| animatable(k.property) && slot_has(slot, k.at))
        .map(|k| (k.property, k.at, k.value))
        .collect();
    if have.is_empty() {
        return None;
    }
    have.sort_by(key_order);
    for (id, _) in anim_list(slot) {
        let mut want = anim_keys(slot, id, &props);
        want.sort_by(key_order);
        if want.len() == have.len()
            && want
                .iter()
                .zip(&have)
                .all(|(a, b)| a.0 == b.0 && (a.1 - b.1).abs() < 0.01 && (a.2 - b.2).abs() < 0.01)
        {
            return Some(id);
        }
    }
    None
}

fn batch(editor: &mut Editor, commands: Vec<Command>) -> bool {
    if commands.is_empty() {
        return false;
    }
    editor
        .apply(Command::Batch { commands })
        .map(|o| o.applied)
        .unwrap_or(false)
}

/// The commands that take an animation slot's keys off the clip.
fn clear_cmds(editor: &Editor, clip_id: &str, slot: AnimSlot) -> Vec<Command> {
    let Some(clip) = editor.project().active().clip(clip_id) else {
        return Vec::new();
    };
    clip.keys
        .iter()
        .filter(|k| animatable(k.property) && slot_has(slot, k.at))
        .map(|k| Command::ClearClipKey {
            clip_id: clip_id.to_string(),
            property: k.property,
            at: k.at,
        })
        .collect()
}

/// Take an animation slot's keys off the clip (one undo step).
pub fn clear_anim(editor: &mut Editor, clip_id: &str, slot: AnimSlot) -> bool {
    let cmds = clear_cmds(editor, clip_id, slot);
    batch(editor, cmds)
}

/// Put preset `id` on the clip's `slot`, replacing what was there, in one undo
/// step. A looping animation replaces every animation; an entry or exit keeps
/// the others.
pub fn set_anim(editor: &mut Editor, clip_id: &str, slot: AnimSlot, id: &str) -> bool {
    let Some(p) = props(editor, clip_id) else {
        return false;
    };
    let mut cmds = Vec::new();
    if slot == AnimSlot::Loop {
        cmds.extend(clear_cmds(editor, clip_id, AnimSlot::In));
        cmds.extend(clear_cmds(editor, clip_id, AnimSlot::Out));
    } else {
        cmds.extend(clear_cmds(editor, clip_id, slot));
    }
    for (property, at, value) in anim_keys(slot, id, &p) {
        cmds.push(Command::SetClipKey {
            clip_id: clip_id.to_string(),
            property,
            at,
            value,
            ease: KeyEase::IN_OUT,
        });
    }
    batch(editor, cmds)
}

#[cfg(test)]
mod inspector_tests {
    use super::*;

    /// A project holding one video clip of the first video found in the user's
    /// Videos folder, or `None` when there is none to probe.
    fn with_video() -> Option<(Editor, String, std::path::PathBuf)> {
        let dir = std::path::PathBuf::from(std::env::var("USERPROFILE").ok()?).join("Videos");
        let path = std::fs::read_dir(dir)
            .ok()?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .find(|p| p.extension().is_some_and(|x| x == "mp4"))?;
        let info = concat_media::probe(&path).ok()?;
        let mut ed = Editor::new();
        let id = import(&mut ed, &info, &path)?;
        Some((ed, id, path))
    }

    #[test]
    fn speed_changes_duration_and_undoes() {
        let Some((mut ed, id, _)) = with_video() else {
            return;
        };
        let before = props(&ed, &id).unwrap();
        assert!(set_speed(&mut ed, &id, 2.0));
        let after = props(&ed, &id).unwrap();
        assert!((after.duration - before.duration / 2.0).abs() < 0.05);
        ed.undo();
        assert!((props(&ed, &id).unwrap().duration - before.duration).abs() < 1e-6);
    }

    #[test]
    fn animations_are_one_undo_step_and_detected() {
        let Some((mut ed, id, _)) = with_video() else {
            return;
        };
        assert_eq!(current_anim(&ed, &id, AnimSlot::In), None);
        assert!(set_anim(&mut ed, &id, AnimSlot::In, "zoom-in"));
        assert_eq!(current_anim(&ed, &id, AnimSlot::In), Some("zoom-in"));
        assert_eq!(current_anim(&ed, &id, AnimSlot::Out), None);
        assert!(set_anim(&mut ed, &id, AnimSlot::Out, "fade"));
        assert_eq!(current_anim(&ed, &id, AnimSlot::In), Some("zoom-in"));
        assert_eq!(current_anim(&ed, &id, AnimSlot::Out), Some("fade"));
        // Replacing the entry leaves the exit alone.
        assert!(set_anim(&mut ed, &id, AnimSlot::In, "spin"));
        assert_eq!(current_anim(&ed, &id, AnimSlot::In), Some("spin"));
        assert_eq!(current_anim(&ed, &id, AnimSlot::Out), Some("fade"));
        assert!(clear_anim(&mut ed, &id, AnimSlot::In));
        assert_eq!(current_anim(&ed, &id, AnimSlot::In), None);
        assert_eq!(current_anim(&ed, &id, AnimSlot::Out), Some("fade"));
        // One undo takes back the whole clear.
        ed.undo();
        assert_eq!(current_anim(&ed, &id, AnimSlot::In), Some("spin"));
        // A combined animation replaces entry and exit.
        assert!(set_anim(&mut ed, &id, AnimSlot::Loop, "pulse"));
        assert_eq!(current_anim(&ed, &id, AnimSlot::Loop), Some("pulse"));
    }

    #[test]
    fn transform_and_reset() {
        let Some((mut ed, id, _)) = with_video() else {
            return;
        };
        assert!(set_transform(
            &mut ed,
            &id,
            Some(1.5),
            Some(0.2),
            None,
            Some(30.0)
        ));
        let p = props(&ed, &id).unwrap();
        assert_eq!((p.scale, p.offset_x, p.rotation), (1.5, 0.2, 30.0));
        assert!(set_opacity(&mut ed, &id, 0.4));
        assert!(set_flip(&mut ed, &id, true, true));
        assert!(set_blend(&mut ed, &id, "multiply"));
        assert!(reset_transform(&mut ed, &id));
        let p = props(&ed, &id).unwrap();
        assert_eq!(
            (p.scale, p.offset_x, p.rotation, p.opacity),
            (1.0, 0.0, 0.0, 1.0)
        );
        assert!(!p.flip_h && p.blend.is_empty());
    }

    #[test]
    fn adjust_is_one_effect_and_hidden_from_the_list() {
        let Some((mut ed, id, _)) = with_video() else {
            return;
        };
        assert!(crate::effects::set_adjust(&mut ed, &id, "contrast", 30.0));
        assert!(crate::effects::set_adjust(&mut ed, &id, "saturation", 20.0));
        assert_eq!(crate::effects::applied(&ed, &id).len(), 1);
        assert!(crate::effects::effect_cards(&ed, &id).is_empty());
        let v = crate::effects::adjust_values(&ed, &id);
        assert!(
            v.iter()
                .any(|p| p.info.key == "contrast" && p.current == 30.0)
        );
        assert_eq!(v.len(), 13);
        assert!(crate::effects::reset_adjust(&mut ed, &id));
        assert!(crate::effects::applied(&ed, &id).is_empty());
    }

    #[test]
    fn canvas_ratio_changes_the_frame_and_undoes() {
        let mut ed = Editor::new();
        let (w, h) = video_size(&ed);
        assert_eq!(ratio_id(w, h), "16:9");
        assert!(set_ratio(&mut ed, "9:16"));
        assert_eq!(video_size(&ed), (1080, 1920));
        assert_eq!(ratio_id(1080, 1920), "9:16");
        assert!(!set_ratio(&mut ed, "9:16"), "no change is not an edit");
        assert!(set_ratio(&mut ed, "1:1"));
        assert_eq!(video_size(&ed), (1080, 1080));
        ed.undo();
        assert_eq!(video_size(&ed), (1080, 1920));
        assert_eq!(ratio_id(1000, 900), "custom");
    }

    #[test]
    fn effects_toggle_reset_and_undo() {
        let Some((mut ed, id, _)) = with_video() else {
            return;
        };
        let fx = crate::effects::catalogue()
            .into_iter()
            .find(|e| {
                crate::effects::params_of(&e.id)
                    .iter()
                    .any(|p| crate::effects::is_intensity(&p.key))
            })
            .expect("an effect with an intensity");
        assert!(crate::effects::add(&mut ed, &id, &fx.id));
        let key = crate::effects::params_of(&fx.id)
            .into_iter()
            .find(|p| crate::effects::is_intensity(&p.key))
            .unwrap();
        let v = if key.default < key.max {
            key.max
        } else {
            key.min
        };
        assert!(crate::effects::set_param(&mut ed, &id, 0, &key.key, v));
        assert_eq!(
            crate::effects::effect_cards(&ed, &id)[0]
                .params
                .iter()
                .find(|p| p.info.key == key.key)
                .unwrap()
                .current,
            v
        );
        assert!(crate::effects::set_enabled(&mut ed, &id, 0, false));
        assert!(!crate::effects::effect_cards(&ed, &id)[0].enabled);
        assert!(!crate::effects::set_enabled(&mut ed, &id, 0, false));
        ed.undo();
        assert!(crate::effects::effect_cards(&ed, &id)[0].enabled);
        assert!(crate::effects::reset_params(&mut ed, &id, 0));
        let cur = crate::effects::effect_cards(&ed, &id)[0]
            .params
            .iter()
            .find(|p| p.info.key == key.key)
            .unwrap()
            .current;
        assert_eq!(cur, key.default);
        ed.undo();
        ed.undo();
        ed.undo();
        assert!(crate::effects::applied(&ed, &id).is_empty());
    }

    /// Writes a project (a video and a title) for the UI tests to open:
    /// `COUPCAT_FIXTURE=<path> cargo test -p coupcat-desktop write_fixture -- --ignored`.
    #[test]
    #[ignore]
    fn write_fixture() {
        let out = std::env::var("COUPCAT_FIXTURE").expect("COUPCAT_FIXTURE");
        let (mut ed, _, _) = with_video().expect("a video in Videos");
        crate::text::add_text(&mut ed, 0.0).unwrap();
        crate::project_file::save(&ed, std::path::Path::new(&out)).unwrap();
    }
}

// ---------------------------------------------------------------------------
// Canvas ratio
// ---------------------------------------------------------------------------

/// The canvas shapes the player offers: `(id, label, width, height)`, all
/// 1080 on the short side.
pub const RATIOS: &[(&str, &str, u32, u32)] = &[
    ("16:9", "16:9", 1920, 1080),
    ("9:16", "9:16", 1080, 1920),
    ("1:1", "1:1", 1080, 1080),
    ("4:3", "4:3", 1440, 1080),
    ("3:4", "3:4", 1080, 1440),
];

/// The active timeline's frame size.
pub fn video_size(editor: &Editor) -> (u32, u32) {
    let v = editor.project().active().video;
    (v.width, v.height)
}

/// The id of the canvas shape (see [`RATIOS`]) matching `(w, h)`, or "custom".
pub fn ratio_id(w: u32, h: u32) -> &'static str {
    RATIOS
        .iter()
        .find(|(_, _, rw, rh)| (w as u64) * (*rh as u64) == (h as u64) * (*rw as u64))
        .map(|(id, ..)| *id)
        .unwrap_or("custom")
}

/// Give the active timeline another canvas shape, keeping its frame rate.
pub fn set_ratio(editor: &mut Editor, id: &str) -> bool {
    let Some(&(_, _, width, height)) = RATIOS.iter().find(|(rid, ..)| *rid == id) else {
        return false;
    };
    let timeline_id = editor.project().active().id.clone();
    let mut video = editor.project().active().video;
    if (video.width, video.height) == (width, height) {
        return false;
    }
    video.width = width;
    video.height = height;
    editor
        .apply(Command::SetTimelineVideo { timeline_id, video })
        .map(|o| o.applied)
        .unwrap_or(false)
}
