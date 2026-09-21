//! The export's REAL round trips: synthesize a clip, drive it through a real
//! ffmpeg with a real `Timeline`, and read the OUTPUT back.
//!
//! This file is the reason the project left Media Foundation. Phases 2-4
//! each shipped the claim that "the cut lands where the editor said it
//! would", and every one of them rested on a manual Windows checklist row
//! nobody has run (GAP-117). These execute, on Linux, in CI.
//!
//! An INTEGRATION test rather than a `#[cfg(test)]` module, following
//! `mcp/tests/roundtrip.rs`: it drives an external binary through nothing
//! but the crate's public surface, which is exactly what `tests/` is for.
//! `export.rs` keeps the pure decisions and the structural scans that can
//! only be written where `include_str!("export.rs")` resolves.
//!
//! ## The fixture is the point
//!
//! A duration assertion alone proves almost nothing here. Deleting the LAST
//! two seconds instead of the first yields a file of exactly the same
//! length; a `concat` that paired segment 1's video with segment 2's audio
//! would yield a file that opens, plays, and carries the wrong sound over
//! the wrong picture. So the fixture is three 2-second blocks, each a
//! DIFFERENT COLOUR and a DIFFERENT TONE, and every assertion reads BOTH
//! streams and requires them to name the same block. Run backwards, or
//! scrambled, these fail.
//!
//! ## No new dependency, in the tests either
//!
//! The tone detector is a hand-written Goertzel over raw s16le. The export
//! adds no Rust dependency and that holds for what proves it.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use vault_buddy_core::screen_capture_config::ScreenQuality;
use vault_buddy_core::timeline::Timeline;
use vault_buddy_screen::export::{export, ExportOutcome, ExportRequest};
use vault_buddy_screen::ffmpeg_args::{
    ms_to_ffmpeg_seconds, parse_progress_line, EncodeSettings, ProgressTick,
};
use vault_buddy_screen::staging;
use vault_buddy_screen::ScreenError;

/// Print straight to the process's stderr handle rather than through
/// `eprintln!`.
///
/// libtest captures the print MACROS and shows their output only for a
/// FAILING test, so a skip announced with `eprintln!` is exactly the silent
/// skip this must not be. Writing to the handle bypasses that capture, and
/// the line shows up on an ordinary `cargo test` run.
fn announce(message: &str) {
    use std::io::Write as _;
    let _ = writeln!(std::io::stderr(), "{message}");
}

fn settings(has_audio: bool) -> EncodeSettings {
    EncodeSettings {
        width: FIXTURE_W,
        height: FIXTURE_H,
        fps: FIXTURE_FPS,
        quality: ScreenQuality::Balanced,
        h264_encoder: "libx264".into(),
        has_audio,
    }
}

const FIXTURE_W: u32 = 320;
const FIXTURE_H: u32 = 240;
const FIXTURE_FPS: u32 = 30;
const BLOCK_MS: u64 = 2_000;
/// Duration tolerance. Three frames at the fixture's rate -- container
/// rounding and the muxer's last-packet position differ by about one --
/// and two orders of magnitude tighter than the 2 000 ms any of these
/// cuts moves, so it cannot hide a wrong cut.
const TOLERANCE_MS: u64 = 100;
/// The window sampled for a tone, and the rate it is decoded at. The
/// three tones (300 / 900 / 2700 Hz) are more than an octave apart, so
/// no two can be confused by a leaky window.
const TONE_WINDOW: f64 = 0.4;
const TONE_RATE: u32 = 16_000;

/// One 2-second block of the fixture, named by BOTH of its streams.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Block {
    Red,
    Green,
    Blue,
}

impl Block {
    fn rgb(self) -> (i32, i32, i32) {
        match self {
            // ffmpeg's named "green" is 0x008000, not 0x00ff00.
            Block::Red => (255, 0, 0),
            Block::Green => (0, 128, 0),
            Block::Blue => (0, 0, 255),
        }
    }
    fn tone(self) -> u32 {
        match self {
            Block::Red => 300,
            Block::Green => 900,
            Block::Blue => 2700,
        }
    }
    fn all() -> [Block; 3] {
        [Block::Red, Block::Green, Block::Blue]
    }
}

fn ffmpeg_on_path() -> Option<PathBuf> {
    let exe = if cfg!(windows) {
        "ffmpeg.exe"
    } else {
        "ffmpeg"
    };
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|dir| dir.join(exe))
        .find(|candidate| candidate.is_file())
}

/// The ffmpeg binary, or a VISIBLE skip. A round-trip test that skips in
/// silence is indistinguishable from one that passed, which is how three
/// phases of this feature shipped unproven claims.
macro_rules! ffmpeg_or_skip {
    () => {
        match ffmpeg_on_path() {
            Some(path) => path,
            None => {
                // libtest names each test's thread after the test, so
                // the skip line identifies WHICH round trip did not run
                // without every call site repeating its own name.
                let thread = std::thread::current();
                let who = thread.name().unwrap_or(module_path!()).to_string();
                announce(&format!(
                    "SKIPPED {who}: no ffmpeg on PATH, so this round trip is UNPROVEN \
                     in this run. Install ffmpeg (CI does) to execute it."
                ));
                return;
            }
        }
    };
}

fn ffmpeg_stdout(ffmpeg: &Path, args: &[String]) -> Vec<u8> {
    let out = Command::new(ffmpeg)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .expect("ffmpeg ran");
    assert!(
        out.status.success(),
        "ffmpeg {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    out.stdout
}

fn argv(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|p| (*p).to_string()).collect()
}

/// Three 2-second blocks: red/300 Hz, green/900 Hz, blue/2700 Hz.
fn make_fixture(ffmpeg: &Path, dir: &Path) -> PathBuf {
    let dest = dir.join("fixture.mp4");
    let size = format!("{FIXTURE_W}x{FIXTURE_H}");
    let mut args = argv(&["-hide_banner", "-nostdin", "-loglevel", "error", "-nostats"]);
    for block in Block::all() {
        let (r, g, b) = block.rgb();
        args.extend(argv(&["-f", "lavfi", "-i"]));
        args.push(format!(
            "color=c=0x{r:02x}{g:02x}{b:02x}:s={size}:r={FIXTURE_FPS}:d=2"
        ));
    }
    for block in Block::all() {
        args.extend(argv(&["-f", "lavfi", "-i"]));
        args.push(format!("sine=frequency={}:d=2", block.tone()));
    }
    args.extend(argv(&["-filter_complex"]));
    args.push("[0:v][1:v][2:v]concat=n=3:v=1:a=0[v];[3:a][4:a][5:a]concat=n=3:v=0:a=1[a]".into());
    args.extend(argv(&[
        "-map", "[v]", "-map", "[a]", "-c:v", "libx264", "-pix_fmt", "yuv420p", "-g", "30", "-c:a",
        "aac", "-y",
    ]));
    args.push(dest.to_string_lossy().into_owned());
    ffmpeg_stdout(ffmpeg, &args);
    dest
}

/// A deliberately heavier clip, so a cancel fired a moment after the
/// start lands while ffmpeg is genuinely mid-encode rather than racing
/// a finished one.
fn make_long_fixture(ffmpeg: &Path, dir: &Path) -> PathBuf {
    let dest = dir.join("long.mp4");
    let mut args = argv(&[
        "-hide_banner",
        "-nostdin",
        "-loglevel",
        "error",
        "-nostats",
        "-f",
        "lavfi",
        "-i",
        "testsrc=size=960x540:rate=30:duration=20",
        "-f",
        "lavfi",
        "-i",
        "sine=frequency=440:duration=20",
        // ultrafast for the FIXTURE only -- the export under test still
        // runs the real preset, which is what makes it slow enough.
        "-c:v",
        "libx264",
        "-preset",
        "ultrafast",
        "-pix_fmt",
        "yuv420p",
        "-c:a",
        "aac",
        "-shortest",
        "-y",
    ]);
    args.push(dest.to_string_lossy().into_owned());
    ffmpeg_stdout(ffmpeg, &args);
    dest
}

/// Duration read back from ffmpeg's OWN `-progress` stream while
/// decoding the file to the null muxer.
///
/// Deliberately not ffprobe: decision 1 forbids this module from ever
/// shelling out to one, and a test helper that did would make that
/// structural scan a lie by proximity.
fn probe_duration_ms(ffmpeg: &Path, path: &str) -> u64 {
    let args = argv(&[
        "-hide_banner",
        "-nostdin",
        "-loglevel",
        "error",
        "-nostats",
        "-progress",
        "pipe:1",
        "-i",
        path,
        "-f",
        "null",
        "-",
    ]);
    String::from_utf8_lossy(&ffmpeg_stdout(ffmpeg, &args))
        .lines()
        .filter_map(parse_progress_line)
        .filter_map(|tick| match tick {
            ProgressTick::OutTimeUs(us) => Some(us / 1_000),
            ProgressTick::Done => None,
        })
        .max()
        .expect("the file reported a position")
}

/// The md5 of the ENCODED video packets, copied out without decoding.
///
/// This is the fast path's real proof. A remux copies the H.264
/// bitstream verbatim, so this matches the source exactly; any
/// re-encode changes it, even one that happens to produce a file of the
/// same length. A wall-clock comparison alone could be explained away by
/// a fast machine.
fn video_packet_md5(ffmpeg: &Path, path: &Path) -> String {
    let mut args = argv(&["-v", "error", "-i"]);
    args.push(path.to_string_lossy().into_owned());
    args.extend(argv(&["-map", "0:v", "-c", "copy", "-f", "md5", "-"]));
    String::from_utf8_lossy(&ffmpeg_stdout(ffmpeg, &args))
        .trim()
        .to_string()
}

fn colour_block_at(ffmpeg: &Path, path: &Path, at_ms: u64) -> Block {
    let mut args = argv(&["-v", "error", "-ss"]);
    args.push(ms_to_ffmpeg_seconds(at_ms));
    args.extend(argv(&["-i"]));
    args.push(path.to_string_lossy().into_owned());
    args.extend(argv(&[
        "-frames:v",
        "1",
        "-vf",
        "scale=1:1",
        "-f",
        "rawvideo",
        "-pix_fmt",
        "rgb24",
        "-",
    ]));
    let raw = ffmpeg_stdout(ffmpeg, &args);
    assert!(raw.len() >= 3, "no frame decoded at {at_ms} ms");
    let (r, g, b) = (raw[0] as i32, raw[1] as i32, raw[2] as i32);
    let distance = |block: Block| {
        let (br, bg, bb) = block.rgb();
        (r - br).pow(2) + (g - bg).pow(2) + (b - bb).pow(2)
    };
    Block::all()
        .into_iter()
        .min_by_key(|b| distance(*b))
        .expect("three candidates")
}

/// The strongest present tone in a short window, by Goertzel.
///
/// The three candidates are far enough apart that the winner's magnitude
/// is three orders of magnitude above the others, so this is a decision,
/// not a guess -- and the margin is asserted rather than assumed.
fn tone_block_at(ffmpeg: &Path, path: &Path, at_ms: u64) -> Block {
    let mut args = argv(&["-v", "error", "-ss"]);
    args.push(ms_to_ffmpeg_seconds(at_ms));
    args.extend(argv(&["-t"]));
    args.push(format!("{TONE_WINDOW}"));
    args.extend(argv(&["-i"]));
    args.push(path.to_string_lossy().into_owned());
    args.extend(argv(&[
        "-vn", "-ac", "1", "-ar", "16000", "-f", "s16le", "-",
    ]));
    let raw = ffmpeg_stdout(ffmpeg, &args);
    let samples: Vec<i16> = raw
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| i16::from_le_bytes(*c))
        .collect();
    assert!(samples.len() > 1_000, "no audio decoded at {at_ms} ms");
    let mut ranked: Vec<(Block, f64)> = Block::all()
        .into_iter()
        .map(|b| (b, goertzel(&samples, TONE_RATE as f64, b.tone() as f64)))
        .collect();
    ranked.sort_by(|a, b| b.1.total_cmp(&a.1));
    assert!(
        ranked[0].1 > ranked[1].1 * 10.0,
        "no tone clearly dominates at {at_ms} ms: {ranked:?}"
    );
    ranked[0].0
}

/// Single-frequency magnitude, the standard Goertzel recurrence. Kept
/// here rather than pulling in a DFT crate: the export adds NO Rust
/// dependency, and that holds for its tests too.
fn goertzel(samples: &[i16], sample_rate: f64, freq: f64) -> f64 {
    let n = samples.len() as f64;
    let k = (0.5 + n * freq / sample_rate).floor();
    let w = 2.0 * std::f64::consts::PI * k / n;
    let (cw, sw) = (w.cos(), w.sin());
    let coeff = 2.0 * cw;
    let (mut s1, mut s2) = (0.0f64, 0.0f64);
    for &x in samples {
        let s0 = f64::from(x) + coeff * s1 - s2;
        s2 = s1;
        s1 = s0;
    }
    (s1 - s2 * cw).hypot(s2 * sw) / n
}

/// Both streams' answer for the block playing at `at_ms`. Assert against
/// a PAIR: equal-but-wrong is a scramble, and no duration check sees it.
fn block_at(ffmpeg: &Path, path: &Path, at_ms: u64) -> (Block, Block) {
    (
        colour_block_at(ffmpeg, path, at_ms),
        tone_block_at(ffmpeg, path, at_ms),
    )
}

/// The middle of the `index`-th 2-second block of an output.
fn mid_of(index: u64) -> u64 {
    index * BLOCK_MS + BLOCK_MS / 2
}

/// The destination an export REALLY writes to.
///
/// Not `dir.join("something.mp4")`. Production exports into
/// `staging::export_part_file_name(base)` -- `.<base>.export.mp4.part` -- and
/// every test in this file used to invent a plain `.mp4` name instead. ffmpeg
/// chooses its muxer from the output's EXTENSION unless it is told one, so
/// every one of these round trips passed against a filename the app never
/// produces while the real export died at "Unable to choose an output format
/// for '...export.mp4.part'". The whole point of this file is that the export
/// is proven against what it actually does, so the filename is part of that.
fn export_dest(dir: &Path, base: &str) -> PathBuf {
    dir.join(staging::export_part_file_name(base))
}

fn run_export(
    ffmpeg: &Path,
    source: &Path,
    dest: &Path,
    timeline: &Timeline,
    source_duration_ms: u64,
) -> (ExportOutcome, Vec<u64>) {
    let mut seen = Vec::new();
    let outcome = export(
        ExportRequest {
            ffmpeg,
            source,
            dest,
            timeline,
            source_duration_ms,
            settings: settings(true),
        },
        &AtomicBool::new(false),
        &mut |percent| seen.push(percent),
    )
    .expect("the export ran");
    (outcome, seen)
}

// The fixture itself has to be right before anything built on it means
// anything. This is the control: if ffmpeg, the colour probe or the
// Goertzel were wrong, every assertion below would be measuring noise.
#[test]
fn the_fixture_really_does_carry_three_distinguishable_blocks() {
    let ffmpeg = ffmpeg_or_skip!();
    let dir = tempfile::tempdir().expect("tempdir");
    let src = make_fixture(&ffmpeg, dir.path());
    assert_eq!(block_at(&ffmpeg, &src, mid_of(0)), (Block::Red, Block::Red));
    assert_eq!(
        block_at(&ffmpeg, &src, mid_of(1)),
        (Block::Green, Block::Green)
    );
    assert_eq!(
        block_at(&ffmpeg, &src, mid_of(2)),
        (Block::Blue, Block::Blue)
    );
    let probed = probe_duration_ms(&ffmpeg, &src.to_string_lossy());
    assert!(probed.abs_diff(6_000) <= TOLERANCE_MS, "{probed} ms");
}

// THE test this whole route change was for. A delete that removed the
// WRONG two seconds yields a file of exactly the same length, so the
// duration assertion is the weakest half: the block assertions are what
// prove the surviving footage is the footage the editor showed.
#[test]
fn an_edited_export_produces_a_file_of_the_planned_length_from_the_footage_that_survived() {
    let ffmpeg = ffmpeg_or_skip!();
    let dir = tempfile::tempdir().expect("tempdir");
    let src = make_fixture(&ffmpeg, dir.path());
    let dest = export_dest(dir.path(), "2026-09-21 0848 Edited");
    // Delete the FIRST block: red/300 Hz must be gone, leaving green
    // then blue, 4 s long.
    let timeline = Timeline::whole(6_000).split_at(2_000).delete(0);
    let (outcome, seen) = run_export(&ffmpeg, &src, &dest, &timeline, 6_000);

    assert!(!outcome.remuxed, "an edited export must re-encode");
    assert_eq!(outcome.output_duration_ms, 4_000);
    let probed = probe_duration_ms(&ffmpeg, &dest.to_string_lossy());
    assert!(
        probed.abs_diff(4_000) <= TOLERANCE_MS,
        "planned 4000 ms, file is {probed} ms"
    );
    assert_eq!(
        block_at(&ffmpeg, &dest, mid_of(0)),
        (Block::Green, Block::Green),
        "the block after the deleted one must now play first"
    );
    assert_eq!(
        block_at(&ffmpeg, &dest, mid_of(1)),
        (Block::Blue, Block::Blue)
    );
    // MUTATION M6: without the terminal emit ffmpeg's own last tick
    // stops a percent or two short and the bar never completes.
    assert_eq!(
        seen.last().copied(),
        Some(100),
        "progress ended at {seen:?}"
    );
}

// MUTATION M7's other half, and the fast path's real proof. The packet
// md5 is what makes "without re-encoding" an assertion rather than a
// claim: a transcode of the same footage produces a different bitstream
// even when the durations match to the millisecond.
#[test]
fn an_untouched_export_remuxes_to_the_same_length_without_re_encoding() {
    let ffmpeg = ffmpeg_or_skip!();
    let dir = tempfile::tempdir().expect("tempdir");
    let src = make_fixture(&ffmpeg, dir.path());
    let dest = export_dest(dir.path(), "2026-09-21 0848 Remuxed");
    let timeline = Timeline::whole(6_000);
    let (outcome, seen) = run_export(&ffmpeg, &src, &dest, &timeline, 6_000);

    assert!(outcome.remuxed, "an untouched capture takes the fast path");
    assert_eq!(outcome.output_duration_ms, 6_000);
    let probed = probe_duration_ms(&ffmpeg, &dest.to_string_lossy());
    assert!(probed.abs_diff(6_000) <= TOLERANCE_MS, "{probed} ms");
    assert_eq!(
        video_packet_md5(&ffmpeg, &src),
        video_packet_md5(&ffmpeg, &dest),
        "a remux must copy the encoded video verbatim"
    );
    for (index, block) in Block::all().into_iter().enumerate() {
        assert_eq!(
            block_at(&ffmpeg, &dest, mid_of(index as u64)),
            (block, block)
        );
    }
    assert_eq!(
        seen.last().copied(),
        Some(100),
        "progress ended at {seen:?}"
    );
}

// CLAIM 1, pinned in the repo. `filter_complex` emits the concat inputs
// grouped PER SEGMENT ([v0][a0][v1][a1]...). If ffmpeg wanted all video
// pads then all audio pads, every multi-span export with sound would
// play the wrong audio over the wrong picture -- a file that opens, that
// plays, and that no duration assertion can fault. So this reads BOTH
// streams at all three positions and requires them to agree.
#[test]
fn a_reordered_export_is_as_long_as_its_spans_with_each_block_still_carrying_its_own_sound() {
    let ffmpeg = ffmpeg_or_skip!();
    let dir = tempfile::tempdir().expect("tempdir");
    let src = make_fixture(&ffmpeg, dir.path());
    let dest = export_dest(dir.path(), "2026-09-21 0848 Reordered");
    // Move the first block to the end: green, blue, red.
    let timeline = Timeline::whole(6_000)
        .split_at(2_000)
        .split_at(4_000)
        .reorder(0, 2);
    let (outcome, seen) = run_export(&ffmpeg, &src, &dest, &timeline, 6_000);

    assert!(!outcome.remuxed);
    assert_eq!(outcome.output_duration_ms, 6_000);
    let probed = probe_duration_ms(&ffmpeg, &dest.to_string_lossy());
    assert!(probed.abs_diff(6_000) <= TOLERANCE_MS, "{probed} ms");
    let expected = [Block::Green, Block::Blue, Block::Red];
    for (index, block) in expected.into_iter().enumerate() {
        assert_eq!(
            block_at(&ffmpeg, &dest, mid_of(index as u64)),
            (block, block),
            "block {index} lost its pairing or its order"
        );
    }
    assert_eq!(seen.last().copied(), Some(100));
}

// MUTATION M5. Killing the child is only half a cancel: ffmpeg leaves a
// truncated file behind, and leaving it there offers the user a broken
// export as though it were a saved one.
#[test]
fn cancelling_an_export_leaves_no_output_behind() {
    let ffmpeg = ffmpeg_or_skip!();
    let dir = tempfile::tempdir().expect("tempdir");
    let src = make_long_fixture(&ffmpeg, dir.path());
    let dest = export_dest(dir.path(), "2026-09-21 0848 Cancelled");
    let timeline = Timeline::whole(20_000).split_at(1_000).delete(0);
    let cancel = AtomicBool::new(false);
    let started = Instant::now();
    let outcome = std::thread::scope(|scope| {
        scope.spawn(|| {
            // Long enough that ffmpeg has really started writing, far
            // short of the seconds this re-encode takes.
            std::thread::sleep(Duration::from_millis(250));
            cancel.store(true, Ordering::Relaxed);
        });
        export(
            ExportRequest {
                ffmpeg: &ffmpeg,
                source: &src,
                dest: &dest,
                timeline: &timeline,
                source_duration_ms: 20_000,
                settings: EncodeSettings {
                    width: 960,
                    height: 540,
                    ..settings(true)
                },
            },
            &cancel,
            &mut |_| {},
        )
    });
    let err = outcome.expect_err("a cancelled export must not report success");
    assert_eq!(err, ScreenError::Cancelled);
    assert!(
        !dest.exists(),
        "the truncated output must be deleted, not left behind"
    );
    // The cancel is honoured promptly rather than after the whole
    // encode: this clip takes seconds to re-encode at the real preset.
    announce(&format!(
        "  cancel honoured after {} ms",
        started.elapsed().as_millis()
    ));
}

// DECISION 1 / MUTATION M2, end to end. The sidecar's duration and a
// probed one disagree by a frame or two for the same file; feeding the
// probed number to `is_untouched` turns every untouched capture into a
// silent, lossy, slow full re-encode. The structural scan above forbids
// this module from probing at all; this one shows what it would cost.
#[test]
fn the_fast_path_keys_on_the_sidecars_duration_not_the_files_own() {
    let ffmpeg = ffmpeg_or_skip!();
    let dir = tempfile::tempdir().expect("tempdir");
    let src = make_fixture(&ffmpeg, dir.path());
    let dest = export_dest(dir.path(), "2026-09-21 0848 Sidecar");
    let timeline = Timeline::whole(6_000);
    let probed = probe_duration_ms(&ffmpeg, &src.to_string_lossy());
    if probed == 6_000 {
        announce(
            "  note: this build's probed duration happens to equal the sidecar's, so \
             the end-to-end half of M2 cannot bite in this run; the structural scan \
             the_export_never_probes_the_source... still pins it.",
        );
    } else {
        assert!(
            !timeline.is_untouched(probed),
            "the probed duration ({probed} ms) must disagree with the sidecar's 6000 ms \
             for this fixture to bite"
        );
    }
    let (outcome, _) = run_export(&ffmpeg, &src, &dest, &timeline, 6_000);
    assert!(
        outcome.remuxed,
        "the sidecar says untouched, so this must remux whatever the file probes to"
    );
}

// The fast path's other observable: it does no work. Reported rather than
// only asserted, because the NUMBER is the interesting part -- this is the
// first time in the feature that the difference is measurable anywhere but
// on a developer's own hardware.
//
// HONEST LIMIT, measured: this test is the WEAKEST of the seven. Mutating
// the fast-path key to `timeline.segments.is_empty()` (the plan's M1) makes
// BOTH halves re-encode, and the two timings then land close enough that
// this assertion stayed GREEN while three of its siblings failed. The
// load-bearing proof that the fast path re-encodes nothing is the packet
// md5 in `an_untouched_export_remuxes_to_the_same_length_without_re_encoding`
// -- a transcode cannot reproduce the source's H.264 bitstream, however
// fast the machine. Do not promote this timing into the proof.
#[test]
fn the_remux_is_faster_than_re_encoding_the_same_fixture() {
    let ffmpeg = ffmpeg_or_skip!();
    let dir = tempfile::tempdir().expect("tempdir");
    let src = make_fixture(&ffmpeg, dir.path());

    let remux_dest = export_dest(dir.path(), "2026-09-21 0848 Fast");
    let started = Instant::now();
    run_export(&ffmpeg, &src, &remux_dest, &Timeline::whole(6_000), 6_000);
    let remux_ms = started.elapsed().as_millis();

    // The same six seconds of footage, re-encoded: one segment covering
    // the whole source but split, so it is not untouched.
    let edited = Timeline::whole(6_000).split_at(2_000);
    assert!(!edited.is_untouched(6_000));
    let encode_dest = export_dest(dir.path(), "2026-09-21 0848 Slow");
    let started = Instant::now();
    run_export(&ffmpeg, &src, &encode_dest, &edited, 6_000);
    let encode_ms = started.elapsed().as_millis();

    announce(&format!(
        "  remux {remux_ms} ms vs re-encode {encode_ms} ms on the same 6 s fixture"
    ));
    assert!(
        remux_ms < encode_ms,
        "the fast path must be faster: remux {remux_ms} ms, re-encode {encode_ms} ms"
    );
}
