// SPDX-License-Identifier: AGPL-3.0-or-later
//
//! Background removal (person cutout), on-device.
//!
//! Runs the compiled-in person-matting model (`concat_vision::Segmenter`,
//! through ONNX Runtime loaded dynamically — see main's ORT_DYLIB_PATH) over a
//! clip's frames and stores the masks in the session's project folder. The
//! renderer then reads those masks and cuts the background out, in the composed
//! monitor and the export alike — a clip's `cutout` plus [`project_dir`] is all
//! `flatten_timeline_in` needs. Inference is slow, so it runs on a worker
//! thread and the UI polls [`Job::shared`].

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;

use concat_media::{DecodeOptions, Decoder, FrameSource};
use concat_project::model::Subject;
use concat_vision::{MaskStore, Segmenter, mask_dir};

/// The session's project folder, where masks live. Shared with the clip-list
/// builder so `flatten_timeline_in` names the same mask directory.
pub fn project_dir() -> PathBuf {
    std::env::temp_dir().join("coupcat-project")
}

/// Progress of a running (or finished) cutout computation.
#[derive(Default, Clone)]
pub struct Progress {
    pub active: bool,
    pub done: usize,
    pub total: usize,
    /// Set once finished: the clip id (to apply the cutout to) or an error.
    pub result: Option<Result<String, String>>,
}

/// A running cutout job the UI polls.
#[derive(Clone)]
pub struct Job {
    pub shared: Arc<Mutex<Progress>>,
}

/// Start computing masks for `media_path` on a worker thread; `clip_id` comes
/// back in the result so the UI knows which clip to turn into a cutout.
pub fn start(media_path: String, clip_id: String) -> Job {
    let shared = Arc::new(Mutex::new(Progress {
        active: true,
        ..Progress::default()
    }));
    let bg = shared.clone();
    thread::spawn(move || {
        let outcome = run(&media_path, &bg);
        if let Ok(mut p) = bg.lock() {
            p.active = false;
            p.result = Some(outcome.map(|_| clip_id));
        }
    });
    Job { shared }
}

/// Segment every frame of the media and store the masks.
fn run(media_path: &str, shared: &Arc<Mutex<Progress>>) -> Result<(), String> {
    let info = concat_media::probe(media_path).map_err(|e| format!("Al leer: {e}"))?;
    let video = info
        .video
        .ok_or_else(|| "El clip no tiene vídeo.".to_string())?;
    if let Ok(mut p) = shared.lock() {
        p.total = info
            .duration
            .map(|d| (d.as_f64() * video.frame_rate.fps().as_f64()).round() as usize)
            .unwrap_or(0);
    }

    let seg = Segmenter::selfie().map_err(|e| format!("Al cargar el modelo: {e}"))?;
    let dir = mask_dir(&project_dir(), media_path, Subject::Auto);
    let mut store = MaskStore::open(&dir);
    store
        .set_model(seg.name())
        .map_err(|e| format!("Al preparar la caché: {e}"))?;

    let (iw, ih) = seg.input_size(video.width, video.height);
    let opts = DecodeOptions {
        size: Some((iw, ih)),
        ..Default::default()
    };
    let mut decoder = Decoder::open(media_path, &opts).map_err(|e| format!("Al abrir: {e}"))?;
    seg.begin();

    let mut done = 0;
    while let Some(frame) = decoder
        .next_frame()
        .map_err(|e| format!("Al decodificar: {e}"))?
    {
        let millis = (decoder.position().map(|r| r.as_f64()).unwrap_or(0.0) * 1000.0).round() as u64;
        let mask = seg
            .mask(&frame)
            .map_err(|e| format!("En la inferencia: {e}"))?;
        store.put(millis, &mask).map_err(|e| format!("Al guardar: {e}"))?;
        done += 1;
        if let Ok(mut p) = shared.lock() {
            p.done = done;
        }
    }
    Ok(())
}
