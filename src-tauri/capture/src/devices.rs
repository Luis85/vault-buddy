//! cpal glue: opens the default microphone (and, in meeting mode on
//! Windows, WASAPI loopback on the default output) and pushes raw sample
//! chunks into the session's mpsc channels. All sample-format conversion
//! beyond f32 widening happens in the session worker, not here.

use crate::session::{SourceInput, SourceMsg};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::mpsc::Sender;

pub struct DeviceInfo {
    pub name: String,
    pub is_default: bool,
}

pub struct DeviceList {
    pub inputs: Vec<DeviceInfo>,
    pub outputs: Vec<DeviceInfo>,
}

/// Enumerate capture-relevant devices by name. Never errors: an
/// enumeration failure (or a device-less CI box) yields empty lists —
/// the settings UI shows "System default" alone in that case.
pub fn list_devices() -> DeviceList {
    let host = cpal::default_host();
    let default_in = host.default_input_device().and_then(|d| d.name().ok());
    let default_out = host.default_output_device().and_then(|d| d.name().ok());
    let inputs = host
        .input_devices()
        .map(|it| it.filter_map(|d| d.name().ok()).collect::<Vec<_>>())
        .unwrap_or_default();
    let outputs = host
        .output_devices()
        .map(|it| it.filter_map(|d| d.name().ok()).collect::<Vec<_>>())
        .unwrap_or_default();
    DeviceList {
        inputs: to_device_infos(inputs, &default_in),
        outputs: to_device_infos(outputs, &default_out),
    }
}

fn to_device_infos(names: Vec<String>, default: &Option<String>) -> Vec<DeviceInfo> {
    names
        .into_iter()
        .map(|name| DeviceInfo {
            is_default: Some(&name) == default.as_ref(),
            name,
        })
        .collect()
}

pub struct OpenSources {
    pub inputs: Vec<SourceInput>,
    pub streams: Vec<cpal::Stream>,
    /// Configured-but-missing device fallbacks — recording proceeded on
    /// defaults; the caller surfaces these (stale config never blocks).
    pub warnings: Vec<String>,
}

fn build_stream(
    device: &cpal::Device,
    config: &cpal::SupportedStreamConfig,
    tx: Sender<SourceMsg>,
) -> Result<cpal::Stream, String> {
    let err_tx = tx.clone();
    let on_error = move |e: cpal::StreamError| {
        log::warn!("capture stream error: {e}");
        let _ = err_tx.send(SourceMsg::Lost);
    };
    let stream_config: cpal::StreamConfig = config.config();
    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => device.build_input_stream(
            &stream_config,
            move |data: &[f32], _| {
                let _ = tx.send(SourceMsg::Samples(data.to_vec()));
            },
            on_error,
            None,
        ),
        cpal::SampleFormat::I16 => device.build_input_stream(
            &stream_config,
            move |data: &[i16], _| {
                let samples = data.iter().map(|s| *s as f32 / i16::MAX as f32).collect();
                let _ = tx.send(SourceMsg::Samples(samples));
            },
            on_error,
            None,
        ),
        cpal::SampleFormat::U16 => device.build_input_stream(
            &stream_config,
            move |data: &[u16], _| {
                let samples = data
                    .iter()
                    .map(|s| (*s as f32 / u16::MAX as f32) * 2.0 - 1.0)
                    .collect();
                let _ = tx.send(SourceMsg::Samples(samples));
            },
            on_error,
            None,
        ),
        other => return Err(format!("unsupported sample format {other:?}")),
    }
    .map_err(|e| e.to_string())?;
    stream.play().map_err(|e| e.to_string())?;
    Ok(stream)
}

/// Resolve a configured device by exact name against the live device set;
/// None = not found (caller falls back to the default with a warning).
fn find_by_name<I: Iterator<Item = cpal::Device>>(devices: I, name: &str) -> Option<cpal::Device> {
    let mut devices = devices;
    devices.find(|d| d.name().map(|n| n == name).unwrap_or(false))
}

pub fn open_sources(
    meeting_mode: bool,
    preferred_input: Option<&str>,
    preferred_output: Option<&str>,
) -> Result<OpenSources, String> {
    let host = cpal::default_host();
    let mut inputs = Vec::new();
    let mut streams = Vec::new();
    let mut warnings = Vec::new();
    #[cfg(not(windows))]
    let _ = preferred_output; // loopback (and its device pick) is Windows-only

    let mic = match preferred_input {
        Some(name) => match host
            .input_devices()
            .ok()
            .and_then(|it| find_by_name(it, name))
        {
            Some(device) => Some(device),
            None => {
                warnings.push(format!(
                    "Configured microphone \"{name}\" not found — using the default input device"
                ));
                None
            }
        },
        None => None,
    };
    let mic = match mic {
        Some(device) => device,
        None => host
            .default_input_device()
            .ok_or("No microphone found — check Windows sound settings.")?,
    };
    let mic_config = mic
        .default_input_config()
        .map_err(|e| format!("Microphone unavailable: {e}"))?;
    let (mic_tx, mic_rx) = std::sync::mpsc::channel();
    let mic_name = mic.name().unwrap_or_else(|_| "Microphone".to_string());
    streams.push(build_stream(&mic, &mic_config, mic_tx)?);
    inputs.push(SourceInput {
        name: mic_name,
        rate: mic_config.sample_rate().0,
        channels: mic_config.channels(),
        rx: mic_rx,
    });

    #[cfg(windows)]
    if meeting_mode {
        // WASAPI loopback: cpal exposes it by building an *input* stream on
        // an *output* device — you get exactly what the speakers play.
        let output = match preferred_output {
            Some(name) => match host
                .output_devices()
                .ok()
                .and_then(|it| find_by_name(it, name))
            {
                Some(device) => Some(device),
                None => {
                    warnings.push(format!(
                        "Configured output device \"{name}\" not found — using the default output device"
                    ));
                    None
                }
            },
            None => None,
        };
        let output = match output {
            Some(device) => device,
            None => host
                .default_output_device()
                .ok_or("Desktop audio (loopback) unavailable: no default output device")?,
        };
        let config = output
            .default_output_config()
            .map_err(|e| format!("Desktop audio (loopback) unavailable: {e}"))?;
        let (tx, rx) = std::sync::mpsc::channel();
        let name = format!(
            "{} (loopback)",
            output.name().unwrap_or_else(|_| "Speakers".to_string())
        );
        streams.push(build_stream(&output, &config, tx)?);
        inputs.push(SourceInput {
            name,
            rate: config.sample_rate().0,
            channels: config.channels(),
            rx,
        });
    }
    #[cfg(not(windows))]
    if meeting_mode {
        log::warn!("desktop audio loopback is Windows-only; recording mic only");
    }

    Ok(OpenSources {
        inputs,
        streams,
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// CI runners have no audio devices; these assert the error paths are
    /// clean human-readable Errs (or graceful fallbacks), never panics. On
    /// a dev machine with devices they exercise the success paths instead.
    #[test]
    fn open_sources_never_panics() {
        match open_sources(true, None, None) {
            Ok(open) => {
                assert!(!open.inputs.is_empty());
                assert!(!open.inputs[0].name.is_empty(), "mic source is named");
            }
            Err(message) => {
                assert!(!message.is_empty());
            }
        }
    }

    #[test]
    fn missing_preferred_input_falls_back_with_a_warning() {
        // Stale config must never block recording: an unplugged configured
        // device degrades to the default plus a warning naming it.
        match open_sources(false, Some("No Such Device 9000"), None) {
            Ok(open) => {
                assert!(
                    open.warnings
                        .iter()
                        .any(|w| w.contains("No Such Device 9000")),
                    "warning names the missing device: {:?}",
                    open.warnings
                );
            }
            Err(message) => assert!(!message.is_empty()), // device-less CI
        }
    }

    #[test]
    fn list_devices_is_clean_on_any_machine() {
        let list = list_devices();
        // No panic, and every entry is named; at most one default per side.
        assert!(list.inputs.iter().all(|d| !d.name.is_empty()));
        assert!(list.outputs.iter().all(|d| !d.name.is_empty()));
        assert!(list.inputs.iter().filter(|d| d.is_default).count() <= 1);
        assert!(list.outputs.iter().filter(|d| d.is_default).count() <= 1);
    }
}
