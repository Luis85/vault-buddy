//! `stems.rs`' tests — its own file so the module stays well under the
//! 800-line cap (the `webcam_tests.rs` precedent).

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

use vault_buddy_capture::mixer;

use super::*;
use crate::session::{pacing, Warnings, AUDIO_CHUNK_FRAMES, AUDIO_STALL_CAP};
use crate::staging;
use crate::staging_files::{capture_file_names, companion_file_names};

const BASE: &str = "2026-09-24 1015 Demo";

/// A stereo delivery at 44.1 kHz whose two channels differ, so a missed
/// downmix is visible, and whose values are a non-linear ramp, so a missed
/// resample is visible too.
fn stereo_44k(frames: usize) -> Vec<f32> {
    (0..frames)
        .flat_map(|i| {
            let t = i as f32 / frames as f32;
            [t * t * 0.8, -t * 0.4]
        })
        .collect()
}

fn mono_48k(frames: usize) -> Vec<f32> {
    (0..frames)
        .map(|i| ((i % 97) as f32 / 97.0) - 0.5)
        .collect()
}

// THE property stems exist for (F-05): a stem and the mix cannot drift,
// because a stem IS the slice the mixer sums — after the downmix, after the
// resample to AUDIO_RATE, cut by the same round. Asserted against the
// mixer's own functions applied by hand: teeing the raw delivery (or the
// downmixed-but-not-resampled one) gives 4410 samples where the mixer sums
// 4800, and different values — red either way.
#[test]
fn stems_share_the_mixed_sample_stream() {
    let raw_a = stereo_44k(4410); // 100 ms at 44.1 kHz -> 4800 frames at 48 kHz
    let raw_b = mono_48k(5000);
    let mut mix = Mixdown::new(2, true);
    mix.push(0, &raw_a, 2, 44_100);
    mix.push(1, &raw_b, 1, 48_000);

    let take = pacing::take_frames(&mix.lens(), AUDIO_CHUNK_FRAMES, AUDIO_STALL_CAP);
    assert_eq!(take, 4800, "the round is the shortest source, resampled");
    let round = mix.round(take);

    let a = mixer::resample_linear(&mixer::downmix_to_mono(&raw_a, 2), 44_100, AUDIO_RATE);
    let (a, b) = (&a[..take], &raw_b[..take]);
    assert_eq!(
        round.stereo,
        mixer::mix_n_to_stereo_i16(&[a, b]),
        "the mixed track itself is unchanged by the tee"
    );
    let stems = round.stems.expect("stems are on");
    assert_eq!(stems, vec![stem_pcm(a, take), stem_pcm(b, take)]);
    for stem in &stems {
        assert_eq!(
            stem.len() * 2,
            round.stereo.len(),
            "one stem frame per mixed frame"
        );
    }

    // ... and what the TEE hands the writer is exactly that, on the mixed
    // chunk's own timestamp.
    let (tx, rx) = mpsc::sync_channel(1);
    let mut tee = StemTee::new(tx, Arc::new(AtomicBool::new(false)));
    let (ts, dur) = (Duration::from_millis(85), Duration::from_millis(100));
    tee.send(
        StemChunk {
            pcm: stems.clone(),
            ts,
            dur,
        },
        &Warnings::new(None),
    );
    assert_eq!(
        rx.recv().expect("a chunk"),
        StemChunk {
            pcm: stems,
            ts,
            dur
        }
    );

    // The next round continues from what the first left: 200 buffered
    // frames of B, none of A.
    assert_eq!(mix.lens(), vec![0, 200]);
}

// A stalled source is padded with silence by the mixer; its stem must be
// padded the same way, or the stem would come out SHORTER than the mixed
// chunk it is stamped with and drift from it by the stall.
#[test]
fn a_stalled_inputs_stem_is_padded_like_the_mix_pads_it() {
    let mut mix = Mixdown::new(2, true);
    mix.push(0, &mono_48k(AUDIO_STALL_CAP), 1, 48_000);
    mix.push(1, &mono_48k(10), 1, 48_000);
    let take = pacing::take_frames(&mix.lens(), AUDIO_CHUNK_FRAMES, AUDIO_STALL_CAP);
    let round = mix.round(take);
    let stems = round.stems.expect("stems are on");
    assert_eq!(stems[1].len(), take);
    assert!(
        stems[1][10..].iter().all(|&s| s == 0),
        "silence past the stall"
    );
}

// Default off (`screenAudioStems` false): no stems are cut, and the mixed
// track is BYTE-IDENTICAL to what the same audio mixes to with stems on —
// the tee changes nothing about the recording itself.
#[test]
fn stems_default_off() {
    let raw = stereo_44k(8820);
    let mut off = Mixdown::new(1, false);
    let mut on = Mixdown::new(1, true);
    off.push(0, &raw, 2, 44_100);
    on.push(0, &raw, 2, 44_100);
    let take = pacing::take_frames(&off.lens(), AUDIO_CHUNK_FRAMES, AUDIO_STALL_CAP);
    let (off, on) = (off.round(take), on.round(take));
    assert_eq!(off.stems, None, "stems off cuts no stems");
    assert_eq!(off.stereo, on.stereo, "the mixed track is identical");

    // The Windows arm, pinned by source because it runs in no automated
    // test: the writer is constructed at ONE site, in the `Some` arm of a
    // match whose `None` arm builds nothing, and the audio thread cuts stems
    // only when it was handed a tee.
    let session = include_str!("windows_session.rs");
    let session = session.split("#[cfg(test)]").next().unwrap_or(session);
    let sites: Vec<usize> = session
        .match_indices("StemWriter::start(")
        .map(|(i, _)| i)
        .collect();
    assert_eq!(
        sites.len(),
        1,
        "the stem writer has exactly one construction site"
    );
    let line = &session[session[..sites[0]].rfind('\n').unwrap_or(0)..sites[0]];
    assert!(
        line.contains("Some(params) =>"),
        "built only in the Some arm: {line:?}"
    );
    let after = &session[sites[0]..session.len().min(sites[0] + 200)];
    assert!(
        after.contains("None => None,"),
        "the None arm builds nothing: {after:?}"
    );
    // ... and the request itself: nothing unless the vault turned stems on
    // AND there is an input to keep.
    let dir = Path::new("staging");
    let inputs = ["USB Mic".to_string()];
    assert!(stem_params(false, dir, BASE, &inputs).is_none());
    assert!(stem_params(true, dir, BASE, &[]).is_none());
    assert_eq!(
        stem_params(true, dir, BASE, &inputs).map(|p| p.targets),
        Some(stem_targets(dir, BASE, &inputs))
    );
    let audio = include_str!("audio.rs");
    assert!(
        audio.contains("Mixdown::new(sources.len(), tee.is_some())"),
        "the audio thread must cut stems only when it holds a tee"
    );
}

// Every file a stem is written under is one the rest of the app already
// owns: the hidden part matches the recovery sweep's PATTERN (so a crash
// leaves nothing Foreign), and the published name is exactly the part minus
// its dot and `.part` (the sweep's promotion rule) and is owned by the
// capture once its sidecar lists it — discard, Clear and resolve_source all
// agree on it.
#[test]
fn stem_names_are_owned_parts() {
    let dir = Path::new("staging");
    let targets = stem_targets(dir, BASE, &["USB Mic".into(), "Speakers".into()]);
    assert_eq!(
        targets
            .iter()
            .map(|t| (t.index, t.input.as_str()))
            .collect::<Vec<_>>(),
        vec![(1, "USB Mic"), (2, "Speakers")]
    );
    let listed: Vec<String> = targets
        .iter()
        .map(|t| t.staged.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    for (t, published) in targets.iter().zip(&listed) {
        let part = t.part.file_name().unwrap().to_string_lossy().into_owned();
        assert_eq!(t.part.parent(), Some(dir));
        assert_eq!(part, staging::stem_part_file_name(BASE, t.index));
        assert!(
            part.starts_with('.'),
            "an in-progress stem is hidden: {part}"
        );
        assert_eq!(
            staging::stem_part_base(&part),
            Some((BASE.to_string(), t.index.to_string())),
            "the recovery sweep must own {part}"
        );
        let promoted = part.strip_prefix('.').and_then(|p| p.strip_suffix(".part"));
        assert_eq!(promoted, Some(published.as_str()));
        assert!(capture_file_names(BASE, &listed).contains(published));
        assert!(companion_file_names(BASE, &listed).contains(published));
        assert!(
            !capture_file_names("2026-09-24 1015 Other", &listed).contains(published),
            "another capture must not own {published}"
        );
    }
}

// The sidecar records NAMES derived from the base, never the outcome's path.
#[test]
fn the_sidecar_block_names_each_published_stem_by_its_own_base() {
    let outcomes = [StemOutcome {
        index: 2,
        input: "Speakers".into(),
        file: Path::new("C:/elsewhere/whatever.m4a").to_path_buf(),
    }];
    let entries = stem_sidecar_entries(BASE, &outcomes);
    assert_eq!(entries.len(), 1);
    assert_eq!(
        (
            entries[0].index,
            entries[0].input.as_str(),
            entries[0].file.as_str()
        ),
        (2, "Speakers", "2026-09-24 1015 Demo.stem-2.m4a")
    );
}

#[test]
fn a_stem_is_mono_aac_at_the_mixed_tracks_rate() {
    let format = stem_audio_format();
    assert_eq!((format.sample_rate, format.channels), (AUDIO_RATE, 1));
    format
        .validate()
        .expect("the AAC encoder accepts a stem's format");
}

// The tee never blocks the mixing thread: a writer that fell behind is
// abandoned with ONE warning, and every later round is a no-op.
#[test]
fn a_stem_writer_that_falls_behind_is_abandoned_not_waited_for() {
    let (tx, rx) = mpsc::sync_channel(1);
    let abandoned = Arc::new(AtomicBool::new(false));
    let (warn_tx, warn_rx) = mpsc::channel();
    let warnings = Warnings::new(Some(warn_tx));
    let mut tee = StemTee::new(tx, Arc::clone(&abandoned));
    let chunk = || StemChunk {
        pcm: vec![vec![1; 4]],
        ts: Duration::ZERO,
        dur: Duration::from_millis(1),
    };
    tee.send(chunk(), &warnings);
    assert!(!abandoned.load(Ordering::Relaxed));
    tee.send(chunk(), &warnings); // the channel is full
    tee.send(chunk(), &warnings);
    assert!(abandoned.load(Ordering::Relaxed));
    assert_eq!(
        warn_rx.try_iter().collect::<Vec<_>>(),
        vec![STEMS_FELL_BEHIND]
    );
    assert_eq!(rx.try_iter().count(), 1, "nothing after the abandon");
    assert_eq!(
        STEMS_FELL_BEHIND,
        "The separate audio tracks could not keep up and were stopped. The mixed \
         recording is unaffected."
    );
}

#[test]
fn a_failed_stem_warning_names_the_input_and_what_is_kept() {
    assert_eq!(
        stem_failed_warning("USB Mic"),
        "The separate track for \"USB Mic\" could not be saved. The mixed recording still \
         contains it."
    );
}
