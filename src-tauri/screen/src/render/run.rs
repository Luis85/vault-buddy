//! Running one editor render end to end (tutorial-editor Task 45; R1,
//! F-04, F-41, F-42): refuse what the installed ffmpeg cannot do, write
//! the ASS documents into the job directory, build the argv
//! (`render_args`), run it through the SAME runner the phase-5 export uses
//! (`ffmpeg_run::run`), then VERIFY the file with ffprobe before calling it
//! a product.
//!
//! **The refusal comes first, and it is a refusal.** `render_refusal`
//! answers "can this ffmpeg run this plan at all" before a child exists or
//! a byte is written, naming the missing filter or encoder -- the
//! `export_refusal` posture, and what the shell maps to `encoderUnavailable`.
//! It matters more here than for the export: several filters the graph
//! uses are build-dependent. `perspective`, `eq` and `geq` are GPL-only in
//! ffmpeg's own configure (its `*_filter_deps="gpl"` -- read, not measured:
//! every build on hand is GPL), so an LGPL build lacks them; `ass` needs
//! libass;
//! and `xfade` exists only from **ffmpeg 4.3**, which is therefore the
//! render's version floor. The floor is enforced by this FILTER probe, not
//! by parsing a version banner: a dissolve on an older build is refused
//! naming `xfade` and the floor, and a plan without one is not refused for
//! a filter it never uses.
//!
//! **`required_filters` is derived from the plan's FEATURES**, not read
//! back out of the generated graph: the always-present core filters every
//! graph render may use, plus each optional one exactly when a feature that
//! emits it is present. `run_tests::required_filters_cover_every_filter_the_
//! graph_uses` is the independent check -- it parses the real argv of a
//! plan that uses every feature and fails naming any filter this list lacks.
//! An identity plan (R1) is a stream copy and needs no filter at all.
//!
//! **Two ASS documents, two files** (GAP-173's controller ruling): the
//! pre-zoom teaching cues and card text go to `cues.ass`, the post-zoom
//! burned captions to `captions.ass`, each only when it has content, both
//! in the job's own directory -- the render's scratch space, never a vault.
//!
//! **Verification.** ffmpeg exiting 0 is not proof of a usable product: the
//! output must hold a video stream, an audio stream (a render always mixes
//! one -- `audio_graph`'s `anullsrc` -- and only the remux of a SILENT
//! source legitimately has none), and a duration within one frame + 40 ms of
//! the plan's. A file that fails is DELETED, the runner's own rule for a
//! failed run: leaving it would offer a broken render as a finished one.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::AtomicBool;

use vault_buddy_core::capture_note::write_atomic_replacing;
use vault_buddy_core::editor::model_cues::EffectKind;
use vault_buddy_core::editor::render_plan::RenderPlan;

use super::ass::{build_caption_ass, build_cue_ass};
use super::video_graph::file_input_count;
use super::video_layers::adjustments;
use super::{render_args, AssHooks, FfmpegCapabilities};
use crate::ffmpeg_args::EncodeSettings;
use crate::ffmpeg_run::run;
use crate::ScreenError;

/// The oldest ffmpeg whose filter set can run every render feature
/// (`xfade` arrived in 4.3). Named in a missing-filter refusal.
pub const MIN_FFMPEG_VERSION: &str = "4.3";

/// The audio encoder every graph render uses (`ffmpeg_args::
/// audio_codec_args`).
const AUDIO_ENCODER: &str = "aac";

/// Duration slack on top of one frame (the brief's "+/- 1 frame + 40 ms"):
/// AAC priming and the last packet's length move a container's duration
/// by a few tens of milliseconds.
const DURATION_SLACK_MS: u64 = 40;

/// Filters any graph render may use -- core libavfilter that no build able
/// to run a `filter_complex` at all lacks, so requiring them never refuses
/// a real build; listed so the coverage test holds them to account too.
const CORE_FILTERS: [&str; 19] = [
    "color",
    "overlay",
    "null",
    "trim",
    "setpts",
    "fps",
    "format",
    "scale",
    "pad",
    "crop",
    "anullsrc",
    "atrim",
    "asetpts",
    "volume",
    "adelay",
    "amix",
    "alimiter",
    "aresample",
    "aformat",
];

/// Everything one render needs. Borrowed: the job (Task 46) owns it all.
pub struct RenderRequestNative<'a> {
    pub ffmpeg: &'a Path,
    pub ffprobe: &'a Path,
    pub plan: &'a RenderPlan,
    /// The source files by `PlanSource::input_index`. Only the first
    /// `video_graph::file_input_count(plan)` are read, so a job may pass
    /// its whole source list; too FEW is an error, never `render_args`'
    /// panic.
    pub inputs: &'a [PathBuf],
    /// The job's own scratch directory: the ASS documents are written here.
    pub job_dir: &'a Path,
    pub dest: &'a Path,
    /// Where libass should look for fonts (`AssHooks::fontsdir`).
    pub fontsdir: Option<&'a Path>,
    /// The installed ffmpeg's probed filters and encoders.
    pub caps: &'a FfmpegCapabilities,
    pub settings: EncodeSettings,
}

/// What a finished, verified render produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderOutcome {
    /// The output's PROBED duration, already checked against the plan's.
    pub duration_ms: u64,
    /// R1: an identity plan was stream-copied, not re-encoded.
    pub remuxed: bool,
}

/// What ffprobe says about a rendered file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutputProbe {
    pub duration_ms: Option<u64>,
    pub video_streams: usize,
    pub audio_streams: usize,
}

/// The filters rendering `plan` uses (see the module doc). Empty for an
/// identity plan, which is a stream copy.
pub fn required_filters(plan: &RenderPlan) -> BTreeSet<&'static str> {
    let mut needs = BTreeSet::new();
    if plan.is_identity() {
        return needs;
    }
    needs.extend(CORE_FILTERS);
    for layer in &plan.video_layers {
        use vault_buddy_core::editor::model::{FrameShape, Rotation};
        if layer.flip_y || layer.rotation == Rotation::Deg180 {
            needs.insert("vflip");
        }
        if layer.mirror || layer.rotation == Rotation::Deg180 {
            needs.insert("hflip");
        }
        if matches!(layer.rotation, Rotation::Deg90 | Rotation::Deg270) {
            needs.insert("transpose");
        }
        // The exact filters `video_layers` emits for these adjustments --
        // `eq` is GPL-only, so a sepia-only grade must not demand it.
        for filter in adjustments(layer.adjustments.as_ref()) {
            needs.extend(filter_name(&filter));
        }
        if layer.frame_shape != FrameShape::Rectangle {
            needs.insert("geq");
        }
        surface_filters(&mut needs, layer.opacity, layer.fade_in + layer.fade_out);
        if layer.transition_in.is_some() || layer.transition_out.is_some() {
            needs.insert("xfade");
        }
    }
    for card in &plan.cards {
        surface_filters(&mut needs, card.opacity, card.fade_in + card.fade_out);
        if card.transition_in.is_some() || card.transition_out.is_some() {
            needs.insert("xfade");
        }
    }
    if plan.cues.iter().any(|c| c.kind == EffectKind::Zoom) {
        needs.insert("perspective");
    }
    if build_cue_ass(plan).is_some() || build_caption_ass(plan).is_some() {
        needs.insert("ass");
    }
    for a in &plan.audio {
        if a.speed != 1.0 {
            needs.insert(if a.preserve_pitch {
                "atempo"
            } else {
                "asetrate"
            });
        }
        let crossfades = a.crossfade_in.is_some() || a.crossfade_out.is_some();
        if crossfades {
            needs.insert("acrossfade");
        }
        // An orphaned crossfade half falls back to a plain `afade`.
        if crossfades || a.fade_in > 0 || a.fade_out > 0 {
            needs.insert("afade");
        }
    }
    needs
}

/// The alpha side of a layer or card (`video_layers::alpha`).
fn surface_filters(needs: &mut BTreeSet<&'static str>, opacity: f64, fades_ms: u64) {
    if opacity < 1.0 {
        needs.insert("colorchannelmixer");
    }
    if fades_ms > 0 {
        needs.insert("fade");
    }
}

/// `"eq=brightness=..."` -> `"eq"`, as the static name this module lists.
fn filter_name(filter: &str) -> Option<&'static str> {
    let name = filter.split('=').next()?;
    ["eq", "colorchannelmixer"]
        .into_iter()
        .find(|known| *known == name)
}

/// Why the installed ffmpeg cannot render `plan`, or `None`. `h264_encoder`
/// is the probed encoder the render would pass to `-c:v` (empty: none).
/// An identity plan is a stream copy and is never refused.
pub fn render_refusal(
    plan: &RenderPlan,
    caps: &FfmpegCapabilities,
    h264_encoder: &str,
) -> Option<String> {
    if plan.is_identity() {
        return None;
    }
    let missing: Vec<String> = required_filters(plan)
        .into_iter()
        .filter(|f| !caps.has_filter(f))
        .map(|f| format!("\"{f}\""))
        .collect();
    if !missing.is_empty() {
        return Some(format!(
            "The installed ffmpeg is missing the {} filter{} this render needs. Install a \
             full ffmpeg build ({MIN_FFMPEG_VERSION} or newer, with libass for burned-in \
             text) and render again.",
            missing.join(", "),
            if missing.len() == 1 { "" } else { "s" }
        ));
    }
    if h264_encoder.is_empty() {
        return Some(
            "The installed ffmpeg has no H.264 encoder to render with. Install a build with \
             libx264."
                .into(),
        );
    }
    if !caps.has_encoder(h264_encoder) {
        return Some(format!(
            "The installed ffmpeg no longer lists the \"{h264_encoder}\" encoder. Check the \
             ffmpeg in Buddy settings and render again."
        ));
    }
    if !caps.has_encoder(AUDIO_ENCODER) {
        return Some(
            "The installed ffmpeg has no \"aac\" audio encoder to render with. Install a full \
             ffmpeg build."
                .into(),
        );
    }
    None
}

/// Writes `cues.ass` (pre-zoom) and `captions.ass` (post-zoom) into
/// `job_dir`, each only when its document has content; returns their paths.
pub fn write_ass_documents(
    plan: &RenderPlan,
    job_dir: &Path,
) -> Result<(Option<PathBuf>, Option<PathBuf>), ScreenError> {
    let write = |name: &str, text: Option<String>| -> Result<Option<PathBuf>, ScreenError> {
        let Some(text) = text else {
            return Ok(None);
        };
        let path = job_dir.join(name);
        write_atomic_replacing(&path, &text)
            .map_err(|e| ScreenError::Io(format!("could not write {}: {e}", path.display())))?;
        Ok(Some(path))
    };
    Ok((
        write("cues.ass", build_cue_ass(plan))?,
        write("captions.ass", build_caption_ass(plan))?,
    ))
}

/// Reads `ffprobe -show_entries format=duration:stream=codec_type -of
/// default=noprint_wrappers=1` output.
pub fn parse_output_probe(text: &str) -> OutputProbe {
    let mut probe = OutputProbe {
        duration_ms: None,
        video_streams: 0,
        audio_streams: 0,
    };
    for line in text.lines().map(str::trim) {
        match line.split_once('=') {
            Some(("codec_type", "video")) => probe.video_streams += 1,
            Some(("codec_type", "audio")) => probe.audio_streams += 1,
            Some(("duration", value)) => {
                probe.duration_ms = value
                    .parse::<f64>()
                    .ok()
                    .filter(|s| s.is_finite() && *s >= 0.0)
                    .map(|s| (s * 1_000.0).round() as u64);
            }
            _ => {}
        }
    }
    probe
}

/// Checks a probed output against the plan (see the module doc); returns
/// its duration. `needs_audio` is false only for the remux of a silent
/// source.
pub fn verify_output(
    probe: &OutputProbe,
    expected_ms: u64,
    fps: u32,
    needs_audio: bool,
) -> Result<u64, String> {
    if probe.video_streams == 0 {
        return Err("the render holds no video stream".into());
    }
    if needs_audio && probe.audio_streams == 0 {
        return Err("the render holds no audio stream".into());
    }
    let got = probe
        .duration_ms
        .ok_or_else(|| "the render reports no duration".to_string())?;
    let tolerance = 1_000_u64.div_ceil(u64::from(fps.max(1))) + DURATION_SLACK_MS;
    if got.abs_diff(expected_ms) > tolerance {
        return Err(format!(
            "the render is {got} ms long where {expected_ms} ms were planned"
        ));
    }
    Ok(got)
}

/// Runs ffprobe over the finished render.
fn probe_output(ffprobe: &Path, path: &Path) -> Result<OutputProbe, ScreenError> {
    let mut command = Command::new(ffprobe);
    command
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration:stream=codec_type",
            "-of",
            "default=noprint_wrappers=1",
        ])
        .arg(path)
        .stdin(Stdio::null());
    // No console window for the probe either (`ffmpeg_run`'s rule).
    let creation_flags = crate::ffmpeg_run::creation_flags_for(cfg!(windows));
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(creation_flags);
    }
    #[cfg(not(windows))]
    {
        debug_assert_eq!(creation_flags, 0);
    }
    let out = command.output().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            ScreenError::ToolMissing
        } else {
            ScreenError::Io(format!("could not start ffprobe: {e}"))
        }
    })?;
    if !out.status.success() {
        return Err(ScreenError::Sink(format!(
            "ffprobe could not read the render: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(parse_output_probe(&String::from_utf8_lossy(&out.stdout)))
}

/// Renders `req.plan` to `req.dest` (see the module doc), reporting
/// whole-percent progress and honouring `cancel` (`ffmpeg_run::run`).
pub fn render(
    req: RenderRequestNative<'_>,
    cancel: &AtomicBool,
    on_progress: &mut dyn FnMut(u64),
) -> Result<RenderOutcome, ScreenError> {
    let plan = req.plan;
    if let Some(message) = render_refusal(plan, req.caps, &req.settings.h264_encoder) {
        return Err(ScreenError::Refused(message));
    }
    let needed = file_input_count(plan);
    let Some(inputs) = req.inputs.get(..needed) else {
        return Err(ScreenError::Io(format!(
            "the render reads {needed} source files but the job supplied {}",
            req.inputs.len()
        )));
    };
    let remuxed = plan.is_identity();
    let (cues, captions) = if remuxed {
        (None, None)
    } else {
        write_ass_documents(plan, req.job_dir)?
    };
    let hooks = AssHooks {
        cues: cues.as_deref(),
        captions: captions.as_deref(),
        fontsdir: req.fontsdir,
    };
    let args = render_args(plan, inputs, req.dest, hooks, &req.settings);
    run(
        req.ffmpeg,
        &args,
        req.dest,
        plan.duration_ms,
        cancel,
        on_progress,
    )?;
    let needs_audio = !remuxed || plan.inputs.iter().any(|i| i.has_audio);
    let verified = probe_output(req.ffprobe, req.dest).and_then(|probe| {
        verify_output(&probe, plan.duration_ms, plan.canvas.fps, needs_audio)
            .map_err(ScreenError::Sink)
    });
    match verified {
        Ok(duration_ms) => Ok(RenderOutcome {
            duration_ms,
            remuxed,
        }),
        Err(e) => {
            if let Err(remove) = std::fs::remove_file(req.dest) {
                log::warn!(
                    "editor render: could not remove the unverified output {}: {remove}",
                    req.dest.display()
                );
            }
            Err(e)
        }
    }
}
