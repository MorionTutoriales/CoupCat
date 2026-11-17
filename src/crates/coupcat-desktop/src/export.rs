// SPDX-License-Identifier: AGPL-3.0-or-later
//
//! Exporting the timeline to a video file.
//!
//! The engine does the whole render: `concat_export::flatten` turns the
//! project's active timeline into a flat clip list, and `concat_export::render`
//! composites it (on its own wgpu device), mixes the audio and muxes an MP4.
//! It is heavy and blocking, so it runs on a worker thread; the UI polls
//! [`Handle::shared`] for progress.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use concat_export::{ExportRequest, Reporter};
use concat_project::Editor;

/// Live progress of a running (or finished) export.
#[derive(Clone, Default)]
pub struct Progress {
    /// True while the worker is still rendering.
    pub active: bool,
    /// Frames (or the stage's unit) done.
    pub frame: i64,
    /// Total frames, when known.
    pub total: i64,
    /// What the engine is doing now ("mixing audio", "muxing"…).
    pub stage: String,
    /// Set once, when the render finishes: the output path or an error.
    pub done: Option<Result<String, String>>,
}

/// A running export the UI can poll and cancel.
#[derive(Clone)]
pub struct Handle {
    /// Progress snapshot the worker writes and the UI reads.
    pub shared: Arc<Mutex<Progress>>,
    cancel: Arc<AtomicBool>,
}

impl Handle {
    /// Ask the export to stop at the next frame or stage.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

/// Build an [`ExportRequest`] for the editor's active timeline, writing to
/// `output`. Uses the timeline's own frame size, rate and canvas, H.264 at a
/// sensible default quality.
pub fn build_request(editor: &Editor, output: String) -> Result<ExportRequest, String> {
    let project = editor.project();
    let timeline = project.active();
    let v = timeline.video;
    let clips = crate::text::clips(editor, v.width, v.height);
    if clips.is_empty() {
        return Err("No hay nada en el timeline para exportar.".to_string());
    }
    Ok(ExportRequest {
        output,
        width: v.width,
        height: v.height,
        rate_num: v.rate_num,
        rate_den: v.rate_den,
        crf: 23,
        preset: "medium".to_string(),
        codec: Default::default(), // H.264
        ten_bit: false,
        rate_mode: Default::default(), // VBR
        bitrate_kbps: 0,
        color_range: Default::default(),
        color_space: v.color_space,
        hdr: false,
        effect_time_offset: 0.0,
        canvas: v.canvas,
        clips,
    })
}

/// Spawn the render on a worker thread and return a handle to poll.
pub fn start(request: ExportRequest) -> Handle {
    let shared = Arc::new(Mutex::new(Progress {
        active: true,
        stage: "preparando".to_string(),
        ..Progress::default()
    }));
    let cancel = Arc::new(AtomicBool::new(false));

    let shared_bg = shared.clone();
    let cancel_bg = cancel.clone();
    thread::spawn(move || {
        let mut progress = |frame: i64, total: i64, stage: &'static str| {
            if let Ok(mut p) = shared_bg.lock() {
                p.frame = frame;
                p.total = total;
                p.stage = stage.to_string();
            }
        };
        let reporter = Reporter {
            progress: &mut progress,
            cancel: &cancel_bg,
        };
        let result = concat_export::render(&request, reporter);
        if let Ok(mut p) = shared_bg.lock() {
            p.active = false;
            p.done = Some(result);
        }
    });

    Handle { shared, cancel }
}
