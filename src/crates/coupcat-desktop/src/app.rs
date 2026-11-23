// SPDX-License-Identifier: AGPL-3.0-or-later
//
//! The root component: it owns the state, starts the background tasks that
//! feed it, and composes the interface regions from `ui`.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use dioxus::prelude::*;

use crate::editor;
use crate::monitor::{self, Cmd, Monitor};
use crate::ui::banners::banners;
use crate::ui::files::{Files, Pending};
use crate::ui::inspector::inspector_panel;
use crate::ui::resources::{ResTab, resources_panel};
use crate::ui::stage::stage_panel;
use crate::ui::timeline::{Drag, TL_HEIGHT_MAX, TL_HEIGHT_MIN, timeline_panel};
use crate::ui::topbar::topbar;
use crate::ui::{Ctx, STYLE};
use crate::{captions, cutout, effects, export, peaks, project_file, thumbs};

pub fn app() -> Element {
    let monitor = use_hook(Monitor::spawn);
    let project = use_hook(|| Rc::new(RefCell::new(concat_project::Editor::new())));

    // Composed-monitor snapshot, mirrored from the monitor thread.
    let mut frame = use_signal(|| None::<String>);
    let mut pos = use_signal(|| 0.0_f64);
    let mut dur = use_signal(|| 0.0_f64);
    let mut playing = use_signal(|| false);
    // True while the user is dragging the timeline playhead to scrub.
    let mut scrubbing = use_signal(|| false);
    // Height of the timeline panel in pixels (drag its top edge to change it),
    // and the in-progress resize: (mouse y at the start, height at the start).
    let mut tl_height = use_signal(|| None::<f64>);
    // The window's height, kept up to date from the webview (see below).
    let mut win_h = use_signal(|| 800.0_f64);
    use_future(move || async move {
        let mut ev = dioxus::document::eval(
            "const send = () => dioxus.send(window.innerHeight); window.addEventListener('resize', send); send();",
        );
        while let Ok(h) = ev.recv::<f64>().await {
            win_h.set(h);
        }
    });
    let mut resizing = use_signal(|| None::<(f64, f64)>);
    // Per-lane state the engine has no notion of (yet), keyed by track id:
    // locked lanes refuse edits, and lanes can carry a custom name. Not saved
    // with the project until project files exist.
    let locked_tracks = use_signal(HashSet::<String>::new);
    let track_names = use_signal(HashMap::<String, String>::new);
    // The lane whose name is being edited, if any.
    let renaming = use_signal(|| None::<String>);
    let mut loading = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);

    // The edit, derived from the Editor after each command. Seeded from the
    // Editor itself so a fresh project already shows (and can drop onto) its
    // lanes, instead of an empty view with no track ids.
    let mut timeline = use_signal(|| editor::view(&project.borrow()));
    // The resources panel: active tab, its category (0 is "Todo") and the search.
    let res_tab = use_signal(|| ResTab::Media);
    let mut pps = use_signal(|| crate::ui::timeline::PPS_DEFAULT);
    let snap = use_signal(|| true);
    let looping = use_signal(|| false);
    let stage_full = use_signal(|| false);
    // Esc leaves the full-screen player (the button does the work).
    use_future(|| async move {
        let _ = dioxus::document::eval(
            r#"document.addEventListener('keydown', function(e){
                if (e.key === 'Escape' && document.querySelector('main.stage.full')) {
                    var b = document.getElementById('btn-fullscreen');
                    if (b) b.click();
                }
            });"#,
        );
    });
    // Ctrl + wheel over the timeline zooms it around the cursor. Done in script
    // so the page's own zoom can be refused; the scale lives here.
    use_future(move || async move {
        let mut ev = dioxus::document::eval(
            r#"document.addEventListener('wheel', function(e){
                if (!e.ctrlKey) return;
                var sc = e.target.closest && e.target.closest('.tl-scroll');
                if (!sc) return;
                e.preventDefault();
                var r = sc.getBoundingClientRect();
                dioxus.send([e.deltaY, e.clientX - r.left, sc.scrollLeft]);
            }, { passive: false });"#,
        );
        while let Ok((dy, x, scroll)) = ev.recv::<(f64, f64, f64)>().await {
            let old = pps();
            let new = crate::ui::timeline::clamp_pps(old * (-dy * 0.0015).exp());
            if (new - old).abs() > 1e-9 {
                let t = (scroll + x) / old;
                pps.set(new);
                let _ = dioxus::document::eval(&crate::ui::timeline::zoom_script(t, old, new, Some(x)));
            }
        }
    });
    let insp_tab = use_signal(|| crate::ui::inspector::InspTab::Video);
    let anim_slot = use_signal(|| editor::AnimSlot::In);
    let res_cat = use_signal(|| 0usize);
    let res_search = use_signal(String::new);
    // The imported-media library (bin).
    let media = use_signal(Vec::<editor::MediaView>::new);
    // Media id being dragged from the library onto the timeline.
    let mut dragging_media = use_signal(|| None::<String>);
    let mut dragging_fx = use_signal(|| None::<String>);

    // Spacebar toggles play/pause (unless typing in a field). Done in JS so it
    // works wherever focus is; it just clicks the transport button.
    use_future(|| async move {
        let _ = dioxus::document::eval(
            r#"document.addEventListener('keydown', function(e){
if ((e.ctrlKey || e.metaKey) && !e.altKey) {
                    var kz = (e.key || '').toLowerCase();
                    if (kz === 'z' || kz === 'y') {
                        var a = document.activeElement;
                        var tg = a ? a.tagName : '';
                        var ty = a && a.type ? a.type : '';
                        var typing = tg === 'TEXTAREA' ||
                            (tg === 'INPUT' && ['text','search','number','tel','url','email','password'].indexOf(ty) >= 0);
                        if (!typing) {
                            e.preventDefault();
                            var redo = kz === 'y' || (kz === 'z' && e.shiftKey);
                            var ub = document.getElementById(redo ? 'btn-redo' : 'btn-undo');
                            if (ub && !ub.disabled) ub.click();
                            return;
                        }
                    }
                }
                if ((e.ctrlKey || e.metaKey) && !e.altKey && !e.shiftKey) {
                    var k = (e.key || '').toLowerCase();
                    var id = k === 's' ? 'btn-save' : k === 'o' ? 'btn-open' : k === 'n' ? 'btn-new' : null;
                    if (id) {
                        e.preventDefault();
                        var btn = document.getElementById(id);
                        if (btn) btn.click();
                        return;
                    }
                }
                if (e.code === 'Space') {
                    var t = document.activeElement;
                    var tag = t ? t.tagName : '';
                    if (tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT') return;
                    e.preventDefault();
                    var b = document.querySelector('.ctrl');
                    if (b && !b.disabled) b.click();
                }
            });
            /* The track headers have no scrollbar: they follow the lanes' vertical
               scroll, and the wheel over them scrolls the lanes. */
            document.addEventListener('scroll', function(e){
                var t = e.target;
                if (t && t.classList && t.classList.contains('tl-scroll')) {
                    var h = document.querySelector('.tl-heads');
                    if (h) h.scrollTop = t.scrollTop;
                }
            }, true);
            document.addEventListener('wheel', function(e){
                var t = e.target;
                if (t && t.closest && t.closest('.tl-heads')) {
                    var s = document.querySelector('.tl-scroll');
                    if (s) s.scrollTop += e.deltaY;
                }
            }, { passive: true });"#,
        );
    });

    // In-progress clip drag (move / trim).
    let drag = use_signal(|| None::<Drag>);
    // The lane index under the cursor during a drag (for moving across tracks).
    let drop_lane = use_signal(|| None::<usize>);
    // Bumped when background waveform peaks arrive, to re-render the timeline.
    let mut wave_rev = use_signal(|| 0u64);
    // Bumped when a background library thumbnail lands, to re-render the bin.
    let mut thumb_rev = use_signal(|| 0u64);
    use_future(move || async move {
        loop {
            if peaks::take_dirty() {
                let v = wave_rev();
                wave_rev.set(v + 1);
            }
            if thumbs::take_dirty() {
                let v = thumb_rev();
                thumb_rev.set(v + 1);
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    });
    // The selected clip (for the effects panel).
    let selected = use_signal(|| None::<String>);
    // The built-in effect catalogue, listed once.
    let fx_catalogue = use_hook(|| Rc::new(effects::catalogue()));

    // Export state.
    let mut export_handle = use_signal(|| None::<export::Handle>);
    let mut exporting = use_signal(|| false);
    let mut export_stage = use_signal(String::new);
    let mut export_pct = use_signal(|| 0.0_f64);
    let mut export_msg = use_signal(|| None::<String>);

    // Background-removal (cutout) state.
    let mut cutout_job = use_signal(|| None::<cutout::Job>);
    let mut cutting = use_signal(|| false);
    let mut cut_done = use_signal(|| 0usize);
    let mut cut_total = use_signal(|| 0usize);

    // Auto-captions state.
    let mut cc_job = use_signal(|| None::<captions::Job>);
    let mut generating_cc = use_signal(|| false);
    let mut cc_stage = use_signal(String::new);

    // Poll a running caption job; when it finishes, add the text clips.
    {
        let project = project.clone();
        let monitor = monitor.clone();
        use_future(move || {
            let project = project.clone();
            let monitor = monitor.clone();
            async move {
                loop {
                    if let Some(job) = cc_job() {
                        let p = job.shared.lock().unwrap().clone();
                        cc_stage.set(p.stage.clone());
                        if !p.active {
                            match p.result {
                                Some(Ok(caps)) => {
                                    let tuples: Vec<(f64, f64, String)> = caps
                                        .iter()
                                        .map(|c| (c.start, c.end, c.text.clone()))
                                        .collect();
                                    let n =
                                        editor::add_captions(&mut project.borrow_mut(), &tuples);
                                    timeline.set(editor::view(&project.borrow()));
                                    if let Some(d) = monitor::timeline_data(&project.borrow()) {
                                        monitor.send(Cmd::SetTimeline(d));
                                    }
                                    if n == 0 {
                                        error
                                            .set(Some("No se detectó voz en el clip.".to_string()));
                                    }
                                }
                                Some(Err(e)) => error.set(Some(format!("Subtítulos: {e}"))),
                                None => {}
                            }
                            generating_cc.set(false);
                            cc_job.set(None);
                        }
                    }
                    tokio::time::sleep(Duration::from_millis(200)).await;
                }
            }
        });
    }

    // Poll a running cutout; when it finishes, turn the clip into a cutout.
    {
        let project = project.clone();
        let monitor = monitor.clone();
        use_future(move || {
            let project = project.clone();
            let monitor = monitor.clone();
            async move {
                loop {
                    if let Some(job) = cutout_job() {
                        let p = job.shared.lock().unwrap().clone();
                        cut_done.set(p.done);
                        cut_total.set(p.total);
                        if !p.active {
                            match p.result {
                                Some(Ok(clip_id)) => {
                                    editor::set_cutout(&mut project.borrow_mut(), clip_id, true);
                                    timeline.set(editor::view(&project.borrow()));
                                    if let Some(d) = monitor::timeline_data(&project.borrow()) {
                                        monitor.send(Cmd::SetTimeline(d));
                                    }
                                }
                                Some(Err(e)) => error.set(Some(format!("Quitar fondo: {e}"))),
                                None => {}
                            }
                            cutting.set(false);
                            cutout_job.set(None);
                        }
                    }
                    tokio::time::sleep(Duration::from_millis(150)).await;
                }
            }
        });
    }

    // Poll a running export for progress and completion.
    use_future(move || async move {
        loop {
            if let Some(handle) = export_handle() {
                let p = handle.shared.lock().unwrap().clone();
                export_stage.set(p.stage.clone());
                export_pct.set(if p.total > 0 {
                    (p.frame as f64 / p.total as f64 * 100.0).clamp(0.0, 100.0)
                } else {
                    0.0
                });
                if !p.active {
                    if let Some(result) = p.done {
                        export_msg.set(Some(match result {
                            Ok(path) => format!("✓ Exportado: {path}"),
                            Err(e) => format!("✗ La exportación falló: {e}"),
                        }));
                    }
                    exporting.set(false);
                    export_handle.set(None);
                }
            }
            tokio::time::sleep(Duration::from_millis(150)).await;
        }
    });

    {
        let shared = monitor.shared.clone();
        use_future(move || {
            let shared = shared.clone();
            async move {
                let mut last_gen = u64::MAX;
                loop {
                    {
                        let s = shared.lock().unwrap();
                        if s.generation != last_gen {
                            last_gen = s.generation;
                            frame.set(s.frame.clone());
                            pos.set(s.pos);
                            dur.set(s.dur);
                            playing.set(s.playing);
                            loading.set(s.loading);
                            error.set(s.error.clone());
                        }
                    }
                    tokio::time::sleep(Duration::from_millis(33)).await;
                }
            }
        });
    }

    // ---- Project files: save / open / recents / unsaved changes / recovery ----
    let project_path = use_signal(|| None::<PathBuf>);
    let saved_snap = use_signal(|| project_file::snapshot(&project.borrow()));
    let file_msg = use_signal(|| None::<(bool, String)>);
    // Unsaved work from a previous session, waiting to be restored or dropped.
    let recovery_offer = use_signal(project_file::has_recovery);
    let pending = use_signal(|| None::<Pending>);
    let busy = use_signal(|| false);
    let menu_open = use_signal(|| false);
    // Local time of the last save or autosave, shown in the top bar.
    let saved_at = use_signal(|| None::<String>);
    let shortcuts_open = use_signal(|| false);
    // True when the edit differs from the saved one. Recomputed only when the
    // timeline changes (not on every playback tick).
    let dirty = use_memo({
        let project = project.clone();
        move || {
            let _ = timeline();
            project_file::snapshot(&project.borrow()) != saved_snap()
        }
    });
    let files = Files {
        project: project.clone(),
        monitor: monitor.clone(),
        media,
        timeline,
        selected,
        project_path,
        saved_snap,
        file_msg,
        recovery_offer,
        pending,
        busy,
        locked_tracks,
        track_names,
        renaming,
        saved_at,
    };
    // Autosave: while there are unsaved changes, keep a recovery copy; when
    // everything is saved, drop it. Paused while an old copy is on offer, so
    // it is never overwritten before the person decides.
    let autosave_project = project.clone();
    let autosave_files = files.clone();
    use_future(move || {
        let project = autosave_project.clone();
        let files = autosave_files.clone();
        async move {
            loop {
                tokio::time::sleep(Duration::from_secs(30)).await;
                if recovery_offer() {
                    continue;
                }
                if dirty() {
                    if project_file::write_recovery(&project.borrow()).is_ok() {
                        files.stamp_saved();
                    }
                } else {
                    project_file::clear_recovery();
                }
            }
        }
    });
    // The window title names the project and marks unsaved changes.
    use_effect(move || {
        let name = project_path()
            .map(|p| project_file::name_of(&p))
            .unwrap_or_else(|| "Sin título".to_string());
        let mark = if dirty() { "● " } else { "" };
        dioxus::desktop::window().set_title(&format!("{mark}{name} — CoupCat"));
    });

    // Everything the interface regions share.
    let c = Ctx {
        project: project.clone(),
        monitor: monitor.clone(),
        fx_catalogue: fx_catalogue.clone(),
        files: files.clone(),
        frame,
        pos,
        dur,
        playing,
        scrubbing,
        tl_height,
        win_h,
        resizing,
        locked_tracks,
        track_names,
        renaming,
        loading,
        error,
        timeline,
        media,
        dragging_media,
        dragging_fx,
        drag,
        drop_lane,
        wave_rev,
        thumb_rev,
        selected,
        export_handle,
        exporting,
        export_stage,
        export_pct,
        export_msg,
        cutout_job,
        cutting,
        cut_done,
        cut_total,
        cc_job,
        generating_cc,
        cc_stage,
        project_path,
        file_msg,
        recovery_offer,
        pending,
        menu_open,
        dirty,
        saved_at,
        shortcuts_open,
        res_tab,
        insp_tab,
        pps,
        snap,
        looping,
        stage_full,
        anim_slot,
        res_cat,
        res_search,
    };

    rsx! {
        style { {STYLE} }
        div { class: if dragging_fx().is_some() { "app fx-drag" } else { "app" },
            onmousemove: move |e: MouseEvent| {
                if let Some((y0, h0)) = resizing() {
                    // The button was let go outside the window (no mouseup reached
                    // us): stop, instead of leaving the panel stuck to the pointer.
                    // (Not `onmouseleave`: it fires on leaving any child element.)
                    if !e.held_buttons().contains(dioxus::html::input_data::MouseButton::Primary) {
                        resizing.set(None);
                        return;
                    }
                    // The panel sits at the bottom, so dragging up makes it taller.
                    let h = (h0 + (y0 - e.page_coordinates().y)).clamp(TL_HEIGHT_MIN, TL_HEIGHT_MAX);
                    tl_height.set(Some(h));
                }
            },
            onmouseup: move |_| {
                if dragging_media().is_some() {
                    dragging_media.set(None);
                }
                if dragging_fx().is_some() {
                    dragging_fx.set(None);
                }
                if scrubbing() {
                    scrubbing.set(false);
                }
                if resizing().is_some() {
                    resizing.set(None);
                }
            },
            {topbar(&c)}
            {banners(&c)}
            div { class: "middle",
                {resources_panel(&c)}
                {stage_panel(&c)}
                {inspector_panel(&c)}
            }
            {timeline_panel(&c)}
        }
    }
}
