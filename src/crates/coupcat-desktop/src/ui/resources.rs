// SPDX-License-Identifier: AGPL-3.0-or-later
//
//! The resources panel on the left: one tab per kind of thing that can be added
//! to a project (media, audio, titles, effects, transitions, captions). Under
//! the active tab there are categories on the left, a search box and a grid of
//! cards, like the reference. Media and audio cards are dragged onto the
//! timeline; the others are pressed to apply them to the selected clip (or, for
//! titles, at the playhead).

use std::cell::RefCell;
use std::rc::Rc;

use dioxus::prelude::*;

use super::Ctx;
use super::inspector::TRANSITIONS;
use super::util::{dur_label, kind_icon, library_thumb};
use crate::editor::{self, MediaView, TimelineView};
use crate::monitor::{self, Cmd};
use crate::{captions, effects, text};

/// The tabs of the resources panel.
#[derive(Clone, Copy, PartialEq)]
pub enum ResTab {
    Media,
    Audio,
    Text,
    Effects,
    Transitions,
    Captions,
}

impl ResTab {
    const ALL: [ResTab; 6] = [
        ResTab::Media,
        ResTab::Audio,
        ResTab::Text,
        ResTab::Effects,
        ResTab::Transitions,
        ResTab::Captions,
    ];

    fn label(self) -> &'static str {
        match self {
            ResTab::Media => "Importar",
            ResTab::Audio => "Audio",
            ResTab::Text => "Texto",
            ResTab::Effects => "Efectos",
            ResTab::Transitions => "Transiciones",
            ResTab::Captions => "Subtítulos",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            ResTab::Media => "📁",
            ResTab::Audio => "🎵",
            ResTab::Text => "T",
            ResTab::Effects => "✨",
            ResTab::Transitions => "⇄",
            ResTab::Captions => "💬",
        }
    }

    /// The categories listed after "Todo"; empty when the tab has none.
    fn categories(self) -> Vec<&'static str> {
        match self {
            ResTab::Media => vec!["Vídeo", "Imagen"],
            ResTab::Text => text::PRESET_GROUPS.to_vec(),
            ResTab::Effects => effects::CATEGORIES
                .iter()
                .map(|(label, _)| *label)
                .collect(),
            _ => Vec::new(),
        }
    }

    fn search_hint(self) -> &'static str {
        match self {
            ResTab::Media => "Buscar en la librería",
            ResTab::Audio => "Buscar audio",
            ResTab::Text => "Buscar plantillas de texto",
            ResTab::Effects => "Buscar efectos",
            ResTab::Transitions => "Buscar transiciones",
            ResTab::Captions => "",
        }
    }
}

const MEDIA_EXTS: &[&str] = &[
    "mp4", "mov", "mkv", "webm", "avi", "m4v", "gif", "mp3", "wav", "m4a", "aac", "flac", "ogg",
    "png", "jpg", "jpeg", "webp",
];
const AUDIO_EXTS: &[&str] = &["mp3", "wav", "m4a", "aac", "flac", "ogg"];

/// A hue (0..360) derived from a name, so each effect card gets its own colour.
fn hue(s: &str) -> u32 {
    s.bytes()
        .fold(7u32, |h, b| h.wrapping_mul(31).wrapping_add(b as u32))
        % 360
}

/// The first two letters of a name ("Bleach Bypass" → "Bl"), to tell cards apart
/// until effects get real thumbnails (#22).
fn initials(name: &str) -> String {
    let mut chars = name.chars();
    let first = chars
        .next()
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_default();
    let second = chars
        .next()
        .map(|c| c.to_lowercase().to_string())
        .unwrap_or_default();
    format!("{first}{second}")
}

/// Pick files and add them to the library. The dialog is async so the UI thread
/// (the webview event loop) never blocks; nothing lands on the timeline until
/// the user drags it there.
async fn import_dialog(
    project: Rc<RefCell<concat_project::Editor>>,
    mut media: Signal<Vec<MediaView>>,
    mut timeline: Signal<TimelineView>,
    mut error: Signal<Option<String>>,
    exts: &'static [&'static str],
    title: &'static str,
) {
    let Some(files) = rfd::AsyncFileDialog::new()
        .add_filter("Media", exts)
        .set_title(title)
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
}

pub fn resources_panel(c: &Ctx) -> Element {
    let mut res_tab = c.res_tab;
    let mut res_cat = c.res_cat;
    let mut res_search = c.res_search;
    let thumb_rev = c.thumb_rev;

    let _thumb_rev = thumb_rev(); // subscribe: re-render when thumbnails arrive

    let tab = res_tab();
    let cat = res_cat();
    let categories = tab.categories();
    let q = res_search().trim().to_lowercase();

    let content = match tab {
        ResTab::Media => media_content(c, false, cat, &q),
        ResTab::Audio => media_content(c, true, cat, &q),
        ResTab::Text => text_content(c, cat, &q),
        ResTab::Effects => effects_content(c, cat, &q),
        ResTab::Transitions => transitions_content(c, &q),
        ResTab::Captions => captions_content(c),
    };

    rsx! {
        aside { class: "resources",
            div { class: "res-tabs-wrap",
                div { class: "res-tabs",
                    for t in ResTab::ALL {
                        button {
                            key: "{t.label()}",
                            class: if t == tab { "res-tab on" } else { "res-tab" },
                            onclick: move |_| {
                                res_tab.set(t);
                                res_cat.set(0);
                                res_search.set(String::new());
                            },
                            span { class: "res-tab-icon", "{t.icon()}" }
                            span { class: "res-tab-label", "{t.label()}" }
                        }
                    }
                }
                button {
                    class: "res-tabs-more",
                    title: "Más pestañas",
                    onclick: move |_| {
                        let _ = dioxus::document::eval(
                            "var t=document.querySelector('.res-tabs'); if(t){ if(t.scrollLeft+t.clientWidth>=t.scrollWidth-2){t.scrollTo({left:0,behavior:'smooth'})} else {t.scrollBy({left:140,behavior:'smooth'})} }",
                        );
                    },
                    "»"
                }
            }
            div { class: "res-body",
                if !categories.is_empty() {
                    div { class: "res-cats",
                        button {
                            class: if cat == 0 { "res-cat on" } else { "res-cat" },
                            onclick: move |_| res_cat.set(0),
                            "Todo"
                        }
                        for (i, label) in categories.iter().enumerate() {
                            button {
                                key: "{label}",
                                class: if cat == i + 1 { "res-cat on" } else { "res-cat" },
                                onclick: move |_| res_cat.set(i + 1),
                                "{label}"
                            }
                        }
                    }
                }
                div { class: "res-main",
                    if tab != ResTab::Captions {
                        div { class: "res-search",
                            span { class: "res-search-icon", "🔍" }
                            input {
                                r#type: "text",
                                placeholder: "{tab.search_hint()}",
                                value: "{res_search()}",
                                oninput: move |e: FormEvent| res_search.set(e.value()),
                            }
                        }
                    }
                    {content}
                }
            }
        }
    }
}

/// Media (all kinds, filtered by the category) or, with `audio_only`, the audio
/// files. Cards are dragged onto the timeline.
fn media_content(c: &Ctx, audio_only: bool, cat: usize, q: &str) -> Element {
    let project = c.project.clone();
    let media = c.media;
    let timeline = c.timeline;
    let error = c.error;
    let mut dragging_media = c.dragging_media;

    let all = media();
    let items: Vec<&MediaView> = all
        .iter()
        .filter(|m| {
            let kind_ok = if audio_only {
                m.kind == "audio"
            } else {
                match cat {
                    1 => m.kind == "video",
                    2 => m.kind == "image",
                    _ => true,
                }
            };
            kind_ok && (q.is_empty() || m.name.to_lowercase().contains(q))
        })
        .collect();

    let (exts, title, button) = if audio_only {
        (AUDIO_EXTS, "Importar audio", "＋ Importar audio")
    } else {
        (MEDIA_EXTS, "Importar a la librería", "＋ Importar")
    };
    let empty = if !q.is_empty() || (!all.is_empty() && !audio_only) {
        "Sin resultados."
    } else if audio_only {
        "Importa música o efectos de sonido (mp3, wav, m4a…) para usarlos aquí."
    } else {
        "Importa archivos (vídeo, audio, imagen) para empezar."
    };

    rsx! {
        div { class: "res-actions",
            button {
                class: "import",
                title: "Elegir archivos de tu equipo",
                onclick: move |_| {
                    spawn(import_dialog(project.clone(), media, timeline, error, exts, title));
                },
                "{button}"
            }
        }
        if items.is_empty() {
            div { class: "res-empty", "{empty}" }
        } else {
            div { class: "res-grid",
                for m in items.iter() {
                    div {
                        key: "{m.id}",
                        class: if dragging_media() == Some(m.id.clone()) { "card dragging" } else { "card" },
                        title: "Arrastra a la línea de tiempo",
                        onmousedown: {
                            let id = m.id.clone();
                            move |_| dragging_media.set(Some(id.clone()))
                        },
                        div { class: "card-thumb k-{m.kind}",
                            match library_thumb(m) {
                                Some(src) => rsx! { img { src: "{src}", draggable: "false" } },
                                None => rsx! { span { class: "card-icon", "{kind_icon(m.kind)}" } },
                            }
                            if m.duration.is_some() {
                                span { class: "card-badge", "{dur_label(m.duration)}" }
                            }
                        }
                        div { class: "card-name", "{m.name}" }
                    }
                }
            }
        }
    }
}

/// Title looks. Pressing a card adds the title at the playhead.
fn text_content(c: &Ctx, cat: usize, q: &str) -> Element {
    let project = c.project.clone();
    let monitor = c.monitor.clone();
    let pos = c.pos;
    let mut timeline = c.timeline;
    let mut selected = c.selected;

    let group = if cat == 0 {
        None
    } else {
        text::PRESET_GROUPS.get(cat - 1).copied()
    };
    let items: Vec<&text::Preset> = text::PRESETS
        .iter()
        .filter(|p| {
            group.is_none_or(|g| p.group == g)
                && (q.is_empty() || p.name.to_lowercase().contains(q))
        })
        .collect();

    rsx! {
        div { class: "res-hint", "Pulsa una plantilla para añadir el título en el cursor." }
        if items.is_empty() {
            div { class: "res-empty", "Sin resultados." }
        } else {
            div { class: "res-grid",
                for p in items.iter() {
                    div {
                        key: "{p.id}",
                        class: "card",
                        title: "{p.name}",
                        onclick: {
                            let project = project.clone();
                            let monitor = monitor.clone();
                            let id = p.id;
                            move |_| {
                                if let Some(cid) = text::add_preset(&mut project.borrow_mut(), pos(), id) {
                                    selected.set(Some(cid));
                                }
                                timeline.set(editor::view(&project.borrow()));
                                if let Some(d) = monitor::timeline_data(&project.borrow()) {
                                    monitor.send(Cmd::SetTimeline(d));
                                }
                            }
                        },
                        div { class: "card-thumb card-text",
                            span { style: "{p.css}", "{p.sample}" }
                        }
                        div { class: "card-name", "{p.name}" }
                    }
                }
            }
        }
    }
}

/// The effect catalogue. Pressing a card applies it to the selected clip.
fn effects_content(c: &Ctx, cat: usize, q: &str) -> Element {
    let project = c.project.clone();
    let monitor = c.monitor.clone();
    let fx_catalogue = c.fx_catalogue.clone();
    let selected = c.selected;
    let mut timeline = c.timeline;

    let sel = selected();
    let items: Vec<&effects::EffectInfo> = fx_catalogue
        .iter()
        .filter(|fx| {
            (cat == 0 || fx.cats.contains(&(cat - 1)))
                && (q.is_empty() || fx.name.to_lowercase().contains(q))
        })
        .collect();

    rsx! {
        if sel.is_none() {
            div { class: "res-hint warn", "Selecciona un clip en la línea de tiempo para aplicarle un efecto." }
        }
        if items.is_empty() {
            div { class: "res-empty", "Sin resultados." }
        } else {
            div { class: "res-grid",
                for fx in items.iter() {
                    div {
                        key: "{fx.id}",
                        class: if sel.is_some() { "card" } else { "card disabled" },
                        title: "{fx.name}",
                        onclick: {
                            let project = project.clone();
                            let monitor = monitor.clone();
                            let sel = sel.clone();
                            let fxid = fx.id.clone();
                            move |_| {
                                if let Some(cid) = &sel {
                                    effects::add(&mut project.borrow_mut(), cid, &fxid);
                                    timeline.set(editor::view(&project.borrow()));
                                    if let Some(d) = monitor::timeline_data(&project.borrow()) {
                                        monitor.send(Cmd::SetTimeline(d));
                                    }
                                }
                            }
                        },
                        div {
                            class: "card-thumb card-fx",
                            style: "background: linear-gradient(135deg, hsl({hue(&fx.id)},62%,40%), hsl({(hue(&fx.id) + 45) % 360},58%,22%));",
                            span { class: "card-initial", "{initials(&fx.name)}" }
                        }
                        div { class: "card-name", "{fx.name}" }
                    }
                }
            }
        }
    }
}

/// The transitions. Pressing a card applies it to the cut in front of the
/// selected clip.
fn transitions_content(c: &Ctx, q: &str) -> Element {
    let project = c.project.clone();
    let monitor = c.monitor.clone();
    let selected = c.selected;
    let mut timeline = c.timeline;

    let sel = selected();
    let has_cut = sel
        .as_ref()
        .map(|id| editor::has_incoming_cut(&project.borrow(), id))
        .unwrap_or(false);
    let current = sel
        .as_ref()
        .and_then(|id| editor::get_transition(&project.borrow(), id));
    let cur_kind = current.as_ref().map(|(k, _)| k.clone());
    let cur_dur = current.as_ref().map(|(_, d)| *d).unwrap_or(1.0);

    // (id, label, glyph); the first one removes the transition.
    let glyph = |id: &str| match id {
        "cross-fade" => "◑",
        "fade-black" => "◼",
        "fade-white" => "◻",
        "push" => "⇨",
        "zoom" => "⤢",
        "wipe-left" => "◀",
        "wipe-right" => "▶",
        _ => "⇄",
    };
    let mut items: Vec<(&str, &str, &str)> = vec![("", "Ninguna", "⃠")];
    items.extend(
        TRANSITIONS
            .iter()
            .map(|(id, label)| (*id, *label, glyph(id))),
    );
    let items: Vec<(&str, &str, &str)> = items
        .into_iter()
        .filter(|(_, label, _)| q.is_empty() || label.to_lowercase().contains(q))
        .collect();

    // A card is greyed out without a cut to put it on, and marked when it is the
    // transition the selected clip already has.
    let card_class = |id: &str| -> &'static str {
        if !has_cut {
            "card disabled"
        } else if cur_kind.as_deref().unwrap_or("") == id {
            "card on"
        } else {
            "card"
        }
    };

    rsx! {
        if sel.is_none() {
            div { class: "res-hint warn", "Selecciona un clip en la línea de tiempo para ponerle una transición." }
        } else if !has_cut {
            div { class: "res-hint warn",
                "Coloca este clip justo después de otro en la misma pista para poder poner una transición en el corte."
            }
        }
        if items.is_empty() {
            div { class: "res-empty", "Sin resultados." }
        } else {
            div { class: "res-grid",
                for (id, label, glyph) in items.iter() {
                    div {
                        key: "{label}",
                        class: card_class(id),
                        title: "{label}",
                        onclick: {
                            let project = project.clone();
                            let monitor = monitor.clone();
                            let sel = sel.clone();
                            let id = id.to_string();
                            move |_| {
                                if !has_cut {
                                    return;
                                }
                                if let Some(cid) = &sel {
                                    let kind = if id.is_empty() { None } else { Some(id.as_str()) };
                                    editor::apply_transition(
                                        &mut project.borrow_mut(),
                                        cid,
                                        kind,
                                        cur_dur.max(0.8),
                                    );
                                    timeline.set(editor::view(&project.borrow()));
                                    if let Some(d) = monitor::timeline_data(&project.borrow()) {
                                        monitor.send(Cmd::SetTimeline(d));
                                    }
                                    // The cut is now centred at the clip's (new) start; jump there.
                                    if kind.is_some() {
                                        let start = project
                                            .borrow()
                                            .project()
                                            .active()
                                            .clip(cid)
                                            .map(|cl| cl.start);
                                        if let Some(s) = start {
                                            monitor.send(Cmd::Seek(s));
                                        }
                                    }
                                }
                            }
                        },
                        div { class: "card-thumb card-tr",
                            span { class: "card-initial", "{glyph}" }
                        }
                        div { class: "card-name", "{label}" }
                    }
                }
            }
        }
    }
}

/// Automatic captions for the selected clip.
fn captions_content(c: &Ctx) -> Element {
    let project = c.project.clone();
    let selected = c.selected;
    let mut cc_job = c.cc_job;
    let mut cc_stage = c.cc_stage;
    let mut generating_cc = c.generating_cc;

    let sel = selected();
    let has_media = sel
        .as_ref()
        .and_then(|id| editor::media_path(&project.borrow(), id))
        .is_some();
    let ready = captions::model_ready();

    let on_captions = move |_| {
        if generating_cc() {
            return;
        }
        if let Some(cid) = selected() {
            let timing = editor::clip_timing(&project.borrow(), &cid);
            let path = editor::media_path(&project.borrow(), &cid);
            if let (Some((ss, dur, start)), Some(path)) = (timing, path) {
                cc_stage.set("extrayendo audio".to_string());
                generating_cc.set(true);
                cc_job.set(Some(captions::start(path, ss, dur, start)));
            }
        }
    };

    rsx! {
        div { class: "res-captions",
            div { class: "res-captions-icon", "💬" }
            div { class: "res-captions-title", "Subtítulos automáticos" }
            p { class: "res-captions-text",
                "Reconoce la voz del clip seleccionado en tu equipo (sin enviarlo a ningún servicio) y añade los subtítulos como títulos en la línea de tiempo."
            }
            if !ready {
                div { class: "res-hint warn", "Falta el modelo de voz (ggml-base.bin) junto al programa." }
            } else if sel.is_none() || !has_media {
                div { class: "res-hint warn", "Selecciona un clip con audio en la línea de tiempo." }
            }
            if generating_cc() {
                div { class: "cut-progress", "Subtítulos: {cc_stage()}…" }
            } else {
                button {
                    class: "tool fx-cc",
                    disabled: !ready || sel.is_none() || !has_media,
                    onclick: on_captions,
                    "💬 Generar subtítulos"
                }
            }
        }
    }
}
