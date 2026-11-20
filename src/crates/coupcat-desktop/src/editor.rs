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
pub fn add_clip_on(editor: &mut Editor, media_id: &str, track_id: &str, start: f64) -> Option<String> {
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
        o.id != b.id
            && o.track_id == b.track_id
            && (o.start + o.duration - b.start).abs() < 0.03
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
    let h_cur = get_transition(editor, clip_b).map(|(_, d)| d / 2.0).unwrap_or(0.0);
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
