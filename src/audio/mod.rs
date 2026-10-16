//! Platform audio backends.
//!
//! Every backend exposes the same small surface: enumerate output devices,
//! capture what an output device is playing (loopback), and open a playback
//! stream on an output device. All streams use interleaved stereo `f32`; the
//! platform audio server converts to the device format when needed.

#[cfg(target_os = "linux")]
mod pulse;
#[cfg(target_os = "linux")]
pub use pulse::*;

#[cfg(windows)]
mod wasapi;
#[cfg(windows)]
pub use wasapi::*;

pub const CHANNELS: usize = 2;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceInfo {
    /// Stable backend identifier (PulseAudio sink name / WASAPI endpoint id).
    pub id: String,
    /// Human readable name.
    pub name: String,
}

#[derive(Clone, Debug, Default)]
pub struct DeviceList {
    pub outputs: Vec<DeviceInfo>,
    pub default_id: Option<String>,
}

impl DeviceList {
    pub fn default_device(&self) -> Option<&DeviceInfo> {
        let id = self.default_id.as_ref()?;
        self.outputs.iter().find(|d| &d.id == id)
    }
}

/// Receives interleaved stereo samples from a capture stream.
pub type CaptureCallback = Box<dyn FnMut(&[f32]) + Send + 'static>;
/// Fills an interleaved stereo buffer for a playback stream.
pub type RenderCallback = Box<dyn FnMut(&mut [f32]) + Send + 'static>;
/// Reports a fatal stream error.
pub type ErrorCallback = Box<dyn Fn(String) + Send + Sync + 'static>;

/// A running stream. Dropping it stops the stream.
pub struct Stream {
    _inner: Box<dyn Send>,
}

impl Stream {
    pub(crate) fn new(inner: impl Send + 'static) -> Self {
        Self {
            _inner: Box::new(inner),
        }
    }
}

/// A capture stream together with the sample rate it delivers.
pub struct Capture {
    pub stream: Stream,
    pub sample_rate: u32,
    pub device: DeviceInfo,
}
