//! Real-time audio output: pulls samples `contra_nes::Nes` generates each
//! frame and feeds them to the default output device via `cpal`. NES audio
//! is mono; it's duplicated across however many channels the output device
//! actually wants.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

/// Capped so a paused/minimized window (which stops draining the buffer)
/// can't grow this unboundedly; a few seconds of headroom is plenty.
const MAX_BUFFERED_SAMPLES: usize = 44_100 * 2;

pub struct AudioOutput {
    _stream: cpal::Stream,
    buffer: Arc<Mutex<VecDeque<f32>>>,
    pub sample_rate: f64,
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

        Some(Self { _stream: stream, buffer, sample_rate })
    }

    pub fn push_samples(&self, samples: &[f32]) {
        if samples.is_empty() {
            return;
        }
        let mut buf = self.buffer.lock().unwrap();
        buf.extend(samples.iter().copied());
        while buf.len() > MAX_BUFFERED_SAMPLES {
            buf.pop_front();
        }
    }
}
