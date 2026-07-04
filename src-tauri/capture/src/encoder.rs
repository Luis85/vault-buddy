//! Thin safe wrapper around LAME (mp3lame-encoder). Streaming: every call
//! returns finished MP3 bytes ready to append to the .part file, so the
//! on-disk file is always a valid (if truncated) MP3.

use mp3lame_encoder::{Bitrate, Builder, FlushNoGap, InterleavedPcm};

pub struct Mp3Encoder {
    inner: mp3lame_encoder::Encoder,
}

impl Mp3Encoder {
    pub fn new(sample_rate: u32, bitrate_kbps: u32) -> Result<Self, String> {
        let bitrate = match bitrate_kbps {
            128 => Bitrate::Kbps128,
            160 => Bitrate::Kbps160,
            192 => Bitrate::Kbps192,
            other => {
                log::warn!("unsupported bitrate {other} kbps, falling back to 128");
                Bitrate::Kbps128
            }
        };
        let mut builder = Builder::new().ok_or("failed to init LAME")?;
        builder.set_num_channels(2).map_err(|e| e.to_string())?;
        builder
            .set_sample_rate(sample_rate)
            .map_err(|e| e.to_string())?;
        builder.set_brate(bitrate).map_err(|e| e.to_string())?;
        builder
            .set_quality(mp3lame_encoder::Quality::Good)
            .map_err(|e| e.to_string())?;
        let inner = builder.build().map_err(|e| e.to_string())?;
        Ok(Self { inner })
    }

    pub fn encode(&mut self, interleaved_stereo: &[i16]) -> Result<Vec<u8>, String> {
        let input = InterleavedPcm(interleaved_stereo);
        let mut out = Vec::with_capacity(mp3lame_encoder::max_required_buffer_size(
            interleaved_stereo.len() / 2,
        ));
        self.inner
            .encode_to_vec(input, &mut out)
            .map_err(|e| e.to_string())?;
        Ok(out)
    }

    pub fn finish(mut self) -> Result<Vec<u8>, String> {
        let mut out = Vec::with_capacity(7200);
        self.inner
            .flush_to_vec::<FlushNoGap>(&mut out)
            .map_err(|e| e.to_string())?;
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2 seconds of 440 Hz sine → encode → decode with minimp3 → duration
    /// within tolerance. This is the CI-side proof that the pipeline
    /// produces real, playable MP3 without any audio hardware.
    #[test]
    fn sine_roundtrip_has_expected_duration() {
        let rate = 44_100u32;
        let seconds = 2.0f32;
        let frames = (rate as f32 * seconds) as usize;
        let mut pcm = Vec::with_capacity(frames * 2);
        for i in 0..frames {
            let t = i as f32 / rate as f32;
            let s = ((t * 440.0 * std::f32::consts::TAU).sin() * 0.5 * i16::MAX as f32) as i16;
            pcm.push(s);
            pcm.push(s);
        }

        let mut enc = Mp3Encoder::new(rate, 128).unwrap();
        let mut mp3 = Vec::new();
        // encode in 100ms chunks like the live pipeline does
        for chunk in pcm.chunks(4410 * 2) {
            mp3.extend(enc.encode(chunk).unwrap());
        }
        mp3.extend(enc.finish().unwrap());
        assert!(
            mp3.len() > 10_000,
            "suspiciously small: {} bytes",
            mp3.len()
        );

        let mut decoder = minimp3::Decoder::new(std::io::Cursor::new(mp3));
        let mut decoded_frames = 0usize;
        loop {
            match decoder.next_frame() {
                Ok(frame) => decoded_frames += frame.data.len() / frame.channels,
                Err(minimp3::Error::Eof) => break,
                Err(e) => panic!("decode error: {e:?}"),
            }
        }
        let decoded_secs = decoded_frames as f32 / rate as f32;
        assert!(
            (decoded_secs - seconds).abs() < 0.2,
            "expected ~{seconds}s, decoded {decoded_secs}s"
        );
    }

    #[test]
    fn unsupported_bitrate_falls_back() {
        assert!(Mp3Encoder::new(44_100, 999).is_ok());
    }
}
