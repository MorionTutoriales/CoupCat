// SPDX-License-Identifier: AGPL-3.0-or-later
//
//! The top bar: the project menu and its save indicator on the left, the
//! project name in the middle, the shortcuts and export buttons on the right.

use dioxus::prelude::*;

use super::Ctx;
use super::files::Pending;
use crate::{export, project_file};

pub fn topbar(c: &Ctx) -> Element {
    let project = c.project.clone();
    let files = c.files.clone();
    let mut error = c.error;
    let timeline = c.timeline;
    let mut export_handle = c.export_handle;
    let mut exporting = c.exporting;
    let mut export_stage = c.export_stage;
    let mut export_pct = c.export_pct;
    let mut export_msg = c.export_msg;
    let project_path = c.project_path;
    let mut menu_open = c.menu_open;
    let mut shortcuts_open = c.shortcuts_open;
    let saved_at = c.saved_at;
    let dirty = c.dirty;

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

    let has_clip = timeline().clip_count > 0;

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

    // The menu's items are always in the page (shown or hidden by a class), so
    // the Ctrl+N / Ctrl+O / Ctrl+S shortcuts can reach them while it is closed.
    rsx! {
        header { class: "topbar",
            div { class: "top-left",
                div { class: "brand",
                    span { class: "logo", "🐾" }
                    span { class: "name", "CoupCat" }
                }
                div { class: "menu-wrap",
                    button {
                        class: "tool menu-btn",
                        title: "Proyecto: nuevo, abrir, guardar y recientes",
                        onclick: move |_| menu_open.set(!menu_open()),
                        "Menú ▾"
                    }
                    if menu_open() {
                        div { class: "menu-backdrop", onclick: move |_| menu_open.set(false) }
                    }
                    div { class: if menu_open() { "file-menu open" } else { "file-menu" },
                        button {
                            id: "btn-new", class: "menu-item",
                            onclick: {
                                let f = files.clone();
                                move |_| {
                                    menu_open.set(false);
                                    f.request(Pending::New, dirty());
                                }
                            },
                            span { "Proyecto nuevo" }
                            span { class: "kbd", "Ctrl+N" }
                        }
                        button {
                            id: "btn-open", class: "menu-item",
                            onclick: {
                                let f = files.clone();
                                move |_| {
                                    menu_open.set(false);
                                    f.request(Pending::Open, dirty());
                                }
                            },
                            span { "Abrir proyecto…" }
                            span { class: "kbd", "Ctrl+O" }
                        }
                        button {
                            id: "btn-save", class: "menu-item",
                            onclick: {
                                let f = files.clone();
                                move |_| {
                                    menu_open.set(false);
                                    f.spawn_save(false);
                                }
                            },
                            span { "Guardar" }
                            span { class: "kbd", "Ctrl+S" }
                        }
                        button {
                            id: "btn-saveas", class: "menu-item",
                            onclick: {
                                let f = files.clone();
                                move |_| {
                                    menu_open.set(false);
                                    f.spawn_save(true);
                                }
                            },
                            span { "Guardar como…" }
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
                if let Some(t) = saved_at() {
                    span { class: "saved-ind", title: "Última vez que se guardó (o se autoguardó) el proyecto", "Guardado: {t}" }
                }
            }
            div { class: "top-center",
                span { class: "proj-name", title: "{proj_title}",
                    "{proj_name}"
                    if dirty_now { span { class: "proj-dirty", title: "Hay cambios sin guardar", " ●" } }
                }
            }
            div { class: "top-right",
                button {
                    class: "tool",
                    title: "Atajos de teclado",
                    onclick: move |_| shortcuts_open.set(true),
                    "⌨ Atajos"
                }
                button {
                    class: "export-btn",
                    disabled: !has_clip || exporting(),
                    onclick: on_export,
                    title: "Exportar a MP4",
                    "⬇ Exportar"
                }
            }
        }
    }
}
