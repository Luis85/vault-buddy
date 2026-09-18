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

/// Sum N mono sources into one interleaved stereo i16 buffer.
///
/// Shorter sources are silence-padded to the longest. No per-source gain
/// normalisation is applied: dividing by N would change the levels of every
/// existing two-source meeting recording. `soft_clip` already bounds a
/// summed overload, which is the behaviour the audio domain shipped with.
///
/// Zero sources yields an empty buffer — a silent capture is legal.
pub fn mix_n_to_stereo_i16(sources: &[&[f32]]) -> Vec<i16> {
    let frames = sources.iter().map(|s| s.len()).max().unwrap_or(0);
    let mut out = Vec::with_capacity(frames * 2);
    for i in 0..frames {
        let sum: f32 = sources
            .iter()
            .map(|s| s.get(i).copied().unwrap_or(0.0))
            .sum();
        let sample = (soft_clip(sum) * i16::MAX as f32) as i16;
        out.push(sample);
        out.push(sample);
    }
    out
}

/// The two-source case, kept as the name the existing audio-capture session
/// calls. A thin wrapper so both paths can never drift apart.
pub fn mix_to_stereo_i16(a: &[f32], b: &[f32]) -> Vec<i16> {
    mix_n_to_stereo_i16(&[a, b])
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

    #[test]
    fn mix_n_with_no_sources_is_silence() {
        // Zero selected audio devices is legal — a silent UI demo is a real
        // use case (spec §7.2), so this must be empty output, not a panic.
        assert!(mix_n_to_stereo_i16(&[]).is_empty());
    }

    #[test]
    fn mix_n_with_one_source_duplicates_it_across_both_channels() {
        let out = mix_n_to_stereo_i16(&[&[0.5, -0.5]]);
        assert_eq!(out.len(), 4);
        assert_eq!(out[0], out[1], "L == R");
        assert_eq!(out[2], out[3]);
        assert!((out[0] as f32 / i16::MAX as f32 - (0.5f32).tanh()).abs() < 0.001);
    }

    // Regression: the two-source path is the existing meeting-recording
    // behaviour and must stay byte-identical, or every meeting recording's
    // levels change the day multi-select lands.
    //
    // The expected values are a HARDCODED ORACLE, captured from the
    // two-source mix_to_stereo_i16 as it behaved BEFORE this change.
    // Comparing mix_n_to_stereo_i16(&[&a,&b]) against mix_to_stereo_i16(&a,&b)
    // would be a tautology once the latter becomes a wrapper over the former
    // (Step 3) — it would compare f(x) to f(x) and could never fail.
    #[test]
    fn mix_n_matches_the_two_source_mixer_exactly() {
        let cases: Vec<(Vec<f32>, Vec<f32>, Vec<i16>)> = vec![
            (vec![0.5, 0.5], vec![0.25], vec![20811, 20811, 15142, 15142]),
            (
                vec![],
                vec![0.1, 0.2, 0.3],
                vec![3265, 3265, 6467, 6467, 9545, 9545],
            ),
            (
                vec![0.9, -0.9, 0.4],
                vec![0.9, -0.9, 0.4],
                vec![31023, 31023, -31023, -31023, 21758, 21758],
            ),
            (vec![], vec![], vec![]),
        ];
        for (a, b, expected) in cases {
            assert_eq!(
                mix_n_to_stereo_i16(&[&a, &b]),
                expected,
                "n-source: a={a:?} b={b:?}"
            );
            assert_eq!(
                mix_to_stereo_i16(&a, &b),
                expected,
                "wrapper: a={a:?} b={b:?}"
            );
        }
    }

    #[test]
    fn mix_n_pads_every_shorter_source_with_silence() {
        let out = mix_n_to_stereo_i16(&[&[0.1, 0.1, 0.1], &[0.1], &[0.1, 0.1]]);
        assert_eq!(
            out.len(),
            6,
            "3 frames * 2 channels — the longest source wins"
        );
        let third = out[4] as f32 / i16::MAX as f32;
        assert!(
            (third - (0.1f32).tanh()).abs() < 0.001,
            "only the long source contributes"
        );
    }

    #[test]
    fn mix_n_soft_clips_a_summed_overload_instead_of_wrapping() {
        // Five hot sources sum to 4.0; without soft_clip this wraps to a
        // negative i16 and the audio is destroyed.
        let hot = [0.8f32];
        let sources: Vec<&[f32]> = vec![&hot; 5];
        let out = mix_n_to_stereo_i16(&sources);
        assert_eq!(out.len(), 2);
        assert!(out[0] > 0, "stays positive");
        // No `out[0] <= i16::MAX` assert: out[0] IS an i16, so that comparison
        // is always true and clippy::absurd_extreme_comparisons (deny by
        // default) fails the -D warnings gate. soft_clip bounding the sum is
        // what the positive assert above actually proves.
    }
}
