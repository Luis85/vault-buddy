//! cpal glue: opens the default microphone (and, in meeting mode on
//! Windows, WASAPI loopback on the default output) and pushes raw sample
//! chunks into the session's mpsc channels. All sample-format conversion
//! beyond f32 widening happens in the session worker, not here.

use crate::session::{SourceInput, SourceMsg};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::mpsc::Sender;

pub struct OpenSources {
    pub inputs: Vec<SourceInput>,
    pub streams: Vec<cpal::Stream>,
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

pub fn open_sources(meeting_mode: bool) -> Result<OpenSources, String> {
    let host = cpal::default_host();
    let mut inputs = Vec::new();
    let mut streams = Vec::new();

    let mic = host
        .default_input_device()
        .ok_or("No microphone found — check Windows sound settings.")?;
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
        let output = host
            .default_output_device()
            .ok_or("Desktop audio (loopback) unavailable: no default output device")?;
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

    Ok(OpenSources { inputs, streams })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// CI runners have no audio devices; this asserts the error path is a
    /// clean human-readable Err, not a panic. On a dev machine with a mic
    /// it exercises the success path instead.
    #[test]
    fn open_sources_never_panics() {
        match open_sources(true) {
            Ok(open) => {
                assert!(!open.inputs.is_empty());
                assert!(!open.inputs[0].name.is_empty(), "mic source is named");
            }
            Err(message) => {
                assert!(!message.is_empty());
            }
        }
    }
}
