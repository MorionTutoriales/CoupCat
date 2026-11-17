// SPDX-License-Identifier: AGPL-3.0-or-later
//
//! Audio output for the preview monitor.
//!
//! The export does the full engine mix; for the live monitor this plays the
//! sound of the clip audible at the playhead through the default output device
//! (cpal), decoded from that point with `concat_media`. Enough to hear the
//! video while editing; it is rebuilt on every play/seek so it stays in step
//! with where the playhead is.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use concat_media::{AudioDecoder, AudioOptions, SampleFormat};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

/// Holds the output device and the currently playing stream (dropping the
/// stream stops the sound).
pub struct AudioOut {
    device: cpal::Device,
    config: cpal::StreamConfig,
    sample_format: cpal::SampleFormat,
    rate: u32,
    channels: u16,
    stream: Option<cpal::Stream>,
}

impl AudioOut {
    /// Open the default output device, or `None` if there is none.
    pub fn new() -> Option<AudioOut> {
        let host = cpal::default_host();
        let device = host.default_output_device()?;
        let supported = device.default_output_config().ok()?;
        let sample_format = supported.sample_format();
        let rate = supported.sample_rate().0;
        let channels = supported.channels();
        Some(AudioOut {
            device,
            config: supported.into(),
            sample_format,
            rate,
            channels,
            stream: None,
        })
    }

    /// Stop any current sound.
    pub fn stop(&mut self) {
        self.stream = None;
    }

    /// Play `path`'s audio from `source_start` seconds, at `volume`. Replaces
    /// whatever was playing. A clip with no audio simply plays nothing.
    pub fn play(&mut self, path: &str, source_start: f64, volume: f32) {
        self.stop();

        let opts = AudioOptions {
            start: Some(source_start.max(0.0)),
            duration: None,
            filters: Vec::new(),
            rate: self.rate,
            channels: self.channels,
            format: SampleFormat::F32,
            stream: None,
        };
        let samples = match AudioDecoder::open(path, &opts).and_then(|mut d| d.collect_f32()) {
            Ok(s) if !s.is_empty() => s,
            _ => return,
        };

        let buf = Arc::new(samples);
        let cursor = Arc::new(AtomicUsize::new(0));
        let stream = match self.sample_format {
            cpal::SampleFormat::F32 => self.build::<f32>(buf, cursor, volume),
            cpal::SampleFormat::I16 => self.build::<i16>(buf, cursor, volume),
            cpal::SampleFormat::U16 => self.build::<u16>(buf, cursor, volume),
            _ => None,
        };
        if let Some(stream) = stream {
            let _ = stream.play();
            self.stream = Some(stream);
        }
    }

    fn build<T>(
        &self,
        buf: Arc<Vec<f32>>,
        cursor: Arc<AtomicUsize>,
        volume: f32,
    ) -> Option<cpal::Stream>
    where
        T: cpal::SizedSample + cpal::FromSample<f32>,
    {
        self.device
            .build_output_stream(
                &self.config,
                move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
                    let start = cursor.fetch_add(data.len(), Ordering::Relaxed);
                    for (i, out) in data.iter_mut().enumerate() {
                        let s = buf.get(start + i).copied().unwrap_or(0.0) * volume;
                        *out = T::from_sample(s);
                    }
                },
                |_err| {},
                None,
            )
            .ok()
    }
}
