// SPDX-License-Identifier: AGPL-3.0-or-later
//
//! Library thumbnails: one small still per imported video or image, made in
//! the background and cached.
//!
//! Decoding a frame means opening the file and seeking, which is too slow for
//! the UI thread, so the first [`get`] for a path queues it on a single worker
//! thread and returns `None`; the library keeps showing the kind icon until the
//! thumbnail lands. One worker (not a thread per file) keeps a big import from
//! fighting the preview for every core. The worker flags [`take_dirty`] so the
//! UI knows to re-render, the same way [`crate::peaks`] does for waveforms.
//!
//! A file that cannot be decoded is remembered as failed and keeps its icon.

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Mutex, OnceLock};
use std::thread;

use base64::Engine as _;
use concat_core::frame::Frame;
use concat_core::time::Rational;
use concat_media::{DecodeOptions, Decoder, FrameSource};

/// The thumbnail fits in this box (twice the library tile, for HiDPI).
const MAX_W: u32 = 128;
const MAX_H: u32 = 72;
/// The instant a video's thumbnail is taken from, in seconds: past a fade-in
/// from black, but early enough to stand for the clip.
const AT: f64 = 1.0;
/// JPEG quantiser, 2 (best) to 31 (worst).
const QUALITY: u8 = 5;

/// A thumbnail request: the file, its displayed size and its duration.
struct Job {
    path: String,
    size: Option<(u32, u32)>,
    duration: Option<f64>,
}

/// Per path: `None` while queued or after a failure, `Some(url)` once ready.
static CACHE: OnceLock<Mutex<HashMap<String, Option<String>>>> = OnceLock::new();
static QUEUE: OnceLock<Mutex<Sender<Job>>> = OnceLock::new();
static DIRTY: AtomicBool = AtomicBool::new(false);

fn cache() -> &'static Mutex<HashMap<String, Option<String>>> {
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn queue() -> &'static Mutex<Sender<Job>> {
    QUEUE.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<Job>();
        thread::spawn(move || {
            for job in rx {
                let url = render(Path::new(&job.path), job.size, job.duration);
                if let Ok(mut c) = cache().lock() {
                    c.insert(job.path, url);
                }
                DIRTY.store(true, Ordering::Relaxed);
            }
        });
        Mutex::new(tx)
    })
}

/// The thumbnail for `path` as a `data:` URL if ready; otherwise queue it
/// (once) and return `None`. `size` is the file's displayed size and
/// `duration` its length in seconds, both as probed at import.
pub fn get(path: &str, size: Option<(u32, u32)>, duration: Option<f64>) -> Option<String> {
    {
        let mut c = cache().lock().ok()?;
        if let Some(entry) = c.get(path) {
            return entry.clone();
        }
        // Mark as in flight before queueing so a re-render does not queue twice.
        c.insert(path.to_string(), None);
    }
    let job = Job {
        path: path.to_string(),
        size,
        duration,
    };
    if let Ok(tx) = queue().lock() {
        let _ = tx.send(job);
    }
    None
}

/// Whether a thumbnail finished since the last check (the UI re-renders on
/// true).
pub fn take_dirty() -> bool {
    DIRTY.swap(false, Ordering::Relaxed)
}

/// Decode one representative frame of `path` and encode it as a JPEG `data:`
/// URL, or `None` when the file cannot be decoded.
fn render(path: &Path, size: Option<(u32, u32)>, duration: Option<f64>) -> Option<String> {
    let frame = still(path, size, duration)?;
    let bytes = concat_media::jpeg(&frame, QUALITY).ok()?;
    Some(format!(
        "data:image/jpeg;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(&bytes)
    ))
}

/// The frame a thumbnail is made of: the one near [`AT`] for a clip long
/// enough to have it, else the first. A seek that finds nothing (a duration
/// the container overstated) falls back to the first frame.
fn still(path: &Path, size: Option<(u32, u32)>, duration: Option<f64>) -> Option<Frame> {
    if let Some(at) = seek_time(duration) {
        let start = Rational::new((at * 1000.0).round() as i64, 1000);
        if let Some(frame) = first_frame(path, size, Some(start)) {
            return Some(frame);
        }
    }
    first_frame(path, size, None)
}

/// The first frame decoded from `start` (or the beginning), scaled to fit the
/// thumbnail box.
fn first_frame(path: &Path, size: Option<(u32, u32)>, start: Option<Rational>) -> Option<Frame> {
    // Keyframes only: a thumbnail does not need the exact frame, and this
    // makes it cost one decoded picture instead of a whole GOP. Two threads
    // leave the rest of the machine to the preview.
    let mut options = DecodeOptions::default()
        .limited_to(1)
        .nearest_keyframes()
        .threaded(2);
    if let Some((w, h)) = size {
        let (tw, th) = fit_box(w, h, MAX_W, MAX_H);
        options = options.scaled_to(tw, th);
    }
    if let Some(start) = start {
        options = options.starting_at(start);
    }
    let mut decoder = Decoder::open(path, &options).ok()?;
    decoder.next_frame().ok().flatten()
}

/// Where to take a video's thumbnail from: [`AT`] when the clip is longer
/// than that, else `None` (the first frame).
fn seek_time(duration: Option<f64>) -> Option<f64> {
    match duration {
        Some(d) if d > AT => Some(AT),
        _ => None,
    }
}

/// Fit `(w, h)` inside `max_w` × `max_h`, preserving aspect and never
/// upscaling; even dimensions of at least 2 keep the scaler happy.
fn fit_box(w: u32, h: u32, max_w: u32, max_h: u32) -> (u32, u32) {
    let even = |v: u32| (v.max(2)) & !1;
    if w == 0 || h == 0 {
        return (even(max_w), even(max_h));
    }
    let scale = (max_w as f64 / w as f64)
        .min(max_h as f64 / h as f64)
        .min(1.0);
    let tw = (w as f64 * scale).round() as u32;
    let th = (h as f64 * scale).round() as u32;
    (even(tw), even(th))
}

#[cfg(test)]
mod tests {
    use super::*;
    use concat_core::time::FrameRate;
    use concat_media::{EncodeOptions, Encoder, FrameSink};
    use std::path::PathBuf;

    /// A fresh path in the temp dir, unique to this test run.
    fn temp_file(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("coupcat-thumbs-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }

    #[test]
    fn fit_box_keeps_aspect_inside_the_box() {
        assert_eq!(fit_box(1920, 1080, 128, 72), (128, 72));
        // Portrait phone video: limited by height.
        assert_eq!(fit_box(1080, 1920, 128, 72), (40, 72));
        // Square image.
        assert_eq!(fit_box(500, 500, 128, 72), (72, 72));
        // Never upscale a tiny picture.
        assert_eq!(fit_box(64, 36, 128, 72), (64, 36));
        // Odd sizes round to even.
        assert_eq!(fit_box(33, 33, 128, 72), (32, 32));
        // Unknown size falls back to the box.
        assert_eq!(fit_box(0, 0, 128, 72), (128, 72));
    }

    #[test]
    fn seek_time_skips_short_or_unknown_clips() {
        assert_eq!(seek_time(Some(10.0)), Some(AT));
        assert_eq!(seek_time(Some(0.5)), None);
        assert_eq!(seek_time(Some(AT)), None);
        assert_eq!(seek_time(None), None);
    }

    #[test]
    fn undecodable_file_has_no_thumbnail() {
        concat_media::init();
        let path = temp_file("not-a-video.mp4");
        std::fs::write(&path, b"this is not a video").unwrap();
        assert!(render(&path, Some((1920, 1080)), Some(5.0)).is_none());
        assert!(render(Path::new("does/not/exist.mp4"), None, None).is_none());
    }

    #[test]
    fn video_thumbnail_is_a_scaled_jpeg() {
        concat_media::init();
        let path = temp_file("clip.mp4");
        let mut encoder = Encoder::create(
            &path,
            320,
            180,
            FrameRate::THIRTY,
            &EncodeOptions {
                preset: "ultrafast".to_owned(),
                ..EncodeOptions::default()
            },
        )
        .expect("the linked FFmpeg encodes h264");
        let mut frame = Frame::black(320, 180);
        frame.fill([200, 40, 40, 255]);
        // 2 s at 30 fps, so the thumbnail seeks to 1 s.
        for _ in 0..60 {
            encoder.write_frame(&frame).expect("writes");
        }
        encoder.finish().expect("finishes");

        let still = still(&path, Some((320, 180)), Some(2.0)).expect("a frame");
        assert_eq!((still.width(), still.height()), (128, 72));
        let url = render(&path, Some((320, 180)), Some(2.0)).expect("a thumbnail");
        assert!(url.starts_with("data:image/jpeg;base64,"));
    }

    #[test]
    fn image_thumbnail_is_made_from_the_picture() {
        concat_media::init();
        let path = temp_file("picture.jpg");
        let mut frame = Frame::black(400, 300);
        frame.fill([40, 200, 40, 255]);
        std::fs::write(&path, concat_media::jpeg(&frame, 3).unwrap()).unwrap();

        let still = still(&path, Some((400, 300)), None).expect("a frame");
        assert_eq!((still.width(), still.height()), (96, 72));
        assert!(render(&path, Some((400, 300)), None).is_some());
    }
}
