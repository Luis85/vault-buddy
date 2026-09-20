//! The export's ffmpeg argument vectors, its `filter_complex` graph, and its
//! `-progress` parsing — all PURE, and that is the whole point of the route.
//!
//! Under the Media Foundation design the export's correctness lived in COM
//! calls that execute in no automated test on any platform. Shelling out to a
//! user-installed ffmpeg turns it into a function from an edit plan to an
//! argv list, which is testable everywhere. Task 6 runs these arguments
//! against a real binary in CI; the tests below pin the intent.
//!
//! Three traps this module exists to design out:
//!
//! 1. **`out_time_ms` in ffmpeg's `-progress` output is NOT milliseconds.** It
//!    is a long-standing quirk that the field reports MICROseconds, the same
//!    value as `out_time_us`. A parser that read it as milliseconds would
//!    report progress 1000x too fast, so the bar hits 100% almost at once and
//!    then sits there for the rest of the export — which looks like a hang on
//!    exactly the long exports progress exists for. `parse_progress_line`
//!    reads `out_time_us` ONLY and a test forbids `out_time_ms`.
//! 2. **Times are formatted with integer arithmetic, never from a float.**
//!    `4500 ms / 1000.0` can render as `4.4999999999999996`, and `trim=` takes
//!    the string literally, so a cut would land a millisecond off the one the
//!    user approved. `ms_to_ffmpeg_seconds` builds `{s}.{ms:03}` from integers.
//! 3. **A zero-length span must never reach the filter graph.**
//!    `trim=start=4:end=4` yields an EMPTY stream and `concat` then fails with
//!    a message that names none of this. `select::plan` already drops them,
//!    but it lives in another module and only one of the two is tested against
//!    the editor's shared fixtures, so `filter_complex` ASSERTS the invariant
//!    rather than assuming it. Callers plan through `select::plan`; a panic
//!    here means a caller hand-built a span list, which is a defect, not a
//!    user-reachable state.
//!
//! Every element of every returned vector is its own `String`. Joining them
//! into one shell-style command line would make ffmpeg see a single unknown
//! option, and would split any source path containing a space into two
//! arguments — staged capture names routinely contain spaces.

use crate::select::PlanSpan;
use std::path::Path;
use vault_buddy_core::screen_capture_config::{bitrate_bps, ScreenQuality};

/// Audio bitrate for the re-encoded track. Fixed rather than quality-scaled:
/// screen-capture audio is speech and system sound, where 192k is already
/// transparent, and the quality preset exists to trade VIDEO size against
/// detail (spec §12).
const AUDIO_BITRATE: &str = "192k";

/// What an edited export needs to know about the file it is producing.
/// `h264_encoder` is resolved by probing the installed ffmpeg (Task 4), never
/// hardcoded — a build without `libx264` still has `libopenh264` or a
/// hardware encoder, and naming one that is absent fails the whole export.
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

/// Milliseconds as the exact decimal seconds `trim=`/`atrim=` take.
///
/// Integer arithmetic on purpose (trap 2): a float round-trip can render
/// 4500 ms as `4.4999999999999996`, and the filter takes the string it is
/// given. 4500 is always `"4.500"`.
pub fn ms_to_ffmpeg_seconds(ms: u64) -> String {
    format!("{}.{:03}", ms / 1000, ms % 1000)
}

/// The flags every invocation carries, up to and including `-i <source>`.
///
/// `-nostdin` because the child inherits no console in a `windows_subsystem
/// = "windows"` build and an ffmpeg that stops to ask a question would hang
/// the export thread; `-progress pipe:1` puts the machine-readable progress
/// stream on stdout, where `parse_progress_line` reads it, leaving stderr for
/// the error text a failure reports.
fn common_prefix(source: &Path) -> Vec<String> {
    vec![
        "-hide_banner".into(),
        "-nostdin".into(),
        "-loglevel".into(),
        "error".into(),
        "-nostats".into(),
        "-progress".into(),
        "pipe:1".into(),
        "-i".into(),
        source.to_string_lossy().into_owned(),
    ]
}

/// The UNEDITED fast path: copy both streams into a new container.
///
/// No decode, no re-encode, no quality loss, and near-instant regardless of
/// the recording's length. Keyed by the caller on
/// `Timeline::is_untouched(source_duration_ms)` — never on `Option::is_none()`,
/// which a resumed edit makes false.
pub fn remux_args(source: &Path, dest: &Path) -> Vec<String> {
    let mut args = common_prefix(source);
    args.extend([
        "-c".into(),
        "copy".into(),
        // Moves the index to the front so the file plays before it has been
        // fully downloaded/synced — and so Obsidian's preview can seek it.
        "-movflags".into(),
        "+faststart".into(),
        "-y".into(),
        dest.to_string_lossy().into_owned(),
    ]);
    args
}

/// The `trim`/`atrim` + `concat` graph for an edited export.
///
/// One pair per span, numbered by PLAN index — the order of the `concat`
/// inputs IS the user's reorder. Emitting them in source order instead
/// produces a file that is valid and playable and plays the blocks in the
/// wrong sequence, which no later stage can detect.
///
/// # Panics
/// On a zero-length span (trap 3). `select::plan` never produces one.
pub fn filter_complex(spans: &[PlanSpan], has_audio: bool) -> String {
    let mut chains: Vec<String> = Vec::with_capacity(spans.len() * 2);
    let mut inputs = String::new();
    for (i, span) in spans.iter().enumerate() {
        assert!(
            span.source_start_ms < span.source_end_ms,
            "zero-length span {span:?} would make concat fail on an empty stream"
        );
        let start = ms_to_ffmpeg_seconds(span.source_start_ms);
        let end = ms_to_ffmpeg_seconds(span.source_end_ms);
        // setpts/asetpts rebase each trimmed piece to zero; without them
        // concat receives its inputs still carrying source timestamps and the
        // output holds the original gaps.
        chains.push(format!(
            "[0:v]trim=start={start}:end={end},setpts=PTS-STARTPTS[v{i}]"
        ));
        inputs.push_str(&format!("[v{i}]"));
        if has_audio {
            chains.push(format!(
                "[0:a]atrim=start={start}:end={end},asetpts=PTS-STARTPTS[a{i}]"
            ));
            inputs.push_str(&format!("[a{i}]"));
        }
    }
    // concat takes its inputs grouped per segment, video then audio.
    let audio_streams = u8::from(has_audio);
    let pads = if has_audio { "[outv][outa]" } else { "[outv]" };
    chains.push(format!(
        "{inputs}concat=n={}:v=1:a={audio_streams}{pads}",
        spans.len()
    ));
    chains.join(";")
}

/// The EDITED path: one `filter_complex` pass that trims, restamps and
/// concatenates, then re-encodes.
///
/// `-g <fps>` is a one-second keyframe interval, matching what the capture
/// declared; without it ffmpeg's default GOP makes seeking in the saved file
/// coarser than seeking in the staged one the user just edited.
pub fn reencode_args(
    source: &Path,
    dest: &Path,
    spans: &[PlanSpan],
    settings: &EncodeSettings,
) -> Vec<String> {
    let mut args = common_prefix(source);
    args.extend([
        "-filter_complex".into(),
        filter_complex(spans, settings.has_audio),
        "-map".into(),
        "[outv]".into(),
    ]);
    if settings.has_audio {
        args.extend(["-map".into(), "[outa]".into()]);
    }
    let bitrate = bitrate_bps(
        settings.quality,
        settings.width,
        settings.height,
        settings.fps,
    );
    args.extend([
        "-c:v".into(),
        settings.h264_encoder.clone(),
        "-preset".into(),
        "medium".into(),
        "-b:v".into(),
        bitrate.to_string(),
        // Windows' own players and Obsidian's <video> both refuse 4:4:4 H.264.
        "-pix_fmt".into(),
        "yuv420p".into(),
        "-g".into(),
        settings.fps.to_string(),
    ]);
    if settings.has_audio {
        args.extend([
            "-c:a".into(),
            "aac".into(),
            "-b:a".into(),
            AUDIO_BITRATE.into(),
        ]);
    }
    args.extend([
        "-movflags".into(),
        "+faststart".into(),
        "-y".into(),
        dest.to_string_lossy().into_owned(),
    ]);
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
    use crate::select::plan;
    use vault_buddy_core::timeline::Timeline;

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

    #[test]
    fn a_single_span_trims_video_and_audio_and_concatenates_one_of_each() {
        let spans = plan(&Timeline::whole(9_000).split_at(4_000).delete(0));
        let f = filter_complex(&spans, true);
        assert!(
            f.contains("[0:v]trim=start=4.000:end=9.000,setpts=PTS-STARTPTS[v0]"),
            "{f}"
        );
        assert!(
            f.contains("[0:a]atrim=start=4.000:end=9.000,asetpts=PTS-STARTPTS[a0]"),
            "{f}"
        );
        assert!(f.contains("[v0][a0]concat=n=1:v=1:a=1[outv][outa]"), "{f}");
    }

    // The ORDER of the concat inputs is the reorder. Emitting them in source
    // order instead of plan order produces a file whose blocks play in the
    // wrong sequence -- valid, playable, and not what the user approved.
    #[test]
    fn spans_are_concatenated_in_plan_order_not_source_order() {
        let t = Timeline::whole(9_000)
            .split_at(3_000)
            .split_at(6_000)
            .reorder(0, 2);
        let spans = plan(&t);
        let f = filter_complex(&spans, false);
        let i0 = f.find("trim=start=3.000").expect("first plan span");
        let i1 = f.find("trim=start=6.000").expect("second plan span");
        let i2 = f.find("trim=start=0.000").expect("the moved span");
        assert!(i0 < i1 && i1 < i2, "spans emitted out of plan order: {f}");
        assert!(f.contains("[v0][v1][v2]concat=n=3:v=1:a=0[outv]"), "{f}");
    }

    // A silent capture is legal (spec 6.5). Emitting atrim for a file with
    // no audio track makes ffmpeg fail with "Stream specifier ':a' matches
    // no streams", which names nothing the user did.
    #[test]
    fn a_silent_capture_emits_no_audio_filters_and_no_audio_output_pad() {
        let spans = plan(&Timeline::whole(5_000));
        let f = filter_complex(&spans, false);
        assert!(!f.contains("atrim"), "{f}");
        assert!(!f.contains("[outa]"), "{f}");
        assert!(f.contains("a=0"), "{f}");
    }

    #[test]
    fn the_encode_maps_the_filter_outputs_and_carries_the_quality_bitrate() {
        let spans = plan(&Timeline::whole(9_000).split_at(4_000).delete(0));
        let args = reencode_args(
            Path::new("in.mp4"),
            Path::new("out.mp4"),
            &spans,
            &settings(true),
        );
        let joined = args.join(" ");
        assert!(joined.contains("-map [outv]"), "{joined}");
        assert!(joined.contains("-map [outa]"), "{joined}");
        assert!(joined.contains("-c:v libx264"), "{joined}");
        assert!(joined.contains("-c:a aac"), "{joined}");
        // One-second keyframe interval, matching what the capture declared.
        assert!(joined.contains("-g 30"), "{joined}");
        let expected = vault_buddy_core::screen_capture_config::bitrate_bps(
            ScreenQuality::Balanced,
            1920,
            1080,
            30,
        );
        assert!(joined.contains(&format!("-b:v {expected}")), "{joined}");
    }

    #[test]
    fn a_silent_encode_maps_only_video_and_names_no_audio_codec() {
        let spans = plan(&Timeline::whole(5_000));
        let args = reencode_args(
            Path::new("in.mp4"),
            Path::new("out.mp4"),
            &spans,
            &settings(false),
        );
        let joined = args.join(" ");
        assert!(joined.contains("-map [outv]"), "{joined}");
        assert!(!joined.contains("[outa]"), "{joined}");
        assert!(!joined.contains("-c:a"), "{joined}");
    }

    // Every argument is a separate argv entry. Building one string with
    // spaces makes ffmpeg see a single unknown option, and a source path
    // containing a space becomes two arguments.
    //
    // BOTH builders are covered on purpose. The plan's mutation row for this
    // test named `reencode_args` while the test as written only called
    // `remux_args`, so collapsing the EDITED path's whole argv into one
    // String left every test green -- and the edited path is the one this
    // module exists for.
    #[test]
    fn a_path_with_spaces_and_quotes_survives_as_one_argument() {
        let src = Path::new("/tmp/2026-09-20 1432 Figma \"design\".mp4");
        let args = remux_args(src, Path::new("/tmp/out.mp4"));
        assert!(
            args.iter()
                .any(|a| a == "/tmp/2026-09-20 1432 Figma \"design\".mp4"),
            "remux source path was split or escaped: {args:?}"
        );

        let spans = plan(&Timeline::whole(9_000).split_at(4_000).delete(0));
        let args = reencode_args(src, Path::new("/tmp/out.mp4"), &spans, &settings(true));
        assert!(
            args.iter()
                .any(|a| a == "/tmp/2026-09-20 1432 Figma \"design\".mp4"),
            "encode source path was split or escaped: {args:?}"
        );
        // The graph is one argument too: fused to -filter_complex or to the
        // -map that follows it, ffmpeg reads it as an unknown option.
        assert!(
            args.iter().any(|a| a.starts_with("[0:v]trim=")),
            "the filter graph must be its own argv entry: {args:?}"
        );
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

    // select::plan already drops zero-length spans, but it lives in another
    // module and only one of the two is tested against the editor's shared
    // fixtures. trim=start=4:end=4 yields an empty stream and concat then
    // fails naming none of this.
    #[test]
    fn a_zero_length_span_is_refused_rather_than_emitted() {
        let bad = [PlanSpan {
            source_start_ms: 4_000,
            source_end_ms: 4_000,
            output_start_ms: 0,
        }];
        assert!(std::panic::catch_unwind(|| filter_complex(&bad, false)).is_err());
    }
}
