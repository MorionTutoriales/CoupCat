// SPDX-License-Identifier: AGPL-3.0-or-later
//
//! The media library (bin) on the left.

use dioxus::prelude::*;

use super::Ctx;
use super::util::{dur_label, kind_icon, library_thumb};

pub fn library_panel(c: &Ctx) -> Element {
    let media = c.media;
    let mut dragging_media = c.dragging_media;
    let thumb_rev = c.thumb_rev;

    let _thumb_rev = thumb_rev(); // subscribe: re-render when thumbnails arrive

    rsx! {
        aside { class: "library",
            div { class: "lib-head", "Librería" }
            div { class: "lib-list",
                if media().is_empty() {
                    div { class: "lib-empty", "Importa archivos (vídeo, audio, imagen) para empezar." }
                } else {
                    for m in media().iter() {
                        div {
                            key: "{m.id}",
                            class: if dragging_media() == Some(m.id.clone()) { "lib-item dragging" } else { "lib-item" },
                            title: "Arrastra al timeline",
                            onmousedown: {
                                let id = m.id.clone();
                                move |_| dragging_media.set(Some(id.clone()))
                            },
                            match library_thumb(m) {
                                Some(src) => rsx! {
                                    div { class: "lib-kind lib-thumb k-{m.kind}",
                                        img { src: "{src}", draggable: "false" }
                                    }
                                },
                                None => rsx! {
                                    div { class: "lib-kind k-{m.kind}", "{kind_icon(m.kind)}" }
                                },
                            }
                            div { class: "lib-meta",
                                span { class: "lib-name", "{m.name}" }
                                span { class: "lib-dur", "{dur_label(m.duration)}" }
                            }
                        }
                    }
                }
            }
        }
    }
}
