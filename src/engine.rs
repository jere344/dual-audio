//! Capture one device and fan the audio out to several output devices.
//!
//! ```text
//! loopback capture ──▶ per-output jitter buffer (volume) ──▶ output stream
//! ```
//!
//! Each output owns a small FIFO. The capture callback pushes into every FIFO
//! and the output callbacks pull from their own. The sync delay is the amount
//! of audio kept buffered before playback starts, so changing it never needs a
//! stream restart. Clock drift between devices is absorbed by trimming the
//! FIFO when it grows too large and re-priming it after an underrun.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::audio::{self, CHANNELS, DeviceInfo, Stream};

pub const MAX_DELAY_MS: u32 = 2000;
pub const MAX_VOLUME: f32 = 1.5;
/// Audio buffered on top of the sync delay before an output starts playing.
const PRIME_MS: u32 = 30;
/// How far above its target an output's buffer may grow before it is trimmed.
const SLACK_MS: u32 = 120;

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

#[derive(Default)]
struct AtomicF32(AtomicU32);

impl AtomicF32 {
    fn new(v: f32) -> Self {
        Self(AtomicU32::new(v.to_bits()))
    }
    fn get(&self) -> f32 {
        f32::from_bits(self.0.load(Ordering::Relaxed))
    }
    fn set(&self, v: f32) {
        self.0.store(v.to_bits(), Ordering::Relaxed);
    }
}

#[derive(Default)]
struct Fifo {
    samples: VecDeque<f32>,
    primed: bool,
    applied_delay_ms: u32,
}

/// Shared state of one output; touched by the capture thread, the output
/// thread and the UI.
pub struct Output {
    pub device: DeviceInfo,
    volume: AtomicF32,
    delay_ms: AtomicU32,
    fifo: Mutex<Fifo>,
    error: Mutex<Option<String>>,
    playing: AtomicBool,
}

impl Output {
    fn new(device: DeviceInfo, volume: f32, delay_ms: u32) -> Self {
        Self {
            device,
            volume: AtomicF32::new(volume.clamp(0.0, MAX_VOLUME)),
            delay_ms: AtomicU32::new(delay_ms.min(MAX_DELAY_MS)),
            fifo: Mutex::new(Fifo::default()),
            error: Mutex::new(None),
            playing: AtomicBool::new(false),
        }
    }

    fn push(&self, data: &[f32], rate: u32) {
        let vol = self.volume.get();
        let delay = self.delay_ms.load(Ordering::Relaxed);
        let mut fifo = lock(&self.fifo);
        if vol == 1.0 {
            fifo.samples.extend(data);
        } else {
            fifo.samples
                .extend(data.iter().map(|s| (s * vol).clamp(-1.0, 1.0)));
        }
        let limit = samples_for_ms(rate, delay + PRIME_MS + SLACK_MS);
        if fifo.samples.len() > limit {
            let keep = samples_for_ms(rate, delay + PRIME_MS);
            let excess = fifo.samples.len() - keep;
            fifo.samples.drain(..excess);
        }
    }

    fn render(&self, out: &mut [f32], rate: u32) {
        let delay = self.delay_ms.load(Ordering::Relaxed);
        let mut fifo = lock(&self.fifo);
        if delay > fifo.applied_delay_ms {
            // Longer delay: pause until enough audio is buffered again.
            fifo.primed = false;
        }
        fifo.applied_delay_ms = delay;
        if !fifo.primed && fifo.samples.len() >= samples_for_ms(rate, delay + PRIME_MS) {
            fifo.primed = true;
        }
        if !fifo.primed {
            out.fill(0.0);
            self.playing.store(false, Ordering::Relaxed);
            return;
        }
        let n = out.len().min(fifo.samples.len());
        for (dst, src) in out.iter_mut().zip(fifo.samples.drain(..n)) {
            *dst = src;
        }
        if n < out.len() {
            out[n..].fill(0.0);
            fifo.primed = false;
        }
        self.playing.store(true, Ordering::Relaxed);
    }

    pub fn set_volume(&self, volume: f32) {
        self.volume.set(volume.clamp(0.0, MAX_VOLUME));
    }

    pub fn set_delay_ms(&self, delay_ms: u32) {
        self.delay_ms
            .store(delay_ms.min(MAX_DELAY_MS), Ordering::Relaxed);
    }

    pub fn error(&self) -> Option<String> {
        lock(&self.error).clone()
    }

    pub fn is_playing(&self) -> bool {
        self.playing.load(Ordering::Relaxed)
    }
}

fn samples_for_ms(rate: u32, ms: u32) -> usize {
    (rate as u64 * ms as u64 / 1000) as usize * CHANNELS
}

#[derive(Default)]
struct Shared {
    outputs: Mutex<Vec<Arc<Output>>>,
    sample_rate: AtomicU32,
    peak: AtomicF32,
    last_data_ms: AtomicU64,
    capture_error: Mutex<Option<String>>,
    failed: AtomicBool,
}

/// Parameters for an output that should receive the shared audio.
#[derive(Clone, Debug)]
pub struct OutputSpec {
    pub device: DeviceInfo,
    pub volume: f32,
    pub delay_ms: u32,
}

/// A running share session. Dropping it stops all streams.
pub struct Engine {
    shared: Arc<Shared>,
    sample_rate: u32,
    source: DeviceInfo,
    started: Instant,
    outputs: HashMap<String, (Arc<Output>, Option<Stream>)>,
    capture: Option<Stream>,
}

impl Engine {
    /// Starts capturing `source` (`None` = system default output) and plays it on `outputs`.
    pub fn start(source: Option<&DeviceInfo>, outputs: &[OutputSpec]) -> Result<Self, String> {
        let shared = Arc::new(Shared::default());
        let started = Instant::now();

        let on_data = {
            let shared = shared.clone();
            move |data: &[f32]| {
                let rate = shared.sample_rate.load(Ordering::Relaxed);
                let peak = data.iter().fold(0.0f32, |m, s| m.max(s.abs()));
                shared.peak.set(peak);
                shared
                    .last_data_ms
                    .store(started.elapsed().as_millis() as u64, Ordering::Relaxed);
                for out in lock(&shared.outputs).iter() {
                    out.push(data, rate);
                }
            }
        };
        let on_error = {
            let shared = shared.clone();
            move |msg: String| {
                *lock(&shared.capture_error) = Some(msg);
                shared.failed.store(true, Ordering::Relaxed);
            }
        };

        let capture = audio::open_capture(source, Box::new(on_data), Box::new(on_error))?;
        shared
            .sample_rate
            .store(capture.sample_rate, Ordering::Relaxed);

        let mut engine = Self {
            shared,
            sample_rate: capture.sample_rate,
            source: capture.device,
            started,
            outputs: HashMap::new(),
            capture: Some(capture.stream),
        };
        for spec in outputs {
            engine.add_output(spec);
        }
        Ok(engine)
    }

    pub fn source(&self) -> &DeviceInfo {
        &self.source
    }

    /// Adds an output; failures are reported through [`Output::error`].
    pub fn add_output(&mut self, spec: &OutputSpec) {
        if self.outputs.contains_key(&spec.device.id) {
            return;
        }
        let out = Arc::new(Output::new(spec.device.clone(), spec.volume, spec.delay_ms));
        let rate = self.sample_rate;
        let render = {
            let out = out.clone();
            move |buf: &mut [f32]| out.render(buf, rate)
        };
        let on_error = {
            let out = out.clone();
            move |msg: String| *lock(&out.error) = Some(msg)
        };
        let stream =
            match audio::open_output(&spec.device, rate, Box::new(render), Box::new(on_error)) {
                Ok(stream) => {
                    lock(&self.shared.outputs).push(out.clone());
                    Some(stream)
                }
                Err(e) => {
                    *lock(&out.error) = Some(e);
                    None
                }
            };
        self.outputs.insert(spec.device.id.clone(), (out, stream));
    }

    pub fn remove_output(&mut self, id: &str) {
        lock(&self.shared.outputs).retain(|o| o.device.id != id);
        self.outputs.remove(id);
    }

    pub fn output(&self, id: &str) -> Option<&Arc<Output>> {
        self.outputs.get(id).map(|(o, _)| o)
    }

    pub fn output_count(&self) -> usize {
        self.outputs.len()
    }

    /// Peak level of the most recent captured block, or 0 when no audio is flowing.
    pub fn peak(&self) -> f32 {
        let now = self.started.elapsed().as_millis() as u64;
        let last = self.shared.last_data_ms.load(Ordering::Relaxed);
        if last == 0 || now.saturating_sub(last) > 300 {
            0.0
        } else {
            self.shared.peak.get()
        }
    }

    /// Set when the capture stream died (device removed, server gone, ...).
    pub fn capture_error(&self) -> Option<String> {
        if self.shared.failed.load(Ordering::Relaxed) {
            lock(&self.shared.capture_error).clone()
        } else {
            None
        }
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.capture.take();
        lock(&self.shared.outputs).clear();
        self.outputs.clear();
    }
}

/// Plays a short sine tone on `device` from a background thread.
pub fn play_test_tone(device: DeviceInfo, on_error: impl Fn(String) + Send + 'static) {
    std::thread::spawn(move || {
        const RATE: u32 = 48_000;
        const SECONDS: f32 = 0.9;
        const FREQ: f32 = 440.0;
        const VOLUME: f32 = 0.25;
        let total = (RATE as f32 * SECONDS) as usize;
        let fade = (RATE / 100) as usize;
        let mut i = 0usize;
        let render = move |buf: &mut [f32]| {
            for frame in buf.chunks_exact_mut(CHANNELS) {
                let v = if i < total {
                    let env = (i.min(total - i) as f32 / fade as f32).min(1.0);
                    let t = i as f32 / RATE as f32;
                    VOLUME * env * (std::f32::consts::TAU * FREQ * t).sin()
                } else {
                    0.0
                };
                frame.fill(v);
                i += 1;
            }
        };
        match audio::open_output(&device, RATE, Box::new(render), Box::new(|_| {})) {
            Ok(stream) => {
                std::thread::sleep(Duration::from_secs_f32(SECONDS + 0.4));
                drop(stream);
            }
            Err(e) => on_error(e),
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// End-to-end check against a live audio server. Needs two scratch sinks:
    /// `DUAL_TEST_SRC=<sink id> DUAL_TEST_DST=<sink id> cargo test -- --ignored`
    #[test]
    #[ignore]
    fn mirrors_audio_between_devices() {
        let (Ok(src), Ok(dst)) = (
            std::env::var("DUAL_TEST_SRC"),
            std::env::var("DUAL_TEST_DST"),
        ) else {
            panic!("set DUAL_TEST_SRC and DUAL_TEST_DST");
        };
        let list = audio::list_devices().unwrap();
        let find = |id: &str| list.outputs.iter().find(|d| d.id == id).unwrap().clone();
        let (src, dst) = (find(&src), find(&dst));

        // Feed a continuous tone into the source device.
        let mut phase = 0.0f32;
        let tone = audio::open_output(
            &src,
            48_000,
            Box::new(move |buf| {
                for frame in buf.chunks_exact_mut(CHANNELS) {
                    phase = (phase + 440.0 / 48_000.0).fract();
                    frame.fill(0.5 * (std::f32::consts::TAU * phase).sin());
                }
            }),
            Box::new(|e| panic!("{e}")),
        )
        .unwrap();

        let spec = OutputSpec {
            device: dst.clone(),
            volume: 0.5,
            delay_ms: 200,
        };
        let engine = Engine::start(Some(&src), &[spec]).unwrap();

        // Listen to what reaches the destination device.
        let peak = Arc::new(AtomicF32::default());
        let seen = peak.clone();
        let probe = audio::open_capture(
            Some(&dst),
            Box::new(move |d| {
                let p = d.iter().fold(0.0f32, |m, s| m.max(s.abs()));
                seen.set(seen.get().max(p));
            }),
            Box::new(|e| panic!("{e}")),
        )
        .unwrap();

        std::thread::sleep(Duration::from_millis(1500));
        let src_peak = engine.peak();
        let dst_peak = peak.get();
        let out = engine.output(&dst.id).unwrap();
        println!("source peak {src_peak:.3}, destination peak {dst_peak:.3}");
        assert!(out.error().is_none(), "{:?}", out.error());
        assert!(out.is_playing());
        assert!(src_peak > 0.3, "capture saw {src_peak}");
        // Volume 0.5 applied to a 0.5 tone.
        assert!((dst_peak - 0.25).abs() < 0.05, "destination saw {dst_peak}");
        drop((probe, engine, tone));
    }

    #[test]
    fn fifo_honours_delay_and_trims() {
        let out = Output::new(
            DeviceInfo {
                id: "x".into(),
                name: "x".into(),
            },
            1.0,
            100,
        );
        let rate = 1000;
        let mut buf = vec![0.0; 20];
        out.push(&vec![1.0; samples_for_ms(rate, 50)], rate);
        out.render(&mut buf, rate);
        assert!(buf.iter().all(|&s| s == 0.0), "must wait for delay + prime");
        out.push(&vec![1.0; samples_for_ms(rate, 100)], rate);
        out.render(&mut buf, rate);
        assert!(buf.iter().all(|&s| s == 1.0));
        out.push(&vec![1.0; samples_for_ms(rate, 1000)], rate);
        let len = lock(&out.fifo).samples.len();
        assert_eq!(len, samples_for_ms(rate, 100 + PRIME_MS));
    }
}
