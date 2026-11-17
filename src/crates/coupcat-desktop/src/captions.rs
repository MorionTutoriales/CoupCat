// SPDX-License-Identifier: AGPL-3.0-or-later
//
//! Automatic captions (speech to text) with whisper.cpp.
//!
//! Decodes a clip's audio to 16 kHz mono via `concat_media`, transcribes it
//! with a Whisper model (`whisper-rs`, built against MSVC), and returns the
//! segments as timed [`Caption`]s in timeline seconds. The UI then drops a text
//! clip for each. Heavy, so it runs on a worker thread; the UI polls
//! [`Job::shared`]. The model file (`ggml-base.bin`) sits beside the exe.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;

use concat_media::{AudioDecoder, AudioOptions, SampleFormat};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

/// One caption, in timeline seconds.
#[derive(Clone)]
pub struct Caption {
    pub start: f64,
    pub end: f64,
    pub text: String,
}

/// The Whisper model file, expected beside the exe.
pub fn model_path() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|d| d.join("ggml-base.bin")))
        .unwrap_or_else(|| PathBuf::from("ggml-base.bin"))
}

/// Whether the model is present (the button is hidden otherwise).
pub fn model_ready() -> bool {
    model_path().exists()
}

/// Progress of a running (or finished) transcription.
#[derive(Default, Clone)]
pub struct Progress {
    pub active: bool,
    pub stage: String,
    /// Set once finished: the captions (in timeline seconds) or an error.
    pub result: Option<Result<Vec<Caption>, String>>,
}

/// A running caption job the UI polls.
#[derive(Clone)]
pub struct Job {
    pub shared: Arc<Mutex<Progress>>,
}

/// Transcribe `[source_start, source_start+duration)` of `media_path` and place
/// the captions from `clip_start` on the timeline.
pub fn start(media_path: String, source_start: f64, duration: f64, clip_start: f64) -> Job {
    let shared = Arc::new(Mutex::new(Progress {
        active: true,
        stage: "extrayendo audio".to_string(),
        ..Progress::default()
    }));
    let bg = shared.clone();
    thread::spawn(move || {
        let outcome = run(&media_path, source_start, duration, clip_start, &bg);
        if let Ok(mut p) = bg.lock() {
            p.active = false;
            p.result = Some(outcome);
        }
    });
    Job { shared }
}

fn run(
    media_path: &str,
    source_start: f64,
    duration: f64,
    clip_start: f64,
    shared: &Arc<Mutex<Progress>>,
) -> Result<Vec<Caption>, String> {
    let model = model_path();
    if !model.exists() {
        return Err("Falta el modelo de voz (ggml-base.bin) junto al ejecutable.".to_string());
    }

    // Decode the clip's audio window to 16 kHz mono f32.
    let opts = AudioOptions {
        start: Some(source_start.max(0.0)),
        duration: Some(duration.max(0.0)),
        filters: Vec::new(),
        rate: 16_000,
        channels: 1,
        format: SampleFormat::F32,
        stream: None,
    };
    let mut decoder = AudioDecoder::open(media_path, &opts).map_err(|e| format!("Audio: {e}"))?;
    let pcm = decoder.collect_f32().map_err(|e| format!("Audio: {e}"))?;
    if pcm.is_empty() {
        return Err("El clip no tiene audio para transcribir.".to_string());
    }

    set_stage(shared, "transcribiendo");
    let ctx = WhisperContext::new_with_params(&model, WhisperContextParameters::default())
        .map_err(|e| format!("Modelo: {e}"))?;
    let mut state = ctx.create_state().map_err(|e| format!("Whisper: {e}"))?;
    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    params.set_language(Some("auto"));
    params.set_print_progress(false);
    params.set_print_special(false);
    params.set_print_realtime(false);
    state
        .full(params, &pcm)
        .map_err(|e| format!("Transcripción: {e}"))?;

    let n = state.full_n_segments();
    let mut captions = Vec::new();
    for i in 0..n {
        if let Some(seg) = state.get_segment(i) {
            let text = seg
                .to_str_lossy()
                .map(|c| c.into_owned())
                .unwrap_or_default()
                .trim()
                .to_string();
            if text.is_empty() {
                continue;
            }
            // Whisper timestamps are centiseconds, relative to the decoded
            // window; place them from the clip's start on the timeline.
            let t0 = seg.start_timestamp() as f64 / 100.0;
            let t1 = seg.end_timestamp() as f64 / 100.0;
            captions.push(Caption {
                start: clip_start + t0,
                end: clip_start + t1.max(t0 + 0.3),
                text,
            });
        }
    }
    Ok(captions)
}

fn set_stage(shared: &Arc<Mutex<Progress>>, stage: &str) {
    if let Ok(mut p) = shared.lock() {
        p.stage = stage.to_string();
    }
}
