//! ffmpeg toolchain: resolving a user-installed `ffmpeg` (config override →
//! registry-augmented PATH → bare fallback), the `ffprobe` beside it, what
//! H.264 encoder the build actually has, and the source facts an export needs.
//! The IPC surface (`detect_ffmpeg` / `set_ffmpeg_path`) mirrors
//! `detect_pandoc` / `set_pandoc_path` exactly.
//!
//! ffmpeg is USER-INSTALLED and detected, never bundled — the posture the
//! Document Import domain set for Pandoc and for the same reason: a ~100 MB
//! copyleft binary does not belong inside an MIT light-installer app, and not
//! distributing it keeps its licence the user's business. Everything
//! tool-agnostic (the registry-fresh PATH, the candidate order, the headless
//! `Command`, the bounded runner) is shared from `external_tool`.
//!
//! ffmpeg needs a **capability axis Pandoc never had**. Pandoc asks only "is
//! it new enough"; ffmpeg must also answer "can it encode H.264", because the
//! unedited export is a `-c copy` remux that needs no encoder at all while an
//! edited one does, and minimal LGPL builds ship without `libx264`. A build
//! with no H.264 encoder is still returned — it can remux — and only the
//! edited path refuses, naming what is missing.

use std::path::Path;
use vault_buddy_core::capture_config;
use vault_buddy_screen::render::{parse_filters_output, FfmpegCapabilities};

use crate::external_tool::{candidates_for, run_capturing, tool_command, Capture, PROBE_TIMEOUT};

/// A resolved ffmpeg toolchain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FfmpegTools {
    pub ffmpeg: String,
    pub ffprobe: String,
    pub version: (u32, u32),
    /// The H.264 encoder to pass to `-c:v`, chosen from what THIS build has.
    /// `None` is a real answer, not a failure: a minimal LGPL build can still
    /// remux an untouched capture. Never defaulted — see `pick_h264_encoder`.
    pub h264_encoder: Option<String>,
    /// The banner's first line, for display in settings.
    pub version_line: String,
}

/// H.264 encoders we know how to drive, in preference order. `libx264` is the
/// quality baseline; `h264_mf` is the Media Foundation wrapper a GPL-free
/// Windows build has; the hardware encoders come last because a busy GPU
/// silently drops frames.
const H264_ENCODERS: [&str; 5] = ["libx264", "h264_mf", "h264_nvenc", "h264_qsv", "h264_amf"];

/// Leading ASCII digits of `s` as a u32, or None when it does not start with
/// one. `7.0-latest-win64-gpl` splits to a minor component of
/// `0-latest-win64-gpl`, which a plain `parse()` rejects.
fn leading_u32(s: &str) -> Option<u32> {
    let digits: String = s.chars().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() {
        return None;
    }
    digits.parse().ok()
}

/// First line of `ffmpeg -version` is `ffmpeg version <x.y...> ...`; return
/// (major, minor). Tolerates the `n` prefix the Windows autobuilds carry
/// (`n7.0-latest-win64-gpl`).
///
/// A date-stamped build (`ffmpeg version 2024-01-01-git-abc123`) returns
/// **None**: those are git snapshots with no numeric version to compare, and
/// falling through to the next candidate is better than inventing one.
pub(crate) fn parse_ffmpeg_version(stdout: &str) -> Option<(u32, u32)> {
    let first = stdout.lines().next()?;
    let mut tokens = first.split_whitespace();
    if tokens.next()? != "ffmpeg" || tokens.next()? != "version" {
        return None;
    }
    let raw = tokens.next()?;
    let ver = raw.strip_prefix('n').unwrap_or(raw);
    let mut parts = ver.split('.');
    let major = leading_u32(parts.next()?)?;
    // `parts.next()?` (not `unwrap_or("0")`, Pandoc's rule): a token with no
    // dot at all is the date-stamped snapshot case, which must not resolve.
    let minor = leading_u32(parts.next()?)?;
    Some((major, minor))
}

/// The best H.264 encoder in `ffmpeg -hide_banner -encoders` output, or None
/// when this build has none of the ones we know how to drive.
///
/// The name column is the SECOND whitespace token of a line
/// (` V..... libx264   H.264 ...`) and is matched with `==`, never `contains`:
/// `libx264rgb` and `h264_mfx` both contain a name we accept but are different
/// codecs, and passing one to `-c:v` would encode something the args builder
/// did not mean.
pub(crate) fn pick_h264_encoder(encoders_stdout: &str) -> Option<String> {
    let names: Vec<&str> = encoders_stdout
        .lines()
        .filter_map(|l| l.split_whitespace().nth(1))
        .collect();
    H264_ENCODERS
        .iter()
        .find(|want| names.iter().any(|n| n == *want))
        .map(|s| (*s).to_string())
}

/// What an export needs to know about the staged source file.
///
/// Read by the export worker: the encoder needs the real pixel dimensions
/// and whether an audio track exists at all, and the companion note records
/// the same dimensions -- the file's own, never the staged sidecar's
/// hand-editable copies. `has_video`/`audio_rate` were added for the
/// tutorial editor's media import (Task 24): a general import can probe a
/// pure-audio file, which the screen-capture export path never could (a
/// capture always has video) — see `probe_source` for how the export path
/// keeps its own "no video, no export" refusal even though
/// `parse_probe_output` itself now accepts an audio-only file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SourceFacts {
    pub width: u32,
    pub height: u32,
    pub has_video: bool,
    pub has_audio: bool,
    /// The first audio stream's sample rate. Read by the media import
    /// (`editor::media_probe::probe_media`): an audio-only file whose one stream reports no rate
    /// is a stream ffprobe found but cannot describe — a damaged file —
    /// and is refused rather than registered as a silent asset.
    pub audio_rate: Option<u32>,
}

/// One stream block of ffprobe's `default=noprint_wrappers=1` output.
#[derive(Debug, Default)]
struct ProbeStream<'a> {
    kind: &'a str,
    width: Option<u32>,
    height: Option<u32>,
    sample_rate: Option<u32>,
    /// `DISPOSITION:attached_pic=1` — an embedded cover picture, which
    /// ffprobe lists as a video stream. Only reported when the query asks
    /// for `stream_disposition=attached_pic` (the import's does).
    attached_pic: bool,
}

/// Split ffprobe output into its stream blocks, GROUPING on each
/// `codec_type=` line rather than scanning for the first `width=` or
/// `sample_rate=` anywhere: a file whose audio stream comes first would
/// otherwise lend its own (absent or `N/A`) dimensions to the video's.
/// Fields before the first `codec_type=` belong to no stream and are
/// dropped; an unparseable number (`N/A`) reads as absent.
fn probe_streams(stdout: &str) -> Vec<ProbeStream<'_>> {
    let mut streams: Vec<ProbeStream> = Vec::new();
    for line in stdout.lines() {
        let Some((key, value)) = line.trim().split_once('=') else {
            continue;
        };
        if key == "codec_type" {
            streams.push(ProbeStream {
                kind: value,
                ..ProbeStream::default()
            });
            continue;
        }
        let Some(stream) = streams.last_mut() else {
            continue;
        };
        match key {
            "width" => stream.width = value.parse().ok(),
            "height" => stream.height = value.parse().ok(),
            "sample_rate" => stream.sample_rate = value.parse().ok(),
            "DISPOSITION:attached_pic" => stream.attached_pic = value == "1",
            _ => {}
        }
    }
    streams
}

/// The facts both parsers share, with the frame-size rule as the one thing
/// that differs: the FIRST video stream that is not cover art supplies the
/// dimensions and must satisfy `frame_ok` (a video stream that exists but
/// fails it is a hard `None`, never silently "no video"); the FIRST audio
/// stream supplies the sample rate; `has_audio` is any audio stream at all.
/// `None` also when there is neither video nor audio.
fn facts_from_streams(
    streams: &[ProbeStream],
    frame_ok: impl Fn(u32, u32) -> bool,
) -> Option<SourceFacts> {
    let audio = streams.iter().find(|s| s.kind == "audio");
    let has_audio = audio.is_some();
    let audio_rate = audio.and_then(|s| s.sample_rate);
    match streams
        .iter()
        .find(|s| s.kind == "video" && !s.attached_pic)
    {
        Some(video) => {
            let (width, height) = (video.width?, video.height?);
            frame_ok(width, height).then_some(SourceFacts {
                width,
                height,
                has_video: true,
                has_audio,
                audio_rate,
            })
        }
        None if has_audio => Some(SourceFacts {
            width: 0,
            height: 0,
            has_video: false,
            has_audio,
            audio_rate,
        }),
        None => None,
    }
}

/// Parse `ffprobe -show_entries stream=codec_type,width,height,sample_rate
/// -of default=noprint_wrappers=1` output for the EXPORT path.
///
/// Returns `None` when there is NEITHER a usable video stream NOR any audio
/// stream. A video stream that IS present but whose dimensions are absent,
/// zero, or ODD is also a hard `None`: the export encodes H.264 at the
/// SOURCE size, 4:2:0 requires even dimensions, and a zero would reach the
/// encoder as an invalid frame size and fail deep in the filter graph with
/// an unreadable error. A file with no video but a real audio stream
/// succeeds with `has_video: false`; the export's own "video required" rule
/// lives in `probe_source`. The media import does NOT use this rule — see
/// `parse_import_probe`.
pub(crate) fn parse_probe_output(stdout: &str) -> Option<SourceFacts> {
    facts_from_streams(&probe_streams(stdout), |w, h| {
        w != 0 && h != 0 && w % 2 == 0 && h % 2 == 0
    })
}

/// The media IMPORT's reading of the same output (Task 25 fix round 1):
/// non-zero dimensions, parity IGNORED — an imported asset is scaled onto
/// one of the editor's even canvases, so its native frame size never
/// reaches an encoder, and a 1366x767 capture or an odd-sized phone clip is
/// ordinary media. Cover art (`DISPOSITION:attached_pic=1`) is not video,
/// so an audio-only `.mp4` with an embedded picture reads as audio.
pub(crate) fn parse_import_probe(stdout: &str) -> Option<SourceFacts> {
    facts_from_streams(&probe_streams(stdout), |w, h| w != 0 && h != 0)
}

/// Convert this shell's own ffprobe facts into the core-owned `ProbeFacts`
/// `asset_from_probe` reads (F22: `core` cannot name `SourceFacts`, which
/// is `pub(crate)` here and carries `audio_rate`, a field `asset_from_probe`
/// has no use for). `duration_ms` is separate because `SourceFacts` itself
/// has no duration field — the export path never needed one, and Task 25
/// (media import, this function's caller) reads it from ffprobe's
/// `format=duration` separately (`editor::media_probe`).
pub(crate) fn source_facts_to_probe_facts(
    facts: &SourceFacts,
    duration_ms: u64,
) -> vault_buddy_core::editor::probe::ProbeFacts {
    vault_buddy_core::editor::probe::ProbeFacts {
        duration_ms,
        width: facts.has_video.then_some(facts.width),
        height: facts.has_video.then_some(facts.height),
        has_video: facts.has_video,
        has_audio: facts.has_audio,
    }
}

/// Probe one ffmpeg candidate: `<program> -version`, bounded by PROBE_TIMEOUT.
/// Returns its (major, minor) and the banner's first line, from one spawn.
fn probe_ffmpeg(program: &str) -> Option<((u32, u32), String)> {
    let mut cmd = tool_command(program);
    cmd.arg("-version");
    let (ok, stdout) = run_capturing(cmd, PROBE_TIMEOUT, Capture::Stdout).ok()?;
    if !ok {
        return None;
    }
    let version = parse_ffmpeg_version(&stdout)?;
    let line = stdout
        .lines()
        .next()
        .map(|l| l.trim().to_string())
        .unwrap_or_default();
    Some((version, line))
}

/// Resolve the `ffprobe` to pair with a resolved `ffmpeg`.
///
/// **The sibling next to the chosen ffmpeg wins.** A machine can carry both on
/// PATH from different installs, and pairing an ffmpeg with a stranger's
/// ffprobe is how a portable build ends up probed by a system one. Mismatched
/// versions are not an error — ffprobe is only asked for width/height and
/// whether an audio track exists — so a PATH fallback, and finally the bare
/// name, are both acceptable.
fn resolve_ffprobe(ffmpeg: &str) -> String {
    let sibling = Path::new(ffmpeg).parent().map(|dir| {
        dir.join(if cfg!(windows) {
            "ffprobe.exe"
        } else {
            "ffprobe"
        })
    });
    if let Some(p) = sibling {
        if p.is_file() {
            return p.to_string_lossy().to_string();
        }
    }
    candidates_for("ffprobe", None)
        .into_iter()
        .next()
        .unwrap_or_else(|| "ffprobe".to_string())
}

/// Resolve an ffmpeg toolchain across the ordered candidates (config override
/// → registry-augmented PATH → bare `ffmpeg`). The FIRST candidate whose
/// `-version` banner parses wins; a candidate that runs but reports no H.264
/// encoder is still returned (it can remux). None when nothing runs.
pub(crate) fn resolve_working_ffmpeg() -> Option<FfmpegTools> {
    let override_path = capture_config::load_config().document_import.ffmpeg_path;
    for program in candidates_for("ffmpeg", override_path.as_deref()) {
        let Some((version, version_line)) = probe_ffmpeg(&program) else {
            continue;
        };
        let mut cmd = tool_command(&program);
        cmd.args(["-hide_banner", "-encoders"]);
        // A failed encoder probe is "no known encoder", not "no ffmpeg": the
        // remux path still works and the edited path refuses with a message.
        let encoders = run_capturing(cmd, PROBE_TIMEOUT, Capture::Stdout)
            .ok()
            .filter(|(ok, _)| *ok)
            .map(|(_, out)| out)
            .unwrap_or_default();
        return Some(FfmpegTools {
            ffprobe: resolve_ffprobe(&program),
            ffmpeg: program,
            version,
            h264_encoder: pick_h264_encoder(&encoders),
            version_line,
        });
    }
    None
}

/// What the resolved ffmpeg can do, for the editor render's refusal
/// (tutorial-editor Task 45, F21): its `-filters` and `-encoders` listings,
/// parsed by `screen::render::parse_filters_output` into the `screen`-owned
/// `FfmpegCapabilities` -- this shell owns only the two spawns, since
/// `screen::render::run::render_refusal` consumes the type and `screen`
/// cannot depend on this crate.
///
/// A listing that fails or times out is logged and read as EMPTY, which
/// refuses a graph render naming the first filter it lacks: reported, never
/// guessed. Both listings sit inside `run_capturing`'s 64 KiB capture cap
/// (measured on 9.0.1: `-filters` 41,986 bytes, `-encoders` 14,713); a
/// build whose `-filters` outgrew it would lose its alphabetical TAIL --
/// `xfade` among it -- and be refused for a dissolve it could render.
#[allow(dead_code)] // First production reader: the render job (Task 46).
pub(crate) fn probe_capabilities(tools: &FfmpegTools) -> FfmpegCapabilities {
    let listing = |flag: &str| {
        let mut cmd = tool_command(&tools.ffmpeg);
        cmd.args(["-hide_banner", flag]);
        match run_capturing(cmd, PROBE_TIMEOUT, Capture::Stdout) {
            Ok((true, out)) => out,
            Ok((false, _)) => {
                log::warn!("ffmpeg {flag} exited unsuccessfully; reading no capabilities");
                String::new()
            }
            Err(e) => {
                log::warn!("ffmpeg {flag} could not be run: {e}");
                String::new()
            }
        }
    };
    parse_filters_output(&listing("-filters")).with_encoders_output(&listing("-encoders"))
}

/// Ask ffprobe for the facts an export needs about `path`.
///
/// NOTE: this invocation has NOT been executed against a real ffprobe in this
/// repository's container (no ffmpeg installed); `parse_probe_output` is
/// fixture-tested and is where the parsing correctness lives. Task 6 installs
/// ffmpeg in CI, which is where this call first runs for real.
pub(crate) fn probe_source(tools: &FfmpegTools, path: &Path) -> Result<SourceFacts, String> {
    let mut cmd = tool_command(&tools.ffprobe);
    cmd.args([
        "-v",
        "error",
        "-show_entries",
        "stream=codec_type,width,height",
    ])
    .args(["-of", "default=noprint_wrappers=1"])
    .arg(path);
    let (ok, stdout) = run_capturing(cmd, PROBE_TIMEOUT, Capture::Stdout)
        .map_err(|e| format!("Could not run ffprobe: {e}"))?;
    if !ok {
        return Err("ffprobe could not read the capture.".to_string());
    }
    let facts = parse_probe_output(&stdout)
        .ok_or_else(|| "The capture has no usable video stream.".to_string())?;
    // `parse_probe_output` now succeeds for an audio-only file too (Task
    // 24, for the general media-import prober) — the screen-capture
    // export path keeps its OWN "video required" refusal here rather than
    // silently exporting an audio-only "capture" with a 0x0 frame.
    if !facts.has_video {
        return Err("The capture has no usable video stream.".to_string());
    }
    Ok(facts)
}

/// The ffmpeg detection status the settings UI renders, mirroring
/// `PandocStatus`.
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FfmpegStatus {
    pub installed: bool,
    pub version: Option<String>,
    pub path: Option<String>,
    pub ffprobe_path: Option<String>,
    /// None means "this build cannot encode H.264": remux still works, saving
    /// an EDITED capture does not. Reported, never guessed.
    pub h264_encoder: Option<String>,
    /// The raw configured override (None → using PATH), so the settings
    /// field can seed itself without a second command.
    pub configured_path: Option<String>,
}

impl FfmpegStatus {
    fn missing(configured_path: Option<String>) -> Self {
        Self {
            installed: false,
            version: None,
            path: None,
            ffprobe_path: None,
            h264_encoder: None,
            configured_path,
        }
    }
}

fn detect_blocking() -> FfmpegStatus {
    let configured = capture_config::load_config()
        .document_import
        .ffmpeg_path
        .filter(|p| !p.trim().is_empty());
    match resolve_working_ffmpeg() {
        Some(tools) => FfmpegStatus {
            installed: true,
            version: Some(tools.version_line),
            path: Some(tools.ffmpeg),
            ffprobe_path: Some(tools.ffprobe),
            h264_encoder: tools.h264_encoder,
            configured_path: configured,
        },
        None => FfmpegStatus::missing(configured),
    }
}

/// Detect ffmpeg on demand (settings-open + Recheck). Async + spawn_blocking:
/// spawning a subprocess is blocking I/O and must stay off the main thread.
#[tauri::command]
pub async fn detect_ffmpeg() -> FfmpegStatus {
    tauri::async_runtime::spawn_blocking(detect_blocking)
        .await
        .unwrap_or_else(|_| FfmpegStatus::missing(None))
}

/// App-global ffmpeg path override (None → PATH lookup). Serialized behind
/// `config_write_lock()`; round-tripped by serialize_config.
///
/// ASYNC for the same reason as `set_pandoc_path`: the shared
/// `config_write_lock()` can be held across a full task-vault scan, so this
/// fsync'd write must run off the main thread.
///
/// READ-MODIFY-WRITE, not a fresh struct: `update_document_import_config`
/// replaces the WHOLE section, so constructing one from this field alone would
/// silently delete the sibling `pandoc_path` override. The read happens under
/// the lock so a concurrent Pandoc save cannot be lost between them.
#[tauri::command]
pub async fn set_ffmpeg_path(ffmpeg_path: Option<String>) -> Result<(), String> {
    let path = ffmpeg_path
        .as_deref()
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(str::to_string);
    let _guard = capture_config::config_write_lock();
    let mut di = capture_config::load_config().document_import;
    di.ffmpeg_path = path;
    capture_config::update_document_import_config(di)
}

#[cfg(test)]
mod tests {
    use super::*;

    // F21: the shell's half of the capability probe really spawns the two
    // listings and hands them to `screen`'s parser -- on a host with ffmpeg
    // the render's own filters and encoders come back. SKIPS VISIBLY
    // without ffmpeg (a skip is not a pass).
    #[test]
    fn probe_capabilities_reads_the_installed_ffmpeg() {
        let Some(tools) = resolve_working_ffmpeg() else {
            eprintln!(
                "SKIP probe_capabilities_reads_the_installed_ffmpeg: no ffmpeg resolved,                  so the capability probe is UNPROVEN in this run"
            );
            return;
        };
        let caps = probe_capabilities(&tools);
        for filter in ["overlay", "amix", "anullsrc", "color"] {
            assert!(caps.has_filter(filter), "{filter} missing from {tools:?}");
        }
        assert!(caps.has_encoder("aac"), "aac missing from {tools:?}");
        if let Some(h264) = &tools.h264_encoder {
            assert!(caps.has_encoder(h264), "the picked {h264} is not listed");
        }
    }

    #[test]
    fn parses_the_version_from_a_real_banner_line() {
        assert_eq!(
            parse_ffmpeg_version("ffmpeg version 6.1.1-3ubuntu5 Copyright (c)"),
            Some((6, 1))
        );
        assert_eq!(
            parse_ffmpeg_version("ffmpeg version n7.0-latest-win64-gpl"),
            Some((7, 0))
        );
        assert_eq!(
            parse_ffmpeg_version("ffmpeg version 2024-01-01-git-abc123 Copyright"),
            None
        );
        assert_eq!(parse_ffmpeg_version("pandoc 3.1.9"), None);
        assert_eq!(parse_ffmpeg_version(""), None);
    }

    // The ORDER is the decision: libx264 is the quality baseline, h264_mf is
    // what a GPL-free Windows build has, and the hardware encoders are last
    // because a busy GPU silently drops frames.
    #[test]
    fn picks_the_best_available_h264_encoder_in_a_fixed_order() {
        let all = " V..... libx264   H.264\n V..... h264_mf   H.264\n V..... h264_nvenc H.264\n";
        assert_eq!(pick_h264_encoder(all).as_deref(), Some("libx264"));
        let no_x264 = " V..... h264_mf   H.264\n V..... h264_nvenc H.264\n";
        assert_eq!(pick_h264_encoder(no_x264).as_deref(), Some("h264_mf"));
        let hw_only = " V..... h264_nvenc H.264\n";
        assert_eq!(pick_h264_encoder(hw_only).as_deref(), Some("h264_nvenc"));
    }

    // A minimal LGPL build with no H.264 encoder at all must be REPORTED, not
    // guessed at: it can still remux an untouched capture, and only the
    // edited path has to refuse. Returning a default here would produce an
    // ffmpeg invocation that fails with an unreadable error much later.
    #[test]
    fn a_build_with_no_h264_encoder_reports_none_rather_than_defaulting() {
        assert_eq!(
            pick_h264_encoder(" V..... vp9 VP9\n A..... aac AAC\n"),
            None
        );
        assert_eq!(pick_h264_encoder(""), None);
    }

    // `libx264rgb` and `h264_v4l2m2m` both CONTAIN an encoder name we accept.
    // Matching on a substring would pick a codec the args builder did not
    // mean, so the scan must match the whole token in the name column.
    #[test]
    fn a_longer_encoder_name_that_contains_ours_is_not_mistaken_for_it() {
        assert_eq!(pick_h264_encoder(" V..... libx264rgb H.264 RGB\n"), None);
        assert_eq!(pick_h264_encoder(" V..... h264_mfx  something\n"), None);
    }

    #[test]
    fn reads_width_height_and_the_presence_of_an_audio_track_from_ffprobe() {
        // -show_entries stream=codec_type,width,height -of default=noprint_wrappers=1
        let both = "codec_type=video\nwidth=1920\nheight=1080\ncodec_type=audio\n";
        assert_eq!(
            parse_probe_output(both),
            Some(SourceFacts {
                width: 1920,
                height: 1080,
                has_video: true,
                has_audio: true,
                audio_rate: None,
            })
        );
    }

    // A silent capture is legal (spec 6.5) and must probe cleanly, not fail.
    #[test]
    fn a_capture_with_no_audio_track_probes_as_silent_rather_than_failing() {
        let video_only = "codec_type=video\nwidth=1280\nheight=720\n";
        assert_eq!(
            parse_probe_output(video_only),
            Some(SourceFacts {
                width: 1280,
                height: 720,
                has_video: true,
                has_audio: false,
                audio_rate: None,
            })
        );
    }

    // Genuinely NO stream information at all (no codec_type line whatsoever,
    // or a stray field with no codec_type context) means this function can
    // say nothing about the file — a hard `None`. This used to also cover a
    // literal "codec_type=audio\n" line; that sub-case moved to
    // `probe_output_reports_audio_only_files` below once Task 24 taught this
    // function to answer for an audio-only file instead of refusing it.
    #[test]
    fn a_file_with_no_stream_information_probes_as_none() {
        assert_eq!(parse_probe_output(""), None);
        assert_eq!(parse_probe_output("width=1920\n"), None);
    }

    // Task 24 (tutorial editor media import): a general import can probe a
    // pure-audio file (no video stream), and this must now succeed rather
    // than refuse — `probe_source` (the screen-capture export path) is what
    // keeps requiring video, by checking `has_video` itself after this call.
    #[test]
    fn probe_output_reports_audio_only_files() {
        let audio_only = "codec_type=audio\nsample_rate=44100\n";
        assert_eq!(
            parse_probe_output(audio_only),
            Some(SourceFacts {
                width: 0,
                height: 0,
                has_video: false,
                has_audio: true,
                audio_rate: Some(44_100),
            })
        );
    }

    #[test]
    fn an_odd_or_zero_dimension_is_refused_rather_than_reaching_the_encoder() {
        // H.264 4:2:0 requires even dimensions; ffmpeg would fail with an
        // unreadable error deep in the filter graph. A video stream that
        // exists but is unusable must stay a hard refusal, never silently
        // read as "no video, but there's audio".
        assert_eq!(
            parse_probe_output("codec_type=video\nwidth=0\nheight=1080\n"),
            None
        );
        assert_eq!(
            parse_probe_output("codec_type=video\nwidth=1921\nheight=1080\n"),
            None
        );
    }

    // F22: the shell-side conversion must carry every field
    // `asset_from_probe` reads — a width/height mixup or a dropped
    // has_video/has_audio would silently misclassify an imported asset.
    #[test]
    fn source_facts_converts_to_probe_facts() {
        let video = SourceFacts {
            width: 1920,
            height: 1080,
            has_video: true,
            has_audio: true,
            audio_rate: None,
        };
        let facts = source_facts_to_probe_facts(&video, 12_345);
        assert_eq!(facts.duration_ms, 12_345);
        assert_eq!(facts.width, Some(1920));
        assert_eq!(facts.height, Some(1080));
        assert!(facts.has_video);
        assert!(facts.has_audio);

        // An audio-only source carries no real width/height (0x0 is a
        // placeholder, not a real frame size) — the conversion must map
        // those to `None`, never pass the placeholder zeros through.
        let audio = SourceFacts {
            width: 0,
            height: 0,
            has_video: false,
            has_audio: true,
            audio_rate: Some(44_100),
        };
        let facts = source_facts_to_probe_facts(&audio, 9_000);
        assert_eq!(facts.duration_ms, 9_000);
        assert_eq!(facts.width, None);
        assert_eq!(facts.height, None);
        assert!(!facts.has_video);
        assert!(facts.has_audio);
    }
}
