// SPDX-License-Identifier: AGPL-3.0-or-later
//
//! Waveform peaks for audio clips, fetched in the background and cached.
//!
//! `concat_media::extract` decodes a file's audio into min/max peaks; that is
//! slow, so the first [`get`] for a path starts a worker and returns `None`,
//! and the UI shows the waveform once it lands. The worker flags [`take_dirty`]
//! so the UI knows to re-render.

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;

use concat_media::Peaks;
use concat_media::peaks::extract;

static CACHE: OnceLock<Mutex<HashMap<String, Option<Arc<Peaks>>>>> = OnceLock::new();
static DIRTY: AtomicBool = AtomicBool::new(false);

fn cache() -> &'static Mutex<HashMap<String, Option<Arc<Peaks>>>> {
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// The peaks for `path` if ready; otherwise start fetching and return `None`.
pub fn get(path: &str) -> Option<Arc<Peaks>> {
    {
        let c = cache().lock().unwrap();
        if let Some(entry) = c.get(path) {
            return entry.clone();
        }
    }
    // Mark as in-flight and fetch off-thread.
    cache().lock().unwrap().insert(path.to_string(), None);
    let p = path.to_string();
    thread::spawn(move || {
        let peaks = extract(Path::new(&p), 80, None).ok().map(Arc::new);
        if let Ok(mut c) = cache().lock() {
            c.insert(p, peaks);
        }
        DIRTY.store(true, Ordering::Relaxed);
    });
    None
}

/// Whether a fetch finished since the last check (the UI re-renders on true).
pub fn take_dirty() -> bool {
    DIRTY.swap(false, Ordering::Relaxed)
}

/// A filled-waveform SVG for the media window `[source_start, +duration]`,
/// sized `width_px` × `HEIGHT`, as an inline `<svg>` string.
pub fn waveform_svg(peaks: &Peaks, source_start: f64, duration: f64, width_px: f64) -> String {
    const HEIGHT: f64 = 34.0;
    let mid = HEIGHT / 2.0;
    let cols = (width_px / 2.5).clamp(8.0, 160.0) as usize;
    let dur = duration.max(1e-3);

    let mut top = String::new();
    let mut bottom = String::new();
    for i in 0..=cols {
        let f = i as f64 / cols as f64;
        let t0 = (source_start + f * dur) as f32;
        let t1 = (source_start + (f + 1.0 / cols as f64) * dur) as f32;
        let (lo, hi) = peaks.extremes(t0, t1);
        let amp = (hi.abs().max(lo.abs()) as f64).clamp(0.0, 1.0) * (mid - 1.0);
        let x = f * width_px;
        top.push_str(&format!("{x:.1},{:.1} ", mid - amp));
        // Bottom envelope is prepended so the polygon closes right-to-left.
        bottom.insert_str(0, &format!("{x:.1},{:.1} ", mid + amp));
    }
    format!(
        "<svg class=\"wave\" viewBox=\"0 0 {width_px:.0} {HEIGHT}\" preserveAspectRatio=\"none\"><polygon points=\"{top}{bottom}\" fill=\"rgba(255,255,255,0.35)\"/></svg>"
    )
}
