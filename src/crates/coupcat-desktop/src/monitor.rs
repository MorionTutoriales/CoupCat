// SPDX-License-Identifier: AGPL-3.0-or-later
//
//! The composed monitor: the real timeline frame at the playhead.
//!
//! Unlike a single-clip decode, this composites the whole timeline — every
//! track, trim and placement — exactly as the exporter does, through
//! `concat_export::preview_frame` (which decodes via a reader pool so scrubbing
//! revisits are cache hits, and draws on the engine's shared headless wgpu
//! compositor). It runs on its own thread; the UI polls [`Shared`].

use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use base64::Engine as _;
use concat_core::frame::Frame;
use concat_export::{ClipKind, ExportClip, PreviewFrameRequest};

use crate::audio::AudioOut;
use concat_media::ReaderPool;
use concat_project::Editor;
use concat_project::model::ColorSpace;

/// Longest edge of the composed preview, for speed. The frame keeps the
/// timeline's aspect.
const PREVIEW_W: u32 = 854;
/// Reader-pool cache size and reader count.
const CACHE_BYTES: usize = 192 * 1024 * 1024;
const MAX_READERS: usize = 6;

/// The flattened timeline plus its frame format: everything the monitor needs
/// to composite a frame at any instant.
#[derive(Clone)]
pub struct TimelineData {
    pub clips: Vec<ExportClip>,
    pub width: u32,
    pub height: u32,
    pub rate_num: i64,
    pub rate_den: i64,
    pub canvas: [u8; 4],
    pub color_space: ColorSpace,
    pub duration: f64,
}

/// A command from the UI to the monitor thread.
pub enum Cmd {
    /// The timeline changed: re-flatten and re-render the current instant.
    SetTimeline(TimelineData),
    /// Jump to this many seconds and show that composed frame.
    Seek(f64),
    /// Play from the current instant.
    Play,
    /// Stop.
    Pause,
    /// There is nothing to show any more (a new or empty project): drop the
    /// timeline and the picture, so the monitor does not keep the last frame.
    Clear,
}

/// What the UI renders from; the monitor thread writes it.
#[derive(Default, Clone)]
pub struct Shared {
    pub generation: u64,
    pub frame: Option<String>,
    pub pos: f64,
    pub dur: f64,
    pub playing: bool,
    pub loading: bool,
    pub error: Option<String>,
}

/// Handle the UI keeps.
#[derive(Clone)]
pub struct Monitor {
    tx: Sender<Cmd>,
    pub shared: Arc<Mutex<Shared>>,
}

impl Monitor {
    pub fn spawn() -> Self {
        let (tx, rx) = mpsc::channel();
        let shared = Arc::new(Mutex::new(Shared::default()));
        let shared_bg = shared.clone();
        thread::spawn(move || run(rx, shared_bg));
        Monitor { tx, shared }
    }

    pub fn send(&self, cmd: Cmd) {
        let _ = self.tx.send(cmd);
    }
}

/// Flatten the editor's active timeline into [`TimelineData`], or `None` when
/// the timeline is empty.
pub fn timeline_data(editor: &Editor) -> Option<TimelineData> {
    let project = editor.project();
    let timeline = project.active();
    let v = timeline.video;
    let clips = crate::text::clips(editor, v.width, v.height);
    if clips.is_empty() {
        return None;
    }
    let duration = timeline
        .clips
        .iter()
        .map(|c| c.start + c.duration)
        .fold(0.0_f64, f64::max);
    Some(TimelineData {
        clips,
        width: v.width,
        height: v.height,
        rate_num: v.rate_num,
        rate_den: v.rate_den,
        canvas: v.canvas,
        color_space: v.color_space,
        duration,
    })
}

fn run(rx: Receiver<Cmd>, shared: Arc<Mutex<Shared>>) {
    let pool = ReaderPool::new(CACHE_BYTES, MAX_READERS);
    let mut audio = AudioOut::new();
    let mut data: Option<TimelineData> = None;
    let mut time = 0.0_f64;
    let mut playing = false;

    loop {
        let cmd = if playing {
            match rx.try_recv() {
                Ok(c) => Some(c),
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => return,
            }
        } else {
            match rx.recv() {
                Ok(c) => Some(c),
                Err(_) => return,
            }
        };

        if let Some(cmd) = cmd {
            match cmd {
                Cmd::SetTimeline(d) => {
                    time = time.clamp(0.0, d.duration.max(0.0));
                    data = Some(d);
                    playing = false;
                    stop_audio(&mut audio);
                    render(&pool, data.as_ref(), time, false, &shared);
                }
                Cmd::Seek(t) => {
                    if let Some(d) = data.as_ref() {
                        time = t.clamp(0.0, d.duration.max(0.0));
                        if playing {
                            start_audio(&mut audio, d, time);
                        } else {
                            stop_audio(&mut audio);
                        }
                        render(&pool, data.as_ref(), time, false, &shared);
                    }
                }
                Cmd::Play => {
                    if let Some(d) = data.as_ref() {
                        playing = true;
                        start_audio(&mut audio, d, time);
                    }
                }
                Cmd::Pause => {
                    playing = false;
                    stop_audio(&mut audio);
                    set_playing(&shared, false);
                }
                Cmd::Clear => {
                    data = None;
                    time = 0.0;
                    playing = false;
                    stop_audio(&mut audio);
                    let mut s = shared.lock().unwrap();
                    s.frame = None;
                    s.pos = 0.0;
                    s.dur = 0.0;
                    s.playing = false;
                    s.loading = false;
                    s.error = None;
                    s.generation += 1;
                }
            }
        }

        if playing {
            if let Some(d) = data.as_ref() {
                let started = Instant::now();
                render(&pool, Some(d), time, true, &shared);
                let fps = d.rate_num as f64 / d.rate_den.max(1) as f64;
                let step = 1.0 / fps.max(1.0);
                time += step;
                if time >= d.duration {
                    time = d.duration;
                    playing = false;
                    stop_audio(&mut audio);
                    render(&pool, Some(d), time, false, &shared);
                } else if let Some(rest) =
                    Duration::from_secs_f64(step).checked_sub(started.elapsed())
                {
                    thread::sleep(rest);
                }
            }
        }
    }
}

/// Stop any preview sound.
fn stop_audio(audio: &mut Option<AudioOut>) {
    if let Some(a) = audio.as_mut() {
        a.stop();
    }
}

/// Play the sound of the clip audible at `time`, from its source position.
fn start_audio(audio: &mut Option<AudioOut>, d: &TimelineData, time: f64) {
    let Some(a) = audio.as_mut() else { return };
    match audible_at(&d.clips, time) {
        Some((path, source_start, volume)) => a.play(&path, source_start, volume),
        None => a.stop(),
    }
}

/// The clip audible at `time` (the first video/audio clip there with sound),
/// as `(path, source position, volume)`.
fn audible_at(clips: &[ExportClip], t: f64) -> Option<(String, f64, f32)> {
    clips
        .iter()
        .find(|c| {
            matches!(c.kind, ClipKind::Video | ClipKind::Audio)
                && !c.muted
                && c.has_audio != Some(false)
                && t >= c.start
                && t < c.start + c.duration
        })
        .map(|c| {
            let into = c.source_start + (t - c.start) * c.speed;
            (c.path.clone(), into, c.volume as f32)
        })
}

/// Composite the frame at `time` and publish it.
fn render(
    pool: &ReaderPool,
    data: Option<&TimelineData>,
    time: f64,
    playing: bool,
    shared: &Arc<Mutex<Shared>>,
) {
    let Some(d) = data else { return };
    let (width, height) = fit_width(d.width, d.height, PREVIEW_W);

    let request = PreviewFrameRequest {
        time,
        width,
        height,
        rate_num: d.rate_num,
        rate_den: d.rate_den,
        clips: d.clips.clone(),
        color_space: d.color_space,
        canvas: d.canvas,
    };

    match concat_export::preview_frame(pool, &request) {
        Ok(rgba) => match Frame::from_rgba(width, height, rgba) {
            Some(frame) => publish(shared, &frame, time, d.duration, playing),
            None => set_error(shared, "El frame compuesto tiene un tamaño inesperado.".to_string()),
        },
        Err(e) => set_error(shared, format!("Al componer el frame: {e}")),
    }
}

fn publish(shared: &Arc<Mutex<Shared>>, frame: &Frame, pos: f64, dur: f64, playing: bool) {
    let bytes = match concat_media::jpeg(frame, 82) {
        Ok(b) => b,
        Err(_) => return,
    };
    let url = format!(
        "data:image/jpeg;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(&bytes)
    );
    let mut s = shared.lock().unwrap();
    s.frame = Some(url);
    s.pos = pos;
    s.dur = dur;
    s.playing = playing;
    s.loading = false;
    s.error = None;
    s.generation += 1;
}

fn set_playing(shared: &Arc<Mutex<Shared>>, playing: bool) {
    let mut s = shared.lock().unwrap();
    s.playing = playing;
    s.generation += 1;
}

fn set_error(shared: &Arc<Mutex<Shared>>, msg: String) {
    let mut s = shared.lock().unwrap();
    s.error = Some(msg);
    s.loading = false;
    s.playing = false;
    s.generation += 1;
}

/// Fit `(w, h)` into `max_w` wide, preserving aspect, never upscaling; even
/// dimensions keep the compositor happy.
fn fit_width(w: u32, h: u32, max_w: u32) -> (u32, u32) {
    if w == 0 || h == 0 {
        return (max_w & !1, (max_w * 9 / 16) & !1);
    }
    if w <= max_w {
        return (w.max(2) & !1, h.max(2) & !1);
    }
    let scale = max_w as f64 / w as f64;
    let nh = ((h as f64 * scale).round() as u32).max(2);
    (max_w & !1, nh & !1)
}

/// `h:mm:ss.cs` or `m:ss.cs` for a duration in seconds.
pub fn fmt_time(secs: f64) -> String {
    if secs <= 0.0 {
        return "0:00".to_string();
    }
    let total_cs = (secs * 100.0).round() as u64;
    let cs = total_cs % 100;
    let total_s = total_cs / 100;
    let s = total_s % 60;
    let m = (total_s / 60) % 60;
    let h = total_s / 3600;
    if h > 0 {
        format!("{h}:{m:02}:{s:02}.{cs:02}")
    } else {
        format!("{m}:{s:02}.{cs:02}")
    }
}
