// SPDX-License-Identifier: AGPL-3.0-or-later
//
//! The timeline: ruler, track headers, lanes, clips and the status bar.

use dioxus::prelude::*;

use super::Ctx;
use crate::editor;
use crate::monitor::{self, Cmd, fmt_time};
use crate::peaks;

/// Pixels per second on the timeline.
pub const PPS: f64 = 50.0;

/// Timeline panel height: tall enough for the four starter lanes, the ruler and
/// the status bar; the user can drag it between these bounds.
pub const TL_HEIGHT_DEFAULT: f64 = 270.0;
pub const TL_HEIGHT_MIN: f64 = 130.0;
pub const TL_HEIGHT_MAX: f64 = 520.0;

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
                let dsec = dpx / PPS;
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

    let tl_width = tl_seconds * PPS + 40.0;

    let playhead_x = pos() * PPS;

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
        footer { class: "timeline", style: "height: {tl_height()}px;",
          div {
              class: "tl-resizer",
              title: "Arrastra para cambiar la altura de la línea de tiempo",
              onmousedown: move |e: MouseEvent| {
                  e.stop_propagation();
                  resizing.set(Some((e.page_coordinates().y, tl_height())));
              },
          }
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
                                monitor.send(Cmd::Seek((e.element_coordinates().x / PPS).max(0.0)));
                            }
                        },
                        onmousemove: {
                            let monitor = monitor.clone();
                            move |e: MouseEvent| {
                                if scrubbing() {
                                    monitor.send(Cmd::Seek((e.element_coordinates().x / PPS).max(0.0)));
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
                        for i in 0..=(tl_seconds.ceil() as i64) {
                            div {
                                key: "tick-{i}",
                                class: if i % 5 == 0 { "tick major" } else { "tick" },
                                style: "left: {i as f64 * PPS}px;",
                                if i % 5 == 0 {
                                    span { class: "tick-label", "{fmt_time(i as f64)}" }
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
                                    monitor.send(Cmd::Seek((e.element_coordinates().x / PPS).max(0.0)));
                                }
                            },
                            onmousemove: {
                                let monitor = monitor.clone();
                                move |e: MouseEvent| {
                                    if scrubbing() {
                                        monitor.send(Cmd::Seek((e.element_coordinates().x / PPS).max(0.0)));
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
                                        on_drop_media(tid, (e.element_coordinates().x / PPS).max(0.0));
                                    }
                                }
                            },
                            {tl.clips.iter().filter(move |c| c.track == lane).map(|c| {
                                        let mut left = c.start * PPS;
                                        let mut width = (c.duration * PPS).max(3.0);
                                        if let Some(d) = drag_now.as_ref() {
                                            if d.clip_id == c.id {
                                                let dpx = d.cur_px - d.start_px;
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
