//! Windows backend: WASAPI through cpal. Building an input stream on an
//! output (render) device makes cpal open it in loopback mode, and output
//! streams use AUTOCONVERTPCM so any sample rate is accepted.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{BufferSize, Device, SampleFormat, StreamConfig};

use super::{
    CHANNELS, Capture, CaptureCallback, DeviceInfo, DeviceList, ErrorCallback, RenderCallback,
    Stream,
};

fn host() -> cpal::Host {
    cpal::default_host()
}

fn device_id(device: &Device) -> Option<String> {
    device.id().ok().map(|id| id.to_string())
}

fn device_name(device: &Device, fallback: usize) -> String {
    device
        .description()
        .ok()
        .map(|d| d.name().trim().to_owned())
        .filter(|n| !n.is_empty() && !n.starts_with('{'))
        .unwrap_or_else(|| format!("Audio output #{fallback}"))
}

fn find_output(id: &str) -> Result<Device, String> {
    host()
        .output_devices()
        .map_err(|e| e.to_string())?
        .find(|d| device_id(d).as_deref() == Some(id))
        .ok_or_else(|| "output device not found".to_owned())
}

pub fn list_devices() -> Result<DeviceList, String> {
    let host = host();
    let outputs = host
        .output_devices()
        .map_err(|e| e.to_string())?
        .enumerate()
        .filter_map(|(i, d)| {
            Some(DeviceInfo {
                id: device_id(&d)?,
                name: device_name(&d, i + 1),
            })
        })
        .collect();
    let default_id = host.default_output_device().as_ref().and_then(device_id);
    Ok(DeviceList {
        outputs,
        default_id,
    })
}

/// Pushes `data` (any channel count) to `out` as interleaved stereo.
fn to_stereo<T: Copy>(data: &[T], channels: usize, conv: impl Fn(T) -> f32, out: &mut Vec<f32>) {
    out.clear();
    if channels == 0 {
        return;
    }
    for frame in data.chunks_exact(channels) {
        let l = conv(frame[0]);
        let r = if channels > 1 { conv(frame[1]) } else { l };
        out.push(l);
        out.push(r);
    }
}

/// Records what `source` (or the default output) is playing, via WASAPI loopback.
pub fn open_capture(
    source: Option<&DeviceInfo>,
    mut on_data: CaptureCallback,
    on_error: ErrorCallback,
) -> Result<Capture, String> {
    let device = match source {
        Some(dev) => find_output(&dev.id)?,
        None => host()
            .default_output_device()
            .ok_or("no default output device")?,
    };
    let info = DeviceInfo {
        id: device_id(&device).unwrap_or_default(),
        name: source
            .map(|d| d.name.clone())
            .unwrap_or_else(|| device_name(&device, 0)),
    };
    let supported = device
        .default_output_config()
        .map_err(|e| format!("cannot query '{}': {e}", info.name))?;
    let channels = supported.channels() as usize;
    let sample_rate = supported.sample_rate();
    let config = StreamConfig {
        channels: supported.channels(),
        sample_rate,
        buffer_size: BufferSize::Default,
    };

    let err = move |e: cpal::Error| on_error(e.to_string());
    let mut stereo = Vec::new();
    let stream = match supported.sample_format() {
        SampleFormat::F32 => device.build_input_stream::<f32, _, _>(
            config,
            move |data, _| {
                to_stereo(data, channels, |s| s, &mut stereo);
                on_data(&stereo);
            },
            err,
            None,
        ),
        SampleFormat::I16 => device.build_input_stream::<i16, _, _>(
            config,
            move |data, _| {
                to_stereo(data, channels, |s| s as f32 / 32768.0, &mut stereo);
                on_data(&stereo);
            },
            err,
            None,
        ),
        SampleFormat::I32 => device.build_input_stream::<i32, _, _>(
            config,
            move |data, _| {
                to_stereo(data, channels, |s| s as f32 / 2_147_483_648.0, &mut stereo);
                on_data(&stereo);
            },
            err,
            None,
        ),
        other => return Err(format!("unsupported capture format {other:?}")),
    }
    .map_err(|e| format!("cannot capture '{}': {e}", info.name))?;
    stream.play().map_err(|e| e.to_string())?;

    Ok(Capture {
        stream: Stream::new(stream),
        sample_rate,
        device: info,
    })
}

/// Plays audio produced by `render` on `device`.
pub fn open_output(
    device: &DeviceInfo,
    sample_rate: u32,
    mut render: RenderCallback,
    on_error: ErrorCallback,
) -> Result<Stream, String> {
    let dev = find_output(&device.id)?;
    let config = StreamConfig {
        channels: CHANNELS as u16,
        sample_rate,
        buffer_size: BufferSize::Default,
    };
    let stream = dev
        .build_output_stream::<f32, _, _>(
            config,
            move |data, _| render(data),
            move |e| on_error(e.to_string()),
            None,
        )
        .map_err(|e| format!("cannot open '{}': {e}", device.name))?;
    stream.play().map_err(|e| e.to_string())?;
    Ok(Stream::new(stream))
}
