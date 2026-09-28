//! Caption authoring (Task 36; F-34, F-35): `setCaptionSettings`,
//! `addCaption`, `updateCaption`, `splitCaption`, `removeCaptions`, and the
//! native-only `InternalCommand::ImportCaptions` behind
//! `editor_import_captions`.
//!
//! **Cue times are SOURCE time, stored verbatim** -- the same rule
//! `cues.rs` documents for teaching cues (`global-constraints.md`: "Effect/
//! caption startMs/endMs and marker sourceMs are SOURCE time"), and for the
//! same reason: a cue then survives `moveClips`/`trimClip`/`setSpeed`
//! untouched and follows `splitClip` through `cue_follow.rs`. The span
//! check is `cues::check_cue_range`, shared, so a caption and a teaching
//! cue accept exactly the same ranges.
//!
//! **The one place output time turns into source time is
//! `plan_caption_import`.** A subtitle file's times are OUTPUT times
//! relative to the clip's own start (the reference editor's "imported
//! timing begins at 00:00 of the visible clip"), so each is mapped through
//! the inverse of `time::output_at` -- `in_ms + round(relative · speed)`.
//! A cue that starts at or past the clip's output end is SKIPPED and
//! counted (the brief: "reported, not silently dropped"); one that starts
//! inside and runs past the end is cut at the clip's source end, the
//! reference's own rule. An import with nothing inside the clip is refused
//! outright rather than applied as an empty (and, with `replace`,
//! destructive) edit.
//!
//! **Defaults are the reference editor's** (`captions.js`'s
//! `captionDefaults`): enabled, burned in, 30 px, bottom, with a readable
//! background -- created the first time a cue or a setting needs a
//! `CaptionSettings` to live in. Font size is bounded to the reference's
//! own `18..=56`.

use crate::editor::captions_io::ParsedCue;
use crate::editor::commands::clips::{ensure_unlocked, find_clip, invalid_request};
use crate::editor::commands::cues::check_cue_range;
use crate::editor::commands::payloads::{
    AddCaptionPayload, ImportCaptionsPayload, ImportedCaptionCue, RemoveCaptionsPayload,
    SetCaptionSettingsPayload, SplitCaptionPayload, UpdateCaptionPayload,
};
use crate::editor::error::EditorError;
use crate::editor::ids::new_entity_id;
use crate::editor::limits;
use crate::editor::model::{Clip, Project};
use crate::editor::model_cues::{CaptionCue, CaptionPosition, CaptionSettings};
use crate::editor::time::clip_output_duration;
use crate::editor::validate::speed_or_default;
use crate::editor::{Map, Num};

/// The reference editor's default caption font size, in canvas px at 720p.
pub const DEFAULT_FONT_SIZE: i64 = 30;
/// `captions.js`' own `font_size<18||font_size>56` refusal -- the same
/// pair `validate_project` enforces (GAP-179), declared once in `limits`.
pub const FONT_SIZE_MIN: f64 = limits::CAPTION_FONT_SIZE_MIN;
pub const FONT_SIZE_MAX: f64 = limits::CAPTION_FONT_SIZE_MAX;

/// A fresh `CaptionSettings` with the reference editor's defaults and no
/// cues.
pub(crate) fn default_settings() -> CaptionSettings {
    CaptionSettings {
        enabled: true,
        burn_in: true,
        font_size: Num::from(DEFAULT_FONT_SIZE),
        position: CaptionPosition::Bottom,
        background: true,
        cues: Vec::new(),
        extra: Map::new(),
    }
}

/// What `plan_caption_import` made of a parsed file: the cues that land on
/// the clip, already in its SOURCE time, and how many fell outside it.
#[derive(Debug, Clone, PartialEq)]
pub struct CaptionImportPlan {
    pub cues: Vec<ImportedCaptionCue>,
    pub skipped: usize,
}

fn find_caption<'a>(project: &'a Project, id: &str) -> Result<&'a CaptionCue, EditorError> {
    project
        .captions
        .as_ref()
        .and_then(|s| s.cues.iter().find(|c| c.id == id))
        .ok_or_else(|| invalid_request(format!("caption {id} does not resolve")))
}

/// The clip a caption belongs to, refused when its track is locked.
fn editable_clip<'a>(project: &'a Project, clip_id: &str) -> Result<&'a Clip, EditorError> {
    let clip = find_clip(project, clip_id)?;
    ensure_unlocked(project, &clip.track_id)?;
    Ok(clip)
}

/// Trimmed, non-empty, at most `MAX_CAPTION_TEXT_CHARS` characters.
fn checked_text(text: &str) -> Result<String, EditorError> {
    let text = text.trim();
    if text.is_empty() {
        return Err(invalid_request("A caption needs some text"));
    }
    if text.chars().count() > limits::MAX_CAPTION_TEXT_CHARS {
        return Err(invalid_request(format!(
            "Caption text is limited to {} characters",
            limits::MAX_CAPTION_TEXT_CHARS
        )));
    }
    Ok(text.to_string())
}

fn ensure_room(existing: usize, adding: usize) -> Result<(), EditorError> {
    if existing + adding > limits::MAX_CAPTIONS {
        return Err(invalid_request(format!(
            "A project may hold at most {} captions",
            limits::MAX_CAPTIONS
        )));
    }
    Ok(())
}

fn cue_count(project: &Project) -> usize {
    project.captions.as_ref().map_or(0, |s| s.cues.len())
}

fn caption_mut<'a>(project: &'a mut Project, id: &str) -> &'a mut CaptionCue {
    project
        .captions
        .as_mut()
        .and_then(|s| s.cues.iter_mut().find(|c| c.id == id))
        .expect("caption id was resolved against this same project")
}

/// `setCaptionSettings{enabled?, burnIn?, fontSize?, position?,
/// background?}`: at least one field; the font size inside `18..=56`.
pub(super) fn set_caption_settings(
    project: &Project,
    payload: &SetCaptionSettingsPayload,
) -> Result<(Project, String), EditorError> {
    let SetCaptionSettingsPayload {
        enabled,
        burn_in,
        font_size,
        position,
        background,
    } = payload;
    if enabled.is_none()
        && burn_in.is_none()
        && font_size.is_none()
        && position.is_none()
        && background.is_none()
    {
        return Err(invalid_request(
            "setCaptionSettings must set at least one field",
        ));
    }
    if let Some(size) = font_size {
        let in_range = size
            .as_f64()
            .is_some_and(|v| (FONT_SIZE_MIN..=FONT_SIZE_MAX).contains(&v));
        if !in_range {
            return Err(invalid_request(format!(
                "Caption font size must be between {FONT_SIZE_MIN} and {FONT_SIZE_MAX}"
            )));
        }
    }
    let mut candidate = project.clone();
    let settings = candidate.captions.get_or_insert_with(default_settings);
    settings.enabled = enabled.unwrap_or(settings.enabled);
    settings.burn_in = burn_in.unwrap_or(settings.burn_in);
    settings.background = background.unwrap_or(settings.background);
    settings.position = position.unwrap_or(settings.position);
    if let Some(size) = font_size {
        settings.font_size = size.clone();
    }
    Ok((candidate, "Caption settings".to_string()))
}

/// `addCaption{clipId, startMs, endMs, text}` -- source time inside the
/// clip's own source range.
pub(super) fn add_caption(
    project: &Project,
    payload: &AddCaptionPayload,
) -> Result<(Project, String), EditorError> {
    let clip = editable_clip(project, &payload.clip_id)?;
    check_cue_range(payload.start_ms, payload.end_ms, clip)?;
    let text = checked_text(&payload.text)?;
    ensure_room(cue_count(project), 1)?;

    let mut candidate = project.clone();
    candidate
        .captions
        .get_or_insert_with(default_settings)
        .cues
        .push(CaptionCue {
            id: new_entity_id("caption"),
            clip_id: payload.clip_id.clone(),
            start_ms: payload.start_ms,
            end_ms: payload.end_ms,
            text,
            extra: Map::new(),
        });
    Ok((candidate, "Add caption".to_string()))
}

/// `updateCaption{captionId, startMs?, endMs?, text?}` -- the span AFTER
/// the patch must still lie inside the clip.
pub(super) fn update_caption(
    project: &Project,
    payload: &UpdateCaptionPayload,
) -> Result<(Project, String), EditorError> {
    let cue = find_caption(project, &payload.caption_id)?;
    let clip = editable_clip(project, &cue.clip_id)?;
    if payload.start_ms.is_none() && payload.end_ms.is_none() && payload.text.is_none() {
        return Err(invalid_request("updateCaption must set at least one field"));
    }
    let start_ms = payload.start_ms.unwrap_or(cue.start_ms);
    let end_ms = payload.end_ms.unwrap_or(cue.end_ms);
    check_cue_range(start_ms, end_ms, clip)?;
    let text = payload.text.as_deref().map(checked_text).transpose()?;

    let mut candidate = project.clone();
    let target = caption_mut(&mut candidate, &payload.caption_id);
    target.start_ms = start_ms;
    target.end_ms = end_ms;
    if let Some(text) = text {
        target.text = text;
    }
    Ok((candidate, "Edit caption".to_string()))
}

/// Divides `text`'s words in the same proportion as the time: the left
/// half gets `round(words · fraction)` of them, clamped so each half keeps
/// at least one. A single word cannot be divided, so both halves keep it
/// rather than one being left with nothing -- the commands refuse an empty
/// caption (`checked_text`) and, since Task 38 closed docs/Gaps.md
/// GAP-179, so does `validate_project` -- an empty half is stopped here.
fn split_words(text: &str, fraction: f64) -> (String, String) {
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.len() < 2 {
        return (text.to_string(), text.to_string());
    }
    let left = ((words.len() as f64 * fraction).round() as usize).clamp(1, words.len() - 1);
    (words[..left].join(" "), words[left..].join(" "))
}

/// `splitCaption{captionId, atMs}` -- `atMs` is SOURCE time strictly inside
/// the cue. Both halves stay on the cue's own clip (unlike `splitClip`'s
/// cue reassignment, nothing here changes which footage a cue follows);
/// the left keeps the id, the right gets a fresh one.
pub(super) fn split_caption(
    project: &Project,
    payload: &SplitCaptionPayload,
) -> Result<(Project, String), EditorError> {
    let cue = find_caption(project, &payload.caption_id)?;
    editable_clip(project, &cue.clip_id)?;
    let at = payload.at_ms;
    if at <= cue.start_ms || at >= cue.end_ms {
        return Err(invalid_request(format!(
            "The split point {at} must be inside the caption's [{}, {})",
            cue.start_ms, cue.end_ms
        )));
    }
    ensure_room(cue_count(project), 1)?;
    let fraction = (at - cue.start_ms) as f64 / (cue.end_ms - cue.start_ms) as f64;
    let (left_text, right_text) = split_words(&cue.text, fraction);

    let mut right = cue.clone();
    right.id = new_entity_id("caption");
    right.start_ms = at;
    right.text = right_text;

    let mut candidate = project.clone();
    let left = caption_mut(&mut candidate, &payload.caption_id);
    left.end_ms = at;
    left.text = left_text;
    if let Some(settings) = candidate.captions.as_mut() {
        settings.cues.push(right);
    }
    Ok((candidate, "Split caption".to_string()))
}

/// `removeCaptions{captionIds}` -- every id must resolve, on an unlocked
/// track; an empty list is refused rather than recorded as a no-op step.
pub(super) fn remove_captions(
    project: &Project,
    payload: &RemoveCaptionsPayload,
) -> Result<(Project, String), EditorError> {
    if payload.caption_ids.is_empty() {
        return Err(invalid_request(
            "removeCaptions needs at least one caption id",
        ));
    }
    for id in &payload.caption_ids {
        let cue = find_caption(project, id)?;
        editable_clip(project, &cue.clip_id)?;
    }
    let mut candidate = project.clone();
    if let Some(settings) = candidate.captions.as_mut() {
        settings
            .cues
            .retain(|c| !payload.caption_ids.contains(&c.id));
    }
    let label = if payload.caption_ids.len() == 1 {
        "Delete caption"
    } else {
        "Delete captions"
    };
    Ok((candidate, label.to_string()))
}

/// The inverse of `time::output_at` for a clip-RELATIVE output instant:
/// the source instant that plays `relative_ms` after the clip starts.
fn source_of_relative(clip: &Clip, speed: f64, relative_ms: u64) -> u64 {
    clip.in_ms + (relative_ms as f64 * speed).round() as u64
}

/// Maps parsed cues (clip-relative OUTPUT time) onto `clip_id`'s SOURCE
/// time -- see the module doc. Refuses a locked or unknown clip, and a
/// file with nothing inside the clip.
pub fn plan_caption_import(
    project: &Project,
    clip_id: &str,
    parsed: &[ParsedCue],
) -> Result<CaptionImportPlan, EditorError> {
    let clip = editable_clip(project, clip_id)?;
    let speed = speed_or_default(clip.speed.as_ref());
    let visible_ms = clip_output_duration(clip.in_ms, clip.out_ms, speed);
    let mut cues = Vec::new();
    let mut skipped = 0;
    for cue in parsed {
        let start_ms = source_of_relative(clip, speed, cue.start_ms);
        let end_ms = source_of_relative(clip, speed, cue.end_ms).min(clip.out_ms);
        if cue.start_ms >= visible_ms || end_ms <= start_ms {
            skipped += 1;
            continue;
        }
        cues.push(ImportedCaptionCue {
            start_ms,
            end_ms,
            text: cue.text.clone(),
        });
    }
    if cues.is_empty() {
        return Err(invalid_request(format!(
            "None of the {} cues falls inside this clip. Imported times start at 00:00 of the clip.",
            parsed.len()
        )));
    }
    Ok(CaptionImportPlan { cues, skipped })
}

/// `InternalCommand::ImportCaptions{clipId, cues, replace}` -- `cues` are
/// already SOURCE time (`plan_caption_import`). ONE history step, so Undo
/// removes the whole import; `replace` drops this clip's existing cues in
/// that same step, and no other clip's.
pub(super) fn import_captions(
    project: &Project,
    payload: &ImportCaptionsPayload,
) -> Result<(Project, String), EditorError> {
    let clip = editable_clip(project, &payload.clip_id)?;
    if payload.cues.is_empty() {
        return Err(invalid_request("There are no captions to import"));
    }
    let mut imported = Vec::with_capacity(payload.cues.len());
    for cue in &payload.cues {
        check_cue_range(cue.start_ms, cue.end_ms, clip)?;
        imported.push(CaptionCue {
            id: new_entity_id("caption"),
            clip_id: payload.clip_id.clone(),
            start_ms: cue.start_ms,
            end_ms: cue.end_ms,
            text: checked_text(&cue.text)?,
            extra: Map::new(),
        });
    }

    let mut candidate = project.clone();
    let settings = candidate.captions.get_or_insert_with(default_settings);
    if payload.replace {
        settings.cues.retain(|c| c.clip_id != payload.clip_id);
    }
    ensure_room(settings.cues.len(), imported.len())?;
    settings.cues.extend(imported);
    let label = if payload.replace {
        "Replace captions"
    } else {
        "Import captions"
    };
    Ok((candidate, label.to_string()))
}

// Tests live in the sibling `captions_tests.rs`, the `cues.rs` precedent.
#[cfg(test)]
#[path = "captions_tests.rs"]
mod tests;
