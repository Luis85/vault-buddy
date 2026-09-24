//! The editor's render (tutorial-editor Tasks 42-44; R1, F-04): a frozen
//! `core::editor::render_plan::RenderPlan` turned into ONE ffmpeg argv,
//! which `ffmpeg_run::run` then runs -- the same runner the phase-5 export
//! uses. PURE: nothing here spawns, probes or reads a file, so the graph's
//! correctness is testable on every platform (the `ffmpeg_args` posture).
//!
//! - `expr` -- filtergraph escaping and number formatting;
//! - `video_layers` -- one visual item's chain;
//! - `video_graph` -- the composed stage, the zoom and the two hooks.
//!
//! **Video only, for now.** Task 44 adds the audio graph; until then a
//! render maps the video label alone. Task 43 fills the two hooks with the
//! ASS documents. No production caller exists yet (the render job is a
//! later task), so no user can reach a silent render.

pub mod expr;
pub mod video_graph;
pub mod video_layers;

use std::path::{Path, PathBuf};

use vault_buddy_core::editor::render_plan::RenderPlan;

use crate::ffmpeg_args::{output_args, remux_args, runner_flags, video_codec_args, EncodeSettings};
use expr::{escape_filter_value, seconds};
use video_graph::{build_video_graph_with_hooks, file_input_count};

/// The ffmpeg argv that renders `plan` to `dest`.
///
/// `inputs[i]` is the file behind the plan's input index `i`. `settings`
/// carries the encoder (probed, never hardcoded -- `ffmpeg_args`' rule) and
/// the dimensions/rate the bitrate is sized for, which the caller sets to
/// the canvas. `ass_path` is burned at the PRE-zoom hook (`[vcomp]`), the
/// hook the brief names; Task 43 adds the post-zoom document for burned
/// captions (controller ruling, GAP-173).
///
/// R1: an identity plan is `ffmpeg_args::remux_args` of its one source --
/// no graph, no re-encode, and the output at SOURCE resolution (F5).
///
/// # Panics
/// When `inputs` does not hold exactly one path per planned file input: a
/// caller defect, never a user-reachable state (the `filter_complex`
/// zero-span precedent).
pub fn render_args(
    plan: &RenderPlan,
    inputs: &[PathBuf],
    dest: &Path,
    ass_path: Option<&Path>,
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
    let pre_zoom =
        ass_path.map(|p| format!("ass=filename={}", escape_filter_value(&p.to_string_lossy())));
    let (extra, graph, out) = build_video_graph_with_hooks(plan, pre_zoom.as_deref(), None);
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
    args.extend(["-filter_complex".into(), graph, "-map".into(), out]);
    args.extend(video_codec_args(settings));
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
