// SPDX-License-Identifier: AGPL-3.0-or-later
//
//! The timeline toolbar: the edit tools that act at the playhead.
//!
//! Undo, redo, split, delete, title, the snapping toggle and, at the right,
//! the zoom of the timeline (#14, #18).

use dioxus::prelude::*;

use super::Ctx;
use super::timeline::{PPS_DEFAULT, clamp_pps, pps_to_slider, slider_to_pps, zoom_script};
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

    let mut snap = c.snap;
    let mut pps_sig = c.pps;
    let pps = pps_sig();

    // Zoom around the playhead, keeping it where it is on screen.
    let zoom_to = move |new: f64| {
        let mut pps_sig = pps_sig;
        let old = pps_sig();
        let new = clamp_pps(new);
        if (new - old).abs() < 1e-9 {
            return;
        }
        pps_sig.set(new);
        let _ = dioxus::document::eval(&zoom_script(pos(), old, new, None));
    };

    // Fit the whole project in the visible width.
    let on_fit = {
        let duration = c.timeline.peek().duration.max(c.dur.peek().max(0.0));
        move |_| {
            let mut ev = dioxus::document::eval(
                "dioxus.send((document.querySelector('.tl-scroll')||{clientWidth:800}).clientWidth)",
            );
            spawn(async move {
                if let Ok(w) = ev.recv::<f64>().await {
                    let new = clamp_pps((w - 60.0).max(100.0) / duration.max(1.0));
                    pps_sig.set(new);
                    let _ = dioxus::document::eval(
                        "(function(){var s=document.querySelector('.tl-scroll');if(s)setTimeout(function(){s.scrollLeft=0},40)})()",
                    );
                }
            });
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
            span { class: "tb-sep" }
            button {
                id: "btn-snap",
                class: if snap() { "tool on" } else { "tool" },
                title: if snap() { "Imán activado: los clips se pegan a los bordes y al cabezal" } else { "Imán desactivado" },
                onclick: move |_| snap.set(!snap()),
                "🧲 Imán"
            }
            span { class: "tb-grow" }
            div { class: "tl-zoom",
                button { id: "zoom-out", class: "tool", title: "Alejar", onclick: move |_| zoom_to(pps / 1.25), "−" }
                input {
                    id: "zoom-range",
                    class: "tl-zoom-range",
                    r#type: "range", min: "0", max: "100", step: "0.5",
                    title: "Zoom de la línea de tiempo (Ctrl + rueda)",
                    value: "{pps_to_slider(pps)}",
                    oninput: move |e: FormEvent| {
                        if let Ok(v) = e.value().parse::<f64>() { zoom_to(slider_to_pps(v)); }
                    },
                }
                button { id: "zoom-in", class: "tool", title: "Acercar", onclick: move |_| zoom_to(pps * 1.25), "＋" }
                button { id: "zoom-fit", class: "tool", disabled: !has_clip, title: "Ajustar todo el proyecto a la pantalla", onclick: on_fit, "⤢ Ajustar" }
                button {
                    id: "zoom-reset",
                    class: "tool",
                    disabled: (pps - PPS_DEFAULT).abs() < 1e-6,
                    title: "Volver al zoom inicial",
                    onclick: move |_| zoom_to(PPS_DEFAULT),
                    "1:1"
                }
            }
        }
    }
}
