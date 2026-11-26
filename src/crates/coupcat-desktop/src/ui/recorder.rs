// SPDX-License-Identifier: AGPL-3.0-or-later
//
//! The screen recorder's interface (#51): a dialog to set up a take, a banner
//! while it records, and the import of the finished file into the library.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use dioxus::prelude::*;

use super::Ctx;
use super::resources::ResTab;
use crate::editor;
use crate::monitor::Cmd;
use crate::recorder::{self, Devices, Recording, Settings};

/// Where the recorder is.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Idle,
    Starting,
    Recording,
    Saving,
}

/// What the person chose in the dialog.
#[derive(Clone, PartialEq)]
pub struct Form {
    pub screen: usize,
    /// Index into the microphone list; `None` records without sound.
    pub mic: Option<usize>,
    /// Record what the computer plays.
    pub system: bool,
    pub fps: u32,
    pub cursor: bool,
    pub minimize: bool,
    pub folder: PathBuf,
}

/// The recorder's state, shared by the dialog, the banner and the take.
#[derive(Clone)]
pub struct Recorder {
    pub open: Signal<bool>,
    pub status: Signal<Status>,
    pub seconds: Signal<u64>,
    /// `None` while the screens and microphones are being listed.
    pub devices: Signal<Option<Result<Devices, String>>>,
    pub form: Signal<Form>,
    pub error: Signal<Option<String>>,
    /// Set by the banner: `Some(true)` keeps the take, `Some(false)` drops it.
    pub ending: Signal<Option<bool>>,
    /// The capture in progress.
    session: Rc<RefCell<Option<Recording>>>,
}

pub fn use_recorder() -> Recorder {
    Recorder {
        open: use_signal(|| false),
        status: use_signal(|| Status::Idle),
        seconds: use_signal(|| 0),
        devices: use_signal(|| None),
        form: use_signal(|| Form {
            screen: 0,
            mic: Some(0),
            system: true,
            fps: 30,
            cursor: true,
            minimize: true,
            folder: recorder::default_folder(),
        }),
        error: use_signal(|| None),
        ending: use_signal(|| None),
        session: use_hook(|| Rc::new(RefCell::new(None))),
    }
}

/// Run slow work on its own thread without freezing the interface.
async fn off_thread<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(work());
    });
    loop {
        match rx.try_recv() {
            Ok(value) => return Some(value),
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => return None,
        }
    }
}

/// Show the dialog and list what can be recorded (again each time, since
/// monitors and microphones come and go).
pub fn open_dialog(c: &Ctx) {
    let mut open = c.rec.open;
    let mut devices = c.rec.devices;
    let mut error = c.rec.error;
    let mut form = c.rec.form;
    if (c.rec.status)() != Status::Idle || open() {
        return;
    }
    open.set(true);
    error.set(None);
    devices.set(None);
    spawn(async move {
        let found = off_thread(recorder::devices)
            .await
            .unwrap_or_else(|| Err("No se pudieron listar los dispositivos.".to_string()));
        if let Ok(d) = &found {
            let mut f = form.write();
            if f.screen >= d.screens.len() {
                f.screen = 0;
            }
            if f.mic.is_some_and(|m| m >= d.mics.len()) {
                f.mic = (!d.mics.is_empty()).then_some(0);
            }
        }
        devices.set(Some(found));
    });
}

/// The local date and time, fit for a file name (the webview knows the zone).
async fn file_stamp() -> String {
    let mut ev = dioxus::document::eval(
        "const d=new Date(),p=n=>String(n).padStart(2,'0');\
         dioxus.send(`${d.getFullYear()}-${p(d.getMonth()+1)}-${p(d.getDate())} ${p(d.getHours())}-${p(d.getMinutes())}-${p(d.getSeconds())}`)",
    );
    ev.recv::<String>().await.unwrap_or_default()
}

/// Record with the dialog's settings until the person (or a failure) ends the
/// take, then put the file in the library.
async fn run_take(c: Ctx) {
    let rec = c.rec.clone();
    let Recorder {
        mut open,
        mut status,
        mut seconds,
        devices,
        form,
        mut error,
        mut ending,
        session,
    } = rec;
    let Some(Ok(found)) = devices() else { return };
    let form = form();
    let Some(screen) = found.screens.get(form.screen).cloned() else {
        return;
    };
    let settings = Settings {
        screen,
        mic: form.mic.and_then(|m| found.mics.get(m).cloned()),
        system: form.system && found.system.is_some(),
        fps: form.fps,
        cursor: form.cursor,
    };

    status.set(Status::Starting);
    error.set(None);
    ending.set(None);
    seconds.set(0);
    // The preview's sound must not end up in the microphone.
    c.monitor.send(Cmd::Pause);
    let name = format!("Grabación {}", file_stamp().await);
    let folder = form.folder.clone();
    let started = off_thread(move || recorder::start(&settings, &folder, &name))
        .await
        .unwrap_or_else(|| Err("La grabación no pudo empezar.".to_string()));
    match started {
        Ok(recording) => *session.borrow_mut() = Some(recording),
        Err(why) => {
            error.set(Some(why));
            status.set(Status::Idle);
            return;
        }
    }
    open.set(false);
    status.set(Status::Recording);
    if form.minimize {
        dioxus::desktop::window().set_minimized(true);
    }

    // Watch the take until it is ended or ffmpeg gives up.
    let mut failure = None;
    let keep = loop {
        tokio::time::sleep(Duration::from_millis(200)).await;
        if let Some(keep) = ending() {
            break keep;
        }
        let mut slot = session.borrow_mut();
        let Some(recording) = slot.as_mut() else {
            break false;
        };
        let now = recording.seconds();
        if now != seconds() {
            seconds.set(now);
        }
        if let Some(why) = recording.failed() {
            failure = Some(why);
            break true;
        }
    };

    status.set(Status::Saving);
    let window = dioxus::desktop::window();
    window.set_minimized(false);
    window.set_focus();
    let taken = session.borrow_mut().take();
    let mut file_msg = c.file_msg;
    match taken {
        Some(recording) if keep => {
            let saved = off_thread(move || {
                recording.stop().and_then(|path| {
                    concat_media::probe(&path)
                        .map(|info| (path.clone(), info))
                        .map_err(|e| {
                            format!("No se pudo leer la grabación {}: {e}", path.display())
                        })
                })
            })
            .await
            .unwrap_or_else(|| Err("No se pudo guardar la grabación.".to_string()));
            match saved {
                Ok((path, info)) => {
                    editor::import_media(&mut c.project.borrow_mut(), &info, &path);
                    let (mut media, mut timeline) = (c.media, c.timeline);
                    media.set(editor::media_list(&c.project.borrow()));
                    timeline.set(editor::view(&c.project.borrow()));
                    // Show the library where the new take is.
                    let (mut tab, mut cat, mut search) = (c.res_tab, c.res_cat, c.res_search);
                    tab.set(ResTab::Media);
                    cat.set(0);
                    search.set(String::new());
                    let done = format!("Grabación añadida a la librería: {}", path.display());
                    file_msg.set(Some(match failure {
                        Some(why) => (false, format!("{why} Se guardó lo grabado: {}", path.display())),
                        None => (true, done),
                    }));
                }
                Err(why) => file_msg.set(Some((false, failure.unwrap_or(why)))),
            }
        }
        Some(recording) => {
            recording.cancel();
            file_msg.set(Some((true, "Grabación descartada.".to_string())));
        }
        None => {}
    }
    ending.set(None);
    status.set(Status::Idle);
}

/// `mm:ss`, or `h:mm:ss` past an hour.
fn clock(secs: u64) -> String {
    if secs >= 3600 {
        format!("{}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60)
    } else {
        format!("{:02}:{:02}", secs / 60, secs % 60)
    }
}

/// The strip under the top bar while a take is being recorded or saved.
pub fn recorder_banner(c: &Ctx) -> Element {
    let status = (c.rec.status)();
    let mut ending = c.rec.ending;
    match status {
        Status::Recording => {
            let time = clock((c.rec.seconds)());
            rsx! {
                div { class: "rec-bar",
                    span { class: "rec-dot" }
                    span { "Grabando la pantalla · {time}" }
                    span { class: "spacer" }
                    button {
                        id: "rec-stop", class: "tool rec-stop",
                        disabled: ending().is_some(),
                        onclick: move |_| ending.set(Some(true)),
                        "■ Detener"
                    }
                    button {
                        id: "rec-cancel", class: "tool",
                        title: "Descartar esta grabación",
                        disabled: ending().is_some(),
                        onclick: move |_| ending.set(Some(false)),
                        "Cancelar"
                    }
                }
            }
        }
        Status::Saving => rsx! {
            div { class: "rec-bar", "Guardando la grabación…" }
        },
        _ => rsx! {},
    }
}

/// The set-up dialog.
pub fn recorder_dialog(c: &Ctx) -> Element {
    let mut open = c.rec.open;
    if !open() {
        return rsx! {};
    }
    let status = (c.rec.status)();
    let busy = status != Status::Idle;
    let devices = (c.rec.devices)();
    let mut form = c.rec.form;
    let f = form();
    let error = (c.rec.error)();
    let ready = matches!(devices, Some(Ok(_))) && !busy;
    let folder = f.folder.display().to_string();
    let ctx = c.clone();

    let body = match &devices {
        None => rsx! { div { class: "modal-text", "Buscando pantallas y micrófonos…" } },
        Some(Err(why)) => rsx! { div { class: "modal-text rec-error", "{why}" } },
        Some(Ok(d)) => rsx! {
            label { class: "rec-row",
                span { "Pantalla" }
                select {
                    id: "rec-screen", disabled: busy,
                    onchange: move |e: FormEvent| {
                        if let Ok(i) = e.value().parse() { form.write().screen = i; }
                    },
                    for (i, s) in d.screens.iter().enumerate() {
                        option { value: "{i}", selected: i == f.screen, "Pantalla {i + 1} ({s.width}×{s.height})" }
                    }
                }
            }
            label { class: "rec-row",
                span { "Micrófono" }
                select {
                    id: "rec-mic", disabled: busy,
                    onchange: move |e: FormEvent| form.write().mic = e.value().parse().ok(),
                    option { value: "", selected: f.mic.is_none(), "Sin micrófono" }
                    for (i, name) in d.mics.iter().enumerate() {
                        option { value: "{i}", selected: f.mic == Some(i), "{name}" }
                    }
                }
            }
            label { class: "rec-check",
                input {
                    id: "rec-system", r#type: "checkbox",
                    checked: f.system && d.system.is_some(),
                    disabled: busy || d.system.is_none(),
                    onchange: move |e: FormEvent| form.write().system = e.checked(),
                }
                match &d.system {
                    Some(name) => rsx! { span { class: "rec-device", title: "{name}", "Grabar el audio del sistema ({name})" } },
                    None => rsx! { span { "Audio del sistema: no hay dispositivo de salida" } },
                }
            }
            label { class: "rec-row",
                span { "Fotogramas" }
                select {
                    id: "rec-fps", disabled: busy,
                    onchange: move |e: FormEvent| {
                        if let Ok(v) = e.value().parse() { form.write().fps = v; }
                    },
                    for fps in [30u32, 60] {
                        option { value: "{fps}", selected: fps == f.fps, "{fps} fps" }
                    }
                }
            }
            label { class: "rec-check",
                input {
                    r#type: "checkbox", checked: f.cursor, disabled: busy,
                    onchange: move |e: FormEvent| form.write().cursor = e.checked(),
                }
                "Mostrar el cursor"
            }
            label { class: "rec-check",
                input {
                    id: "rec-minimize", r#type: "checkbox", checked: f.minimize, disabled: busy,
                    onchange: move |e: FormEvent| form.write().minimize = e.checked(),
                }
                "Minimizar CoupCat al grabar (vuelve a abrirlo para detener)"
            }
            div { class: "rec-row",
                span { "Guardar en" }
                span { class: "rec-folder", title: "{folder}", "{folder}" }
                button {
                    class: "tool", disabled: busy,
                    onclick: move |_| {
                        spawn(async move {
                            if let Some(dir) = rfd::AsyncFileDialog::new()
                                .set_title("Carpeta para las grabaciones")
                                .pick_folder()
                                .await
                            {
                                form.write().folder = dir.path().to_path_buf();
                            }
                        });
                    },
                    "Cambiar…"
                }
            }
        },
    };

    rsx! {
        div { class: "modal-back",
            div { class: "modal rec-modal",
                div { class: "modal-title", "Grabar pantalla" }
                {body}
                if let Some(why) = error {
                    div { class: "modal-text rec-error", "{why}" }
                }
                div { class: "modal-btns",
                    button { class: "tool", disabled: busy, onclick: move |_| open.set(false), "Cerrar" }
                    button {
                        id: "rec-start", class: "export-btn", disabled: !ready,
                        onclick: move |_| { spawn(run_take(ctx.clone())); },
                        if status == Status::Starting { "Empezando…" } else { "● Grabar" }
                    }
                }
            }
        }
    }
}
