// SPDX-License-Identifier: AGPL-3.0-or-later
//
//! CoupCat desktop shell (M1 → M2).
//!
//! A Dioxus window over the Concat engine. The edit is a
//! `concat_project::Editor` ([`editor`]); preview and playback run on the
//! [`monitor`] thread. The UI sends editing commands and playback commands and
//! renders the timeline and the current frame.

#![windows_subsystem = "windows"]

mod audio;
mod captions;
mod cutout;
mod editor;
mod effects;
mod export;
mod monitor;
mod peaks;
mod text;

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use dioxus::prelude::*;

use editor::TimelineView;
use monitor::{Cmd, Monitor, fmt_time};

/// Pixels per second on the timeline.
const PPS: f64 = 50.0;

/// Built-in transition kinds the engine resolves on a cut: `(id, label)`.
const TRANSITIONS: &[(&str, &str)] = &[
    ("cross-fade", "Fundido cruzado"),
    ("fade-black", "Fundido a negro"),
    ("fade-white", "Fundido a blanco"),
    ("push", "Empujar"),
    ("zoom", "Zoom"),
    ("wipe-left", "Barrido ←"),
    ("wipe-right", "Barrido →"),
];

/// What a timeline drag is doing to a clip.
#[derive(Clone, Copy, PartialEq)]
enum DragMode {
    /// Slide the whole clip along its lane.
    Move,
    /// Drag the left edge (trim the head / in-point).
    TrimStart,
    /// Drag the right edge (trim the tail).
    TrimEnd,
}

/// An in-progress drag of a clip on the timeline.
#[derive(Clone)]
struct Drag {
    mode: DragMode,
    clip_id: String,
    track_id: String,
    orig_start: f64,
    orig_dur: f64,
    /// Mouse page-x where the drag began, and where it is now (pixels).
    start_px: f64,
    cur_px: f64,
}

/// The window/taskbar icon, decoded from the embedded PNG.
fn load_icon() -> Option<dioxus::desktop::tao::window::Icon> {
    let data = include_bytes!("../assets/icon-64.png");
    let decoder = png::Decoder::new(std::io::Cursor::new(&data[..]));
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf[..info.buffer_size()].to_vec(),
        png::ColorType::Rgb => buf[..info.buffer_size()]
            .chunks_exact(3)
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        _ => return None,
    };
    dioxus::desktop::tao::window::Icon::from_rgba(rgba, info.width, info.height).ok()
}

fn main() {
    concat_media::init();
    // Point ONNX Runtime (ort load-dynamic) at the onnxruntime.dll beside the
    // exe, for background removal.
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dll) = exe.parent().map(|d| d.join("onnxruntime.dll")) {
            if dll.exists() {
                unsafe { std::env::set_var("ORT_DYLIB_PATH", dll) };
            }
        }
    }
    use dioxus::desktop::tao::dpi::LogicalSize;
    use dioxus::desktop::{Config, WindowBuilder};
    let window = WindowBuilder::new()
        .with_title("CoupCat")
        .with_window_icon(load_icon())
        .with_inner_size(LogicalSize::new(1280.0, 800.0))
        .with_min_inner_size(LogicalSize::new(900.0, 600.0))
        // Dioxus' default desktop window comes up always-on-top, which makes it
        // sit over every other window — even the file-open dialog. Turn it off.
        .with_always_on_top(false);
    dioxus::LaunchBuilder::desktop()
        .with_cfg(Config::new().with_window(window))
        .launch(app);
}

fn app() -> Element {
    let monitor = use_hook(Monitor::spawn);
    let project = use_hook(|| Rc::new(RefCell::new(concat_project::Editor::new())));

    // Composed-monitor snapshot, mirrored from the monitor thread.
    let mut frame = use_signal(|| None::<String>);
    let mut pos = use_signal(|| 0.0_f64);
    let mut dur = use_signal(|| 0.0_f64);
    let mut playing = use_signal(|| false);
    // True while the user is dragging the timeline playhead to scrub.
    let mut scrubbing = use_signal(|| false);
    let mut loading = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);

    // The edit, derived from the Editor after each command. Seeded from the
    // Editor itself so a fresh project already shows (and can drop onto) its
    // lanes, instead of an empty view with no track ids.
    let mut timeline = use_signal(|| editor::view(&project.borrow()));
    // The imported-media library (bin).
    let mut media = use_signal(Vec::<editor::MediaView>::new);
    // Media id being dragged from the library onto the timeline.
    let mut dragging_media = use_signal(|| None::<String>);

    // Spacebar toggles play/pause (unless typing in a field). Done in JS so it
    // works wherever focus is; it just clicks the transport button.
    use_future(|| async move {
        let _ = dioxus::document::eval(
            r#"document.addEventListener('keydown', function(e){
                if (e.code === 'Space') {
                    var t = document.activeElement;
                    var tag = t ? t.tagName : '';
                    if (tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT') return;
                    e.preventDefault();
                    var b = document.querySelector('.ctrl');
                    if (b && !b.disabled) b.click();
                }
            });"#,
        );
    });

    // In-progress clip drag (move / trim).
    let mut drag = use_signal(|| None::<Drag>);
    // The lane index under the cursor during a drag (for moving across tracks).
    let mut drop_lane = use_signal(|| None::<usize>);
    // Bumped when background waveform peaks arrive, to re-render the timeline.
    let mut wave_rev = use_signal(|| 0u64);
    use_future(move || async move {
        loop {
            if peaks::take_dirty() {
                let v = wave_rev();
                wave_rev.set(v + 1);
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    });
    // The selected clip (for the effects panel).
    let mut selected = use_signal(|| None::<String>);
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
    // Drop a library item onto a lane at a timeline position.
    let on_drop_media = {
        let project = project.clone();
        let monitor = monitor.clone();
        move |track_id: String, start: f64| {
            let Some(media_id) = dragging_media() else {
                return;
            };
            dragging_media.set(None);
            if let Some(cid) =
                editor::add_clip_on(&mut project.borrow_mut(), &media_id, &track_id, start)
            {
                selected.set(Some(cid));
            }
            timeline.set(editor::view(&project.borrow()));
            if let Some(d) = monitor::timeline_data(&project.borrow()) {
                monitor.send(Cmd::SetTimeline(d));
            }
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

    let on_seek = {
        let monitor = monitor.clone();
        move |e: FormEvent| {
            if let Ok(v) = e.value().parse::<f64>() {
                monitor.send(Cmd::Seek(v));
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
            editor::split_at(&mut project.borrow_mut(), pos());
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
            editor::delete_at(&mut project.borrow_mut(), pos());
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

    let on_captions = {
        let project = project.clone();
        move |_| {
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
    let on_drag_move = move |e: MouseEvent| {
        let mut d = drag();
        if let Some(inner) = d.as_mut() {
            inner.cur_px = e.page_coordinates().x;
            drag.set(d);
        }
    };
    // A fresh end-of-drag handler each time (used on both mouseup and
    // mouseleave, and a closure capturing !Copy state can't be reused).
    let mut end_drag = {
        let project = project.clone();
        let monitor = monitor.clone();
        move || {
            let project = project.clone();
            let monitor = monitor.clone();
            move |_: MouseEvent| {
                let Some(d) = drag() else { return };
                drag.set(None);
                // Where to drop: the lane under the cursor, else the clip's own.
                // Resolve it (and clear drop_lane) BEFORE the minimal-move guard,
                // so a vertical drag to another lane is never dropped on the floor.
                let target_track = drop_lane()
                    .and_then(|li| timeline().track_ids.get(li).cloned())
                    .unwrap_or_else(|| d.track_id.clone());
                drop_lane.set(None);
                let dpx = d.cur_px - d.start_px;
                // Changing lanes counts as a move even with no horizontal travel.
                let changed_track = d.mode == DragMode::Move && target_track != d.track_id;
                if dpx.abs() < 2.0 && !changed_track {
                    return;
                }
                let dsec = dpx / PPS;
                {
                    let mut ed = project.borrow_mut();
                    match d.mode {
                        DragMode::Move => {
                            editor::move_clip(
                                &mut ed,
                                d.clip_id,
                                d.orig_start + dsec,
                                target_track,
                            );
                        }
                        DragMode::TrimStart => {
                            editor::trim(&mut ed, d.clip_id, true, dsec);
                        }
                        DragMode::TrimEnd => {
                            editor::trim(&mut ed, d.clip_id, false, dsec);
                        }
                    }
                }
                timeline.set(editor::view(&project.borrow()));
                if let Some(data) = monitor::timeline_data(&project.borrow()) {
                    monitor.send(Cmd::SetTimeline(data));
                }
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
    let drag_now = drag();
    let drop_now = drop_lane();
    let media_drag = dragging_media().is_some();
    let _wave_rev = wave_rev(); // subscribe: re-render when peaks arrive
    let selected_now = selected();
    let has_clip = tl.clip_count > 0;
    let fx_cards = selected_now
        .as_ref()
        .map(|id| effects::effect_cards(&project.borrow(), id))
        .unwrap_or_default();
    let text_style = selected_now
        .as_ref()
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
    let selected_has_media = selected_now
        .as_ref()
        .and_then(|id| editor::media_path(&project.borrow(), id))
        .is_some();
    let selected_cutout = selected_now
        .as_ref()
        .map(|id| editor::has_cutout(&project.borrow(), id))
        .unwrap_or(false);
    let audio = selected_now
        .as_ref()
        .and_then(|id| editor::audio_info(&project.borrow(), id));
    let selected_name = selected_now.as_ref().and_then(|id| {
        tl.clips
            .iter()
            .find(|c| &c.id == id)
            .map(|c| c.name.clone())
    });
    let tl_seconds = tl.duration.max(dur()).max(10.0);
    let tl_width = tl_seconds * PPS + 40.0;
    let playhead_x = pos() * PPS;

    rsx! {
        style { {STYLE} }
        div { class: "app",
            onmouseup: move |_| {
                if dragging_media().is_some() {
                    dragging_media.set(None);
                }
                if scrubbing() {
                    scrubbing.set(false);
                }
            },
            header { class: "topbar",
                div { class: "brand",
                    span { class: "logo", "🐾" }
                    span { class: "name", "CoupCat" }
                    span { class: "milestone", "M2" }
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

            div { class: "middle",
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
                                    div { class: "lib-kind k-{m.kind}", "{kind_icon(m.kind)}" }
                                    div { class: "lib-meta",
                                        span { class: "lib-name", "{m.name}" }
                                        span { class: "lib-dur", "{dur_label(m.duration)}" }
                                    }
                                }
                            }
                        }
                    }
                }
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
                aside { class: "fx",
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
                                if captions::model_ready() {
                                    if generating_cc() {
                                        div { class: "cut-progress", "Subtítulos: {cc_stage()}…" }
                                    } else {
                                        button { class: "tool fx-cc", onclick: on_captions, "💬 Generar subtítulos" }
                                    }
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
                    if selected_now.is_some() {
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
                    div { class: "fx-head", "Efectos" }
                    match selected_now.as_ref() {
                        None => rsx! {
                            div { class: "fx-empty", "Selecciona un clip en el timeline para añadir filtros." }
                        },
                        Some(_) => rsx! {
                            div { class: "fx-sel", "Clip: {selected_name.clone().unwrap_or_default()}" }
                            div { class: "fx-applied",
                                if fx_cards.is_empty() {
                                    div { class: "fx-none", "Sin efectos aún" }
                                } else {
                                    for card in fx_cards.iter() {
                                        div { class: "fx-card", key: "{card.index}",
                                            div { class: "fx-card-head",
                                                span { class: "fx-card-name", "{card.name}" }
                                                button {
                                                    class: "fx-chip-x",
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
                                            if card.params.is_empty() {
                                                div { class: "fx-noparams", "Sin ajustes" }
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
                                                            label { class: "fx-param-label", "{p.label}" }
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
                            div { class: "fx-add-title", "Añadir efecto ({fx_catalogue.len()})" }
                            div { class: "fx-list",
                                for fx in fx_catalogue.iter() {
                                    button {
                                        key: "{fx.id}",
                                        class: "fx-item",
                                        onclick: {
                                            let project = project.clone();
                                            let monitor = monitor.clone();
                                            let sel = selected_now.clone();
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
                                        "{fx.name}"
                                    }
                                }
                            }
                        },
                    }
                }
            }

            div { class: "scrub-row",
                input {
                    class: "scrubber", r#type: "range", min: "0",
                    max: "{dur():.3}", step: "0.01", value: "{pos():.3}",
                    disabled: !has_clip, oninput: on_seek,
                }
            }

            footer { class: "timeline",
                div {
                    class: "tl-scroll",
                    onmousemove: on_drag_move,
                    onmouseup: end_drag(),
                    onmouseleave: end_drag(),
                    div {
                        class: if scrubbing() { "lanes scrubbing" } else { "lanes" },
                        style: "width: {tl_width}px;",
                        div {
                            class: "tl-ruler",
                            style: "width: {tl_width}px;",
                            onmousedown: {
                                let monitor = monitor.clone();
                                move |e: MouseEvent| {
                                    e.stop_propagation();
                                    scrubbing.set(true);
                                    monitor.send(Cmd::Seek((e.element_coordinates().x / PPS).max(0.0)));
                                }
                            },
                            onmousemove: {
                                let monitor = monitor.clone();
                                move |e: MouseEvent| {
                                    if scrubbing() {
                                        monitor.send(Cmd::Seek((e.element_coordinates().x / PPS).max(0.0)));
                                    }
                                }
                            },
                            for i in 0..=(tl_seconds.ceil() as i64) {
                                div {
                                    key: "tick-{i}",
                                    class: if i % 5 == 0 { "tick major" } else { "tick" },
                                    style: "left: {i as f64 * PPS}px;",
                                    if i % 5 == 0 {
                                        span { class: "tick-label", "{fmt_time(i as f64)}" }
                                    }
                                }
                            }
                        }
                        for lane in 0..tl.track_count.max(1) {
                            div {
                                class: if (drop_now == Some(lane) && drag_now.is_some()) || media_drag { "lane drop" } else { "lane" },
                                onmousedown: {
                                    let monitor = monitor.clone();
                                    move |e: MouseEvent| {
                                        scrubbing.set(true);
                                        monitor.send(Cmd::Seek((e.element_coordinates().x / PPS).max(0.0)));
                                    }
                                },
                                onmousemove: {
                                    let monitor = monitor.clone();
                                    move |e: MouseEvent| {
                                        if scrubbing() {
                                            monitor.send(Cmd::Seek((e.element_coordinates().x / PPS).max(0.0)));
                                        }
                                    }
                                },
                                onmouseenter: move |_| {
                                    if drag().is_some() {
                                        drop_lane.set(Some(lane));
                                    }
                                },
                                onmouseup: {
                                    let mut on_drop_media = on_drop_media.clone();
                                    let track_ids = tl.track_ids.clone();
                                    move |e: MouseEvent| {
                                        if let Some(tid) = track_ids.get(lane).cloned() {
                                            on_drop_media(tid, (e.element_coordinates().x / PPS).max(0.0));
                                        }
                                    }
                                },
                                {tl.clips.iter().filter(move |c| c.track == lane).map(|c| {
                                            let mut left = c.start * PPS;
                                            let mut width = (c.duration * PPS).max(3.0);
                                            if let Some(d) = drag_now.as_ref() {
                                                if d.clip_id == c.id {
                                                    let dpx = d.cur_px - d.start_px;
                                                    match d.mode {
                                                        DragMode::Move => left += dpx,
                                                        DragMode::TrimStart => { left += dpx; width -= dpx; }
                                                        DragMode::TrimEnd => width += dpx,
                                                    }
                                                }
                                            }
                                            let left = left.max(0.0);
                                            let width = width.max(3.0);
                                            let sel_cls = if selected_now.as_deref() == Some(c.id.as_str()) {
                                                " sel"
                                            } else {
                                                ""
                                            };
                                            let mk = |mode: DragMode| {
                                                let cid = c.id.clone();
                                                let tid = c.track_id.clone();
                                                let (os, od) = (c.start, c.duration);
                                                move |e: MouseEvent| {
                                                    e.stop_propagation();
                                                    selected.set(Some(cid.clone()));
                                                    let x = e.page_coordinates().x;
                                                    drag.set(Some(Drag {
                                                        mode,
                                                        clip_id: cid.clone(),
                                                        track_id: tid.clone(),
                                                        orig_start: os,
                                                        orig_dur: od,
                                                        start_px: x,
                                                        cur_px: x,
                                                    }));
                                                }
                                            };
                                            let wave = if c.kind == "audio" {
                                                c.media_path
                                                    .as_ref()
                                                    .and_then(|p| peaks::get(p))
                                                    .map(|pk| peaks::waveform_svg(&pk, c.source_start, c.duration, width))
                                            } else {
                                                None
                                            };
                                            rsx! {
                                                div {
                                                    key: "{c.id}",
                                                    class: "clip k-{c.kind}{sel_cls}",
                                                    style: "left: {left}px; width: {width}px;",
                                                    title: "{c.name}",
                                                    onmousedown: mk(DragMode::Move),
                                                    div { class: "handle hleft", onmousedown: mk(DragMode::TrimStart) }
                                                    if let Some(w) = wave.as_ref() {
                                                        div { class: "wave-wrap", dangerous_inner_html: "{w}" }
                                                    }
                                                    if c.has_transition {
                                                        div { class: "clip-tr", title: "Transición en el corte", "⇄" }
                                                    }
                                                    span { class: "clip-name", "{c.name}" }
                                                    div { class: "handle hright", onmousedown: mk(DragMode::TrimEnd) }
                                                }
                                            }
                                        })}
                                    }
                                }
                        if has_clip {
                            div {
                                class: "tl-playhead", style: "left: {playhead_x}px;",
                                div {
                                    class: "tl-playhead-grab",
                                    onmousedown: move |e: MouseEvent| {
                                        e.stop_propagation();
                                        scrubbing.set(true);
                                    },
                                }
                            }
                        }
                    }
                }
                div { class: "tl-status",
                    "Arrastra el cabezal (o la regla) para moverte · el cuerpo del clip para moverlo · los bordes para recortar · {tl.clip_count} clip(s) · {tl.track_count} pista(s)"
                }
            }
        }
    }
}

/// A badge glyph for a media kind.
fn kind_icon(kind: &str) -> &'static str {
    match kind {
        "audio" => "♪",
        "image" => "▣",
        _ => "▶",
    }
}

/// `m:ss` for an optional duration in seconds.
fn dur_label(secs: Option<f64>) -> String {
    match secs {
        Some(s) if s > 0.0 => {
            let total = s.round() as u64;
            format!("{}:{:02}", total / 60, total % 60)
        }
        _ => String::new(),
    }
}

/// A `#rrggbb` string for an `<input type=color>`, from the document's colour
/// (which may carry alpha or be malformed).
fn color6(s: &str) -> String {
    let t = s.trim();
    if t.starts_with('#') && t.len() >= 7 {
        t[..7].to_string()
    } else {
        "#ffffff".to_string()
    }
}

const STYLE: &str = r#"
    * { box-sizing: border-box; margin: 0; padding: 0; }
    html, body { height: 100%; }
    body {
        font-family: "Segoe UI", system-ui, sans-serif;
        background: #14151a; color: #e8e9ee; user-select: none; overflow: hidden;
    }
    .app { display: flex; flex-direction: column; height: 100vh; }
    .topbar {
        display: flex; align-items: center; gap: 14px; flex-wrap: wrap;
        padding: 10px 16px; background: #1c1e26; border-bottom: 1px solid #2a2d38;
    }
    .brand { display: flex; align-items: center; gap: 10px; }
    .logo { font-size: 20px; }
    .name { font-weight: 700; font-size: 18px; letter-spacing: .3px; }
    .milestone {
        font-size: 11px; color: #9aa0b4; background: #262a36;
        padding: 2px 8px; border-radius: 999px;
    }
    .transport { display: flex; align-items: center; gap: 10px; }
    .ctrl {
        width: 40px; height: 32px; border: none; border-radius: 8px;
        background: #262a36; color: #e8e9ee; font-size: 14px; cursor: pointer;
    }
    .ctrl:hover:not(:disabled) { background: #303544; }
    .ctrl:disabled { opacity: .4; cursor: default; }
    .time {
        font-family: ui-monospace, "Cascadia Code", monospace;
        font-size: 12px; color: #9aa0b4; min-width: 150px;
    }
    .edit-tools { display: flex; align-items: center; gap: 6px; }
    .tool {
        height: 32px; padding: 0 10px; border: none; border-radius: 8px;
        background: #262a36; color: #e8e9ee; font-size: 12px; cursor: pointer;
    }
    .tool:hover:not(:disabled) { background: #303544; }
    .tool:disabled { opacity: .35; cursor: default; }
    .import {
        margin-left: auto; background: #6c5ce7; color: white; border: none;
        padding: 8px 16px; border-radius: 8px; font-size: 13px; font-weight: 600;
        cursor: pointer;
    }
    .import:hover { background: #5a4bd6; }
    .error {
        background: #3a1f24; color: #ff9aa6; padding: 8px 16px; font-size: 13px;
        border-bottom: 1px solid #5a2a32;
    }
    .middle { flex: 1; display: flex; min-height: 0; }
    .library {
        width: 230px; flex-shrink: 0; display: flex; flex-direction: column;
        background: #1a1c24; border-right: 1px solid #2a2d38; overflow: hidden;
    }
    .lib-head { padding: 10px 14px; font-weight: 700; font-size: 13px; border-bottom: 1px solid #2a2d38; }
    .lib-list { flex: 1; overflow-y: auto; padding: 8px; display: flex; flex-direction: column; gap: 4px; }
    .lib-empty { padding: 10px; font-size: 12px; color: #5a5f72; line-height: 1.5; }
    .lib-item {
        display: flex; align-items: center; gap: 8px; padding: 6px; border-radius: 8px;
        background: #232633; border: 1px solid #2f3340; cursor: pointer;
    }
    .lib-item:hover { background: #2d3142; border-color: #6c5ce7; }
    .lib-item.dragging { background: #6c5ce7; border-color: #6c5ce7; opacity: .8; }
    .lib-kind {
        width: 34px; height: 34px; border-radius: 6px; flex-shrink: 0;
        display: flex; align-items: center; justify-content: center; font-size: 15px; color: #fff;
    }
    .lib-kind.k-video { background: #3b4b8a; }
    .lib-kind.k-audio { background: #6a4a7a; }
    .lib-kind.k-image { background: #3a6a5a; }
    .lib-meta { display: flex; flex-direction: column; min-width: 0; }
    .lib-name { font-size: 12px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
    .lib-dur { font-size: 11px; color: #9aa0b4; }
    .stage {
        flex: 1; display: flex; align-items: center; justify-content: center;
        padding: 16px; background: #0e0f13; overflow: hidden; min-height: 0;
    }
    .fx {
        width: 280px; flex-shrink: 0; display: flex; flex-direction: column;
        background: #1a1c24; border-left: 1px solid #2a2d38; overflow: hidden;
    }
    .fx-head { padding: 10px 14px; font-weight: 700; font-size: 13px; border-bottom: 1px solid #2a2d38; }
    .fx-empty { padding: 14px; font-size: 12px; color: #5a5f72; line-height: 1.5; }
    .fx-sel { padding: 8px 14px 2px; font-size: 12px; color: #9aa0b4; }
    .fx-applied {
        padding: 6px 12px 8px; display: flex; flex-direction: column; gap: 8px;
        max-height: 52%; overflow-y: auto;
    }
    .fx-none { font-size: 12px; color: #5a5f72; }
    .fx-card { background: #232633; border: 1px solid #2f3340; border-radius: 8px; padding: 8px 10px; }
    .fx-card-head { display: flex; align-items: center; justify-content: space-between; }
    .fx-card-name { font-size: 12px; font-weight: 600; }
    .fx-noparams { font-size: 11px; color: #5a5f72; margin-top: 4px; }
    .fx-param { display: flex; flex-direction: column; gap: 3px; margin-top: 7px; }
    .fx-param-label { font-size: 11px; color: #9aa0b4; }
    .fx-slider-row { display: flex; align-items: center; gap: 8px; }
    .fx-range { flex: 1; accent-color: #6c5ce7; }
    .fx-val {
        font-size: 11px; color: #c7ccdb; min-width: 50px; text-align: right;
        font-family: ui-monospace, monospace;
    }
    .fx-toggle {
        align-self: flex-start; background: #2f3340; color: #e8e9ee; border: 1px solid #3a3f52;
        border-radius: 6px; padding: 3px 14px; font-size: 12px; cursor: pointer;
    }
    .fx-toggle.on { background: #2f7a55; border-color: #39925f; }
    .fx-select {
        background: #1a1c24; color: #e8e9ee; border: 1px solid #3a3f52; border-radius: 6px;
        padding: 4px 8px; font-size: 12px;
    }
    .txt-panel { border-bottom: 1px solid #2a2d38; }
    .txt-body { padding: 8px 14px 12px; display: flex; flex-direction: column; gap: 8px; }
    .txt-area {
        width: 100%; resize: vertical; background: #12141a; color: #e8e9ee;
        border: 1px solid #3a3f52; border-radius: 6px; padding: 8px; font-size: 13px;
        font-family: inherit;
    }
    .txt-color { width: 48px; height: 28px; background: none; border: 1px solid #3a3f52; border-radius: 6px; cursor: pointer; }
    .txt-align { display: flex; gap: 4px; }
    .txt-align .al {
        flex: 1; background: #2f3340; color: #e8e9ee; border: 1px solid #3a3f52;
        border-radius: 6px; padding: 4px; font-size: 13px; cursor: pointer;
    }
    .txt-align .al.on { background: #6c5ce7; border-color: #6c5ce7; }
    .tr-panel { border-bottom: 1px solid #2a2d38; }
    .tr-body { padding: 8px 14px 12px; display: flex; flex-direction: column; gap: 8px; }
    .tr-body .fx-select { width: 100%; }
    .tr-body .fx-select:disabled { opacity: .5; }
    .tr-hint { font-size: 11px; color: #b6b06a; line-height: 1.4; }
    .cut-panel { border-bottom: 1px solid #2a2d38; }
    .cut-body { padding: 8px 14px 12px; display: flex; flex-direction: column; gap: 6px; }
    .cut-body .tool { width: 100%; }
    .fx-cut { background: #2f6f7a; }
    .fx-cut:hover { background: #398a97; }
    .fx-cc { background: #5a4bd6; }
    .fx-cc:hover { background: #6c5ce7; }
    .cut-progress { font-size: 12px; color: #9ad5e0; }
    .audio-panel { border-bottom: 1px solid #2a2d38; }
    .audio-body { padding: 8px 14px 12px; display: flex; flex-direction: column; gap: 4px; }
    .audio-body .tool { width: 100%; }
    .au-muted { background: #5a2a32; }
    .au-muted:hover { background: #6e333d; }
    .clip-tr {
        position: absolute; left: 1px; top: 50%; transform: translateY(-50%);
        width: 18px; height: 18px; border-radius: 50%; background: #ffd166; color: #14151a;
        display: flex; align-items: center; justify-content: center; font-size: 11px;
        z-index: 3; box-shadow: 0 1px 3px rgba(0,0,0,.5); pointer-events: none;
    }
    .fx-chip {
        display: flex; align-items: center; gap: 4px; background: #6c5ce7; color: #fff;
        border-radius: 999px; padding: 3px 4px 3px 10px; font-size: 12px;
    }
    .fx-chip-x {
        border: none; background: rgba(255,255,255,.2); color: #fff; width: 18px; height: 18px;
        border-radius: 50%; cursor: pointer; line-height: 1; font-size: 12px;
    }
    .fx-chip-x:hover { background: rgba(255,255,255,.4); }
    .fx-add-title {
        padding: 8px 14px 4px; font-size: 11px; text-transform: uppercase;
        letter-spacing: .5px; color: #6b7185;
    }
    .fx-list { flex: 1; overflow-y: auto; padding: 4px 10px 12px; display: flex; flex-direction: column; gap: 3px; }
    .fx-item {
        text-align: left; background: #232633; color: #e8e9ee; border: 1px solid #2f3340;
        border-radius: 6px; padding: 7px 10px; font-size: 12px; cursor: pointer;
    }
    .fx-item:hover { background: #2d3142; border-color: #6c5ce7; }
    .frame {
        max-width: 100%; max-height: 100%; border-radius: 6px;
        box-shadow: 0 8px 30px rgba(0,0,0,.5);
    }
    .empty { text-align: center; color: #5a5f72; }
    .empty-title { font-size: 15px; font-weight: 600; margin-bottom: 4px; }
    .empty-sub { font-size: 13px; }
    .scrub-row { padding: 8px 16px; background: #15161d; }
    .scrubber { width: 100%; accent-color: #6c5ce7; cursor: pointer; }
    .scrubber:disabled { opacity: .4; }
    .timeline {
        height: 180px; display: flex; flex-direction: column;
        background: #1c1e26; border-top: 1px solid #2a2d38;
    }
    .tl-scroll { position: relative; flex: 1; overflow-x: auto; overflow-y: hidden; padding: 8px; }
    .lanes { display: flex; flex-direction: column; gap: 4px; min-height: 100%; }
    .lane {
        position: relative; height: 40px; background: #12141a;
        border-radius: 6px; border: 1px solid #22252f;
    }
    .lane.drop { background: #1c1f2e; border-color: #6c5ce7; box-shadow: inset 0 0 0 1px #6c5ce7; }
    .clip {
        position: absolute; top: 3px; bottom: 3px; border-radius: 4px;
        overflow: hidden; display: flex; align-items: center;
        padding: 0 8px; border: 1px solid rgba(255,255,255,.12);
        box-shadow: 0 1px 3px rgba(0,0,0,.4); cursor: grab;
    }
    .clip:active { cursor: grabbing; }
    .clip.sel { border-color: #ffd166; box-shadow: 0 0 0 2px #ffd166, 0 1px 3px rgba(0,0,0,.4); }
    .handle {
        position: absolute; top: 0; bottom: 0; width: 7px; cursor: ew-resize;
        background: rgba(255,255,255,.18);
    }
    .handle:hover { background: rgba(255,255,255,.4); }
    .handle.hleft { left: 0; border-radius: 4px 0 0 4px; }
    .handle.hright { right: 0; border-radius: 0 4px 4px 0; }
    .k-video { background: linear-gradient(#3b4b8a, #2f3c70); }
    .k-image { background: linear-gradient(#3a6a5a, #2f5649); }
    .k-audio { background: linear-gradient(#6a4a7a, #563c63); }
    .k-text  { background: linear-gradient(#8a7a3b, #70632f); }
    .k-layer { background: linear-gradient(#7a3b4b, #632f3c); }
    .k-shape { background: linear-gradient(#3b6a7a, #2f5663); }
    .wave-wrap {
        position: absolute; inset: 1px 2px; pointer-events: none; opacity: .9;
    }
    .wave-wrap svg { width: 100%; height: 100%; display: block; }
    .clip-name {
        font-size: 11px; color: #e8e9ee; white-space: nowrap;
        text-overflow: ellipsis; overflow: hidden; pointer-events: none;
    }
    .tl-ruler {
        position: relative; flex: none; height: 22px; margin-bottom: 2px;
        background: #15161d; border-radius: 6px; border: 1px solid #22252f;
        cursor: pointer; overflow: hidden; user-select: none;
    }
    .tl-ruler .tick { position: absolute; top: 10px; bottom: 0; width: 1px; background: #2a2d38; }
    .tl-ruler .tick.major { top: 6px; background: #3a3f4f; }
    .tl-ruler .tick-label {
        position: absolute; top: -4px; left: 3px; font-size: 10px;
        color: #9aa0b4; white-space: nowrap; pointer-events: none;
    }
    .lanes.scrubbing .clip { pointer-events: none; }
    .tl-playhead {
        position: absolute; top: 8px; bottom: 8px; width: 2px; background: #ff5c7a;
        box-shadow: 0 0 6px #ff5c7a; pointer-events: none; z-index: 5;
    }
    .tl-playhead-grab {
        position: absolute; top: -3px; left: -6px; width: 14px; height: 13px;
        background: #ff5c7a; border-radius: 3px 3px 5px 5px; cursor: ew-resize;
        pointer-events: auto; box-shadow: 0 1px 3px rgba(0,0,0,.5);
    }
    .tl-playhead-grab:hover { background: #ff7a93; }
    .tl-status {
        padding: 6px 16px; font-size: 11px; color: #9aa0b4;
        border-top: 1px solid #2a2d38; background: #191b22;
    }
    .tool.export { background: #2f7a55; }
    .tool.export:hover:not(:disabled) { background: #39925f; }
    .export-bar {
        position: relative; height: 26px; background: #15161d;
        border-bottom: 1px solid #2a2d38; overflow: hidden;
    }
    .export-fill {
        position: absolute; top: 0; left: 0; bottom: 0;
        background: linear-gradient(90deg, #2f7a55, #39b06f); transition: width .15s;
    }
    .export-label {
        position: absolute; inset: 0; display: flex; align-items: center;
        justify-content: center; font-size: 12px; color: #e8e9ee;
    }
    .export-done {
        display: flex; align-items: center; gap: 10px; padding: 8px 16px;
        font-size: 13px; border-bottom: 1px solid #2a2d38;
    }
    .export-done.ok { background: #1f3a2a; color: #9af0b8; }
    .export-done.err { background: #3a1f24; color: #ff9aa6; }
    .dismiss {
        margin-left: auto; background: transparent; border: none; cursor: pointer;
        color: inherit; font-size: 14px; opacity: .7;
    }
    .dismiss:hover { opacity: 1; }
"#;
