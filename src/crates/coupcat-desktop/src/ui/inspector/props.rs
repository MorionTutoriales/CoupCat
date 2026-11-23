// SPDX-License-Identifier: AGPL-3.0-or-later
//
//! The inspector tabs that edit a clip's placement, speed, animation and
//! colour. Each one is a plain function of the shared [`Ctx`] and the selected
//! clip's id, like the regions in `app`.

use std::rc::Rc;

use dioxus::prelude::*;

use super::super::Ctx;
use crate::editor::{self, AnimSlot};
use crate::effects;
use crate::monitor::{self, Cmd};

/// Redraw the timeline view and hand the monitor the new edit.
fn refresh(c: &Ctx) {
    let mut timeline = c.timeline;
    timeline.set(editor::view(&c.project.borrow()));
    if let Some(d) = monitor::timeline_data(&c.project.borrow()) {
        c.monitor.send(Cmd::SetTimeline(d));
    }
}

/// A callback taking the edited value.
type OnValue = Rc<dyn Fn(f64)>;

/// One labelled control: a slider and a number box that edit the same value.
/// `shown = value * mult` is what the person reads (e.g. 1.0 → 100 %).
#[allow(clippy::too_many_arguments)]
fn num_row(
    label: &str,
    id: &str,
    min: f64,
    max: f64,
    step: f64,
    value: f64,
    mult: f64,
    unit: &str,
    on_value: OnValue,
) -> Element {
    let shown = value * mult;
    let (smin, smax) = (min * mult, max * mult);
    let sstep = step * mult;
    let on_a = on_value.clone();
    let on_b = on_value;
    rsx! {
        div { class: "fx-param",
            label { class: "fx-param-label", "{label}" }
            div { class: "fx-slider-row",
                input {
                    class: "fx-range", id: "{id}-range", r#type: "range",
                    min: "{smin}", max: "{smax}", step: "{sstep}",
                    value: "{shown}",
                    oninput: move |e: FormEvent| {
                        if let Ok(v) = e.value().parse::<f64>() { on_a(v / mult); }
                    },
                }
                input {
                    class: "fx-num", id: "{id}-num", r#type: "number",
                    min: "{smin}", max: "{smax}", step: "{sstep}",
                    value: "{(shown * 100.0).round() / 100.0}",
                    onchange: move |e: FormEvent| {
                        if let Ok(v) = e.value().parse::<f64>() { on_b(v / mult); }
                    },
                }
                span { class: "fx-unit", "{unit}" }
            }
        }
    }
}

/// Picture → Transform: scale, position, rotation, opacity, flips, blend.
pub fn transform_section(c: &Ctx, id: &str) -> Element {
    let Some(p) = editor::props(&c.project.borrow(), id) else {
        return rsx! {};
    };
    let mk = |apply: fn(&mut concat_project::Editor, &str, f64) -> bool| -> OnValue {
        let c = c.clone();
        let id = id.to_string();
        Rc::new(move |v| {
            apply(&mut c.project.borrow_mut(), &id, v);
            refresh(&c);
        })
    };
    let on_scale = mk(|e, id, v| editor::set_transform(e, id, Some(v), None, None, None));
    let on_x = mk(|e, id, v| editor::set_transform(e, id, None, Some(v), None, None));
    let on_y = mk(|e, id, v| editor::set_transform(e, id, None, None, Some(v), None));
    let on_rot = mk(|e, id, v| editor::set_transform(e, id, None, None, None, Some(v)));
    let on_op = mk(editor::set_opacity);

    let flip = |horizontal: bool, on: bool| {
        let c = c.clone();
        let id = id.to_string();
        move |_| {
            editor::set_flip(&mut c.project.borrow_mut(), &id, horizontal, !on);
            refresh(&c);
        }
    };
    let blend_now = if p.blend.is_empty() {
        "normal".to_string()
    } else {
        p.blend.clone()
    };
    rsx! {
        div { class: "insp-sec tf-panel",
            div { class: "fx-head", "Transformar" }
            div { class: "tf-body",
                {num_row("Escala", "tf-scale", 0.1, 4.0, 0.01, p.scale, 100.0, "%", on_scale)}
                {num_row("Posición X", "tf-x", -1.0, 1.0, 0.005, p.offset_x, 100.0, "%", on_x)}
                {num_row("Posición Y", "tf-y", -1.0, 1.0, 0.005, p.offset_y, 100.0, "%", on_y)}
                {num_row("Rotación", "tf-rot", -180.0, 180.0, 1.0, p.rotation, 1.0, "°", on_rot)}
                {num_row("Opacidad", "tf-op", 0.0, 1.0, 0.01, p.opacity, 100.0, "%", on_op)}
                div { class: "fx-param",
                    label { class: "fx-param-label", "Voltear" }
                    div { class: "tf-flips",
                        button {
                            class: if p.flip_h { "tool tf-flip on" } else { "tool tf-flip" },
                            id: "tf-flip-h",
                            onclick: flip(true, p.flip_h),
                            "⇋ Horizontal"
                        }
                        button {
                            class: if p.flip_v { "tool tf-flip on" } else { "tool tf-flip" },
                            id: "tf-flip-v",
                            onclick: flip(false, p.flip_v),
                            "⇅ Vertical"
                        }
                    }
                }
                div { class: "fx-param",
                    label { class: "fx-param-label", "Mezcla" }
                    select {
                        class: "fx-select", id: "tf-blend",
                        value: "{blend_now}",
                        onchange: {
                            let c = c.clone();
                            let id = id.to_string();
                            move |e: FormEvent| {
                                editor::set_blend(&mut c.project.borrow_mut(), &id, &e.value());
                                refresh(&c);
                            }
                        },
                        for (val, lab) in BLENDS.iter() {
                            option { value: "{val}", "{lab}" }
                        }
                    }
                }
                button {
                    class: "tool tf-reset", id: "tf-reset",
                    onclick: {
                        let c = c.clone();
                        let id = id.to_string();
                        move |_| {
                            editor::reset_transform(&mut c.project.borrow_mut(), &id);
                            refresh(&c);
                        }
                    },
                    "↺ Restablecer"
                }
            }
        }
    }
}

const BLENDS: &[(&str, &str)] = &[
    ("normal", "Normal"),
    ("multiply", "Multiplicar"),
    ("screen", "Trama"),
    ("add", "Sumar"),
    ("overlay", "Superponer"),
    ("lighten", "Aclarar"),
    ("darken", "Oscurecer"),
    ("softLight", "Luz suave"),
    ("hardLight", "Luz fuerte"),
    ("difference", "Diferencia"),
];

/// Speed tab: rate, quick picks, pitch.
pub fn speed_tab(c: &Ctx, id: &str, has_sound: bool) -> Element {
    let Some(p) = editor::props(&c.project.borrow(), id) else {
        return rsx! {};
    };
    let set = {
        let c = c.clone();
        let id = id.to_string();
        move |v: f64| {
            editor::set_speed(&mut c.project.borrow_mut(), &id, v);
            refresh(&c);
        }
    };
    let on_speed: OnValue = {
        let set = set.clone();
        Rc::new(move |v| set(v))
    };
    let quick = |v: f64| {
        let set = set.clone();
        let on = (p.speed - v).abs() < 0.001;
        rsx! {
            button {
                class: if on { "tool sp-quick on" } else { "tool sp-quick" },
                id: "sp-q-{v}",
                onclick: move |_| set(v),
                "{v}×"
            }
        }
    };
    rsx! {
        div { class: "insp-sec sp-panel",
            div { class: "fx-head", "Velocidad" }
            div { class: "sp-body",
                {num_row("Velocidad", "sp", 0.25, 4.0, 0.05, p.speed.clamp(0.25, 4.0), 1.0, "×", on_speed)}
                div { class: "sp-quicks",
                    {quick(0.5)}
                    {quick(1.0)}
                    {quick(2.0)}
                }
                div { class: "sp-dur", id: "sp-dur", "Duración del clip: {p.duration:.2} s" }
                if has_sound {
                    div { class: "fx-param",
                        label { class: "fx-param-label", "Conservar el tono" }
                        button {
                            class: if p.preserve_pitch { "fx-toggle on" } else { "fx-toggle" },
                            id: "sp-pitch",
                            onclick: {
                                let c = c.clone();
                                let id = id.to_string();
                                let on = p.preserve_pitch;
                                move |_| {
                                    editor::set_preserve_pitch(&mut c.project.borrow_mut(), &id, !on);
                                    refresh(&c);
                                }
                            },
                            if p.preserve_pitch { "Sí" } else { "No" }
                        }
                    }
                }
            }
        }
    }
}

/// Animation tab: entry / exit / combined presets.
pub fn animation_tab(c: &Ctx, id: &str) -> Element {
    let mut slot_sig = c.anim_slot;
    let slot = slot_sig();
    let active = editor::current_anim(&c.project.borrow(), id, slot);
    let slots = [
        (AnimSlot::In, "Entrada", "an-in"),
        (AnimSlot::Out, "Salida", "an-out"),
        (AnimSlot::Loop, "Combinada", "an-loop"),
    ];
    let pick = |anim: Option<&'static str>| {
        let c = c.clone();
        let id = id.to_string();
        move |_| {
            {
                let mut ed = c.project.borrow_mut();
                match anim {
                    Some(a) => {
                        editor::set_anim(&mut ed, &id, slot, a);
                    }
                    None => {
                        editor::clear_anim(&mut ed, &id, slot);
                    }
                }
            }
            refresh(&c);
            // Show the result: jump to where the animation plays.
            let at = {
                let ed = c.project.borrow();
                ed.project().active().clip(&id).map(|cl| match slot {
                    AnimSlot::Out => cl.start + (cl.duration - 0.5).max(0.0),
                    _ => cl.start,
                })
            };
            if let Some(t) = at {
                c.monitor.send(Cmd::Seek(t));
            }
        }
    };
    rsx! {
        div { class: "insp-sec an-panel",
            div { class: "an-slots",
                for (s, lab, sid) in slots.iter() {
                    button {
                        key: "{sid}",
                        id: "{sid}",
                        class: if *s == slot { "an-slot on" } else { "an-slot" },
                        onclick: { let s = *s; move |_| slot_sig.set(s) },
                        "{lab}"
                    }
                }
            }
            div { class: "an-grid",
                button {
                    id: "an-none",
                    class: if active.is_none() { "an-card on" } else { "an-card" },
                    onclick: pick(None),
                    span { class: "an-ico", "∅" }
                    span { class: "an-name", "Ninguna" }
                }
                for (aid, lab) in editor::anim_list(slot).iter() {
                    button {
                        key: "{aid}",
                        id: "an-{aid}",
                        class: if active == Some(*aid) { "an-card on" } else { "an-card" },
                        onclick: pick(Some(*aid)),
                        span { class: "an-ico", "{anim_icon(aid)}" }
                        span { class: "an-name", "{lab}" }
                    }
                }
            }
        }
    }
}

fn anim_icon(id: &str) -> &'static str {
    match id {
        "fade" | "blink" => "◐",
        "zoom-in" | "slow-zoom-in" => "⤢",
        "zoom-out" | "slow-zoom-out" => "⤡",
        "slide-left" => "→",
        "slide-right" => "←",
        "slide-up" => "↓",
        "slide-down" => "↑",
        "spin" | "sway" => "⟳",
        "pulse" => "♥",
        _ => "✦",
    }
}

/// Adjust tab: the engine's colour panel, grouped as it groups it.
pub fn adjust_tab(c: &Ctx, id: &str) -> Element {
    let values = effects::adjust_values(&c.project.borrow(), id);
    let dirty = values
        .iter()
        .any(|v| (v.current - v.info.default).abs() > 1e-9);
    rsx! {
        div { class: "insp-sec adj-panel",
            for (group, glabel) in effects::ADJUST_GROUPS.iter() {
                div { class: "adj-group", key: "{group}",
                    div { class: "fx-head", "{glabel}" }
                    for pv in values.iter().filter(|v| v.info.group == *group) {
                        {
                            let key = pv.info.key.clone();
                            let c2 = c.clone();
                            let cid = id.to_string();
                            let on: OnValue = Rc::new(move |v| {
                                effects::set_adjust(&mut c2.project.borrow_mut(), &cid, &key, v);
                                refresh(&c2);
                            });
                            let label = effects::adjust_label(&pv.info.key, &pv.info.label);
                            let rid = format!("adj-{}", pv.info.key);
                            rsx! {
                                div { key: "{pv.info.key}",
                                    {num_row(&label, &rid, pv.info.min, pv.info.max,
                                        if pv.info.step > 0.0 { pv.info.step } else { 1.0 },
                                        pv.current, 1.0, &pv.info.unit, on)}
                                }
                            }
                        }
                    }
                }
            }
            button {
                class: "tool adj-reset", id: "adj-reset",
                disabled: !dirty,
                onclick: {
                    let c = c.clone();
                    let id = id.to_string();
                    move |_| {
                        effects::reset_adjust(&mut c.project.borrow_mut(), &id);
                        refresh(&c);
                    }
                },
                "↺ Restablecer"
            }
        }
    }
}
