//! Real-time audio output: pulls samples `contra_nes::Nes` generates each
//! frame and feeds them to the default output device via `cpal`. NES audio
//! is mono; it's duplicated across however many channels the output device
//! actually wants.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

/// How far behind real-time the audio buffer is allowed to get before we
/// start dropping the *oldest* samples to catch back up. This bounds
/// latency, not just prevents unbounded memory growth: under normal
/// operation the buffer sits at roughly one video frame's worth of samples
/// (~16ms) because we push once per simulated frame and the callback drains
/// continuously, so 150ms of headroom absorbs OS scheduling jitter without
/// making a pile-up (e.g. the window losing focus for a moment) turn into
/// audible seconds-long lag that never recovers.
const MAX_LATENCY_SECONDS: f64 = 0.15;

pub struct AudioOutput {
    _stream: cpal::Stream,
    buffer: Arc<Mutex<VecDeque<f32>>>,
    pub sample_rate: f64,
    max_buffered_samples: usize,
}

impl AudioOutput {
    /// Opens the default output device and starts playback immediately
    /// (the stream just plays silence until samples are pushed). Returns
    /// `None` if no output device is available or the stream can't be
    /// built; the caller should keep running silently in that case rather
    /// than treating it as fatal.
    pub fn new() -> Option<Self> {
        let host = cpal::default_host();
        let device = host.default_output_device().or_else(|| {
            log::warn!("no default audio output device found; running without sound");
            None
        })?;
        let config = device.default_output_config().ok()?;
        let sample_rate = config.sample_rate().0 as f64;
        let channels = config.channels() as usize;
        let max_buffered_samples = (sample_rate * MAX_LATENCY_SECONDS) as usize;

        let buffer: Arc<Mutex<VecDeque<f32>>> = Arc::new(Mutex::new(VecDeque::new()));
        let buffer_for_callback = buffer.clone();

        let stream_config: cpal::StreamConfig = config.into();
        let stream = device
            .build_output_stream(
                &stream_config,
                move |data: &mut [f32], _| {
                    let mut buf = buffer_for_callback.lock().unwrap();
                    for frame in data.chunks_mut(channels) {
                        let sample = buf.pop_front().unwrap_or(0.0);
                        for out in frame {
                            *out = sample;
                        }
                    }
                },
                move |err| log::error!("audio output stream error: {err}"),
                None,
            )
            .ok()?;

        if let Err(e) = stream.play() {
            log::error!("failed to start audio stream: {e}");
            return None;
        }

        log::info!("audio: {sample_rate} Hz, {channels} channel(s), max latency {}ms", (MAX_LATENCY_SECONDS * 1000.0) as u32);
        Some(Self { _stream: stream, buffer, sample_rate, max_buffered_samples })
    }

    pub fn push_samples(&self, samples: &[f32]) {
        if samples.is_empty() {
            return;
        }
        let mut buf = self.buffer.lock().unwrap();
        buf.extend(samples.iter().copied());
        // Drop from the front (oldest) rather than refusing new samples, so
        // playback always catches back up to low latency instead of queuing
        // an ever-growing backlog of stale audio.
        while buf.len() > self.max_buffered_samples {
            buf.pop_front();
        }
    }
}
