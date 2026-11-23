// SPDX-License-Identifier: AGPL-3.0-or-later
//
//! The timeline: ruler, track headers, lanes, clips and the status bar.

use dioxus::prelude::*;

use super::Ctx;
use super::toolbar::toolbar;
use crate::editor::{self, TimelineView};
use crate::monitor::{self, Cmd, fmt_time};
use crate::peaks;

/// Pixels per second on the timeline at the start, and the zoom's bounds
/// (from a whole multi-hour project down to single frames).
pub const PPS_DEFAULT: f64 = 50.0;
pub const PPS_MIN: f64 = 1.0;
pub const PPS_MAX: f64 = 800.0;

/// How near (in pixels, whatever the zoom) an edge has to come to a snap
/// target to be pulled onto it.
pub const SNAP_PX: f64 = 8.0;

/// Keep a zoom level inside its bounds.
pub fn clamp_pps(pps: f64) -> f64 {
    pps.clamp(PPS_MIN, PPS_MAX)
}

/// The zoom slider's 0..=100 position for a scale, and back (logarithmic, so
/// each step feels like the same change).
pub fn pps_to_slider(pps: f64) -> f64 {
    (clamp_pps(pps) / PPS_MIN).ln() / (PPS_MAX / PPS_MIN).ln() * 100.0
}

pub fn slider_to_pps(v: f64) -> f64 {
    clamp_pps(PPS_MIN * (PPS_MAX / PPS_MIN).powf(v.clamp(0.0, 100.0) / 100.0))
}

/// Script that, once the timeline has re-laid-out at the new scale, scrolls so
/// that time `t` stays under the same screen x (`x`, or where it already is,
/// or the middle when it is off screen).
pub fn zoom_script(t: f64, old: f64, new: f64, x: Option<f64>) -> String {
    let x = match x {
        Some(x) => format!("{x}"),
        None => format!(
            "(function(){{var x={t}*{old}-s.scrollLeft;return (x<0||x>s.clientWidth)?s.clientWidth/2:x}})()"
        ),
    };
    format!(
        "(function(){{var s=document.querySelector('.tl-scroll');if(!s)return;var x={x};setTimeout(function(){{s.scrollLeft=Math.max(0,{t}*{new}-x)}},40)}})()"
    )
}

/// The ruler's tick spacing for a scale: the major step in seconds (labelled,
/// at least ~70 px apart) and the minor step between its ticks.
pub fn ruler_steps(pps: f64) -> (f64, f64) {
    const STEPS: [f64; 17] = [
        0.05, 0.1, 0.2, 0.5, 1.0, 2.0, 5.0, 10.0, 15.0, 30.0, 60.0, 120.0, 300.0, 600.0, 900.0,
        1800.0, 3600.0,
    ];
    let major = STEPS
        .iter()
        .copied()
        .find(|s| s * pps >= 70.0)
        .unwrap_or(7200.0);
    let minor = major / 5.0;
    if minor * pps >= 7.0 {
        (major, minor)
    } else {
        (major, major)
    }
}

/// A ruler label: `m:ss`, with decimals when the step is under a second.
pub fn ruler_label(t: f64, major: f64) -> String {
    if major >= 1.0 {
        return fmt_time(t);
    }
    let m = (t / 60.0).floor() as i64;
    format!("{m}:{:05.2}", t - m as f64 * 60.0)
}

/// Pull a drag onto the nearest snap target. `dsec` is the raw travel in
/// seconds; returns the travel to use and, when it snapped, the time of the
/// guide to draw. Targets are the start of the timeline, the playhead and the
/// edges of every other clip; the edges that snap are the moving ones.
pub fn snap_drag(
    d: &Drag,
    tl: &TimelineView,
    playhead: f64,
    pps: f64,
    dsec: f64,
) -> (f64, Option<f64>) {
    let mut targets = vec![0.0, playhead];
    for c in tl.clips.iter().filter(|c| c.id != d.clip_id) {
        targets.push(c.start);
        targets.push(c.start + c.duration);
    }
    let start = d.orig_start + dsec;
    let end = d.orig_start + d.orig_dur + dsec;
    let edges: &[f64] = match d.mode {
        DragMode::Move => &[start, end],
        DragMode::TrimStart => &[start],
        DragMode::TrimEnd => &[end],
    };
    let threshold = SNAP_PX / pps;
    let mut best: Option<(f64, f64)> = None; // (distance, shift)
    for &edge in edges {
        for &t in &targets {
            let dist = (t - edge).abs();
            if dist <= threshold && best.is_none_or(|(b, _)| dist < b) {
                best = Some((dist, t - edge));
            }
        }
    }
    match best {
        Some((_, shift)) => {
            let guide = edges
                .iter()
                .map(|e| e + shift)
                .find(|e| targets.iter().any(|t| (t - e).abs() < 1e-9));
            (dsec + shift, guide)
        }
        None => (dsec, None),
    }
}

/// Timeline panel height: tall enough for the toolbar, the ruler, the four
/// starter lanes and the status bar; the user can drag it between these bounds.
pub const TL_HEIGHT_DEFAULT: f64 = 312.0;
pub const TL_HEIGHT_MIN: f64 = 170.0;
pub const TL_HEIGHT_MAX: f64 = 560.0;

/// The timeline panel's height in pixels. Until the person drags its edge
/// (custom is None) it follows the window: about 42 % of its height, so the
/// player keeps room on a small window and the full default fits a normal one.
/// Once dragged, the chosen height holds, kept below 70 % of the window.
pub fn panel_height(custom: Option<f64>, window_h: f64) -> f64 {
    match custom {
        None => (window_h * 0.42).clamp(TL_HEIGHT_MIN, TL_HEIGHT_DEFAULT),
        Some(h) => h
            .clamp(TL_HEIGHT_MIN, TL_HEIGHT_MAX)
            .min(window_h * 0.7)
            .max(TL_HEIGHT_MIN),
    }
}

/// What a timeline drag is doing to a clip.
#[derive(Clone, Copy, PartialEq)]
pub enum DragMode {
    /// Slide the whole clip along its lane.
    Move,
    /// Drag the left edge (trim the head / in-point).
    TrimStart,
    /// Drag the right edge (trim the tail).
    TrimEnd,
}

/// An in-progress drag of a clip on the timeline.
#[derive(Clone)]
pub struct Drag {
    pub mode: DragMode,
    pub clip_id: String,
    pub track_id: String,
    pub orig_start: f64,
    pub orig_dur: f64,
    /// Mouse page-x where the drag began, and where it is now (pixels).
    pub start_px: f64,
    pub cur_px: f64,
}

pub fn timeline_panel(c: &Ctx) -> Element {
    let project = c.project.clone();
    let monitor = c.monitor.clone();
    let pos = c.pos;
    let dur = c.dur;
    let mut scrubbing = c.scrubbing;
    let tl_height = c.tl_height;
    let win_h = c.win_h;
    let mut resizing = c.resizing;
    let mut locked_tracks = c.locked_tracks;
    let mut track_names = c.track_names;
    let mut renaming = c.renaming;
    let mut timeline = c.timeline;
    let mut dragging_media = c.dragging_media;
    let mut drag = c.drag;
    let mut drop_lane = c.drop_lane;
    let wave_rev = c.wave_rev;
    let mut selected = c.selected;
    let mut dragging_fx = c.dragging_fx;
    let c_ctx = c.clone();
    let pps = (c.pps)();
    let snap_on = (c.snap)();

    // Drop a library item onto a lane at a timeline position.
    let on_drop_media = {
        let project = project.clone();
        let monitor = monitor.clone();
        move |track_id: String, start: f64| {
            let Some(media_id) = dragging_media() else {
                return;
            };
            dragging_media.set(None);
            // A locked lane refuses new clips.
            if locked_tracks().contains(&track_id) {
                return;
            }
            if let Some(cid) =
                editor::add_clip_on(&mut project.borrow_mut(), &media_id, &track_id, start)
            {
                selected.set(Some(cid));
            }
            timeline.set(editor::view(&project.borrow()));
            if let Some(d) = monitor::timeline_data(&project.borrow()) {
                monitor.send(Cmd::SetTimeline(d));
            }
        }
    };

    // Mute / hide a whole lane. These are engine edits (undoable, and honoured
    // by the preview and the export alike), unlike lock and rename below.
    let on_track_flag = {
        let project = project.clone();
        let monitor = monitor.clone();
        move |track_id: String, flag: concat_project::commands::TrackFlag, value: bool| {
            editor::set_track_flag(&mut project.borrow_mut(), &track_id, flag, value);
            timeline.set(editor::view(&project.borrow()));
            if let Some(d) = monitor::timeline_data(&project.borrow()) {
                monitor.send(Cmd::SetTimeline(d));
            }
        }
    };

    // Add an empty lane at the end of the list, or delete an empty one (see
    // `removable` below).
    let on_add_track = {
        let project = project.clone();
        let monitor = monitor.clone();
        move |_| {
            editor::add_track(&mut project.borrow_mut());
            timeline.set(editor::view(&project.borrow()));
            if let Some(d) = monitor::timeline_data(&project.borrow()) {
                monitor.send(Cmd::SetTimeline(d));
            }
        }
    };

    let on_remove_track = {
        let project = project.clone();
        let monitor = monitor.clone();
        move |track_id: String| {
            editor::remove_track(&mut project.borrow_mut(), &track_id);
            timeline.set(editor::view(&project.borrow()));
            if let Some(d) = monitor::timeline_data(&project.borrow()) {
                monitor.send(Cmd::SetTimeline(d));
            }
        }
    };

    let on_drag_move = move |e: MouseEvent| {
        let mut d = drag();
        if let Some(inner) = d.as_mut() {
            inner.cur_px = e.page_coordinates().x;
            drag.set(d);
        }
    };

    // A fresh end-of-drag handler each time (used on both mouseup and
    // mouseleave, and a closure capturing !Copy state can't be reused).
    let end_drag = {
        let project = project.clone();
        let monitor = monitor.clone();
        move || {
            let project = project.clone();
            let monitor = monitor.clone();
            move |_: MouseEvent| {
                let Some(d) = drag() else { return };
                drag.set(None);
                // Where to drop: the lane under the cursor, else the clip's own.
                // Resolve it (and clear drop_lane) BEFORE the minimal-move guard,
                // so a vertical drag to another lane is never dropped on the floor.
                let mut target_track = drop_lane()
                    .and_then(|li| timeline().track_ids.get(li).cloned())
                    .unwrap_or_else(|| d.track_id.clone());
                drop_lane.set(None);
                // A locked lane takes no clips from other lanes.
                if locked_tracks().contains(&target_track) {
                    target_track = d.track_id.clone();
                }
                let dpx = d.cur_px - d.start_px;
                // Changing lanes counts as a move even with no horizontal travel.
                let changed_track = d.mode == DragMode::Move && target_track != d.track_id;
                if dpx.abs() < 2.0 && !changed_track {
                    return;
                }
                let raw = dpx / pps;
                let dsec = if snap_on {
                    snap_drag(&d, &timeline(), pos(), pps, raw).0
                } else {
                    raw
                };
                {
                    let mut ed = project.borrow_mut();
                    match d.mode {
                        DragMode::Move => {
                            editor::move_clip(
                                &mut ed,
                                d.clip_id,
                                d.orig_start + dsec,
                                target_track,
                            );
                        }
                        DragMode::TrimStart => {
                            editor::trim(&mut ed, d.clip_id, true, dsec);
                        }
                        DragMode::TrimEnd => {
                            editor::trim(&mut ed, d.clip_id, false, dsec);
                        }
                    }
                }
                timeline.set(editor::view(&project.borrow()));
                if let Some(data) = monitor::timeline_data(&project.borrow()) {
                    monitor.send(Cmd::SetTimeline(data));
                }
            }
        }
    };

    let tl = timeline();

    let drag_now = drag();

    let drop_now = drop_lane();

    let media_drag = dragging_media().is_some();

    let _wave_rev = wave_rev(); // subscribe: re-render when peaks arrive

    let selected_now = selected();

    let has_clip = tl.clip_count > 0;

    let tl_seconds = tl.duration.max(dur()).max(10.0);

    let tl_width = tl_seconds * pps + 40.0;

    let playhead_x = pos() * pps;

    let (major, minor) = ruler_steps(pps);
    let tick_count = (tl_seconds / minor).ceil() as i64;
    let per_major = (major / minor).round() as i64;

    // The snap guide of a drag in progress, in seconds.
    let guide = match (drag_now.as_ref(), snap_on) {
        (Some(d), true) => {
            let raw = (d.cur_px - d.start_px) / pps;
            snap_drag(d, &tl, pos(), pps, raw).1
        }
        _ => None,
    };

    // Per-lane drawing state: lock/mute/hide and the CSS classes that show it.
    let locked_now = locked_tracks();

    let names_now = track_names();

    let renaming_now = renaming();

    let lane_count = tl.track_count.max(1);

    let lane_locked: Vec<bool> = (0..lane_count)
        .map(|l| tl.track_ids.get(l).is_some_and(|t| locked_now.contains(t)))
        .collect();

    // The lane "－ Pista" would delete: the bottom-most empty, unlocked one, and
    // never the last remaining lane.
    let removable: Option<String> = if tl.track_count > 1 {
        (0..tl.track_count)
            .rev()
            .find(|&l| !lane_locked[l] && !tl.clips.iter().any(|c| c.track == l))
            .and_then(|l| tl.track_ids.get(l).cloned())
    } else {
        None
    };

    let lane_cls: Vec<String> = (0..lane_count)
        .map(|l| {
            let mut c = String::from("lane");
            if !lane_locked[l] && ((drop_now == Some(l) && drag_now.is_some()) || media_drag) {
                c.push_str(" drop");
            }
            if lane_locked[l] {
                c.push_str(" locked");
            }
            if tl.track_visible.get(l) == Some(&false) {
                c.push_str(" hidden");
            }
            if tl.track_muted.get(l) == Some(&true) {
                c.push_str(" muted");
            }
            c
        })
        .collect();

    rsx! {
        footer { class: "timeline", style: "height: {panel_height(tl_height(), win_h())}px;",
          div {
              class: "tl-resizer",
              title: "Arrastra para cambiar la altura de la línea de tiempo",
              onmousedown: move |e: MouseEvent| {
                  e.stop_propagation();
                  resizing.set(Some((e.page_coordinates().y, panel_height(tl_height(), win_h()))));
              },
          }
          {toolbar(c)}
          div { class: "tl-main",
            div { class: "tl-heads",
                div { class: "tl-heads-spacer" }
                {(0..lane_count).map(|lane| {
                    let tid = tl.track_ids.get(lane).cloned().unwrap_or_default();
                    let name = names_now
                        .get(&tid)
                        .cloned()
                        .unwrap_or_else(|| format!("Pista {}", lane + 1));
                    let muted = tl.track_muted.get(lane) == Some(&true);
                    let hidden = tl.track_visible.get(lane) == Some(&false);
                    let locked = lane_locked[lane];
                    let editing = !tid.is_empty() && renaming_now.as_deref() == Some(tid.as_str());
                    let mut flag_mute = on_track_flag.clone();
                    let mut flag_hide = on_track_flag.clone();
                    let (tid_mute, tid_hide, tid_lock, tid_edit, tid_name) =
                        (tid.clone(), tid.clone(), tid.clone(), tid.clone(), tid.clone());
                    rsx! {
                        div { key: "head-{lane}", class: "tl-head",
                            if editing {
                                input {
                                    class: "tl-head-input",
                                    value: "{name}",
                                    oninput: move |e: FormEvent| {
                                        track_names.write().insert(tid_name.clone(), e.value());
                                    },
                                    onkeydown: move |e: KeyboardEvent| {
                                        let k = e.key().to_string();
                                        if k == "Enter" || k == "Escape" {
                                            // An empty name goes back to the default one.
                                            track_names.write().retain(|_, v| !v.trim().is_empty());
                                            renaming.set(None);
                                        }
                                    },
                                    onblur: move |_| {
                                        track_names.write().retain(|_, v| !v.trim().is_empty());
                                        renaming.set(None);
                                    },
                                }
                            } else {
                                span {
                                    class: "tl-head-name",
                                    title: "Doble clic para renombrar",
                                    ondoubleclick: move |_| {
                                        renaming.set(Some(tid_edit.clone()));
                                        let _ = dioxus::document::eval(
                                            "setTimeout(function(){var i=document.querySelector('.tl-head-input');if(i){i.focus();i.select();}},0)",
                                        );
                                    },
                                    "{name}"
                                }
                            }
                            div { class: "tl-head-btns",
                                button {
                                    class: if muted { "tl-hbtn on" } else { "tl-hbtn" },
                                    title: if muted { "Quitar silencio de la pista" } else { "Silenciar pista" },
                                    onclick: move |_| flag_mute(tid_mute.clone(), concat_project::commands::TrackFlag::Muted, !muted),
                                    if muted { "🔇" } else { "🔊" }
                                }
                                button {
                                    class: if hidden { "tl-hbtn on" } else { "tl-hbtn" },
                                    title: if hidden { "Mostrar pista" } else { "Ocultar pista" },
                                    onclick: move |_| flag_hide(tid_hide.clone(), concat_project::commands::TrackFlag::Visible, hidden),
                                    if hidden { "🚫" } else { "👁" }
                                }
                                button {
                                    class: if locked { "tl-hbtn on" } else { "tl-hbtn" },
                                    title: if locked { "Desbloquear pista" } else { "Bloquear pista" },
                                    onclick: move |_| {
                                        let mut set = locked_tracks.write();
                                        if !set.remove(&tid_lock) {
                                            set.insert(tid_lock.clone());
                                        }
                                    },
                                    if locked { "🔒" } else { "🔓" }
                                }
                            }
                        }
                    }
                })}
                // Stands in for the horizontal scrollbar of the lanes, so both
                // columns reach the same scroll range.
                div { class: "tl-heads-end" }
            }
            div {
                class: "tl-scroll",
                onmousemove: on_drag_move,
                onmouseup: end_drag(),
                onmouseleave: end_drag(),
                div {
                    class: if scrubbing() { "lanes scrubbing" } else { "lanes" },
                    style: "width: {tl_width}px;",
                    div {
                        class: "tl-ruler",
                        style: "width: {tl_width}px;",
                        onmousedown: {
                            let monitor = monitor.clone();
                            move |e: MouseEvent| {
                                e.stop_propagation();
                                scrubbing.set(true);
                                monitor.send(Cmd::Seek((e.element_coordinates().x / pps).max(0.0)));
                            }
                        },
                        onmousemove: {
                            let monitor = monitor.clone();
                            move |e: MouseEvent| {
                                if scrubbing() {
                                    monitor.send(Cmd::Seek((e.element_coordinates().x / pps).max(0.0)));
                                }
                            }
                        },
                        // The playhead's handle lives in the ruler, which stays put when
                        // the lanes scroll vertically, so it can always be grabbed.
                        if has_clip {
                            div { class: "tl-ruler-ph", style: "left: {playhead_x}px;",
                                div {
                                    class: "tl-playhead-grab",
                                    onmousedown: move |e: MouseEvent| {
                                        e.stop_propagation();
                                        scrubbing.set(true);
                                    },
                                }
                            }
                        }
                        for i in 0..=tick_count {
                            div {
                                key: "tick-{i}",
                                class: if i % per_major == 0 { "tick major" } else { "tick" },
                                style: "left: {i as f64 * minor * pps}px;",
                                if i % per_major == 0 {
                                    span { class: "tick-label", "{ruler_label(i as f64 * minor, major)}" }
                                }
                            }
                        }
                    }
                    for lane in 0..tl.track_count.max(1) {
                        div {
                            class: "{lane_cls[lane]}",
                            onmousedown: {
                                let monitor = monitor.clone();
                                move |e: MouseEvent| {
                                    scrubbing.set(true);
                                    monitor.send(Cmd::Seek((e.element_coordinates().x / pps).max(0.0)));
                                }
                            },
                            onmousemove: {
                                let monitor = monitor.clone();
                                move |e: MouseEvent| {
                                    if scrubbing() {
                                        monitor.send(Cmd::Seek((e.element_coordinates().x / pps).max(0.0)));
                                    }
                                }
                            },
                            onmouseenter: move |_| {
                                if drag().is_some() {
                                    drop_lane.set(Some(lane));
                                }
                            },
                            onmouseup: {
                                let mut on_drop_media = on_drop_media.clone();
                                let track_ids = tl.track_ids.clone();
                                move |e: MouseEvent| {
                                    if let Some(tid) = track_ids.get(lane).cloned() {
                                        on_drop_media(tid, (e.element_coordinates().x / pps).max(0.0));
                                    }
                                }
                            },
                            {tl.clips.iter().filter(move |c| c.track == lane).map(|c| {
                                        let mut left = c.start * pps;
                                        let mut width = (c.duration * pps).max(3.0);
                                        if let Some(d) = drag_now.as_ref() {
                                            if d.clip_id == c.id {
                                                let raw = (d.cur_px - d.start_px) / pps;
                                                let dpx = if snap_on {
                                                    snap_drag(d, &tl, pos(), pps, raw).0 * pps
                                                } else {
                                                    d.cur_px - d.start_px
                                                };
                                                match d.mode {
                                                    DragMode::Move => left += dpx,
                                                    DragMode::TrimStart => { left += dpx; width -= dpx; }
                                                    DragMode::TrimEnd => width += dpx,
                                                }
                                            }
                                        }
                                        let left = left.max(0.0);
                                        let width = width.max(3.0);
                                        let sel_cls = if selected_now.as_deref() == Some(c.id.as_str()) {
                                            " sel"
                                        } else {
                                            ""
                                        };
                                        let clip_locked = locked_now.contains(&c.track_id);
                                        let mk = |mode: DragMode| {
                                            let cid = c.id.clone();
                                            let tid = c.track_id.clone();
                                            let (os, od) = (c.start, c.duration);
                                            move |e: MouseEvent| {
                                                e.stop_propagation();
                                                selected.set(Some(cid.clone()));
                                                // A locked lane's clips can be selected, not edited.
                                                if clip_locked {
                                                    return;
                                                }
                                                let x = e.page_coordinates().x;
                                                drag.set(Some(Drag {
                                                    mode,
                                                    clip_id: cid.clone(),
                                                    track_id: tid.clone(),
                                                    orig_start: os,
                                                    orig_dur: od,
                                                    start_px: x,
                                                    cur_px: x,
                                                }));
                                            }
                                        };
                                        let wave = if c.kind == "audio" {
                                            c.media_path
                                                .as_ref()
                                                .and_then(|p| peaks::get(p))
                                                .map(|pk| peaks::waveform_svg(&pk, c.source_start, c.duration, width))
                                        } else {
                                            None
                                        };
                                        rsx! {
                                            div {
                                                key: "{c.id}",
                                                class: "clip k-{c.kind}{sel_cls}",
                                                style: "left: {left}px; width: {width}px;",
                                                title: "{c.name}",
                                                onmousedown: mk(DragMode::Move),
                                                onmouseup: {
                                                    let cid = c.id.clone();
                                                    let ctx = c_ctx.clone();
                                                    let takes_fx = (c.kind == "video" || c.kind == "image") && !clip_locked;
                                                    move |e: MouseEvent| {
                                                        if let (true, Some(fx)) = (takes_fx, dragging_fx()) {
                                                            e.stop_propagation();
                                                            dragging_fx.set(None);
                                                            super::resources::apply_effect(&ctx, &cid, &fx);
                                                        }
                                                    }
                                                },
                                                div { class: "handle hleft", onmousedown: mk(DragMode::TrimStart) }
                                                if let Some(w) = wave.as_ref() {
                                                    div { class: "wave-wrap", dangerous_inner_html: "{w}" }
                                                }
                                                if c.has_transition {
                                                    div { class: "clip-tr", title: "Transición en el corte", "⇄" }
                                                }
                                                span { class: "clip-name", "{c.name}" }
                                                div { class: "handle hright", onmousedown: mk(DragMode::TrimEnd) }
                                            }
                                        }
                                    })}
                                }
                            }
                    if has_clip {
                        div { class: "tl-playhead", style: "left: {playhead_x}px;" }
                    }
                    if let Some(g) = guide {
                        div { class: "tl-snap", style: "left: {g * pps}px;" }
                    }
                }
            }
          }
            div { class: "tl-status",
                span { class: "tl-status-text",
                    "Arrastra el cabezal (o la regla) para moverte · el cuerpo del clip para moverlo · los bordes para recortar · {tl.clip_count} clip(s) · {tl.track_count} pista(s)"
                }
                button {
                    class: "tl-sbtn",
                    title: "Añadir una pista vacía al final de la lista",
                    onclick: on_add_track,
                    "＋ Pista"
                }
                button {
                    class: "tl-sbtn",
                    disabled: removable.is_none(),
                    title: "Quitar la última pista vacía (no se quita si tiene clips, está bloqueada o es la única)",
                    onclick: {
                        let mut remove = on_remove_track.clone();
                        let target = removable.clone();
                        move |_| {
                            if let Some(id) = target.clone() {
                                remove(id);
                            }
                        }
                    },
                    "－ Pista vacía"
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drag(mode: DragMode, start: f64, dur: f64) -> Drag {
        Drag {
            mode,
            clip_id: "a".into(),
            track_id: "t".into(),
            orig_start: start,
            orig_dur: dur,
            start_px: 0.0,
            cur_px: 0.0,
        }
    }

    #[test]
    fn ruler_steps_keep_labels_apart_at_any_zoom() {
        for pps in [1.0, 2.0, 7.0, 50.0, 200.0, 800.0] {
            let (major, minor) = ruler_steps(pps);
            assert!(major * pps >= 70.0, "{pps}: {major}");
            assert!(minor <= major);
        }
        assert_eq!(ruler_steps(50.0).0, 2.0);
    }

    #[test]
    fn zoom_slider_round_trips() {
        for pps in [1.0, 25.0, 50.0, 800.0] {
            assert!((slider_to_pps(pps_to_slider(pps)) - pps).abs() < 1e-6);
        }
    }

    #[test]
    fn snapping_pulls_edges_within_the_pixel_threshold_at_any_zoom() {
        let tl = TimelineView::default();
        // Alone: only 0 and the playhead are targets. A move that leaves the
        // clip's start 0.1 s from 0 snaps at 50 px/s (5 px) but not at 200.
        let d = drag(DragMode::Move, 1.0, 2.0);
        let (s, g) = snap_drag(&d, &tl, 10.0, 50.0, -0.9);
        assert!((s + 1.0).abs() < 1e-9 && g == Some(0.0), "{s} {g:?}");
        let (s, g) = snap_drag(&d, &tl, 10.0, 200.0, -0.9);
        assert!((s + 0.9).abs() < 1e-9 && g.is_none());
        // The end edge snaps to the playhead.
        let (s, g) = snap_drag(&d, &tl, 5.0, 50.0, 1.95);
        assert!((s - 2.0).abs() < 1e-9 && g == Some(5.0), "{s} {g:?}");
        // A trim only snaps the edge it moves.
        let t = drag(DragMode::TrimEnd, 1.0, 2.0);
        let (s, _) = snap_drag(&t, &tl, 3.05, 50.0, 0.0);
        assert!((s - 0.05).abs() < 1e-9);
    }
}
