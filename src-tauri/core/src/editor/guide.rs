//! Guided-onboarding progress (Task 55; F-46, F-47; ADR R16, R18): the
//! validated shape of `editor-prefs\guide-progress.json`, an APPLICATION
//! preference kept apart from every project, its render snapshots and its
//! Undo history.
//!
//! **One source of truth for the lessons.** The 22 step ids (and the
//! chapter each belongs to) are read from the SAME `steps.json` the editor
//! webview ships (`src/editor/guide/steps.json`, a verbatim copy of the
//! concept bundle's `onboarding.steps.json`), compiled in with
//! `include_str!` — so a lesson added, renamed or retired in that file is
//! the lesson this validator knows, with no second list to forget. The
//! retired-id map is likewise one file both sides read,
//! `src/editor/guide/retired-steps.json` (`{ "<retired step id>":
//! "<chapter id>" }`, empty today).
//!
//! **Two postures.** A SAVE is strict ([`validate_for_save`]): a closed
//! schema (`deny_unknown_fields`), known step ids only, a raw size bound —
//! the only strings the document can hold are step ids and the `motion`
//! enum, so no path, media name or project detail can ever be written into
//! it. A READ is lenient ([`parse_stored`]): the file is ours, but it may
//! predate a content change, so an id this build no longer knows is mapped
//! (a retired id to its chapter's first step, anything else to the guide's
//! first step) or dropped — never an error the editor has to render.

use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::error::{EditorError, EditorErrorCode};

/// The raw size bound, for the stored file and for an incoming save alike.
pub const MAX_GUIDE_PROGRESS_BYTES: usize = 16 * 1024;

const STEPS_JSON: &str = include_str!("../../../../src/editor/guide/steps.json");
const RETIRED_JSON: &str = include_str!("../../../../src/editor/guide/retired-steps.json");

/// The guide's motion preference (`GuideProgress.preferences.motion`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GuideMotion {
    System,
    Reduced,
    Full,
}

/// `GuideProgress.preferences` — presentation only, never guide state, so
/// a restart keeps them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GuidePreferences {
    pub dimming: bool,
    pub motion: GuideMotion,
}

/// The Contract reference's `GuideProgress`, camelCase on the wire and on
/// disk. `current_step_id` is present-even-when-null.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GuideProgress {
    pub content_revision: u32,
    pub current_step_id: Option<String>,
    pub reviewed: Vec<String>,
    pub explored: Vec<String>,
    pub invitation_dismissed: bool,
    pub active: bool,
    pub collapsed: bool,
    pub completed: bool,
    pub preferences: GuidePreferences,
}

impl Default for GuideProgress {
    /// Nothing saved yet: revision 0 (no content has been seen), no step,
    /// dimming on and motion following the system (the reference's own
    /// fresh state).
    fn default() -> Self {
        Self {
            content_revision: 0,
            current_step_id: None,
            reviewed: Vec::new(),
            explored: Vec::new(),
            invitation_dismissed: false,
            active: false,
            collapsed: false,
            completed: false,
            preferences: GuidePreferences {
                dimming: true,
                motion: GuideMotion::System,
            },
        }
    }
}

#[derive(Deserialize)]
struct StepEntry {
    id: String,
    chapter: String,
}

struct Content {
    steps: Vec<StepEntry>,
    retired: Vec<(String, String)>,
}

/// The compiled-in lesson list. Both documents are part of this binary, so
/// a parse failure is a build defect `the_compiled_steps_are_the_22_lessons_in_order`
/// catches, never a runtime condition.
fn content() -> &'static Content {
    static CONTENT: OnceLock<Content> = OnceLock::new();
    CONTENT.get_or_init(|| {
        let steps: Vec<StepEntry> =
            serde_json::from_str(STEPS_JSON).expect("steps.json is compiled in and valid");
        let retired: serde_json::Map<String, Value> = serde_json::from_str(RETIRED_JSON)
            .expect("retired-steps.json is compiled in and valid");
        let retired = retired
            .into_iter()
            .map(|(id, chapter)| (id, chapter.as_str().unwrap_or_default().to_owned()))
            .collect();
        Content { steps, retired }
    })
}

/// Every lesson id, in lesson order.
pub fn known_step_ids() -> Vec<&'static str> {
    content().steps.iter().map(|s| s.id.as_str()).collect()
}

fn is_known(id: &str) -> bool {
    content().steps.iter().any(|s| s.id == id)
}

fn first_step_of(chapter: &str) -> Option<&'static str> {
    content()
        .steps
        .iter()
        .find(|s| s.chapter == chapter)
        .map(|s| s.id.as_str())
}

/// Where a stored step id resumes: itself when known; a retired id's
/// chapter's first step; anything else the guide's first step (the only
/// "nearest chapter" an id with no mapping has).
fn resolve_step_with(id: &str, retired: &[(String, String)]) -> &'static str {
    if let Some(known) = content().steps.iter().find(|s| s.id == id) {
        return known.id.as_str();
    }
    let chapter_first = retired
        .iter()
        .find(|(old, _)| old == id)
        .and_then(|(_, chapter)| first_step_of(chapter));
    chapter_first.unwrap_or_else(|| content().steps[0].id.as_str())
}

/// [`resolve_step_with`] over the compiled-in retired map.
pub fn resolve_step_id(id: &str) -> &'static str {
    resolve_step_with(id, &content().retired)
}

fn invalid(message: impl Into<String>) -> EditorError {
    EditorError::new(EditorErrorCode::InvalidRequest, message)
}

/// Known ids only, first occurrence kept.
fn known_unique(ids: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for id in ids {
        if is_known(id) && !out.contains(id) {
            out.push(id.clone());
        }
    }
    out
}

/// The strict gate on `editor_save_guide_progress`. The size is measured
/// on the RAW document before anything else, and no refusal message echoes
/// a value the document carried — an unknown id may be exactly the path
/// this validator exists to keep out of the file, and of the log.
pub fn validate_for_save(raw: &Value) -> Result<GuideProgress, EditorError> {
    let len = serde_json::to_vec(raw)
        .map(|b| b.len())
        .unwrap_or(usize::MAX);
    if len > MAX_GUIDE_PROGRESS_BYTES {
        return Err(invalid(format!(
            "Guide progress is {len} bytes, exceeding the {MAX_GUIDE_PROGRESS_BYTES} byte maximum."
        )));
    }
    let progress: GuideProgress = serde_json::from_value(raw.clone())
        .map_err(|_| invalid("Guide progress is not in the expected shape."))?;
    if progress.content_revision == 0 {
        return Err(invalid(
            "Guide progress must name the guide content revision it was made with.",
        ));
    }
    let ids = progress
        .current_step_id
        .iter()
        .chain(&progress.reviewed)
        .chain(&progress.explored);
    for id in ids {
        if !is_known(id) {
            return Err(invalid(
                "Guide progress names a lesson this version does not know.",
            ));
        }
    }
    Ok(GuideProgress {
        reviewed: known_unique(&progress.reviewed),
        explored: known_unique(&progress.explored),
        ..progress
    })
}

/// The strict read of a progress FILE the user picked (Task 57, F-47:
/// Restore progress file): the RAW byte bound first — `validate_for_save`
/// measures a re-serialized value, which padding would slip past — then
/// JSON, then [`validate_for_save`] itself, so a restored file can hold
/// exactly what a save may write and nothing else (a project file, an
/// unknown lesson and a path are all refused, never echoed).
pub fn parse_progress_file(bytes: &[u8]) -> Result<GuideProgress, EditorError> {
    if bytes.len() > MAX_GUIDE_PROGRESS_BYTES {
        return Err(invalid(format!(
            "That file is too large to be guide progress (the limit is {MAX_GUIDE_PROGRESS_BYTES} bytes)."
        )));
    }
    let raw: Value = serde_json::from_slice(bytes)
        .map_err(|_| invalid("That file is not Vault Buddy guide progress."))?;
    validate_for_save(&raw)
}

/// The lenient read of the stored file: `None` when it is oversized or not
/// a progress document at all (the caller logs and uses the default);
/// otherwise the document with every step id resolved or dropped.
pub fn parse_stored(bytes: &[u8]) -> Option<GuideProgress> {
    if bytes.len() > MAX_GUIDE_PROGRESS_BYTES {
        return None;
    }
    let stored: GuideProgress = serde_json::from_slice(bytes).ok()?;
    Some(normalize_with(stored, &content().retired))
}

fn normalize_with(stored: GuideProgress, retired: &[(String, String)]) -> GuideProgress {
    GuideProgress {
        current_step_id: stored
            .current_step_id
            .as_deref()
            .map(|id| resolve_step_with(id, retired).to_owned()),
        reviewed: known_unique(&stored.reviewed),
        explored: known_unique(&stored.explored),
        ..stored
    }
}

#[cfg(test)]
#[path = "guide_tests.rs"]
mod tests;
