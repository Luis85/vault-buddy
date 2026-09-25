//! The ffmpeg argument pieces the editor's render builds its argv from —
//! the identity remux, the H.264/AAC encode tails, the runner flags — and
//! the `-progress` parsing. All PURE, and that is the whole point of the
//! route (spec §3: a user-installed ffmpeg, never a bundled one): a function
//! from a plan to an argv list is testable everywhere.
//!
//! Born as the phase-5 export's arguments; Task 59 retired that export, and
//! with it the edited-export graph (`filter_complex`/`reencode_args`) the
//! render's own `render::render_args` superseded.
//!
//! Two traps this module exists to design out:
//!
//! 1. **`out_time_ms` in ffmpeg's `-progress` output is NOT milliseconds.** It
//!    is a long-standing quirk that the field reports MICROseconds, the same
//!    value as `out_time_us`. A parser that read it as milliseconds would
//!    report progress 1000x too fast, so the bar hits 100% almost at once and
//!    then sits there for the rest of the run — which looks like a hang on
//!    exactly the long runs progress exists for. `parse_progress_line`
//!    reads `out_time_us` ONLY and a test forbids `out_time_ms`.
//!
//!    MEASURED, not assumed. Against ffmpeg 6.1.1 the `-progress` stream
//!    carries `bitrate drop_frames dup_frames fps frame out_time out_time_ms
//!    out_time_us progress speed stream_0_0_q total_size` — so `out_time_us`
//!    really is emitted and reading it alone loses nothing — and one tick
//!    read `out_time_us=5990748`, `out_time_ms=5990748`,
//!    `out_time=00:00:05.990748`: the two numeric fields are byte-identical
//!    and both are microseconds. If a build is ever found that emits
//!    `out_time_ms` but NOT `out_time_us`, the fix is to read `out_time_ms`
//!    AS MICROSECONDS — a deliberate design decision, not a relaxation of
//!    `out_time_ms_is_refused_because_its_name_lies_about_its_units`.
//! 2. **Times are formatted with integer arithmetic, never from a float.**
//!    `4500 ms / 1000.0` can render as `4.4999999999999996`, and a filter
//!    takes the string literally, so a cut would land a millisecond off the
//!    one the user approved. `ms_to_ffmpeg_seconds` builds `{s}.{ms:03}` from
//!    integers.
//!
//! Every element of every returned vector is its own `String`. Joining them
//! into one shell-style command line would make ffmpeg see a single unknown
//! option, and would split any source path containing a space into two
//! arguments — staged capture names routinely contain spaces.

use std::path::Path;
use vault_buddy_core::screen_capture_config::{bitrate_bps, ScreenQuality};

/// Audio bitrate for an encoded track. Fixed rather than quality-scaled:
/// screen-capture audio is speech and system sound, where 192k is already
/// transparent, and the quality preset exists to trade VIDEO size against
/// detail (spec §12).
const AUDIO_BITRATE: &str = "192k";

/// The output container, named EXPLICITLY rather than inferred.
///
/// ffmpeg picks its muxer from the output's file EXTENSION unless `-f` says
/// otherwise, and a run never writes to a `.mp4`: the render writes
/// `jobs\<jobId>\out.mp4.part` (the retired export wrote
/// `.<base>.export.mp4.part`) so that a killed or crashed run can never be
/// mistaken for a finished one. `.part` names no format, so without this
/// every run died before writing a byte (GAP-161):
///
/// ```text
/// Unable to choose an output format for '....export.mp4.part';
/// use a standard extension for the filename or specify the format manually.
/// ```
///
/// That is ffmpeg telling us to do exactly this. The `.part` convention is
/// load-bearing across both capture domains and is NOT what changes here --
/// being explicit about the format is correct regardless of what the file is
/// called, and it is what makes the two facts independent.
const OUTPUT_FORMAT: [&str; 2] = ["-f", "mp4"];

/// What an encode needs to know about the file it is producing.
///
/// `Debug` so a failed export can name the settings it ran with in one log
/// line; `Clone` because it costs nothing and the worker hands it around.
/// `h264_encoder` is resolved by probing the installed ffmpeg (Task 4), never
/// hardcoded — a build without `libx264` still has `libopenh264` or a
/// hardware encoder, and naming one that is absent fails the whole export.
#[derive(Debug, Clone)]
pub struct EncodeSettings {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub quality: ScreenQuality,
    pub h264_encoder: String,
    pub has_audio: bool,
}

/// One line of ffmpeg's `-progress` stream, reduced to the only two things
/// the exporter acts on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgressTick {
    /// Output position in MICROseconds, straight from `out_time_us`.
    OutTimeUs(u64),
    /// `progress=end` — ffmpeg has written its last packet.
    Done,
}

/// Encoders that read x264's `-preset` vocabulary.
///
/// EXACT names, never a prefix match: `libx264rgb` is a different encoder
/// that merely shares the prefix, and the same substring trap was already
/// caught once in Task 4's encoder picker.
const PRESET_ENCODERS: [&str; 2] = ["libx264", "libx265"];

/// The `-preset` pair for an encoder, or nothing.
///
/// `-preset` is a libx264/libx265 PRIVATE option while `h264_encoder` is
/// resolved by probing the user's own ffmpeg (Task 4), so the edited export
/// can legitimately be handed `h264_mf`, `h264_nvenc`, `h264_qsv` or
/// `h264_amf` on a build with no libx264 — which is the whole point of not
/// bundling one. Those read a different preset vocabulary, and naming x264's
/// risks failing the export at the payoff, after the user has already
/// recorded and edited.
///
/// MEASURED against ffmpeg 6.1.1, because this module was written with no
/// ffmpeg available and the paragraph above previously stated the failure
/// more strongly than the tool actually behaves:
/// - `libx264 -preset medium` is accepted (exit 0), and `libx264 -preset
///   bogus_value` is a HARD failure (exit 234, "invalid preset"). So an
///   encoder that HAS `-preset` and rejects the VALUE really does kill the
///   export.
/// - An encoder with NO `-preset` option at all (`mpeg4`, and on this build
///   `h264_vaapi` / `h264_v4l2m2m`) does NOT fail: ffmpeg warns "Codec
///   AVOption preset ... has not been used for any stream" and encodes
///   anyway, exit 0.
/// - `h264_qsv` and `h264_nvenc` both happen to accept the literal `medium`
///   here, so for those two the x264 preset would have worked by luck.
///
/// None of that changes the rule. Sending a preset an encoder never asked
/// for buys nothing on the builds where it is harmless, and on the ones
/// where the value is out of vocabulary it loses the whole export. Omitting
/// it takes the encoder's own default, which is always valid.
///
/// An unknown or empty encoder fails SAFE rather than loud: no preset means
/// the encoder's own default, which is always valid. Refusing a missing or
/// unusable encoder is `export_refusal`'s decision (Task 6) and duplicating
/// it here would put one rule in two places that can disagree.
pub fn preset_args(encoder: &str) -> Vec<String> {
    if PRESET_ENCODERS.contains(&encoder) {
        return vec!["-preset".into(), "medium".into()];
    }
    Vec::new()
}

/// Milliseconds as the exact decimal seconds `trim=`/`atrim=` take.
///
/// Integer arithmetic on purpose (trap 2): a float round-trip can render
/// 4500 ms as `4.4999999999999996`, and the filter takes the string it is
/// given. 4500 is always `"4.500"`.
pub fn ms_to_ffmpeg_seconds(ms: u64) -> String {
    format!("{}.{:03}", ms / 1000, ms % 1000)
}

/// The flags a one-input invocation carries, up to and including
/// `-i <source>`.
///
/// `-nostdin` because the child inherits no console in a `windows_subsystem
/// = "windows"` build and an ffmpeg that stops to ask a question would hang
/// the job thread; `-progress pipe:1` puts the machine-readable progress
/// stream on stdout, where `parse_progress_line` reads it, leaving stderr for
/// the error text a failure reports.
fn common_prefix(source: &Path) -> Vec<String> {
    let mut args = runner_flags();
    args.extend(["-i".into(), source.to_string_lossy().into_owned()]);
    args
}

/// Every flag `common_prefix` puts BEFORE the inputs -- shared with the
/// editor's render (`render::render_args`), which has several inputs rather
/// than one and still needs `-progress pipe:1` for `ffmpeg_run` to read.
pub(crate) fn runner_flags() -> Vec<String> {
    vec![
        "-hide_banner".into(),
        "-nostdin".into(),
        "-loglevel".into(),
        "error".into(),
        "-nostats".into(),
        "-progress".into(),
        "pipe:1".into(),
    ]
}

/// The IDENTITY fast path: copy both streams into a new container.
///
/// No decode, no re-encode, no quality loss, and near-instant regardless of
/// the recording's length. Keyed by the render on `RenderPlan::is_identity`
/// (R1) — an untouched capture rendered whole.
pub fn remux_args(source: &Path, dest: &Path) -> Vec<String> {
    let mut args = common_prefix(source);
    args.extend([
        "-c".into(),
        "copy".into(),
        // Moves the index to the front so the file plays before it has been
        // fully downloaded/synced — and so Obsidian's preview can seek it.
        "-movflags".into(),
        "+faststart".into(),
    ]);
    args.extend(OUTPUT_FORMAT.map(String::from));
    args.extend(["-y".into(), dest.to_string_lossy().into_owned()]);
    args
}

/// The H.264 video encode every graph render carries (the phase-5 export's
/// edited path encoded through it too, until Task 59).
pub(crate) fn video_codec_args(settings: &EncodeSettings) -> Vec<String> {
    let bitrate = bitrate_bps(
        settings.quality,
        settings.width,
        settings.height,
        settings.fps,
    );
    let mut args = vec!["-c:v".into(), settings.h264_encoder.clone()];
    args.extend(preset_args(&settings.h264_encoder));
    args.extend([
        "-b:v".into(),
        bitrate.to_string(),
        // Windows' own players and Obsidian's <video> both refuse 4:4:4 H.264.
        "-pix_fmt".into(),
        "yuv420p".into(),
        "-g".into(),
        settings.fps.to_string(),
    ]);
    args
}

/// The AAC encode every graph render's sound carries (Task 44). The render
/// always names it: `render::audio_graph` never omits an audio stream (a
/// silent project still gets `anullsrc`).
pub(crate) fn audio_codec_args() -> Vec<String> {
    vec![
        "-c:a".into(),
        "aac".into(),
        "-b:a".into(),
        AUDIO_BITRATE.into(),
    ]
}

/// faststart, the explicit container (see `OUTPUT_FORMAT`) and the
/// destination -- the tail every encoded output shares.
pub(crate) fn output_args(dest: &Path) -> Vec<String> {
    let mut args: Vec<String> = vec!["-movflags".into(), "+faststart".into()];
    args.extend(OUTPUT_FORMAT.map(String::from));
    args.extend(["-y".into(), dest.to_string_lossy().into_owned()]);
    args
}

/// One `key=value` line of the `-progress` stream.
///
/// Reads `out_time_us` and `progress=end` and nothing else. `out_time_ms` is
/// refused BY NAME (trap 1) — it reports microseconds despite its name, so
/// accepting it as milliseconds would run the progress bar 1000x fast.
/// A value that does not parse as a `u64` — `N/A` before the first frame, a
/// negative position, an empty value — is `None` rather than a 0 or a wrapped
/// number: a missing tick costs nothing, while a bogus one moves the bar
/// backwards.
pub fn parse_progress_line(line: &str) -> Option<ProgressTick> {
    let line = line.trim();
    if let Some(value) = line.strip_prefix("out_time_us=") {
        return value
            .trim()
            .parse::<u64>()
            .ok()
            .map(ProgressTick::OutTimeUs);
    }
    if line == "progress=end" {
        return Some(ProgressTick::Done);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(has_audio: bool) -> EncodeSettings {
        EncodeSettings {
            width: 1920,
            height: 1080,
            fps: 30,
            quality: ScreenQuality::Balanced,
            h264_encoder: "libx264".into(),
            has_audio,
        }
    }

    #[test]
    fn milliseconds_format_as_exact_decimal_seconds() {
        assert_eq!(ms_to_ffmpeg_seconds(0), "0.000");
        assert_eq!(ms_to_ffmpeg_seconds(4_500), "4.500");
        assert_eq!(ms_to_ffmpeg_seconds(1), "0.001");
        assert_eq!(ms_to_ffmpeg_seconds(61_002), "61.002");
    }

    // THE fast path. -c copy is what makes it lossless and near-instant; an
    // args builder that silently emitted an encoder here would produce a
    // correct-looking file after a full transcode, which is the exact
    // failure the Media Foundation design could not test for at all.
    #[test]
    fn the_remux_copies_streams_and_never_names_an_encoder() {
        let args = remux_args(Path::new("in.mp4"), Path::new("out.mp4"));
        let joined = args.join(" ");
        assert!(joined.contains("-c copy"), "remux must copy: {joined}");
        assert!(
            !joined.contains("libx264"),
            "remux must not encode: {joined}"
        );
        assert!(
            !joined.contains("-filter_complex"),
            "remux must not filter: {joined}"
        );
        assert!(
            joined.contains("+faststart"),
            "the index must move to the front for playback"
        );
    }

    // The encode tail every graph render carries: the probed encoder, its
    // quality-scaled bitrate, 4:2:0 and a one-second keyframe interval.
    #[test]
    fn the_video_encode_carries_the_quality_bitrate_and_a_one_second_gop() {
        let joined = video_codec_args(&settings(true)).join(" ");
        assert!(joined.contains("-c:v libx264"), "{joined}");
        assert!(joined.contains("-pix_fmt yuv420p"), "{joined}");
        assert!(joined.contains("-g 30"), "{joined}");
        let expected = vault_buddy_core::screen_capture_config::bitrate_bps(
            ScreenQuality::Balanced,
            1920,
            1080,
            30,
        );
        assert!(joined.contains(&format!("-b:v {expected}")), "{joined}");
        assert_eq!(audio_codec_args().join(" "), "-c:a aac -b:a 192k");
    }

    // Every argument is a separate argv entry. Building one string with
    // spaces makes ffmpeg see a single unknown option, and a source path
    // containing a space becomes two arguments. (The render's own many-input
    // argv is pinned the same way in `render::render_args_tests`.)
    #[test]
    fn a_path_with_spaces_and_quotes_survives_as_one_argument() {
        let src = Path::new("/tmp/2026-09-20 1432 Figma \"design\".mp4");
        let args = remux_args(src, Path::new("/tmp/out.mp4"));
        assert!(
            args.iter()
                .any(|a| a == "/tmp/2026-09-20 1432 Figma \"design\".mp4"),
            "remux source path was split or escaped: {args:?}"
        );
    }

    // REGRESSION: the export wrote    }

    // REGRESSION (GAP-161): the phase-5 export wrote to
    // `.<base>.export.mp4.part` and named no format, so ffmpeg -- which
    // infers its muxer from the extension -- died with "Unable to choose an
    // output format" before writing a byte, and it shipped green because
    // every round trip invented a plain `.mp4` destination. The render
    // writes a `.part` too (`jobs\<jobId>\out.mp4.part`); its round trips
    // SKIP when ffmpeg is absent, so this is the half that runs everywhere.
    // Both shapes that reach a dest: the identity remux, and the tail every
    // graph render ends with.
    #[test]
    fn both_tails_name_the_output_format_because_the_dest_is_a_dot_part() {
        let dest = Path::new("/jobs/job-1/out.mp4.part");
        let mut graph = vec!["-i".to_string(), "in.mp4".to_string()];
        graph.extend(output_args(dest));

        for (label, args) in [
            ("remux", remux_args(Path::new("in.mp4"), dest)),
            ("graph", graph),
        ] {
            let f = args
                .iter()
                .position(|a| a == "-f")
                .unwrap_or_else(|| panic!("{label} named no output format: {args:?}"));
            assert_eq!(args[f + 1], "mp4", "{label} named the wrong format");
            // -f is an OUTPUT option: after the last -i and before the dest,
            // or ffmpeg reads it as the INPUT's format and rejects the source.
            let input = args.iter().position(|a| a == "-i").expect("an input");
            let dest_at = args.len() - 1;
            assert!(
                input < f && f < dest_at,
                "{label} put -f outside the output options: {args:?}"
            );
        }
    }

    #[test]
    fn progress_is_read_from_out_time_us_and_the_end_marker() {
        assert_eq!(
            parse_progress_line("out_time_us=4000000"),
            Some(ProgressTick::OutTimeUs(4_000_000))
        );
        assert_eq!(
            parse_progress_line("progress=end"),
            Some(ProgressTick::Done)
        );
        assert_eq!(parse_progress_line("progress=continue"), None);
        assert_eq!(parse_progress_line("frame=100"), None);
        assert_eq!(parse_progress_line(""), None);
    }

    // ffmpeg's `out_time_ms` field reports MICROseconds, not milliseconds --
    // a long-standing quirk. Reading it as milliseconds makes the bar reach
    // 100% a thousand times too early and then sit there for the rest of a
    // long export, which is precisely the case progress exists for.
    #[test]
    fn out_time_ms_is_refused_because_its_name_lies_about_its_units() {
        assert_eq!(parse_progress_line("out_time_ms=4000000"), None);
    }

    #[test]
    fn a_negative_or_unparseable_progress_value_is_ignored_rather_than_wrapping() {
        assert_eq!(parse_progress_line("out_time_us=N/A"), None);
        assert_eq!(parse_progress_line("out_time_us=-1"), None);
        assert_eq!(parse_progress_line("out_time_us="), None);
    }

    // -preset is a libx264/libx265 PRIVATE option, and h264_encoder is
    // probe-resolved (Task 4), so the edited export can be handed
    // h264_mf/nvenc/qsv/amf on a build without libx264 -- which is the whole
    // point of "user-installed". Those read a different preset vocabulary
    // entirely, so naming x264's makes ffmpeg fail on an unknown or
    // wrongly-valued option and the export dies at the payoff.
    #[test]
    fn the_x264_family_gets_a_preset_and_the_hardware_encoders_get_none() {
        let medium = vec!["-preset".to_string(), "medium".to_string()];
        assert_eq!(preset_args("libx264"), medium);
        assert_eq!(preset_args("libx265"), medium);
        for hw in ["h264_mf", "h264_nvenc", "h264_qsv", "h264_amf"] {
            assert!(
                preset_args(hw).is_empty(),
                "{hw} reads a different preset vocabulary"
            );
        }
    }

    // Fail SAFE, not loud: refusing a missing or unusable encoder is
    // export_refusal's decision (Task 6), and duplicating it here would put
    // the rule in two places that can disagree. No preset means the
    // encoder's own default, which is always valid.
    #[test]
    fn an_unknown_or_empty_encoder_gets_no_preset_rather_than_a_refusal() {
        assert!(preset_args("").is_empty());
        assert!(preset_args("h264_videotoolbox").is_empty());
    }

    // libx264rgb is a DIFFERENT encoder that merely shares a prefix. The
    // same substring trap was caught once already in Task 4's encoder
    // picker; matching the family by prefix reintroduces it here.
    #[test]
    fn libx264rgb_is_not_the_x264_family_a_prefix_match_would_claim() {
        assert!(
            preset_args("libx264rgb").is_empty(),
            "a prefix match claimed libx264rgb for the x264 family"
        );
    }

    // The helper being right does not prove the encode tail consults it.
    #[test]
    fn the_encoders_preset_rule_reaches_the_argument_vector() {
        let args = video_codec_args(&settings(false));
        assert!(
            args.join(" ").contains("-c:v libx264 -preset medium"),
            "{args:?}"
        );

        let mut hw = settings(false);
        hw.h264_encoder = "h264_nvenc".into();
        let joined = video_codec_args(&hw).join(" ");
        assert!(joined.contains("-c:v h264_nvenc"), "{joined}");
        assert!(!joined.contains("-preset"), "{joined}");
    }
}
