//! Before-you-share checks (Task 54; F-45, F-33, F-38; SCREENS 07:
//! "Checks shows actionable findings rather than an invented quality
//! score"). `run_checks` reads a project and three shell facts and answers
//! a list of findings, each with a stable id (`chk-<code>-<target>`), a
//! severity, a sentence, the object it concerns and the one action that
//! reveals it. There is no score, total or grade anywhere in the reply
//! (`no_quality_score_is_ever_emitted`).
//!
//! **Only `blocking` stops a render**, and only two things block: a file
//! the render needs is missing (the render's own refusal, `render_plan::
//! plan`, names exactly the files of clips on VISIBLE tracks, so a missing
//! file only hidden tracks use is a warning), and an empty timeline.
//! Everything else is a warning or a note the user may knowingly ship.
//!
//! **The rules read the SAME algebra the render and the preview read**,
//! never a second copy: output spans through `time` (a cue's through
//! `cue_output_span`, the captions list's rule), audibility through
//! `render_plan_audio::is_audible` (the mixer's solo rule), numbers through
//! `render_plan::f64_of`. Two geometric constants are mirrors, named where
//! they live: a step cue's pill (`src/editor/cueShapes.ts`) and the caption
//! box's insets (`CaptionOverlay.vue`).
//!
//! **Heuristics say so.** "May clip" is a sum of linear gains over
//! overlapping sound, never a loudness measurement; a caption's reading
//! speed and its fit are estimates from character counts. The Checks dialog
//! states that these inspect the edit, not the meaning of the tutorial.
//!
//! The shell facts (`checks_commands.rs`): `missing` is the session's
//! missing-media set (the same `missing_media` the open result and the
//! reconnect dialog report), `pending_takes` the webcam takes still open in
//! the session, `with_audio` the assets whose `sources.json` record has a
//! sound track (the `CommandContext` fact `detachAudio` reads) -- without
//! it every synchronized webcam, a video-only file, would read as sound.

use std::collections::BTreeSet;

use serde::Serialize;

use super::model::{Asset, Clip, Project, Track, TrackKind};
use super::model_cues::EffectKind;
use super::render_plan::f64_of;
use super::render_plan_audio::is_audible;
use super::time::{self, ClipSpan};
use super::validate::speed_or_default;

#[path = "checks_layout.rs"]
mod layout;
use layout::{canvas_review, text_collisions};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Severity {
    Blocking,
    Warning,
    Info,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CheckCode {
    MissingMedia,
    EmptyProject,
    NoDestination,
    Gap,
    AllMuted,
    Clipping,
    ExcludedCaptions,
    CaptionOverlap,
    CaptionDensity,
    PendingTake,
    TransparentClip,
    TextCollision,
    PrivacyCover,
    CanvasReview,
}

impl CheckCode {
    /// The wire spelling, which is also the id's middle segment.
    pub fn as_str(self) -> &'static str {
        match self {
            CheckCode::MissingMedia => "missingMedia",
            CheckCode::EmptyProject => "emptyProject",
            CheckCode::NoDestination => "noDestination",
            CheckCode::Gap => "gap",
            CheckCode::AllMuted => "allMuted",
            CheckCode::Clipping => "clipping",
            CheckCode::ExcludedCaptions => "excludedCaptions",
            CheckCode::CaptionOverlap => "captionOverlap",
            CheckCode::CaptionDensity => "captionDensity",
            CheckCode::PendingTake => "pendingTake",
            CheckCode::TransparentClip => "transparentClip",
            CheckCode::TextCollision => "textCollision",
            CheckCode::PrivacyCover => "privacyCover",
            CheckCode::CanvasReview => "canvasReview",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TargetKind {
    Project,
    Asset,
    Track,
    Clip,
    Effect,
    Caption,
    Marker,
}

/// The object a finding concerns; `id` is `null` for the project itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CheckTarget {
    pub kind: TargetKind,
    pub id: Option<String>,
}

/// What the finding's button reveals (the frontend's `checkReveal.ts`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CheckAction {
    Reconnect,
    Select,
    OpenCaptions,
    OpenLayout,
    OpenAudio,
    OpenWebcam,
    ReviewCanvas,
    SetDestination,
}

/// Contract reference `CheckFinding` -- an IPC envelope, camelCase (every
/// field is already one word).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CheckFinding {
    pub id: String,
    pub severity: Severity,
    pub code: CheckCode,
    pub message: String,
    pub target: CheckTarget,
    pub action: Option<CheckAction>,
}

/// A gap on the top video track longer than this is noted.
const GAP_MS: u64 = 1_000;
/// A clip less opaque than this is "almost fully transparent".
const TRANSPARENT_BELOW: f64 = 0.05;
/// The safe area: text stays this fraction inside every edge.
const SAFE_MARGIN: f64 = 0.05;
/// Float slack for edges that land exactly on a limit (0.65 + 0.3).
const EPS: f64 = 1e-6;
/// `src/editor/captionRules.ts`' `DENSITY_LIMIT_CPS`.
const DENSITY_LIMIT_CPS: f64 = 20.0;
/// `src/editor/cueShapes.ts`: the average glyph width as a fraction of a
/// font size (a step pill's and a caption line's width estimate).
const GLYPH_WIDTH: f64 = 0.55;

/// What every rule reads.
struct Ctx<'a> {
    project: &'a Project,
    missing: &'a BTreeSet<String>,
    pending_takes: usize,
    with_audio: &'a BTreeSet<String>,
}

type Rule = fn(&Ctx) -> Vec<CheckFinding>;

/// In the order the dialog lists codes within a severity.
const RULES: [Rule; 14] = [
    missing_media,
    empty_project,
    no_destination,
    gaps,
    all_muted,
    clipping,
    excluded_captions,
    caption_overlaps,
    caption_density,
    pending_take,
    transparent_clips,
    text_collisions,
    privacy_covers,
    canvas_review,
];

/// Every finding for `project`, in `RULES` order, each rule's in project
/// order -- the same project always reads the same way.
pub fn run_checks(
    project: &Project,
    missing: &BTreeSet<String>,
    pending_takes: usize,
    with_audio: &BTreeSet<String>,
) -> Vec<CheckFinding> {
    let ctx = Ctx {
        project,
        missing,
        pending_takes,
        with_audio,
    };
    RULES.iter().flat_map(|rule| rule(&ctx)).collect()
}

// ---- shared helpers ---------------------------------------------------------

fn finding(
    severity: Severity,
    code: CheckCode,
    message: String,
    target: CheckTarget,
    action: Option<CheckAction>,
) -> CheckFinding {
    let segment = target.id.as_deref().unwrap_or("project");
    CheckFinding {
        id: format!("chk-{}-{segment}", code.as_str()),
        severity,
        code,
        message,
        target,
        action,
    }
}

fn on(kind: TargetKind, id: &str) -> CheckTarget {
    CheckTarget {
        kind,
        id: Some(id.to_string()),
    }
}

fn whole_project() -> CheckTarget {
    CheckTarget {
        kind: TargetKind::Project,
        id: None,
    }
}

fn span(clip: &Clip) -> ClipSpan {
    ClipSpan {
        start_ms: clip.start_ms,
        in_ms: clip.in_ms,
        out_ms: clip.out_ms,
        speed: speed_or_default(clip.speed.as_ref()),
    }
}

fn track_of<'a>(project: &'a Project, clip: &Clip) -> Option<&'a Track> {
    project.tracks.iter().find(|t| t.id == clip.track_id)
}

fn asset_of<'a>(project: &'a Project, id: &str) -> Option<&'a Asset> {
    project.assets.iter().find(|a| a.id == id)
}

/// The asset whose bytes a clip plays: a detached audio asset's video.
fn root_asset<'a>(project: &'a Project, clip: &'a Clip) -> &'a str {
    asset_of(project, &clip.asset_id)
        .and_then(|a| a.linked_asset.as_deref())
        .unwrap_or(&clip.asset_id)
}

fn on_visible_video(project: &Project, clip: &Clip) -> bool {
    track_of(project, clip).is_some_and(|t| t.visible && t.kind == TrackKind::Video)
}

/// `m:ss.t`, the captions list's `formatOutputTime`.
fn clock(ms: u64) -> String {
    format!(
        "{}:{:02}.{}",
        ms / 60_000,
        (ms / 1_000) % 60,
        (ms % 1_000) / 100
    )
}

fn count(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

// ---- missingMedia, emptyProject, noDestination --------------------------

fn missing_media(ctx: &Ctx) -> Vec<CheckFinding> {
    let p = ctx.project;
    ctx.missing
        .iter()
        .filter_map(|id| {
            let users: Vec<&Clip> = p.clips.iter().filter(|c| root_asset(p, c) == id).collect();
            if users.is_empty() {
                return None;
            }
            let needed = users
                .iter()
                .any(|c| track_of(p, c).is_some_and(|t| t.visible));
            let name = asset_of(p, id).map_or(id.as_str(), |a| a.name.as_str());
            let (severity, message) = if needed {
                (
                    Severity::Blocking,
                    format!("\"{name}\" is missing, and the render needs it. Reconnect the original file."),
                )
            } else {
                (
                    Severity::Warning,
                    format!("\"{name}\" is missing. Only hidden tracks use it, so the render skips it."),
                )
            };
            Some(finding(
                severity,
                CheckCode::MissingMedia,
                message,
                on(TargetKind::Asset, id),
                Some(CheckAction::Reconnect),
            ))
        })
        .collect()
}

fn empty_project(ctx: &Ctx) -> Vec<CheckFinding> {
    if !ctx.project.clips.is_empty() {
        return Vec::new();
    }
    vec![finding(
        Severity::Blocking,
        CheckCode::EmptyProject,
        "The timeline is empty. Place a clip on it before rendering.".to_string(),
        whole_project(),
        None,
    )]
}

fn no_destination(ctx: &Ctx) -> Vec<CheckFinding> {
    if !ctx.project.destination.vault.trim().is_empty() {
        return Vec::new();
    }
    vec![finding(
        Severity::Warning,
        CheckCode::NoDestination,
        "No vault is set to publish into. Choose where this tutorial goes.".to_string(),
        whole_project(),
        Some(CheckAction::SetDestination),
    )]
}

// ---- gap ----------------------------------------------------------------

/// Gaps longer than `GAP_MS` on the TOP visible video track that has any
/// clip (`tracks[0]` is on top), leading gaps included, trailing ones not
/// (the render ends with its longest track). Reported on the clip after
/// the gap. A lower track may fill it, which is why this is only a note.
fn gaps(ctx: &Ctx) -> Vec<CheckFinding> {
    let p = ctx.project;
    let Some(track) = p.tracks.iter().find(|t| {
        t.kind == TrackKind::Video && t.visible && p.clips.iter().any(|c| c.track_id == t.id)
    }) else {
        return Vec::new();
    };
    let mut clips: Vec<&Clip> = p.clips.iter().filter(|c| c.track_id == track.id).collect();
    clips.sort_by_key(|c| (c.start_ms, c.id.as_str()));
    let mut covered = 0;
    let mut out = Vec::new();
    for clip in clips {
        if clip.start_ms > covered + GAP_MS {
            out.push(finding(
                Severity::Info,
                CheckCode::Gap,
                format!(
                    "No clip on {} from {} to {}.",
                    track.name,
                    clock(covered),
                    clock(clip.start_ms)
                ),
                on(TargetKind::Clip, &clip.id),
                Some(CheckAction::Select),
            ));
        }
        covered = covered.max(time::clip_output_end(&span(clip)));
    }
    out
}

// ---- allMuted, clipping -------------------------------------------------

/// A clip whose bytes have a sound track, with the linear gain it reaches
/// the mix at (clip x track), or `None` when it is silenced in the render.
struct Sound<'a> {
    clip: &'a Clip,
    gain: Option<f64>,
}

fn sounds<'a>(ctx: &Ctx<'a>) -> Vec<Sound<'a>> {
    let p = ctx.project;
    p.clips
        .iter()
        .filter(|c| ctx.with_audio.contains(root_asset(p, c)))
        .map(|clip| {
            let gain = track_of(p, clip)
                .filter(|t| t.visible && is_audible(&p.tracks, clip, t))
                .map(|t| f64_of(Some(&clip.volume), 1.0) * f64_of(Some(&t.volume), 1.0))
                .filter(|g| *g > 0.0);
            Sound { clip, gain }
        })
        .collect()
}

fn all_muted(ctx: &Ctx) -> Vec<CheckFinding> {
    let sounds = sounds(ctx);
    if sounds.is_empty() {
        return Vec::new();
    }
    let message = if ctx.project.master_gain <= 0.0 {
        "The master level is at zero, so the render has no audio."
    } else if sounds.iter().all(|s| s.gain.is_none()) {
        "Every clip with sound is muted or silenced, so the render has no audio."
    } else {
        return Vec::new();
    };
    vec![finding(
        Severity::Warning,
        CheckCode::AllMuted,
        message.to_string(),
        whole_project(),
        Some(CheckAction::OpenAudio),
    )]
}

/// Two clips a transition joins crossfade by design; their overlap is not
/// a sum.
fn crossfaded(project: &Project, a: &str, b: &str) -> bool {
    project
        .transitions
        .iter()
        .any(|t| (t.from == a && t.to == b) || (t.from == b && t.to == a))
}

/// Audible sound whose summed linear gain (x master) goes over unity where
/// clips overlap: "may clip", reported on the clip whose start takes the
/// sum over. The sum only rises where a clip starts, so the starts are the
/// only instants that need checking.
fn clipping(ctx: &Ctx) -> Vec<CheckFinding> {
    let p = ctx.project;
    let live: Vec<(&Clip, f64, u64)> = sounds(ctx)
        .into_iter()
        .filter_map(|s| Some((s.clip, s.gain?, time::clip_output_end(&span(s.clip)))))
        .collect();
    let mut out = Vec::new();
    for &(clip, gain, _) in &live {
        let at = clip.start_ms;
        let others: f64 = live
            .iter()
            .filter(|(c, _, end)| c.id != clip.id && c.start_ms <= at && at < *end)
            .filter(|(c, _, _)| !crossfaded(p, &c.id, &clip.id))
            .map(|(_, g, _)| g)
            .sum();
        let total = (gain + others) * p.master_gain;
        if others > 0.0 && total > 1.0 + EPS {
            out.push(finding(
                Severity::Warning,
                CheckCode::Clipping,
                format!(
                    "\"{}\" overlaps other sound at a combined level of {}%, which may clip.",
                    clip.name,
                    (total * 100.0).round()
                ),
                on(TargetKind::Clip, &clip.id),
                Some(CheckAction::OpenAudio),
            ));
        }
    }
    out
}

// ---- captions -------------------------------------------------------------

/// One caption as the Captions list shows it (`captionRules.captionRows`):
/// a cue still inside its clip, in OUTPUT time, sorted by start then end,
/// numbered from 1.
struct CaptionRow<'a> {
    id: &'a str,
    text: &'a str,
    start: u64,
    end: u64,
    index: usize,
}

fn caption_rows(project: &Project) -> Vec<CaptionRow<'_>> {
    let Some(settings) = project.captions.as_ref() else {
        return Vec::new();
    };
    let mut rows: Vec<CaptionRow> = settings
        .cues
        .iter()
        .filter_map(|q| {
            let clip = project.clips.iter().find(|c| c.id == q.clip_id)?;
            let (start, end) = time::cue_output_span(&span(clip), q.start_ms, q.end_ms)?;
            Some(CaptionRow {
                id: &q.id,
                text: &q.text,
                start,
                end,
                index: 0,
            })
        })
        .collect();
    rows.sort_by_key(|r| (r.start, r.end));
    for (i, row) in rows.iter_mut().enumerate() {
        row.index = i + 1;
    }
    rows
}

fn excluded_captions(ctx: &Ctx) -> Vec<CheckFinding> {
    let enabled = ctx.project.captions.as_ref().is_some_and(|c| c.enabled);
    let n = caption_rows(ctx.project).len();
    if enabled || n == 0 {
        return Vec::new();
    }
    vec![finding(
        Severity::Warning,
        CheckCode::ExcludedCaptions,
        format!(
            "Captions are turned off, so {} will not appear in the render.",
            count(n, "caption", "captions")
        ),
        whole_project(),
        Some(CheckAction::OpenCaptions),
    )]
}

/// `captionRules.captionNotices`' overlap half: a cue starting before the
/// latest-running earlier cue ends, reported on the later cue.
fn caption_overlaps(ctx: &Ctx) -> Vec<CheckFinding> {
    let rows = caption_rows(ctx.project);
    let mut latest: Option<&CaptionRow> = None;
    let mut out = Vec::new();
    for row in &rows {
        if let Some(prev) = latest.filter(|prev| row.start < prev.end) {
            out.push(finding(
                Severity::Warning,
                CheckCode::CaptionOverlap,
                format!("Captions {} and {} overlap.", prev.index, row.index),
                on(TargetKind::Caption, row.id),
                Some(CheckAction::OpenCaptions),
            ));
        }
        if latest.is_none_or(|prev| row.end > prev.end) {
            latest = Some(row);
        }
    }
    out
}

/// Characters per second of OUTPUT, the captions list's `cps`.
fn cps(row: &CaptionRow) -> f64 {
    let seconds = (row.end - row.start) as f64 / 1_000.0;
    if seconds > 0.0 {
        row.text.chars().count() as f64 / seconds
    } else {
        0.0
    }
}

fn caption_density(ctx: &Ctx) -> Vec<CheckFinding> {
    caption_rows(ctx.project)
        .iter()
        .filter(|row| cps(row) > DENSITY_LIMIT_CPS)
        .map(|row| {
            finding(
                Severity::Warning,
                CheckCode::CaptionDensity,
                format!(
                    "Caption {} reads at {:.1} characters per second.",
                    row.index,
                    cps(row)
                ),
                on(TargetKind::Caption, row.id),
                Some(CheckAction::OpenCaptions),
            )
        })
        .collect()
}

// ---- pendingTake, transparentClip ---------------------------------------

fn pending_take(ctx: &Ctx) -> Vec<CheckFinding> {
    let n = ctx.pending_takes;
    if n == 0 {
        return Vec::new();
    }
    let (verb, pronoun) = if n == 1 {
        ("is", "it")
    } else {
        ("are", "them")
    };
    vec![finding(
        Severity::Warning,
        CheckCode::PendingTake,
        format!(
            "{} {verb} not finished. Finish or discard {pronoun} before sharing.",
            count(n, "webcam take", "webcam takes")
        ),
        whole_project(),
        Some(CheckAction::OpenWebcam),
    )]
}

/// A clip on a visible video track below `TRANSPARENT_BELOW` opacity, then
/// every hidden video track that holds clips.
fn transparent_clips(ctx: &Ctx) -> Vec<CheckFinding> {
    let p = ctx.project;
    let faint = p.clips.iter().filter_map(|clip| {
        let opacity = f64_of(Some(&clip.opacity), 1.0);
        (on_visible_video(p, clip) && opacity < TRANSPARENT_BELOW).then(|| {
            finding(
                Severity::Warning,
                CheckCode::TransparentClip,
                format!(
                    "\"{}\" is almost fully transparent ({}% opacity).",
                    clip.name,
                    (opacity * 100.0).round()
                ),
                on(TargetKind::Clip, &clip.id),
                Some(CheckAction::OpenLayout),
            )
        })
    });
    let hidden = p.tracks.iter().filter_map(|track| {
        let n = p.clips.iter().filter(|c| c.track_id == track.id).count();
        (track.kind == TrackKind::Video && !track.visible && n > 0).then(|| {
            finding(
                Severity::Warning,
                CheckCode::TransparentClip,
                format!(
                    "Track {} is hidden, so its {} will not appear in the render.",
                    track.name,
                    count(n, "clip", "clips")
                ),
                on(TargetKind::Track, &track.id),
                Some(CheckAction::OpenLayout),
            )
        })
    });
    faint.chain(hidden).collect()
}

// ---- privacyCover ---------------------------------------------------------

/// F-33: every cover, visible or not. A cover is stationary and the
/// originals under it are never altered, which only the rendered file can
/// show the user.
fn privacy_covers(ctx: &Ctx) -> Vec<CheckFinding> {
    ctx.project
        .effects
        .iter()
        .filter(|e| e.kind == EffectKind::Mask)
        .map(|e| {
            finding(
                Severity::Warning,
                CheckCode::PrivacyCover,
                "Stationary cover \u{2014} check the rendered video; originals are uncensored"
                    .to_string(),
                on(TargetKind::Effect, &e.id),
                Some(CheckAction::Select),
            )
        })
        .collect()
}

#[cfg(test)]
#[path = "checks_tests.rs"]
mod tests;
