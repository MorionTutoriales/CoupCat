// SPDX-License-Identifier: AGPL-3.0-or-later
//
//! The player: a card with the composed preview, the time, the play button and
//! the seek bar.

use dioxus::prelude::*;

use super::Ctx;
use crate::editor;
use crate::monitor::{self, Cmd, fmt_time};

pub fn stage_panel(c: &Ctx) -> Element {
    let monitor = c.monitor.clone();
    let frame = c.frame;
    let loading = c.loading;
    let pos = c.pos;
    let dur = c.dur;
    let playing = c.playing;
    let mut timeline = c.timeline;
    let mut looping = c.looping;
    let mut stage_full = c.stage_full;
    let project = c.project.clone();
    let mut dragging_fx = c.dragging_fx;
    let ctx = c.clone();

    let on_playpause = {
        let monitor = monitor.clone();
        move |_| {
            if playing() {
                monitor.send(Cmd::Pause);
            } else {
                monitor.send(Cmd::Play);
            }
        }
    };

    let on_seek = {
        let monitor = monitor.clone();
        move |e: FormEvent| {
            if let Ok(v) = e.value().parse::<f64>() {
                monitor.send(Cmd::Seek(v));
            }
        }
    };

    let on_ratio = {
        let project = project.clone();
        let monitor = monitor.clone();
        move |e: FormEvent| {
            editor::set_ratio(&mut project.borrow_mut(), &e.value());
            timeline.set(editor::view(&project.borrow()));
            if let Some(d) = monitor::timeline_data(&project.borrow()) {
                monitor.send(Cmd::SetTimeline(d));
            }
        }
    };

    let on_loop = {
        let monitor = monitor.clone();
        move |_| {
            looping.set(!looping());
            monitor.send(Cmd::SetLoop(looping()));
        }
    };

    // Full screen: the window takes the whole display and the player fills it.
    let on_full = move |_| {
        let on = !stage_full();
        stage_full.set(on);
        dioxus::desktop::window().set_fullscreen(on);
    };

    let (cw, ch) = editor::video_size(&project.borrow());
    let ratio = editor::ratio_id(cw, ch);

    let tl = timeline();
    let has_clip = tl.clip_count > 0;
    let tl_seconds = tl.duration.max(dur()).max(10.0);

    rsx! {
        main { class: if stage_full() { "stage full" } else { "stage" },
            div { class: "card-head stage-head",
                span { "Reproductor" }
                select {
                    id: "ratio-select",
                    class: "fx-select ratio-select",
                    title: "Proporción del lienzo del proyecto",
                    value: "{ratio}",
                    onchange: on_ratio,
                    for (id, label, _, _) in editor::RATIOS.iter() {
                        option { key: "{id}", value: "{id}", "{label}" }
                    }
                    if ratio == "custom" {
                        option { value: "custom", "{cw}×{ch}" }
                    }
                }
            }
            div {
                class: "stage-view",
                onmouseup: move |_| {
                    let Some(fx) = dragging_fx() else { return };
                    dragging_fx.set(None);
                    // The picture on top at the playhead takes the effect.
                    let t = pos();
                    let target = timeline()
                        .clips
                        .iter()
                        .filter(|cl| (cl.kind == "video" || cl.kind == "image") && cl.start <= t && t < cl.start + cl.duration)
                        .min_by_key(|cl| cl.track)
                        .map(|cl| cl.id.clone());
                    if let Some(id) = target {
                        super::resources::apply_effect(&ctx, &id, &fx);
                    }
                },
                match frame() {
                    Some(src) => rsx! { img { class: "frame", src: "{src}" } },
                    None => rsx! {
                        div { class: "empty",
                            if loading() {
                                p { class: "empty-title", "Cargando…" }
                            } else {
                                p { class: "empty-title", "Sin clip" }
                                p { class: "empty-sub", "Importa un vídeo para empezar." }
                            }
                        }
                    },
                }
            }
            div { class: "stage-ctrl",
                span { class: "time",
                    span { class: "time-now", "{fmt_time(pos())}" }
                    span { class: "time-total", " / {fmt_time(tl_seconds)}" }
                }
                button { class: "ctrl", disabled: !has_clip, onclick: on_playpause,
                    title: "Reproducir / pausar (barra espaciadora)",
                    if playing() { "⏸" } else { "▶" }
                }
                span { class: "stage-ctrl-end",
                    button {
                        id: "btn-loop",
                        class: if looping() { "ctrl sm on" } else { "ctrl sm" },
                        title: "Repetir en bucle",
                        onclick: on_loop,
                        "🔁"
                    }
                    button {
                        id: "btn-fullscreen",
                        class: "ctrl sm",
                        title: if stage_full() { "Salir de pantalla completa (Esc)" } else { "Pantalla completa" },
                        onclick: on_full,
                        if stage_full() { "🗗" } else { "⛶" }
                    }
                }
            }
            div { class: "scrub-row",
                input {
                    class: "scrubber", r#type: "range", min: "0",
                    max: "{dur():.3}", step: "0.01", value: "{pos():.3}",
                    disabled: !has_clip, oninput: on_seek,
                }
            }
        }
    }
}
