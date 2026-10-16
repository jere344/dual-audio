//! Linux backend: talks the PulseAudio native protocol directly (pure Rust),
//! so it works with both PulseAudio and PipeWire (via pipewire-pulse).
//! Loopback capture records from a sink's `.monitor` source.

use std::ffi::{CStr, CString};

use futures::executor::block_on;
use pulseaudio::protocol::{
    self, ChannelMap, ChannelVolume, PlaybackStreamParams, Prop, Props, RecordStreamParams,
    SampleFormat, SampleSpec,
    stream::{BufferAttr, StreamFlags},
};
use pulseaudio::{AsPlaybackSource, Client};

use super::{
    CHANNELS, Capture, CaptureCallback, DeviceInfo, DeviceList, ErrorCallback, RenderCallback,
    Stream,
};

const CLIENT_NAME: &CStr = c"Dual Audio";
const BYTES_PER_FRAME: u32 = (CHANNELS * size_of::<f32>()) as u32;
const CAPTURE_FRAGMENT_MS: u32 = 10;
const PLAYBACK_TARGET_MS: u32 = 40;

fn connect() -> Result<Client, String> {
    Client::from_env(CLIENT_NAME).map_err(|e| format!("cannot connect to the sound server: {e}"))
}

fn cstr(value: &CStr) -> String {
    value.to_string_lossy().into_owned()
}

fn bytes_for_ms(rate: u32, ms: u32) -> u32 {
    rate * ms / 1000 * BYTES_PER_FRAME
}

fn stream_props(media_name: &str) -> Props {
    let mut props = Props::new();
    if let Ok(name) = CString::new(media_name) {
        props.set(Prop::MediaName, name.as_c_str());
    }
    props.set(Prop::ApplicationIconName, c"audio-headphones");
    props
}

fn stereo_spec(rate: u32) -> SampleSpec {
    SampleSpec {
        format: SampleFormat::Float32Le,
        channels: CHANNELS as u8,
        sample_rate: rate,
    }
}

pub fn list_devices() -> Result<DeviceList, String> {
    let client = connect()?;
    let server = block_on(client.server_info()).map_err(|e| e.to_string())?;
    let sinks = block_on(client.list_sinks()).map_err(|e| e.to_string())?;
    let outputs = sinks
        .into_iter()
        .filter(|sink| sink.monitor_source_name.is_some())
        .map(|sink| {
            let id = cstr(&sink.name);
            let name = sink
                .description
                .as_deref()
                .map(cstr)
                .filter(|d| !d.trim().is_empty())
                .unwrap_or_else(|| id.clone());
            DeviceInfo { id, name }
        })
        .collect();
    Ok(DeviceList {
        outputs,
        default_id: server.default_sink_name.as_deref().map(cstr),
    })
}

/// Records what `source` (or the default output) is playing.
pub fn open_capture(
    source: Option<&DeviceInfo>,
    mut on_data: CaptureCallback,
    _on_error: ErrorCallback,
) -> Result<Capture, String> {
    let client = connect()?;
    let sink = match source {
        Some(dev) => {
            let name = CString::new(dev.id.clone()).map_err(|e| e.to_string())?;
            block_on(client.sink_info_by_name(name))
        }
        None => block_on(client.sink_info_by_name(protocol::DEFAULT_SINK.to_owned())),
    }
    .map_err(|e| format!("output device not found: {e}"))?;
    let monitor = sink
        .monitor_source_name
        .clone()
        .ok_or("this output has no monitor source")?;
    let rate = sink.sample_spec.sample_rate.clamp(8000, 384_000);
    let device = DeviceInfo {
        id: cstr(&sink.name),
        name: sink
            .description
            .as_deref()
            .map(cstr)
            .unwrap_or_else(|| cstr(&sink.name)),
    };

    let params = RecordStreamParams {
        sample_spec: stereo_spec(rate),
        channel_map: ChannelMap::stereo(),
        source_name: Some(monitor),
        buffer_attr: BufferAttr {
            fragment_size: bytes_for_ms(rate, CAPTURE_FRAGMENT_MS),
            ..Default::default()
        },
        flags: StreamFlags {
            adjust_latency: true,
            no_move: true,
            ..Default::default()
        },
        props: stream_props("Dual Audio capture"),
        ..Default::default()
    };

    let mut samples: Vec<f32> = Vec::new();
    let mut partial: Vec<u8> = Vec::new();
    let callback = move |data: &[u8]| {
        partial.extend_from_slice(data);
        let whole = partial.len() / size_of::<f32>() * size_of::<f32>();
        samples.clear();
        samples.extend(
            partial[..whole]
                .chunks_exact(4)
                .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])),
        );
        partial.drain(..whole);
        if !samples.is_empty() {
            on_data(&samples);
        }
    };

    let stream = block_on(client.create_record_stream(params, callback))
        .map_err(|e| format!("cannot capture '{}': {e}", device.name))?;
    Ok(Capture {
        stream: Stream::new((client, stream)),
        sample_rate: rate,
        device,
    })
}

/// Plays audio produced by `render` on `device`.
pub fn open_output(
    device: &DeviceInfo,
    sample_rate: u32,
    mut render: RenderCallback,
    _on_error: ErrorCallback,
) -> Result<Stream, String> {
    let client = connect()?;
    let sink_name = CString::new(device.id.clone()).map_err(|e| e.to_string())?;
    let params = PlaybackStreamParams {
        sample_spec: stereo_spec(sample_rate),
        channel_map: ChannelMap::stereo(),
        sink_name: Some(sink_name),
        cvolume: Some(ChannelVolume::norm(CHANNELS as u8)),
        buffer_attr: BufferAttr {
            target_length: bytes_for_ms(sample_rate, PLAYBACK_TARGET_MS),
            ..Default::default()
        },
        flags: StreamFlags {
            adjust_latency: true,
            no_move: true,
            ..Default::default()
        },
        props: stream_props("Dual Audio mirror"),
        ..Default::default()
    };

    let mut samples: Vec<f32> = Vec::new();
    let callback = move |buf: &mut [u8]| {
        let frames = buf.len() / BYTES_PER_FRAME as usize;
        if frames == 0 {
            // Returning 0 would signal end-of-stream; send one silent frame instead.
            let n = buf.len().min(BYTES_PER_FRAME as usize).max(1);
            buf[..n].fill(0);
            return n;
        }
        samples.clear();
        samples.resize(frames * CHANNELS, 0.0);
        render(&mut samples);
        for (dst, s) in buf.chunks_exact_mut(4).zip(&samples) {
            dst.copy_from_slice(&s.to_le_bytes());
        }
        frames * BYTES_PER_FRAME as usize
    };

    let stream = block_on(client.create_playback_stream(params, callback.as_playback_source()))
        .map_err(|e| format!("cannot open '{}': {e}", device.name))?;
    Ok(Stream::new((client, stream)))
}
