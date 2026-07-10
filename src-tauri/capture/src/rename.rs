//! Post-save rename: retitle a finished capture (mp3 + companion note)
//! under the same safety rails as the save path — pairwise reservation,
//! non-replacing renames, ownership filters. Audio first: the mp3 move is
//! the arbiter, and a note failure after a successful mp3 move degrades
//! to a warning (the note is repairable by hand; the audio is not).

use std::path::PathBuf;
use vault_buddy_core::capture_note::retarget_embed;
use vault_buddy_core::capture_note::write_note_collision_safe;
use vault_buddy_core::capture_paths::RenamePlan;

pub struct RenameOutcome {
    pub mp3: PathBuf,
    pub note: Option<PathBuf>,
    pub warning: Option<String>,
}

pub fn execute(plan: &RenamePlan) -> Result<RenameOutcome, String> {
    let old_mp3_name = plan
        .mp3_from
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    // Read the note BEFORE moving anything: the embed rewrite needs the
    // old text, and a read failure should not strand a half-done pair.
    let note_read = match std::fs::read_to_string(&plan.note_from) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("cannot read the companion note: {e}")),
    };

    let (mp3_to, note_to) =
        crate::recovery::rename_into_reserved(&plan.mp3_from, &plan.dir, &plan.new_base)?;
    log::info!(
        "capture: renamed {} -> {}",
        plan.mp3_from.display(),
        mp3_to.display()
    );
    let new_mp3_name = mp3_to
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();

    let old_stem = plan
        .mp3_from
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let new_stem = mp3_to
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    // Move the transcript sidecar on the same non-replacing rails (GAP-04).
    // The reserved base guaranteed `<new>.transcript.md` free; if something
    // claimed it since, the squatter wins and the transcript STAYS at its
    // old name with its old embed — suffixing it alone would orphan it from
    // transcript_path(new mp3). Audio-first: never fails the rename.
    let transcript_from = vault_buddy_core::transcript::transcript_path(&plan.mp3_from);
    let transcript_to = vault_buddy_core::transcript::transcript_path(&mp3_to);
    let (transcript_moved, transcript_error) = if transcript_from.is_file() {
        match vault_buddy_core::capture_paths::rename_noreplace(&transcript_from, &transcript_to) {
            Ok(()) => (true, None),
            Err(e) => (
                false,
                Some(format!("the transcript could not be moved: {e}")),
            ),
        }
    } else {
        (false, None)
    };

    let (note, note_error) = match note_read {
        Ok(Some(text)) => {
            let mut retargeted = retarget_embed(&text, &old_mp3_name, &new_mp3_name);
            if transcript_moved {
                retargeted = retarget_embed(
                    &retargeted,
                    &format!("{old_stem}.transcript"),
                    &format!("{new_stem}.transcript"),
                );
            }
            // Collision-safe exclusive create at the reserved name: a
            // sync-client race costs a suffix, never a clobbered file.
            match write_note_collision_safe(&note_to, &retargeted) {
                Ok(written) => match std::fs::remove_file(&plan.note_from) {
                    Ok(()) => (Some(written), None),
                    Err(e) => (
                        Some(written),
                        Some(format!("the old note could not be removed: {e}")),
                    ),
                },
                Err(e) => (None, Some(format!("the note could not be rewritten: {e}"))),
            }
        }
        Ok(None) => (None, None),
        Err(e) => (None, Some(e)),
    };

    let mut issues: Vec<String> = Vec::new();
    if let Some(e) = transcript_error {
        issues.push(e);
    }
    if let Some(e) = note_error {
        issues.push(format!("{e}; note: {}", plan.note_from.display()));
    }
    let warning = (!issues.is_empty()).then(|| {
        let warning = format!(
            "Recording renamed, but needs attention ({}). Audio: {}",
            issues.join("; "),
            mp3_to.display()
        );
        log::warn!("capture: {warning}");
        warning
    });

    Ok(RenameOutcome {
        mp3: mp3_to,
        note,
        warning,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use vault_buddy_core::capture_paths::{is_capture_base, rename_plan};

    fn seed(dir: &std::path::Path) -> (PathBuf, PathBuf) {
        let mp3 = dir.join("2026-07-04 1405 Meeting.mp3");
        let note = dir.join("2026-07-04 1405 Meeting.md");
        std::fs::write(&mp3, "mp3 bytes").unwrap();
        std::fs::write(
            &note,
            "---\nvault: \"W\"\n---\n\n![[2026-07-04 1405 Meeting.mp3]]\n\n\
             ## Transcript\n\n![[2026-07-04 1405 Meeting.transcript]]\n",
        )
        .unwrap();
        (mp3, note)
    }

    #[test]
    fn renames_pair_and_retargets_embed() {
        let dir = tempfile::tempdir().unwrap();
        let (mp3, note) = seed(dir.path());
        let plan = rename_plan(&mp3, "Standup").unwrap();
        let outcome = execute(&plan).unwrap();
        assert_eq!(outcome.mp3, dir.path().join("2026-07-04 1405 Standup.mp3"));
        assert_eq!(
            outcome.note.as_deref(),
            Some(dir.path().join("2026-07-04 1405 Standup.md").as_path())
        );
        assert!(outcome.warning.is_none(), "{:?}", outcome.warning);
        assert!(!mp3.exists(), "old mp3 moved");
        assert!(!note.exists(), "old note moved");
        let text = std::fs::read_to_string(outcome.note.unwrap()).unwrap();
        assert!(text.contains("![[2026-07-04 1405 Standup.mp3]]"), "{text}");
        assert!(!text.contains("Meeting.mp3"), "old embed gone: {text}");
        // recovery must still recognize the retitled files as ours
        let stem = outcome.mp3.file_stem().unwrap().to_string_lossy();
        assert!(is_capture_base(&stem));
    }

    #[test]
    fn collision_on_the_new_name_advances_the_suffix() {
        let dir = tempfile::tempdir().unwrap();
        let (mp3, _note) = seed(dir.path());
        std::fs::write(dir.path().join("2026-07-04 1405 Standup.mp3"), "taken").unwrap();
        let plan = rename_plan(&mp3, "Standup").unwrap();
        let outcome = execute(&plan).unwrap();
        assert_eq!(
            outcome.mp3,
            dir.path().join("2026-07-04 1405 Standup (2).mp3")
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join("2026-07-04 1405 Standup.mp3")).unwrap(),
            "taken",
            "never clobbers"
        );
        let text = std::fs::read_to_string(outcome.note.unwrap()).unwrap();
        assert!(
            text.contains("![[2026-07-04 1405 Standup (2).mp3]]"),
            "embed targets the suffixed name: {text}"
        );
    }

    #[test]
    fn mp3_without_note_renames_audio_only() {
        let dir = tempfile::tempdir().unwrap();
        let mp3 = dir.path().join("2026-07-04 1405 Voice Note.mp3");
        std::fs::write(&mp3, "mp3 bytes").unwrap();
        let plan = rename_plan(&mp3, "Idea").unwrap();
        let outcome = execute(&plan).unwrap();
        assert_eq!(outcome.mp3, dir.path().join("2026-07-04 1405 Idea.mp3"));
        assert!(outcome.note.is_none());
        assert!(outcome.warning.is_none());
    }

    #[test]
    fn missing_mp3_is_a_clean_error() {
        let dir = tempfile::tempdir().unwrap();
        let mp3 = dir.path().join("2026-07-04 1405 Meeting.mp3");
        let plan = rename_plan(&mp3, "Standup").unwrap();
        assert!(execute(&plan).is_err());
    }

    #[test]
    fn renames_transcript_sidecar_and_retargets_its_embed() {
        // GAP-04: execute moved only the mp3 + note; <old>.transcript.md
        // stayed behind, the note kept embedding the old transcript, and the
        // next launch's backfill re-ran a multi-minute inference for a
        // sidecar nothing embeds.
        let dir = tempfile::tempdir().unwrap();
        let (mp3, _note) = seed(dir.path());
        let transcript = dir.path().join("2026-07-04 1405 Meeting.transcript.md");
        std::fs::write(
            &transcript,
            "---\nvault-buddy-transcript: complete\n---\nwords\n",
        )
        .unwrap();
        let plan = rename_plan(&mp3, "Standup").unwrap();
        let outcome = execute(&plan).unwrap();
        assert!(outcome.warning.is_none(), "{:?}", outcome.warning);
        let new_transcript = dir.path().join("2026-07-04 1405 Standup.transcript.md");
        assert!(new_transcript.is_file(), "transcript moved with the pair");
        assert!(!transcript.exists(), "old transcript gone");
        let text = std::fs::read_to_string(outcome.note.unwrap()).unwrap();
        assert!(
            text.contains("![[2026-07-04 1405 Standup.transcript]]"),
            "transcript embed retargeted: {text}"
        );
        assert!(!text.contains("Meeting.transcript"), "{text}");
    }

    #[test]
    fn transcript_collision_degrades_to_warning_and_keeps_old_embed() {
        // Never-clobber: a file that appeared at the new transcript name
        // after reservation wins; the transcript stays at its OLD name and
        // the note keeps pointing at it (a retargeted embed would dangle).
        let dir = tempfile::tempdir().unwrap();
        let (mp3, _note) = seed(dir.path());
        let transcript = dir.path().join("2026-07-04 1405 Meeting.transcript.md");
        std::fs::write(
            &transcript,
            "---\nvault-buddy-transcript: complete\n---\nwords\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("2026-07-04 1405 Standup.transcript.md"),
            "squatter",
        )
        .unwrap();
        // NOTE: the squatter also blocks the plain base at reservation time
        // (reserve_final checks .transcript.md), so the PAIR advances to
        // " (2)" — and "(2).transcript.md" is free, so the move succeeds.
        // Force the collision instead by squatting the suffixed name too:
        std::fs::write(
            dir.path().join("2026-07-04 1405 Standup (2).transcript.md"),
            "squatter 2",
        )
        .unwrap();
        let plan = rename_plan(&mp3, "Standup").unwrap();
        let outcome = execute(&plan).unwrap();
        // The pair reserves a base whose transcript slot is free, so with
        // both squats the mp3 lands at " (3)" and the transcript moves.
        assert_eq!(
            outcome.mp3,
            dir.path().join("2026-07-04 1405 Standup (3).mp3")
        );
        assert!(dir
            .path()
            .join("2026-07-04 1405 Standup (3).transcript.md")
            .is_file());
        assert_eq!(
            std::fs::read_to_string(dir.path().join("2026-07-04 1405 Standup.transcript.md"))
                .unwrap(),
            "squatter",
            "never clobbers"
        );
    }

    #[test]
    fn missing_transcript_renames_pair_without_warning() {
        let dir = tempfile::tempdir().unwrap();
        let (mp3, _note) = seed(dir.path());
        let plan = rename_plan(&mp3, "Standup").unwrap();
        let outcome = execute(&plan).unwrap();
        assert!(outcome.warning.is_none(), "{:?}", outcome.warning);
        assert!(!dir
            .path()
            .join("2026-07-04 1405 Standup.transcript.md")
            .exists());
        // an absent transcript leaves the (static) embed line alone
        let text = std::fs::read_to_string(outcome.note.unwrap()).unwrap();
        assert!(
            text.contains("![[2026-07-04 1405 Meeting.transcript]]"),
            "{text}"
        );
    }
}
