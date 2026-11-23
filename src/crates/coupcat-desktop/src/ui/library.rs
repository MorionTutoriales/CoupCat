// SPDX-License-Identifier: AGPL-3.0-or-later
//
//! The media library (bin) on the left, with the import button in its header.

use dioxus::prelude::*;

use super::Ctx;
use super::util::{dur_label, kind_icon, library_thumb};
use crate::editor;

pub fn library_panel(c: &Ctx) -> Element {
    let project = c.project.clone();
    let mut media = c.media;
    let mut dragging_media = c.dragging_media;
    let thumb_rev = c.thumb_rev;
    let mut error = c.error;
    let mut timeline = c.timeline;

    let _thumb_rev = thumb_rev(); // subscribe: re-render when thumbnails arrive

    let on_import = {
        let project = project.clone();
        move |_| {
            // Async dialog so the UI thread (webview event loop) never blocks.
            // Multiple files go to the library (bin); nothing is placed on the
            // timeline until the user adds it from there.
            let project = project.clone();
            spawn(async move {
                let Some(files) = rfd::AsyncFileDialog::new()
                    .add_filter(
                        "Media",
                        &[
                            "mp4", "mov", "mkv", "webm", "avi", "m4v", "gif", "mp3", "wav", "m4a",
                            "aac", "flac", "ogg", "png", "jpg", "jpeg", "webp",
                        ],
                    )
                    .set_title("Importar a la librería")
                    .pick_files()
                    .await
                else {
                    return;
                };
                let mut last_err = None;
                for file in &files {
                    let path = file.path().to_path_buf();
                    match concat_media::probe(&path) {
                        Ok(info) => {
                            editor::import_media(&mut project.borrow_mut(), &info, &path);
                        }
                        Err(e) => last_err = Some(format!("{}: {e}", path.display())),
                    }
                }
                media.set(editor::media_list(&project.borrow()));
                timeline.set(editor::view(&project.borrow()));
                error.set(last_err);
            });
        }
    };

    rsx! {
        aside { class: "library",
            div { class: "lib-head",
                span { "Librería" }
                button { class: "import", onclick: on_import, title: "Importar vídeo, audio o imágenes", "＋ Importar" }
            }
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
