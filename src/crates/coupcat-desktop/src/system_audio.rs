// SPDX-License-Identifier: AGPL-3.0-or-later
//
//! The computer's own sound, for the screen recorder (#51): what the default
//! output device plays is captured (WASAPI loopback, which cpal turns on when
//! an input stream is opened on an output device) and handed to ffmpeg over a
//! local socket as raw samples.
//!
//! Windows delivers nothing while nothing is playing, so the stream is laid out
//! by the clock: each block goes where its arrival time says, and whatever the
//! device has not provided in time is sent as silence. The sound track therefore
//! lasts exactly as long as the take and stays in step with the picture.

use std::collections::VecDeque;
use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

/// Sound is always sent as stereo.
pub const CHANNELS: usize = 2;
/// How late the device may be before its gap is filled with silence.
const LATENCY: Duration = Duration::from_millis(120);
/// Blocks this close to the end of the stream are joined to it; a wider gap is
/// real silence.
const JOIN: Duration = Duration::from_millis(40);
/// A stream this far ahead of a block's time means the device runs fast; the
/// block is dropped so the sound does not drift behind the picture.
const AHEAD: Duration = Duration::from_millis(200);

/// The name of the output device whose sound would be recorded.
pub fn device_name() -> Option<String> {
    cpal::default_host().default_output_device()?.name().ok()
}

/// A capture waiting for, or feeding, one reader. Dropping it ends it.
pub struct Capture {
    port: u16,
    rate: u32,
    /// The wall-clock time of the first frame sent, in microseconds.
    epoch: u128,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Capture {
    /// Open the default output device and wait for ffmpeg to connect.
    pub fn open() -> Result<Capture, String> {
        let fail = |e: &dyn std::fmt::Display| format!("No se pudo grabar el audio del sistema: {e}");
        let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| fail(&e))?;
        listener.set_nonblocking(true).map_err(|e| fail(&e))?;
        let port = listener.local_addr().map_err(|e| fail(&e))?.port();
        let stop = Arc::new(AtomicBool::new(false));
        let (ready, opened) = mpsc::channel();
        let flag = stop.clone();
        // A cpal stream may not cross threads: one thread owns it throughout.
        let thread = std::thread::spawn(move || {
            let queue = Arc::new(Mutex::new(VecDeque::new()));
            match listen(queue.clone()) {
                Ok((stream, rate)) => {
                    let started = Instant::now();
                    let epoch = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .map_or(0, |d| d.as_micros());
                    let _ = ready.send(Ok((rate, epoch)));
                    feed(&listener, &queue, rate, started, &flag);
                    drop(stream);
                }
                Err(e) => {
                    let _ = ready.send(Err(e));
                }
            }
        });
        let (rate, epoch) = opened
            .recv()
            .map_err(|e| fail(&e))?
            .map_err(|e| fail(&e))?;
        Ok(Capture {
            port,
            rate,
            epoch,
            stop,
            thread: Some(thread),
        })
    }

    /// When the stream begins, in microseconds of wall-clock time.
    pub fn epoch(&self) -> u128 {
        self.epoch
    }

    /// The ffmpeg input that reads this capture.
    pub fn input_args(&self) -> Vec<String> {
        input_args(self.rate, self.port)
    }
}

pub fn input_args(rate: u32, port: u16) -> Vec<String> {
    [
        "-probesize",
        "32",
        "-analyzeduration",
        "0",
        "-f",
        "s16le",
        "-ar",
        &rate.to_string(),
        "-ch_layout",
        "stereo",
        "-i",
        &format!("tcp://127.0.0.1:{port}"),
    ]
    .map(String::from)
    .to_vec()
}

impl Drop for Capture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Samples as the device delivered them, with the moment they arrived.
struct Block {
    at: Instant,
    frames: Vec<[i16; CHANNELS]>,
}

type Queue = Arc<Mutex<VecDeque<Block>>>;

/// Start capturing the default output device into `queue`.
fn listen(queue: Queue) -> Result<(cpal::Stream, u32), String> {
    let device = cpal::default_host()
        .default_output_device()
        .ok_or("no hay dispositivo de salida")?;
    let config = device.default_output_config().map_err(|e| e.to_string())?;
    let rate = config.sample_rate().0;
    let format = config.sample_format();
    let config: cpal::StreamConfig = config.into();
    let channels = usize::from(config.channels.max(1));
    let stream = match format {
        cpal::SampleFormat::F32 => build(&device, &config, channels, queue, |s: f32| {
            (s.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16
        }),
        cpal::SampleFormat::I16 => build(&device, &config, channels, queue, |s: i16| s),
        cpal::SampleFormat::I32 => {
            build(&device, &config, channels, queue, |s: i32| (s >> 16) as i16)
        }
        other => return Err(format!("formato de audio no admitido ({other})")),
    }
    .map_err(|e| e.to_string())?;
    stream.play().map_err(|e| e.to_string())?;
    Ok((stream, rate))
}

fn build<T: cpal::SizedSample + 'static>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    channels: usize,
    queue: Queue,
    convert: fn(T) -> i16,
) -> Result<cpal::Stream, cpal::BuildStreamError> {
    device.build_input_stream(
        config,
        move |data: &[T], _: &cpal::InputCallbackInfo| {
            let at = Instant::now();
            let frames = data
                .chunks_exact(channels)
                .map(|frame| {
                    // The front pair of a surround device; mono is doubled.
                    let left = convert(frame[0]);
                    let right = frame.get(1).map_or(left, |s| convert(*s));
                    [left, right]
                })
                .collect();
            if let Ok(mut q) = queue.lock() {
                q.push_back(Block { at, frames });
            }
        },
        |e| log_error(&e),
        None,
    )
}

fn log_error(e: &cpal::StreamError) {
    eprintln!("audio del sistema: {e}");
}

/// Wait for the reader, then send it the sound since `started`, in step with
/// the clock, until the capture is dropped or the reader goes away.
fn feed(listener: &TcpListener, queue: &Queue, rate: u32, started: Instant, stop: &AtomicBool) {
    let mut socket: TcpStream = loop {
        if stop.load(Ordering::Relaxed) {
            return;
        }
        match listener.accept() {
            Ok((socket, _)) => break socket,
            Err(_) => std::thread::sleep(Duration::from_millis(5)),
        }
    };
    if socket.set_nonblocking(false).is_err() {
        return;
    }
    let mut sent = 0_u64;
    let mut bytes = Vec::new();
    while !stop.load(Ordering::Relaxed) {
        std::thread::sleep(Duration::from_millis(10));
        bytes.clear();
        if let Ok(mut q) = queue.lock() {
            for block in q.drain(..) {
                if let Some(age) = block.at.checked_duration_since(started) {
                    sent += place(&block.frames, rate, age, sent, &mut bytes);
                }
            }
        }
        let overdue = started.elapsed().saturating_sub(LATENCY);
        sent += silence_until(frames(rate, overdue), sent, &mut bytes);
        if socket.write_all(&bytes).is_err() {
            return;
        }
    }
}

/// How many frames fit in `time`.
fn frames(rate: u32, time: Duration) -> u64 {
    (time.as_secs_f64() * f64::from(rate)) as u64
}

/// Add silence to `out` until the stream, `sent` frames long, reaches `end`.
/// Returns the frames added.
fn silence_until(end: u64, sent: u64, out: &mut Vec<u8>) -> u64 {
    let gap = end.saturating_sub(sent);
    out.resize(out.len() + gap as usize * CHANNELS * 2, 0);
    gap
}

/// Add to `out` a block whose last frame arrived `arrived` after the start, for
/// a stream already `sent` frames long. Returns the frames added.
fn place(
    block: &[[i16; CHANNELS]],
    rate: u32,
    arrived: Duration,
    sent: u64,
    out: &mut Vec<u8>,
) -> u64 {
    let begins = frames(rate, arrived).saturating_sub(block.len() as u64);
    if sent > begins + frames(rate, AHEAD) {
        return 0;
    }
    let gap = if begins > sent + frames(rate, JOIN) {
        silence_until(begins, sent, out)
    } else {
        0
    };
    for frame in block {
        for sample in frame {
            out.extend_from_slice(&sample.to_le_bytes());
        }
    }
    gap + block.len() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: fn(u64) -> Duration = Duration::from_millis;

    #[test]
    fn a_block_after_a_pause_lands_at_its_own_time() {
        let mut out = Vec::new();
        // 10 frames arriving at 1 s on an empty stream: silence, then them.
        let added = place(&[[1, 2]; 10], 1000, MS(1000), 0, &mut out);
        assert_eq!(added, 1000);
        assert!(out[..990 * 4].iter().all(|b| *b == 0));
        assert_eq!(&out[990 * 4..][..4], &[1, 0, 2, 0]);
    }

    #[test]
    fn consecutive_blocks_are_joined_despite_jitter() {
        let mut out = Vec::new();
        // The stream ends at 100; this block would begin at 120.
        assert_eq!(place(&[[1, 1]; 10], 1000, MS(130), 100, &mut out), 10);
        assert_eq!(out.len(), 40);
    }

    #[test]
    fn a_stream_running_ahead_drops_blocks() {
        let mut out = Vec::new();
        assert_eq!(place(&[[1, 1]; 10], 1000, MS(100), 400, &mut out), 0);
        assert!(out.is_empty());
    }

    #[test]
    fn silence_never_goes_backwards() {
        let mut out = Vec::new();
        assert_eq!(silence_until(50, 80, &mut out), 0);
        assert_eq!(silence_until(100, 80, &mut out), 20);
        assert_eq!(out.len(), 80);
    }
}
