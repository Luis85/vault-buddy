//! The render plan's two OUTPUT-time readers for other surfaces (Task 48;
//! F-35, F-43, A13), a child of `render_plan` for its 800-line cap: the
//! subtitle export's cues and the companion note's chapters are the SAME
//! rows the burn-in and the plan's chapter map read, never a second copy.

use super::{chapters_of, clip_span, time, EditorError, PlannedCaption, Project, Window};
use crate::editor::captions_io::ParsedCue;

/// Every caption the Captions list shows, in OUTPUT time inside `window`,
/// sorted by start then end -- the burn-in's rows and the subtitle export's
/// (Task 48) are one rule.
pub(super) fn caption_rows(project: &Project, window: &Window) -> Vec<PlannedCaption> {
    let Some(settings) = project.captions.as_ref() else {
        return Vec::new();
    };
    let mut cues: Vec<PlannedCaption> = settings
        .cues
        .iter()
        .filter_map(|q| {
            let clip = project.clips.iter().find(|c| c.id == q.clip_id)?;
            let (s, e) = time::cue_output_span(&clip_span(clip), q.start_ms, q.end_ms)?;
            let (output_start, output_end, _) = window.clip(s, e)?;
            Some(PlannedCaption {
                id: q.id.clone(),
                output_start,
                output_end,
                text: q.text.clone(),
            })
        })
        .collect();
    cues.sort_by_key(|q| (q.output_start, q.output_end));
    cues
}

/// Task 48 (F-35): every caption over the WHOLE timeline, in output time,
/// for `captions_io::export_srt`/`export_vtt` -- whether or not captions
/// are shown or burned in (an export is its own, explicit request).
pub fn subtitle_cues(project: &Project) -> Vec<ParsedCue> {
    let window = Window {
        start: 0,
        end: time::project_duration(project),
    };
    caption_rows(project, &window)
        .into_iter()
        .map(|q| ParsedCue {
            start_ms: q.output_start,
            end_ms: q.output_end,
            text: q.text,
        })
        .collect()
}

/// Task 48 (F-43, A13): the chapters a render of `range` carries, rebased
/// to its start -- the companion note's list and the plan's are one rule.
pub fn chapters_for(
    project: &Project,
    range: Option<(u64, u64)>,
) -> Result<Vec<(u64, String)>, EditorError> {
    let window = Window::new(time::project_duration(project), range)?;
    Ok(chapters_of(project, &window))
}
