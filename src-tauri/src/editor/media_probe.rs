//! The media import's ffprobe call (Task 25) — split out of `ffmpeg.rs`,
//! which owns resolving the toolchain. It reads `ffprobe` output through
//! `ffmpeg::parse_import_probe`, which ignores frame-size parity on purpose:
//! an import is scaled onto an even canvas (fix round 1; the retired
//! phase-5 export's own probe, which encoded at the source size, needed
//! even dimensions). Error strings are shown beside a file's display name
//! and never carry a path.

use std::path::Path;

use vault_buddy_core::editor::probe::ProbeFacts;

use crate::external_tool::{run_capturing, tool_command, Capture, PROBE_TIMEOUT};
use crate::ffmpeg::{parse_import_probe, source_facts_to_probe_facts, FfmpegTools};

/// ffprobe's `format=duration` line (`duration=12.345000`, seconds) in whole
/// milliseconds, rounded down. `None` for an absent line, `N/A`, a negative
/// or non-finite value — a length ffprobe could not measure is unknown,
/// never zero-by-default.
pub(crate) fn parse_probe_duration_ms(stdout: &str) -> Option<u64> {
    let seconds: f64 = stdout
        .lines()
        .filter_map(|l| l.trim().split_once('='))
        .find(|(key, _)| *key == "duration")?
        .1
        .parse()
        .ok()?;
    if !seconds.is_finite() || seconds < 0.0 {
        return None;
    }
    Some((seconds * 1000.0).floor() as u64)
}

/// Probe an IMPORTED file: its streams (with cover art marked, via
/// `stream_disposition=attached_pic`) plus the container's duration.
///
/// `audio_only` adds `-select_streams a` for a file the allowlist named as
/// audio, so an MP3's or M4A's embedded picture is never even listed. An
/// audio-only result whose stream reports no (or a zero) sample rate is a
/// stream ffprobe found but cannot describe — a damaged file — and is
/// refused rather than registered as a silent asset.
pub(crate) fn probe_media(
    tools: &FfmpegTools,
    path: &Path,
    audio_only: bool,
) -> Result<ProbeFacts, String> {
    let mut cmd = tool_command(&tools.ffprobe);
    cmd.args(["-v", "error"]);
    if audio_only {
        cmd.args(["-select_streams", "a"]);
    }
    cmd.args([
        "-show_entries",
        "stream=codec_type,width,height,sample_rate:stream_disposition=attached_pic:format=duration",
    ])
    .args(["-of", "default=noprint_wrappers=1"])
    .arg(path);
    let (ok, stdout) = run_capturing(cmd, PROBE_TIMEOUT, Capture::Stdout)
        .map_err(|e| format!("Could not run ffprobe: {e}"))?;
    if !ok {
        return Err("ffprobe could not read the file; it may be damaged.".to_string());
    }
    let facts = parse_import_probe(&stdout)
        .ok_or_else(|| "The file has no usable video or audio stream.".to_string())?;
    if !facts.has_video && facts.audio_rate.is_none_or(|rate| rate == 0) {
        return Err("The file's audio stream is unreadable; it may be damaged.".to_string());
    }
    let duration_ms = parse_probe_duration_ms(&stdout).unwrap_or(0);
    Ok(source_facts_to_probe_facts(&facts, duration_ms))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_format_duration_is_read_in_whole_milliseconds() {
        let out = "codec_type=video\nwidth=1280\nheight=720\nduration=12.3456\n";
        assert_eq!(parse_probe_duration_ms(out), Some(12_345));
        for none in [
            "duration=N/A\n",
            "duration=-1\n",
            "duration=inf\n",
            "codec_type=audio\n",
        ] {
            assert_eq!(parse_probe_duration_ms(none), None, "{none:?}");
        }
    }

    // Fix round 1: the import ignores parity (an import is scaled onto an
    // even canvas), still refuses a zero or absent frame size, and treats
    // cover art as not-video.
    #[test]
    fn the_import_rule_ignores_parity_and_cover_art() {
        let odd = "codec_type=video\nwidth=1367\nheight=767\n";
        let facts = parse_import_probe(odd).expect("odd frames import");
        assert_eq!((facts.width, facts.height), (1367, 767));

        assert_eq!(
            parse_import_probe("codec_type=video\nwidth=0\nheight=767\n"),
            None
        );
        assert_eq!(
            parse_import_probe("codec_type=video\nwidth=N/A\nheight=767\n"),
            None
        );

        let cover_only = "codec_type=audio\nsample_rate=44100\nDISPOSITION:attached_pic=0\n\
                          codec_type=video\nwidth=301\nheight=201\nDISPOSITION:attached_pic=1\n";
        let facts = parse_import_probe(cover_only).expect("audio with cover art");
        assert!(facts.has_audio && !facts.has_video);
        assert_eq!(facts.audio_rate, Some(44_100));
        // A real video after the cover art is the one that counts.
        let both = "codec_type=video\nwidth=301\nheight=201\nDISPOSITION:attached_pic=1\n\
                    codec_type=video\nwidth=640\nheight=359\nDISPOSITION:attached_pic=0\n";
        let facts = parse_import_probe(both).unwrap();
        assert_eq!((facts.width, facts.height), (640, 359));
    }

    fn run_ffmpeg(args: &[&str]) -> bool {
        tool_command("ffmpeg")
            .args(["-v", "error", "-y"])
            .args(args)
            .status()
            .is_ok_and(|s| s.success())
    }

    fn tools() -> FfmpegTools {
        FfmpegTools {
            ffmpeg: "ffmpeg".into(),
            ffprobe: "ffprobe".into(),
            version: (0, 0),
            h264_encoder: None,
            version_line: String::new(),
        }
    }

    // `probe_media` against a REAL ffprobe: a synthesized video probes with
    // its dimensions and length, and an MP3 carrying ODD-sized cover art
    // probes as audio (an audio import selects audio streams only). Skips
    // VISIBLY without ffmpeg (a skip proves nothing).
    #[test]
    fn probe_media_reads_real_files_and_ignores_cover_art() {
        if !run_ffmpeg(&["-version"]) {
            eprintln!("SKIP probe_media_reads_real_files_and_ignores_cover_art: no ffmpeg on PATH");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let video = dir.path().join("clip.mp4");
        assert!(run_ffmpeg(&[
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=320x180:rate=10",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=44100",
            "-t",
            "2",
            "-c:v",
            "mpeg4",
            "-c:a",
            "aac",
            video.to_str().unwrap(),
        ]));
        let facts = probe_media(&tools(), &video, false).unwrap();
        assert_eq!((facts.width, facts.height), (Some(320), Some(180)));
        assert!(facts.has_video && facts.has_audio);
        assert!((1_900..=2_200).contains(&facts.duration_ms), "{facts:?}");

        let cover = dir.path().join("cover.jpg");
        assert!(run_ffmpeg(&[
            "-f",
            "lavfi",
            "-i",
            "color=c=red:size=301x201",
            "-frames:v",
            "1",
            cover.to_str().unwrap(),
        ]));
        let song = dir.path().join("song.mp3");
        if !run_ffmpeg(&[
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=44100",
            "-i",
            cover.to_str().unwrap(),
            "-map",
            "0",
            "-map",
            "1",
            "-t",
            "1",
            "-c:a",
            "libmp3lame",
            "-c:v",
            "copy",
            "-disposition:v",
            "attached_pic",
            song.to_str().unwrap(),
        ]) {
            eprintln!("SKIP the MP3 half: this ffmpeg cannot write an MP3 with a picture");
            return;
        }
        let facts = probe_media(&tools(), &song, true).unwrap();
        assert!(facts.has_audio && !facts.has_video, "{facts:?}");
        assert!(facts.duration_ms > 0);
    }

    // Fix round 1 (RED first against the old shared even rule, which
    // refused both with "no usable video or audio stream"): an ODD-sized
    // video imports with its own size, and an audio-only .mp4 whose cover
    // art is odd-sized — classified as VIDEO by name, so no audio selector —
    // reads as audio, reaching `settle_av_import`'s audio-only rule.
    #[test]
    fn import_probe_accepts_odd_frames_and_cover_art_only_mp4s() {
        if !run_ffmpeg(&["-version"]) {
            eprintln!(
                "SKIP import_probe_accepts_odd_frames_and_cover_art_only_mp4s: no ffmpeg on PATH"
            );
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let odd = dir.path().join("odd.mp4");
        assert!(run_ffmpeg(&[
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=321x181:rate=10",
            "-t",
            "1",
            "-c:v",
            "mpeg4",
            odd.to_str().unwrap(),
        ]));
        let facts = probe_media(&tools(), &odd, false).unwrap();
        assert_eq!((facts.width, facts.height), (Some(321), Some(181)));
        assert!(facts.has_video);

        let cover = dir.path().join("cover.png");
        assert!(run_ffmpeg(&[
            "-f",
            "lavfi",
            "-i",
            "color=c=red:size=301x201",
            "-frames:v",
            "1",
            cover.to_str().unwrap(),
        ]));
        let song = dir.path().join("song.mp4");
        assert!(run_ffmpeg(&[
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=44100",
            "-i",
            cover.to_str().unwrap(),
            "-map",
            "0",
            "-map",
            "1",
            "-t",
            "1",
            "-c:a",
            "aac",
            "-c:v",
            "png",
            "-disposition:v",
            "attached_pic",
            song.to_str().unwrap(),
        ]));
        let facts = probe_media(&tools(), &song, false).unwrap();
        assert!(
            facts.has_audio && !facts.has_video,
            "cover art is not video: {facts:?}"
        );
    }
}
