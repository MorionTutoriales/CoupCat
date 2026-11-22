// SPDX-License-Identifier: AGPL-3.0-or-later
//
//! The top bar: project file actions, transport, edit tools, export and import.

use dioxus::prelude::*;

use super::Ctx;
use super::files::Pending;
use crate::editor;
use crate::monitor::{self, Cmd, fmt_time};
use crate::{export, project_file, text};

pub fn topbar(c: &Ctx) -> Element {
    let project = c.project.clone();
    let monitor = c.monitor.clone();
    let files = c.files.clone();
    let pos = c.pos;
    let dur = c.dur;
    let playing = c.playing;
    let locked_tracks = c.locked_tracks;
    let mut error = c.error;
    let mut timeline = c.timeline;
    let mut media = c.media;
    let mut selected = c.selected;
    let mut export_handle = c.export_handle;
    let mut exporting = c.exporting;
    let mut export_stage = c.export_stage;
    let mut export_pct = c.export_pct;
    let mut export_msg = c.export_msg;
    let project_path = c.project_path;
    let mut menu_open = c.menu_open;
    let dirty = c.dirty;

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

    let on_export = {
        let project = project.clone();
        move |_| {
            if exporting() {
                return;
            }
            let project = project.clone();
            spawn(async move {
                let Some(file) = rfd::AsyncFileDialog::new()
                    .add_filter("Vídeo MP4", &["mp4"])
                    .set_title("Exportar vídeo")
                    .set_file_name("coupcat-export.mp4")
                    .save_file()
                    .await
                else {
                    return;
                };
                let output = file.path().to_string_lossy().into_owned();
                match export::build_request(&project.borrow(), output) {
                    Ok(request) => {
                        export_msg.set(None);
                        exporting.set(true);
                        export_stage.set("preparando".to_string());
                        export_pct.set(0.0);
                        export_handle.set(Some(export::start(request)));
                    }
                    Err(e) => error.set(Some(e)),
                }
            });
        }
    };

    let tl = timeline();

    let has_clip = tl.clip_count > 0;

    let tl_seconds = tl.duration.max(dur()).max(10.0);

    // Project file state for the top bar.
    let dirty_now = dirty();

    let proj_name = project_path()
        .map(|p| project_file::name_of(&p))
        .unwrap_or_else(|| "Sin título".to_string());

    let proj_title = project_path()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "Proyecto sin guardar".to_string());

    let recent_list = if menu_open() {
        project_file::recents()
    } else {
        Vec::new()
    };

    rsx! {
        header { class: "topbar",
            div { class: "brand",
                span { class: "logo", "🐾" }
                span { class: "name", "CoupCat" }
                span { class: "milestone", "M2" }
            }
            div { class: "file-tools",
                button {
                    id: "btn-new", class: "tool", title: "Proyecto nuevo (Ctrl+N)",
                    onclick: {
                        let f = files.clone();
                        move |_| f.request(Pending::New, dirty())
                    },
                    "📄"
                }
                button {
                    id: "btn-open", class: "tool", title: "Abrir proyecto (Ctrl+O)",
                    onclick: {
                        let f = files.clone();
                        move |_| f.request(Pending::Open, dirty())
                    },
                    "📂"
                }
                button {
                    id: "btn-save", class: "tool", title: "Guardar (Ctrl+S)",
                    onclick: {
                        let f = files.clone();
                        move |_| f.spawn_save(false)
                    },
                    "💾 Guardar"
                }
                button {
                    class: "tool", title: "Guardar como y proyectos recientes",
                    onclick: move |_| menu_open.set(!menu_open()),
                    "▾"
                }
                if menu_open() {
                    div { class: "menu-backdrop", onclick: move |_| menu_open.set(false) }
                    div { class: "file-menu",
                        button {
                            class: "menu-item",
                            onclick: {
                                let f = files.clone();
                                move |_| {
                                    menu_open.set(false);
                                    f.spawn_save(true);
                                }
                            },
                            "Guardar como…"
                        }
                        div { class: "menu-sep", "Proyectos recientes" }
                        if recent_list.is_empty() {
                            div { class: "menu-empty", "Aún no hay proyectos recientes." }
                        }
                        for p in recent_list.iter() {
                            button {
                                key: "{p.display()}",
                                class: "menu-item",
                                title: "{p.display()}",
                                onclick: {
                                    let f = files.clone();
                                    let p = p.clone();
                                    move |_| {
                                        menu_open.set(false);
                                        f.request(Pending::Recent(p.clone()), dirty());
                                    }
                                },
                                "{project_file::name_of(p)}"
                            }
                        }
                    }
                }
                span { class: "proj-name", title: "{proj_title}",
                    "{proj_name}"
                    if dirty_now { span { class: "proj-dirty", title: "Hay cambios sin guardar", " ●" } }
                }
            }
            div { class: "transport",
                button { class: "ctrl", disabled: !has_clip, onclick: on_playpause,
                    if playing() { "⏸" } else { "▶" }
                }
                span { class: "time", "{fmt_time(pos())} / {fmt_time(tl_seconds)}" }
            }
            div { class: "edit-tools",
                button { class: "tool", disabled: !has_clip, onclick: on_split, title: "Dividir en el cursor", "✂ Dividir" }
                button { class: "tool", disabled: !has_clip, onclick: on_delete, title: "Eliminar en el cursor (ripple)", "🗑 Eliminar" }
                button { class: "tool", disabled: !tl.can_undo, onclick: on_undo, title: "Deshacer", "↶" }
                button { class: "tool", disabled: !tl.can_redo, onclick: on_redo, title: "Rehacer", "↷" }
                button { class: "tool", onclick: on_add_text, title: "Añadir título en el cursor", "T Texto" }
                button { class: "tool export", disabled: !has_clip || exporting(), onclick: on_export, title: "Exportar a MP4", "⬇ Exportar" }
            }
            button { class: "import", onclick: on_import, "＋ Importar" }
        }
    }
}
