//! Pure sample math for the capture pipeline. No I/O, no devices — fully
//! unit-tested on any platform. Linear resampling is deliberate: adaptive
//! drift compensation is an accepted deferral in the spec.

pub fn downmix_to_mono(interleaved: &[f32], channels: u16) -> Vec<f32> {
    let ch = channels.max(1) as usize;
    interleaved
        .chunks(ch)
        .map(|frame| frame.iter().sum::<f32>() / ch as f32)
        .collect()
}

pub fn resample_linear(mono: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32> {
    if from_rate == to_rate || mono.is_empty() {
        return mono.to_vec();
    }
    let out_len = (mono.len() as u64 * to_rate as u64 / from_rate as u64) as usize;
    let step = from_rate as f64 / to_rate as f64;
    (0..out_len)
        .map(|i| {
            let pos = i as f64 * step;
            let idx = pos as usize;
            let frac = (pos - idx as f64) as f32;
            let a = mono[idx.min(mono.len() - 1)];
            let b = mono[(idx + 1).min(mono.len() - 1)];
            a + (b - a) * frac
        })
        .collect()
}

pub fn soft_clip(x: f32) -> f32 {
    let result = x.tanh();
    // Clamp to ensure |result| < 1.0 for platforms where tanh rounds to exactly 1.0
    if result >= 1.0 {
        1.0 - f32::EPSILON
    } else if result <= -1.0 {
        -1.0 + f32::EPSILON
    } else {
        result
    }
}

pub fn mix_to_stereo_i16(a: &[f32], b: &[f32]) -> Vec<i16> {
    let frames = a.len().max(b.len());
    let mut out = Vec::with_capacity(frames * 2);
    for i in 0..frames {
        let sum = a.get(i).copied().unwrap_or(0.0) + b.get(i).copied().unwrap_or(0.0);
        let sample = (soft_clip(sum) * i16::MAX as f32) as i16;
        out.push(sample);
        out.push(sample);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downmix_averages_channel_pairs() {
        assert_eq!(downmix_to_mono(&[1.0, 0.0, 0.5, 0.5], 2), vec![0.5, 0.5]);
        assert_eq!(downmix_to_mono(&[0.25, 0.75], 1), vec![0.25, 0.75]);
    }

    #[test]
    fn resample_identity_when_rates_match() {
        let x = vec![0.1, 0.2, 0.3];
        assert_eq!(resample_linear(&x, 44_100, 44_100), x);
    }

    #[test]
    fn resample_halves_and_doubles_length() {
        let x: Vec<f32> = (0..100).map(|i| i as f32 / 100.0).collect();
        assert_eq!(resample_linear(&x, 88_200, 44_100).len(), 50);
        assert_eq!(resample_linear(&x, 22_050, 44_100).len(), 200);
    }

    #[test]
    fn resample_preserves_a_constant_signal() {
        let x = vec![0.5f32; 480];
        for y in resample_linear(&x, 48_000, 44_100) {
            assert!((y - 0.5).abs() < 1e-6);
        }
    }

    #[test]
    fn soft_clip_bounds_output() {
        assert!(soft_clip(10.0) < 1.0);
        assert!(soft_clip(-10.0) > -1.0);
        assert!(
            (soft_clip(0.1) - 0.1).abs() < 0.01,
            "near-linear when small"
        );
    }

    #[test]
    fn mix_pads_shorter_side_with_silence_and_interleaves_stereo() {
        let out = mix_to_stereo_i16(&[0.5, 0.5], &[0.25]);
        assert_eq!(out.len(), 4); // 2 frames * 2 channels
        assert_eq!(out[0], out[1], "L == R");
        let first = out[0] as f32 / i16::MAX as f32;
        assert!((first - (0.75f32).tanh()).abs() < 0.001);
        let second = out[2] as f32 / i16::MAX as f32;
        assert!(
            (second - (0.5f32).tanh()).abs() < 0.001,
            "b side silence-padded"
        );
    }
}
