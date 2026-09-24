//! The editor's render (tutorial-editor Tasks 42-44; R1, F-04): a frozen
//! `core::editor::render_plan::RenderPlan` turned into ONE ffmpeg argv,
//! which `ffmpeg_run::run` then runs -- the same runner the phase-5 export
//! uses. PURE: nothing here spawns, probes or reads a file, so the graph's
//! correctness is testable on every platform (the `ffmpeg_args` posture).
//!
//! - `expr` -- filtergraph escaping and number formatting;
//! - `video_layers` -- one visual item's chain;
//! - `video_graph` -- the composed stage, the zoom and the two hooks;
//! - `ass` -- the two ASS documents (Task 43) `render_args` burns at those
//!   hooks;
//! - `audio_graph` -- the mixed, delayed, limited audio part (Task 44).
//!
//! **One `filter_complex`, two maps.** `render_args` joins the video part
//! and the audio part with a single `;` into ONE `-filter_complex` string
//! -- ffmpeg parses a graph as one document however many independent
//! chains it holds -- then maps `[vout]` and `audio_graph::OUT_LABEL`
//! (`[aout]`) separately. The audio side is unconditional: unlike the
//! legacy export's `has_audio`-gated map, `audio_graph::build_audio_graph`
//! never omits a stream (a silent project still gets `anullsrc`), so a
//! render's output always carries sound, even none.

pub mod ass;
pub mod audio_graph;
pub mod expr;
pub mod video_graph;
pub mod video_layers;

use std::path::{Path, PathBuf};

use vault_buddy_core::editor::render_plan::RenderPlan;

use crate::ffmpeg_args::{
    audio_codec_args, output_args, remux_args, runner_flags, video_codec_args, EncodeSettings,
};
use audio_graph::build_audio_graph;
use expr::{escape_filter_value, seconds};
use video_graph::{build_video_graph_with_hooks, file_input_count};

/// The ASS documents (`render::ass`, Task 43) to burn into a render, and
/// where libass should look for the font they name. `cues` (teaching cues
/// and card title/subtitle text, `ass::build_cue_ass`) burns at the
/// PRE-zoom hook (`[vcomp]`) so the preview's zoom scales it with the
/// media; `captions` (`ass::build_caption_ass`) burns at the POST-zoom
/// hook (`[vzoomed]`) so the preview keeps it unzoomed on the frame --
/// GAP-173's controller ruling, `video_graph`'s module doc. `fontsdir`
/// tells libass where to find the `Segoe UI`/`Arial` every ASS style
/// names (`ass`'s module doc); it is NEVER a bundled font (AGENTS.md:
/// ffmpeg stays user-installed) -- omitting it lets libass fall back to
/// its own font discovery.
#[derive(Debug, Clone, Copy, Default)]
pub struct AssHooks<'a> {
    pub cues: Option<&'a Path>,
    pub captions: Option<&'a Path>,
    pub fontsdir: Option<&'a Path>,
}

/// One `ass=filename=<escaped path>[:fontsdir=<escaped path>]` hook value
/// (`AssHooks`'s doc): both levels of filtergraph escaping applied to
/// EACH path separately, since `fontsdir` is a second `key=value` pair in
/// the SAME filter option string, not a second filter.
fn ass_hook(path: &Path, fontsdir: Option<&Path>) -> String {
    let mut opt = format!(
        "ass=filename={}",
        escape_filter_value(&path.to_string_lossy())
    );
    if let Some(dir) = fontsdir {
        opt.push_str(&format!(
            ":fontsdir={}",
            escape_filter_value(&dir.to_string_lossy())
        ));
    }
    opt
}

/// The ffmpeg argv that renders `plan` to `dest`.
///
/// `inputs[i]` is the file behind the plan's input index `i`. `settings`
/// carries the encoder (probed, never hardcoded -- `ffmpeg_args`' rule) and
/// the dimensions/rate the bitrate is sized for, which the caller sets to
/// the canvas. `ass` burns Task 43's two documents at the video graph's two
/// hooks (`AssHooks`'s own doc).
///
/// R1: an identity plan is `ffmpeg_args::remux_args` of its one source --
/// no graph, no re-encode, and the output at SOURCE resolution (F5). An
/// identity plan never reaches the graph at all, so a given `ass` hook is
/// silently unused in that case -- `RenderPlan::is_identity` already
/// requires `nothing_drawn_over` (no cues, no captions, no cards), so a
/// caller that built an `AssHooks` from a real plan can never observe this.
///
/// # Panics
/// When `inputs` does not hold exactly one path per planned file input: a
/// caller defect, never a user-reachable state (the `filter_complex`
/// zero-span precedent).
pub fn render_args(
    plan: &RenderPlan,
    inputs: &[PathBuf],
    dest: &Path,
    ass: AssHooks<'_>,
    settings: &EncodeSettings,
) -> Vec<String> {
    assert_eq!(
        inputs.len(),
        file_input_count(plan),
        "one path per planned input"
    );
    if plan.is_identity() {
        return remux_args(&inputs[plan.inputs[0].input_index], dest);
    }
    let pre_zoom = ass.cues.map(|p| ass_hook(p, ass.fontsdir));
    let post_zoom = ass.captions.map(|p| ass_hook(p, ass.fontsdir));
    let (extra, video_graph, video_out) =
        build_video_graph_with_hooks(plan, pre_zoom.as_deref(), post_zoom.as_deref());
    let audio_graph_text = build_audio_graph(plan);
    let mut args = runner_flags();
    for (index, path) in inputs.iter().enumerate() {
        if let Some(loop_ms) = image_loop_ms(plan, index) {
            args.extend([
                "-loop".into(),
                "1".into(),
                "-framerate".into(),
                plan.canvas.fps.to_string(),
                "-t".into(),
                seconds(loop_ms),
            ]);
        }
        args.extend(["-i".into(), path.to_string_lossy().into_owned()]);
    }
    args.extend(extra);
    args.extend([
        "-filter_complex".into(),
        format!("{video_graph};{audio_graph_text}"),
        "-map".into(),
        video_out,
        "-map".into(),
        audio_graph::OUT_LABEL.into(),
    ]);
    args.extend(video_codec_args(settings));
    args.extend(audio_codec_args());
    args.extend(output_args(dest));
    args
}

/// How long a still image input must loop: to the furthest source time any
/// layer reads from it (its asset duration when no layer does). `None` for
/// a timed input.
fn image_loop_ms(plan: &RenderPlan, index: usize) -> Option<u64> {
    let input = plan
        .inputs
        .iter()
        .find(|i| i.input_index == index && i.is_image)?;
    let furthest = plan
        .video_layers
        .iter()
        .filter(|l| l.input == index)
        .map(|l| l.source_out)
        .max();
    Some(furthest.unwrap_or(input.duration_ms))
}

#[cfg(test)]
#[path = "audio_graph_tests.rs"]
mod audio_graph_tests;
#[cfg(test)]
#[path = "render_args_tests.rs"]
mod render_args_tests;
#[cfg(test)]
mod test_support;
#[cfg(test)]
#[path = "video_graph_tests.rs"]
mod video_graph_tests;
#[cfg(test)]
#[path = "video_layers_tests.rs"]
mod video_layers_tests;
