// SPDX-License-Identifier: AGPL-3.0-or-later
//
//! The player: a card with the composed preview, the time, the play button and
//! the seek bar.

use dioxus::prelude::*;

use super::Ctx;
use crate::monitor::{Cmd, fmt_time};

pub fn stage_panel(c: &Ctx) -> Element {
    let monitor = c.monitor.clone();
    let frame = c.frame;
    let loading = c.loading;
    let pos = c.pos;
    let dur = c.dur;
    let playing = c.playing;
    let timeline = c.timeline;

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

    let tl = timeline();
    let has_clip = tl.clip_count > 0;
    let tl_seconds = tl.duration.max(dur()).max(10.0);

    rsx! {
        main { class: "stage",
            div { class: "card-head", "Reproductor" }
            div { class: "stage-view",
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
                span { class: "stage-ctrl-end" }
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
