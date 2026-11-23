// SPDX-License-Identifier: AGPL-3.0-or-later
//
//! Notices under the top bar: crash recovery, file messages, the unsaved-changes
//! dialog, errors and export progress.

use dioxus::prelude::*;

use super::Ctx;
use super::files::Pending;
use crate::project_file;

pub fn banners(c: &Ctx) -> Element {
    let files = c.files.clone();
    let error = c.error;
    let exporting = c.exporting;
    let export_stage = c.export_stage;
    let export_pct = c.export_pct;
    let mut export_msg = c.export_msg;
    let mut file_msg = c.file_msg;
    let mut recovery_offer = c.recovery_offer;
    let mut pending = c.pending;
    let mut shortcuts_open = c.shortcuts_open;
    let dirty = c.dirty;

    // The keyboard shortcuts the app has today, as `(keys, what it does)`.
    const SHORTCUTS: &[(&str, &str)] = &[
        ("Espacio", "Reproducir / pausar"),
        ("Ctrl + N", "Proyecto nuevo"),
        ("Ctrl + O", "Abrir proyecto"),
        ("Ctrl + S", "Guardar"),
    ];

    rsx! {
        if shortcuts_open() {
            div { class: "modal-back", onclick: move |_| shortcuts_open.set(false),
                div { class: "modal", onclick: move |e: MouseEvent| e.stop_propagation(),
                    div { class: "modal-title", "Atajos de teclado" }
                    table { class: "kbd-table",
                        for (keys, what) in SHORTCUTS.iter() {
                            tr { key: "{keys}",
                                td { span { class: "kbd", "{keys}" } }
                                td { "{what}" }
                            }
                        }
                    }
                    div { class: "modal-btns",
                        button { class: "tool", onclick: move |_| shortcuts_open.set(false), "Cerrar" }
                    }
                }
            }
        }

        if recovery_offer() {
            div { class: "recovery",
                span { "Se encontró trabajo sin guardar de una sesión anterior." }
                button {
                    class: "tool",
                    onclick: {
                        let f = files.clone();
                        move |_| f.request(Pending::Recover, dirty())
                    },
                    "Recuperar"
                }
                button {
                    class: "tool",
                    onclick: move |_| {
                        project_file::clear_recovery();
                        recovery_offer.set(false);
                    },
                    "Descartar"
                }
            }
        }

        if let Some((ok, msg)) = file_msg() {
            div {
                class: if ok { "export-done ok" } else { "export-done err" },
                "{msg}"
                button { class: "dismiss", onclick: move |_| file_msg.set(None), "✕" }
            }
        }

        if pending().is_some() {
            div { class: "modal-back",
                div { class: "modal",
                    div { class: "modal-title", "Hay cambios sin guardar" }
                    p { class: "modal-text", "¿Quieres guardar el proyecto antes de continuar?" }
                    div { class: "modal-btns",
                        button {
                            class: "tool export",
                            onclick: {
                                let f = files.clone();
                                move |_| f.resolve(true)
                            },
                            "Guardar"
                        }
                        button {
                            class: "tool",
                            onclick: {
                                let f = files.clone();
                                move |_| f.resolve(false)
                            },
                            "No guardar"
                        }
                        button { class: "tool", onclick: move |_| pending.set(None), "Cancelar" }
                    }
                }
            }
        }

        if let Some(msg) = error() {
            div { class: "error", "⚠ {msg}" }
        }

        if exporting() {
            div { class: "export-bar",
                div { class: "export-fill", style: "width: {export_pct()}%;" }
                span { class: "export-label", "Exportando… {export_stage()} · {export_pct():.0}%" }
            }
        }

        if let Some(msg) = export_msg() {
            div {
                class: if msg.starts_with('✓') { "export-done ok" } else { "export-done err" },
                "{msg}"
                button { class: "dismiss", onclick: move |_| export_msg.set(None), "✕" }
            }
        }
    }
}
