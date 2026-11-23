// SPDX-License-Identifier: AGPL-3.0-or-later
//
//! The timeline toolbar: the edit tools that act at the playhead.
//!
//! Minimal for now (split, delete, undo, redo, title); snapping, zoom and the
//! rest of the tools join it in a later phase of the layout rework (#34).

use dioxus::prelude::*;

use super::Ctx;
use crate::editor;
use crate::monitor::{self, Cmd};
use crate::text;

pub fn toolbar(c: &Ctx) -> Element {
    let project = c.project.clone();
    let monitor = c.monitor.clone();
    let pos = c.pos;
    let locked_tracks = c.locked_tracks;
    let mut timeline = c.timeline;
    let mut selected = c.selected;

    let on_undo = {
        let project = project.clone();
        let monitor = monitor.clone();
        move |_| {
            project.borrow_mut().undo();
            timeline.set(editor::view(&project.borrow()));
            if let Some(d) = monitor::timeline_data(&project.borrow()) {
                monitor.send(Cmd::SetTimeline(d));
            }
        }
    };

    let on_redo = {
        let project = project.clone();
        let monitor = monitor.clone();
        move |_| {
            project.borrow_mut().redo();
            timeline.set(editor::view(&project.borrow()));
            if let Some(d) = monitor::timeline_data(&project.borrow()) {
                monitor.send(Cmd::SetTimeline(d));
            }
        }
    };

    let on_split = {
        let project = project.clone();
        let monitor = monitor.clone();
        move |_| {
            editor::split_at(&mut project.borrow_mut(), pos(), &locked_tracks());
            timeline.set(editor::view(&project.borrow()));
            if let Some(d) = monitor::timeline_data(&project.borrow()) {
                monitor.send(Cmd::SetTimeline(d));
            }
        }
    };

    let on_delete = {
        let project = project.clone();
        let monitor = monitor.clone();
        move |_| {
            editor::delete_at(&mut project.borrow_mut(), pos(), &locked_tracks());
            timeline.set(editor::view(&project.borrow()));
            if let Some(d) = monitor::timeline_data(&project.borrow()) {
                monitor.send(Cmd::SetTimeline(d));
            }
        }
    };

    let on_add_text = {
        let project = project.clone();
        let monitor = monitor.clone();
        move |_| {
            if let Some(id) = text::add_text(&mut project.borrow_mut(), pos()) {
                selected.set(Some(id));
            }
            timeline.set(editor::view(&project.borrow()));
            if let Some(d) = monitor::timeline_data(&project.borrow()) {
                monitor.send(Cmd::SetTimeline(d));
            }
        }
    };

    let tl = timeline();
    let has_clip = tl.clip_count > 0;

    rsx! {
        div { class: "tl-toolbar",
            button { class: "tool", disabled: !tl.can_undo, onclick: on_undo, title: "Deshacer", "↶" }
            button { class: "tool", disabled: !tl.can_redo, onclick: on_redo, title: "Rehacer", "↷" }
            span { class: "tb-sep" }
            button { class: "tool", disabled: !has_clip, onclick: on_split, title: "Dividir en el cursor", "✂ Dividir" }
            button { class: "tool", disabled: !has_clip, onclick: on_delete, title: "Eliminar en el cursor (ripple)", "🗑 Eliminar" }
            span { class: "tb-sep" }
            button { class: "tool", onclick: on_add_text, title: "Añadir título en el cursor", "T Texto" }
        }
    }
}
