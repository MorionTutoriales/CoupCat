// SPDX-License-Identifier: AGPL-3.0-or-later
//
//! The right panel: the selected clip's properties, split into tabs by clip
//! kind. The placement, speed, animation and colour tabs live in `props`.

mod props;

use dioxus::prelude::*;

use super::Ctx;
use super::util::color6;
use crate::editor::{self, InspKind};
use crate::monitor::{self, Cmd};
use crate::{cutout, effects, text};

/// Built-in transition kinds the engine resolves on a cut: `(id, label)`.
pub const TRANSITIONS: &[(&str, &str)] = &[
    ("cross-fade", "Fundido cruzado"),
    ("fade-black", "Fundido a negro"),
    ("fade-white", "Fundido a blanco"),
    ("push", "Empujar"),
    ("zoom", "Zoom"),
    ("wipe-left", "Barrido ←"),
    ("wipe-right", "Barrido →"),
];

/// A tab of the inspector.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InspTab {
    Video,
    Audio,
    Speed,
    Animation,
    Adjust,
    Effects,
    Text,
}

impl InspTab {
    fn label(self) -> &'static str {
        match self {
            InspTab::Video => "Vídeo",
            InspTab::Audio => "Audio",
            InspTab::Speed => "Velocidad",
            InspTab::Animation => "Animación",
            InspTab::Adjust => "Ajustes",
            InspTab::Effects => "Efectos",
            InspTab::Text => "Texto",
        }
    }

    fn id(self) -> &'static str {
        match self {
            InspTab::Video => "insp-video",
            InspTab::Audio => "insp-audio",
            InspTab::Speed => "insp-speed",
            InspTab::Animation => "insp-anim",
            InspTab::Adjust => "insp-adjust",
            InspTab::Effects => "insp-effects",
            InspTab::Text => "insp-text",
        }
    }

    /// The tabs a clip of this kind has.
    fn for_kind(kind: InspKind) -> &'static [InspTab] {
        match kind {
            InspKind::Video => &[
                InspTab::Video,
                InspTab::Effects,
                InspTab::Audio,
                InspTab::Speed,
                InspTab::Animation,
                InspTab::Adjust,
            ],
            InspKind::Image => &[
                InspTab::Video,
                InspTab::Effects,
                InspTab::Animation,
                InspTab::Adjust,
            ],
            InspKind::Audio => &[InspTab::Audio, InspTab::Speed],
            InspKind::Text => &[InspTab::Text, InspTab::Animation],
            InspKind::Other => &[InspTab::Video, InspTab::Effects],
        }
    }
}

pub fn inspector_panel(c: &Ctx) -> Element {
    let project = c.project.clone();
    let monitor = c.monitor.clone();
    let mut timeline = c.timeline;
    let selected = c.selected;
    let mut cutout_job = c.cutout_job;
    let mut cutting = c.cutting;
    let mut cut_done = c.cut_done;
    let mut cut_total = c.cut_total;

    let on_cutout = {
        let project = project.clone();
        move |_| {
            if cutting() {
                return;
            }
            if let Some(cid) = selected() {
                if let Some(path) = editor::media_path(&project.borrow(), &cid) {
                    cut_done.set(0);
                    cut_total.set(0);
                    cutting.set(true);
                    cutout_job.set(Some(cutout::start(path, cid)));
                }
            }
        }
    };

    let on_restore = {
        let project = project.clone();
        let monitor = monitor.clone();
        move |_| {
            if let Some(cid) = selected() {
                editor::set_cutout(&mut project.borrow_mut(), cid, false);
                timeline.set(editor::view(&project.borrow()));
                if let Some(d) = monitor::timeline_data(&project.borrow()) {
                    monitor.send(Cmd::SetTimeline(d));
                }
            }
        }
    };

    // Commit a whole new text style to the selected title and refresh.
    let commit_text = {
        let project = project.clone();
        let monitor = monitor.clone();
        let sel = selected.clone();
        move |style: concat_project::model::TextStyle| {
            if let Some(cid) = sel() {
                text::set_style(&mut project.borrow_mut(), cid, style);
                timeline.set(editor::view(&project.borrow()));
                if let Some(d) = monitor::timeline_data(&project.borrow()) {
                    monitor.send(Cmd::SetTimeline(d));
                }
            }
        }
    };

    let tl = timeline();

    let selected_now = selected();

    // The tabs this clip has; the remembered tab stays when it still applies.
    let kind = selected_now
        .as_ref()
        .and_then(|id| editor::insp_kind(&project.borrow(), id));
    let tabs: &[InspTab] = kind.map(InspTab::for_kind).unwrap_or(&[]);
    let mut insp_tab = c.insp_tab;
    let tab = if tabs.contains(&insp_tab()) {
        insp_tab()
    } else {
        tabs.first().copied().unwrap_or(InspTab::Video)
    };

    let fx_cards = selected_now
        .as_ref()
        .map(|id| effects::effect_cards(&project.borrow(), id))
        .unwrap_or_default();

    let text_style = selected_now
        .as_ref()
        .filter(|_| tab == InspTab::Text)
        .and_then(|id| text::get_style(&project.borrow(), id));

    let cur_transition = selected_now
        .as_ref()
        .and_then(|id| editor::get_transition(&project.borrow(), id));

    let tr_id = cur_transition
        .as_ref()
        .map(|(id, _)| id.clone())
        .unwrap_or_default();

    let tr_dur = cur_transition.as_ref().map(|(_, d)| *d).unwrap_or(1.0);

    let has_cut = selected_now
        .as_ref()
        .map(|id| editor::has_incoming_cut(&project.borrow(), id))
        .unwrap_or(false);

    let selected_has_media = tab == InspTab::Video
        && selected_now
            .as_ref()
            .and_then(|id| editor::media_path(&project.borrow(), id))
            .is_some();

    let selected_cutout = selected_now
        .as_ref()
        .map(|id| editor::has_cutout(&project.borrow(), id))
        .unwrap_or(false);

    let has_sound = selected_now
        .as_ref()
        .and_then(|id| editor::audio_info(&project.borrow(), id))
        .is_some();
    let audio = selected_now
        .as_ref()
        .filter(|_| tab == InspTab::Audio)
        .and_then(|id| editor::audio_info(&project.borrow(), id));

    let selected_name = selected_now.as_ref().and_then(|id| {
        tl.clips
            .iter()
            .find(|c| &c.id == id)
            .map(|c| c.name.clone())
    });

    rsx! {
        aside { class: "fx",
            if !tabs.is_empty() {
                div { class: "res-tabs-wrap insp-tabs",
                    div { class: "res-tabs",
                        for t in tabs.iter() {
                            button {
                                key: "{t.id()}",
                                id: "{t.id()}",
                                class: if *t == tab { "res-tab on" } else { "res-tab" },
                                onclick: { let t = *t; move |_| insp_tab.set(t) },
                                "{t.label()}"
                            }
                        }
                    }
                }
            }
            if let (Some(id), InspTab::Video) = (selected_now.as_ref(), tab) {
                {props::transform_section(c, id)}
            }
            if let (Some(id), InspTab::Speed) = (selected_now.as_ref(), tab) {
                {props::speed_tab(c, id, has_sound)}
            }
            if let (Some(id), InspTab::Animation) = (selected_now.as_ref(), tab) {
                {props::animation_tab(c, id)}
            }
            if let (Some(id), InspTab::Adjust) = (selected_now.as_ref(), tab) {
                {props::adjust_tab(c, id)}
            }
            if let Some(st) = text_style.as_ref() {
                div { class: "txt-panel",
                    div { class: "fx-head", "Texto" }
                    div { class: "txt-body",
                        textarea {
                            class: "txt-area",
                            rows: "2",
                            value: "{st.content}",
                            oninput: {
                                let mut commit = commit_text.clone();
                                let base = st.clone();
                                move |e: FormEvent| {
                                    let mut s = base.clone();
                                    s.content = e.value();
                                    commit(s);
                                }
                            },
                        }
                        div { class: "fx-param",
                            label { class: "fx-param-label", "Tamaño" }
                            div { class: "fx-slider-row",
                                input {
                                    class: "fx-range", r#type: "range",
                                    min: "0.02", max: "0.4", step: "0.005",
                                    value: "{st.font_size}",
                                    oninput: {
                                        let mut commit = commit_text.clone();
                                        let base = st.clone();
                                        move |e: FormEvent| {
                                            if let Ok(v) = e.value().parse::<f64>() {
                                                let mut s = base.clone();
                                                s.font_size = v;
                                                commit(s);
                                            }
                                        }
                                    },
                                }
                                span { class: "fx-val", "{st.font_size:.3}" }
                            }
                        }
                        div { class: "fx-param",
                            label { class: "fx-param-label", "Color" }
                            input {
                                class: "txt-color", r#type: "color",
                                value: "{color6(&st.color)}",
                                oninput: {
                                    let mut commit = commit_text.clone();
                                    let base = st.clone();
                                    move |e: FormEvent| {
                                        let mut s = base.clone();
                                        s.color = e.value();
                                        commit(s);
                                    }
                                },
                            }
                        }
                        div { class: "fx-param",
                            label { class: "fx-param-label", "Alineación" }
                            div { class: "txt-align",
                                button {
                                    class: if matches!(st.align, concat_project::model::TextAlign::Left) { "al on" } else { "al" },
                                    onclick: {
                                        let mut commit = commit_text.clone();
                                        let base = st.clone();
                                        move |_| { let mut s = base.clone(); s.align = concat_project::model::TextAlign::Left; commit(s); }
                                    },
                                    "⇤"
                                }
                                button {
                                    class: if matches!(st.align, concat_project::model::TextAlign::Center) { "al on" } else { "al" },
                                    onclick: {
                                        let mut commit = commit_text.clone();
                                        let base = st.clone();
                                        move |_| { let mut s = base.clone(); s.align = concat_project::model::TextAlign::Center; commit(s); }
                                    },
                                    "↔"
                                }
                                button {
                                    class: if matches!(st.align, concat_project::model::TextAlign::Right) { "al on" } else { "al" },
                                    onclick: {
                                        let mut commit = commit_text.clone();
                                        let base = st.clone();
                                        move |_| { let mut s = base.clone(); s.align = concat_project::model::TextAlign::Right; commit(s); }
                                    },
                                    "⇥"
                                }
                            }
                        }
                    }
                }
            }
            if selected_has_media {
                div { class: "cut-panel",
                    div { class: "fx-head", "Fondo (IA)" }
                    div { class: "cut-body",
                        if cutting() {
                            div { class: "cut-progress",
                                if cut_total() > 0 {
                                    "Quitando fondo… {cut_done()}/{cut_total()}"
                                } else {
                                    "Quitando fondo… {cut_done()} fotogramas"
                                }
                            }
                        } else if selected_cutout {
                            button { class: "tool", onclick: on_restore, "Restaurar fondo" }
                        } else {
                            button { class: "tool fx-cut", onclick: on_cutout, "✂ Quitar fondo (personas)" }
                        }
                    }
                }
            }
            if let Some(a) = audio.as_ref() {
                div { class: "audio-panel",
                    div { class: "fx-head", "Audio" }
                    div { class: "audio-body",
                        button {
                            class: if a.muted { "tool au-muted" } else { "tool" },
                            onclick: {
                                let project = project.clone();
                                let monitor = monitor.clone();
                                let sel = selected_now.clone();
                                let muted = a.muted;
                                move |_| {
                                    if let Some(cid) = &sel {
                                        editor::set_muted(&mut project.borrow_mut(), cid.clone(), !muted);
                                        timeline.set(editor::view(&project.borrow()));
                                        if let Some(d) = monitor::timeline_data(&project.borrow()) {
                                            monitor.send(Cmd::SetTimeline(d));
                                        }
                                    }
                                }
                            },
                            if a.muted { "🔇 Silenciado" } else { "🔊 Con sonido" }
                        }
                        div { class: "fx-param",
                            label { class: "fx-param-label", "Volumen" }
                            div { class: "fx-slider-row",
                                input {
                                    class: "fx-range", r#type: "range", min: "0", max: "2", step: "0.01",
                                    value: "{a.volume}",
                                    oninput: {
                                        let project = project.clone();
                                        let monitor = monitor.clone();
                                        let sel = selected_now.clone();
                                        move |e: FormEvent| {
                                            if let (Some(cid), Ok(v)) = (&sel, e.value().parse::<f64>()) {
                                                editor::set_volume(&mut project.borrow_mut(), cid.clone(), v);
                                                timeline.set(editor::view(&project.borrow()));
                                                if let Some(d) = monitor::timeline_data(&project.borrow()) {
                                                    monitor.send(Cmd::SetTimeline(d));
                                                }
                                            }
                                        }
                                    },
                                }
                                span { class: "fx-val", "{(a.volume * 100.0) as i32}%" }
                            }
                        }
                        div { class: "fx-param",
                            label { class: "fx-param-label", "Fundido de entrada" }
                            div { class: "fx-slider-row",
                                input {
                                    class: "fx-range", r#type: "range", min: "0", max: "3", step: "0.05",
                                    value: "{a.fade_in}",
                                    oninput: {
                                        let project = project.clone();
                                        let monitor = monitor.clone();
                                        let sel = selected_now.clone();
                                        move |e: FormEvent| {
                                            if let (Some(cid), Ok(v)) = (&sel, e.value().parse::<f64>()) {
                                                editor::set_fade_in(&mut project.borrow_mut(), cid.clone(), v);
                                                timeline.set(editor::view(&project.borrow()));
                                                if let Some(d) = monitor::timeline_data(&project.borrow()) {
                                                    monitor.send(Cmd::SetTimeline(d));
                                                }
                                            }
                                        }
                                    },
                                }
                                span { class: "fx-val", "{a.fade_in:.2}s" }
                            }
                        }
                        div { class: "fx-param",
                            label { class: "fx-param-label", "Fundido de salida" }
                            div { class: "fx-slider-row",
                                input {
                                    class: "fx-range", r#type: "range", min: "0", max: "3", step: "0.05",
                                    value: "{a.fade_out}",
                                    oninput: {
                                        let project = project.clone();
                                        let monitor = monitor.clone();
                                        let sel = selected_now.clone();
                                        move |e: FormEvent| {
                                            if let (Some(cid), Ok(v)) = (&sel, e.value().parse::<f64>()) {
                                                editor::set_fade_out(&mut project.borrow_mut(), cid.clone(), v);
                                                timeline.set(editor::view(&project.borrow()));
                                                if let Some(d) = monitor::timeline_data(&project.borrow()) {
                                                    monitor.send(Cmd::SetTimeline(d));
                                                }
                                            }
                                        }
                                    },
                                }
                                span { class: "fx-val", "{a.fade_out:.2}s" }
                            }
                        }
                    }
                }
            }
            if selected_now.is_some() && tab == InspTab::Video {
                div { class: "tr-panel",
                    div { class: "fx-head", "Transición de entrada" }
                    div { class: "tr-body",
                        if !has_cut {
                            div { class: "tr-hint",
                                "Coloca este clip justo después de otro en la misma pista para poder poner una transición en el corte."
                            }
                        }
                        select {
                            class: "fx-select",
                            disabled: !has_cut,
                            value: "{tr_id}",
                            onchange: {
                                let project = project.clone();
                                let monitor = monitor.clone();
                                let sel = selected_now.clone();
                                let dur = tr_dur;
                                move |e: FormEvent| {
                                    if let Some(cid) = &sel {
                                        let v = e.value();
                                        let kind = if v.is_empty() { None } else { Some(v.as_str()) };
                                        editor::apply_transition(&mut project.borrow_mut(), cid, kind, dur.max(0.8));
                                        timeline.set(editor::view(&project.borrow()));
                                        if let Some(d) = monitor::timeline_data(&project.borrow()) {
                                            monitor.send(Cmd::SetTimeline(d));
                                        }
                                        // The cut is now centred at the clip's (new) start; jump there.
                                        if kind.is_some() {
                                            let center = project.borrow().project().active().clip(cid).map(|c| c.start);
                                            if let Some(c) = center {
                                                monitor.send(Cmd::Seek(c));
                                            }
                                        }
                                    }
                                }
                            },
                            option { value: "", "Ninguna" }
                            for (id, label) in TRANSITIONS.iter() {
                                option { value: "{id}", "{label}" }
                            }
                        }
                        if cur_transition.is_some() {
                            div { class: "fx-param",
                                label { class: "fx-param-label", "Duración" }
                                div { class: "fx-slider-row",
                                    input {
                                        class: "fx-range", r#type: "range",
                                        min: "0.1", max: "3", step: "0.05",
                                        value: "{tr_dur}",
                                        oninput: {
                                            let project = project.clone();
                                            let monitor = monitor.clone();
                                            let sel = selected_now.clone();
                                            let id = tr_id.clone();
                                            move |e: FormEvent| {
                                                if let (Some(cid), Ok(v)) = (&sel, e.value().parse::<f64>()) {
                                                    editor::apply_transition(&mut project.borrow_mut(), cid, Some(&id), v);
                                                    timeline.set(editor::view(&project.borrow()));
                                                    if let Some(d) = monitor::timeline_data(&project.borrow()) {
                                                        monitor.send(Cmd::SetTimeline(d));
                                                    }
                                                    let center = project.borrow().project().active().clip(cid).map(|c| c.start);
                                                    if let Some(c) = center {
                                                        monitor.send(Cmd::Seek(c));
                                                    }
                                                }
                                            }
                                        },
                                    }
                                    span { class: "fx-val", "{tr_dur:.2}s" }
                                }
                            }
                        }
                    }
                }
            }
            if tab == InspTab::Effects || selected_now.is_none() {
                div { class: "fx-head", "Efectos" }
            }
            match selected_now.as_ref() {
                None => rsx! {
                    div { class: "fx-empty",
                        "Selecciona un clip en la línea de tiempo para ver y editar sus propiedades, su audio y su fondo."
                    }
                },
                Some(_) if tab != InspTab::Effects => rsx! {},
                Some(_) => rsx! {
                    div { class: "fx-sel", "Clip: {selected_name.clone().unwrap_or_default()}" }
                    div { class: "fx-applied",
                        if fx_cards.is_empty() {
                            div { class: "fx-none",
                                "Sin efectos. Arrastra uno desde la pestaña Efectos de la izquierda sobre el clip o el reproductor."
                            }
                        } else {
                            for card in fx_cards.iter() {
                                div {
                                    class: if card.enabled { "fx-card" } else { "fx-card off" },
                                    key: "{card.index}",
                                    div { class: "fx-card-head",
                                        span { class: "fx-card-name", "{card.name}" }
                                        div { class: "fx-card-btns",
                                        button {
                                            class: if card.enabled { "fx-mini on" } else { "fx-mini" },
                                            title: if card.enabled { "Desactivar este efecto (se conservan sus ajustes)" } else { "Activar este efecto" },
                                            onclick: {
                                                let project = project.clone();
                                                let monitor = monitor.clone();
                                                let sel = selected_now.clone();
                                                let idx = card.index;
                                                let on = card.enabled;
                                                move |_| {
                                                    if let Some(cid) = &sel {
                                                        effects::set_enabled(&mut project.borrow_mut(), cid, idx, !on);
                                                        timeline.set(editor::view(&project.borrow()));
                                                        if let Some(d) = monitor::timeline_data(&project.borrow()) {
                                                            monitor.send(Cmd::SetTimeline(d));
                                                        }
                                                    }
                                                }
                                            },
                                            if card.enabled { "●" } else { "○" }
                                        }
                                        button {
                                            class: "fx-mini",
                                            title: "Restablecer los ajustes de este efecto",
                                            onclick: {
                                                let project = project.clone();
                                                let monitor = monitor.clone();
                                                let sel = selected_now.clone();
                                                let idx = card.index;
                                                move |_| {
                                                    if let Some(cid) = &sel {
                                                        effects::reset_params(&mut project.borrow_mut(), cid, idx);
                                                        timeline.set(editor::view(&project.borrow()));
                                                        if let Some(d) = monitor::timeline_data(&project.borrow()) {
                                                            monitor.send(Cmd::SetTimeline(d));
                                                        }
                                                    }
                                                }
                                            },
                                            "↺"
                                        }
                                        button {
                                            class: "fx-chip-x",
                                            title: "Quitar este efecto",
                                            onclick: {
                                                let project = project.clone();
                                                let monitor = monitor.clone();
                                                let sel = selected_now.clone();
                                                let idx = card.index;
                                                move |_| {
                                                    if let Some(cid) = &sel {
                                                        effects::remove(&mut project.borrow_mut(), cid, idx);
                                                        timeline.set(editor::view(&project.borrow()));
                                                        if let Some(d) = monitor::timeline_data(&project.borrow()) {
                                                            monitor.send(Cmd::SetTimeline(d));
                                                        }
                                                    }
                                                }
                                            },
                                            "×"
                                        }
                                        }
                                    }
                                    if card.params.is_empty() {
                                        div { class: "fx-noparams", "Sin ajustes" }
                                    } else if !card.params.iter().any(|pv| effects::is_intensity(&pv.info.key)) {
                                        div { class: "fx-noparams", "Este efecto no tiene control de intensidad; ajusta sus parámetros." }
                                    }
                                    for pv in card.params.iter() {
                                        {
                                            let p = pv.info.clone();
                                            let cur = pv.current;
                                            let idx = card.index;
                                            let step_attr = if p.step > 0.0 {
                                                format!("{}", p.step)
                                            } else {
                                                "any".to_string()
                                            };
                                            let make_set = {
                                                let project = project.clone();
                                                let monitor = monitor.clone();
                                                let sel = selected_now.clone();
                                                let key = p.key.clone();
                                                move |v: f64| {
                                                    if let Some(cid) = &sel {
                                                        effects::set_param(&mut project.borrow_mut(), cid, idx, &key, v);
                                                        timeline.set(editor::view(&project.borrow()));
                                                        if let Some(d) = monitor::timeline_data(&project.borrow()) {
                                                            monitor.send(Cmd::SetTimeline(d));
                                                        }
                                                    }
                                                }
                                            };
                                            rsx! {
                                                div { class: "fx-param",
                                                    label {
                                                        class: if effects::is_intensity(&p.key) { "fx-param-label strong" } else { "fx-param-label" },
                                                        if effects::is_intensity(&p.key) { "Intensidad" } else { "{p.label}" }
                                                    }
                                                    match p.kind {
                                                        concat_effects::ParamType::Bool => rsx! {
                                                            button {
                                                                class: if cur != 0.0 { "fx-toggle on" } else { "fx-toggle" },
                                                                onclick: {
                                                                    let mut set = make_set.clone();
                                                                    move |_| set(if cur != 0.0 { 0.0 } else { 1.0 })
                                                                },
                                                                if cur != 0.0 { "Sí" } else { "No" }
                                                            }
                                                        },
                                                        concat_effects::ParamType::Enum => rsx! {
                                                            select {
                                                                class: "fx-select",
                                                                value: "{cur}",
                                                                onchange: {
                                                                    let mut set = make_set.clone();
                                                                    move |e: FormEvent| {
                                                                        if let Ok(v) = e.value().parse::<f64>() { set(v); }
                                                                    }
                                                                },
                                                                for (val, lab) in p.values.iter().zip(p.labels.iter()) {
                                                                    option { value: "{val}", "{lab}" }
                                                                }
                                                            }
                                                        },
                                                        _ => rsx! {
                                                            div { class: "fx-slider-row",
                                                                input {
                                                                    class: "fx-range",
                                                                    r#type: "range",
                                                                    min: "{p.min}",
                                                                    max: "{p.max}",
                                                                    step: "{step_attr}",
                                                                    value: "{cur}",
                                                                    oninput: {
                                                                        let mut set = make_set.clone();
                                                                        move |e: FormEvent| {
                                                                            if let Ok(v) = e.value().parse::<f64>() { set(v); }
                                                                        }
                                                                    },
                                                                }
                                                                span { class: "fx-val", "{cur:.2}{p.unit}" }
                                                            }
                                                        },
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    // Adding effects lives in the resources panel's Effects tab; this
                    // panel only edits the ones already on the clip.
                },
            }
        }
    }
}
