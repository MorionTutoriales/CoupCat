// SPDX-License-Identifier: AGPL-3.0-or-later
//
//! The player: the composed preview and its seek bar.

use dioxus::prelude::*;

use super::Ctx;
use crate::monitor::Cmd;

pub fn stage_panel(c: &Ctx) -> Element {
    let frame = c.frame;
    let loading = c.loading;

    rsx! {
        main { class: "stage",
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
    }
}

pub fn scrub_row(c: &Ctx) -> Element {
    let monitor = c.monitor.clone();
    let pos = c.pos;
    let dur = c.dur;
    let timeline = c.timeline;

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

    rsx! {
        div { class: "scrub-row",
            input {
                class: "scrubber", r#type: "range", min: "0",
                max: "{dur():.3}", step: "0.01", value: "{pos():.3}",
                disabled: !has_clip, oninput: on_seek,
            }
        }
    }
}
