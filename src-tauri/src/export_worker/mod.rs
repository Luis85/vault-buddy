//! The `screen-export` thread: turn a staged capture into a playable `.mp4`
//! plus a companion note inside the user's vault — **the ninth sanctioned
//! vault write, and the first time the screen-capture feature touches a
//! vault at all.**
//!
//! Three orderings in here are load-bearing, and every one of them is the
//! difference between a save and a loss:
//!
//! 1. **The staged capture is deleted only after the vault write has
//!    landed** (spec §8.3). Until `commit_into_vault` returns, the staged
//!    `.mp4` and its sidecar are untouched, so any failure leaves the user
//!    exactly where they started — with their recording. Get this backwards
//!    and a failed commit costs them the footage.
//! 2. **Video first, note second, and a note failure is a WARNING** —
//!    `capture::session::finalize`'s posture exactly. The video is
//!    irreplaceable; the note is regenerable prose. Writing the note first
//!    and then failing the commit would leave a note in the vault embedding
//!    a file that is not there.
//! 3. **Containment is asserted BEFORE and AFTER `create_dir_all`** — the
//!    document-import discipline, which is stronger than the audio path's
//!    post-only check. The PRE check is what stops `create_dir_all`
//!    following a pre-existing symlink or junction and creating our
//!    directories outside the vault; the POST check closes the swap-in race.
//!
//! **Almost every refusal is ordered ahead of any vault mutation, and the
//! one that is not rolls itself back.** Everything `prepare` can refuse — a
//! mismatched sidecar, a recovered capture, a missing vault, a missing
//! video, no ffmpeg, an unencodable timeline — answers before a single
//! directory is created. `check_free_space` cannot: a directory that does
//! not exist yet reports no free space at all, so it has to measure the
//! folder it is about to fill. So the ONE mutation an export makes ahead of
//! its last refusal is `prepare_export_dir`'s `create_dir_all`, and every
//! way out that is not a save — the space refusal, a user **Cancel** (spec
//! §14's ordinary exit), an ffmpeg failure, a commit failure — hands
//! `created_dirs` to `rollback_export_dir`, which removes ONLY the
//! directories this export created and ONLY while they are still empty.
//! Without that the ordinary Cancel left an empty `Screen Captures/YYYY/MM`
//! in the user's notes permanently, for a save they explicitly called off.
//!
//! **Every failure deletes the export temp and keeps the staged capture — a
//! deliberate departure from spec §14**, which says a failed vault write
//! keeps the exported temp "with retry". A kept temp is a promise this
//! codebase cannot keep: `screen_recovery` sweeps a stale
//! `.<base>.export.mp4.part`, so the temp would survive an immediate retry
//! and silently vanish before a next-session one, giving the same button two
//! different behaviours. What a retry actually needs is the STAGED capture,
//! which is untouched until the very last step.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use chrono::{Local, NaiveDate};
use tauri::{AppHandle, Manager};
use vault_buddy_core::capture_config;
use vault_buddy_core::capture_note::write_note_collision_safe;
use vault_buddy_core::capture_paths::{capture_dir, safe_recording_root};
use vault_buddy_core::screen_capture_paths::commit_screen_capture;
use vault_buddy_core::screen_note::{render_screen_note, ScreenNoteMeta};
use vault_buddy_core::timeline::Timeline;
use vault_buddy_screen::export::{export, export_refusal, ExportOutcome, ExportRequest};
use vault_buddy_screen::ffmpeg_args::EncodeSettings;
use vault_buddy_screen::{staging, ScreenError};

use crate::export_commands::{cancel_flag, emit_export_progress, timeline_from_sidecar};

mod vault_dir;
use vault_dir::{check_free_space, prepare_export_dir, rollback_export_dir};

/// How an export ended when it did not produce a saved capture.
///
/// `Cancelled` is NOT a failure (spec §14): the user asked, the staged
/// capture and its timeline are untouched, and no error banner is owed.
pub(crate) enum ExportFailure {
    Cancelled,
    Failed(String),
}

/// What a saved capture is, for the event the editor listens to.
pub(crate) struct ExportSummary {
    pub base: String,
    pub video_path: PathBuf,
    pub note_path: Option<PathBuf>,
    pub vault_id: String,
    /// The vault's DISPLAY name, carried beside the id because the editor
    /// window installs no store and has no vault list: `vault_id` is
    /// Obsidian's opaque hex registry key, so a window holding only that
    /// cannot say where the capture went. It is the same string
    /// `render_screen_note` writes into the note's `vault:` key, resolved
    /// once in `prepare` from the live registry.
    pub vault_name: String,
    /// Set when the video landed but its companion note did not — the
    /// degrade-to-warning arm, never a failure.
    pub warning: Option<String>,
}

/// Everything resolved before a single byte is encoded. Assembled in one
/// place so `export_blocking` reads as the three-step sequence it is.
struct Prepared {
    base: String,
    staging: PathBuf,
    staged_mp4: PathBuf,
    temp: PathBuf,
    dir: PathBuf,
    /// The directories `prepare_export_dir` created for this export, deepest
    /// first. Empty when the dated folder was already there.
    created_dirs: Vec<PathBuf>,
    vault_id: String,
    vault_name: String,
    sidecar: staging::StagedSidecar,
    timeline: Timeline,
    settings: EncodeSettings,
    ffmpeg: PathBuf,
    create_note: bool,
    extra_frontmatter: Option<String>,
    body_template: Option<String>,
}

pub(crate) fn export_blocking(app: &AppHandle, base: &str) -> Result<ExportSummary, ExportFailure> {
    let prepared = prepare(app, base).map_err(ExportFailure::Failed)?;
    // A cancel is an ordinary exit (spec §14) and an ffmpeg failure is not,
    // but neither one saved anything, so both owe the vault the same rollback.
    let outcome = run_export(app, &prepared).inspect_err(|_| {
        rollback_export_dir(&prepared.created_dirs);
    })?;
    let committed = commit_into_vault(CommitInputs {
        temp: &prepared.temp,
        dir: &prepared.dir,
        base: &prepared.base,
        staging: &prepared.staging,
        note: prepared.create_note.then(|| {
            note_meta(
                &prepared.sidecar,
                &prepared.vault_name,
                &prepared.settings,
                outcome.output_duration_ms,
                prepared.extra_frontmatter.clone(),
                prepared.body_template.clone(),
            )
        }),
    })
    .map_err(|e| {
        // The commit failed, so the bytes are still in the temp. Delete it:
        // see the module doc on why a kept temp is a promise we cannot keep.
        remove_export_temp(&prepared.temp);
        rollback_export_dir(&prepared.created_dirs);
        ExportFailure::Failed(e)
    })?;
    log::info!(
        "screen export: saved {} to {} (remuxed: {})",
        prepared.base,
        committed.video.display(),
        outcome.remuxed
    );
    Ok(ExportSummary {
        base: prepared.base,
        video_path: committed.video,
        note_path: committed.note,
        vault_id: prepared.vault_id,
        vault_name: prepared.vault_name,
        warning: committed.warning,
    })
}

fn prepare(app: &AppHandle, base: &str) -> Result<Prepared, String> {
    let staging_dir = staging::staging_dir(
        &app.path()
            .app_local_data_dir()
            .map_err(|e| format!("Could not resolve the staging directory: {e}"))?,
    );
    let sidecar = staging::read_sidecar(&staging_dir.join(staging::sidecar_file_name(base)))
        .ok_or_else(|| "That capture's details could not be read.".to_string())?;
    if sidecar.base != base {
        // The load path's own refusal: a sidecar is hand-editable, so its
        // `base` is untrusted input that must never become the identity a
        // vault write is addressed by.
        log::warn!(
            "screen export: sidecar carries base {:?}, not {base:?}; refusing",
            sidecar.base
        );
        return Err("That capture's details do not match its file name.".to_string());
    }
    if sidecar.vault_id.is_empty() {
        // A capture recovered by `screen_recovery` after an interrupted
        // session carries no vault id (and no duration): nothing on disk
        // records them once the sidecar is gone. Saying so is the honest
        // answer — `find_vault("")` would report the vault as missing from
        // Obsidian, which is a different problem with a different remedy.
        return Err(
            "This capture was recovered after Vault Buddy closed unexpectedly, so it no \
             longer records which vault it belongs to. Its video is still on disk, in the \
             screen-captures folder beside Vault Buddy's logs."
                .to_string(),
        );
    }
    let vault = crate::commands::find_vault(&sidecar.vault_id).map_err(|_| {
        "The vault this capture was recorded for is no longer in Obsidian. Open it in \
         Obsidian and try again."
            .to_string()
    })?;
    let vault_path = PathBuf::from(&vault.path);
    if !vault_path.is_dir() {
        return Err("Vault folder not found — was it moved or deleted?".to_string());
    }
    let cfg = capture_config::vault_config(&capture_config::load_config(), &sidecar.vault_id);
    let staged_mp4 = staging_dir.join(staging::mp4_file_name(base));
    if !staged_mp4.is_file() {
        return Err("That capture's video file is missing.".to_string());
    }
    let timeline = timeline_from_sidecar(sidecar.timeline.clone(), sidecar.duration_ms);

    let tools = crate::ffmpeg::resolve_working_ffmpeg().ok_or_else(|| {
        "Saving a screen capture needs ffmpeg, which is not installed. Install it, then set \
         its location in Buddy settings if it is not on your PATH."
            .to_string()
    })?;
    // The FILE's own pixels and its real audio track, not the sidecar's
    // hand-editable copies — the note and the encoder both key on these.
    let facts = crate::ffmpeg::probe_source(&tools, &staged_mp4)?;
    let settings = EncodeSettings {
        width: facts.width,
        height: facts.height,
        fps: cfg.screen_fps,
        quality: cfg.screen_quality,
        h264_encoder: tools.h264_encoder.clone().unwrap_or_default(),
        has_audio: facts.has_audio,
    };
    // Answered before anything is created, so the user is told what is
    // missing rather than handed ffmpeg's own error text.
    if let Some(message) = export_refusal(&timeline, sidecar.duration_ms, &settings) {
        return Err(message);
    }
    // Only NOW is the vault touched. Everything above can refuse, and a
    // refusal must not leave an empty `Screen Captures/2026/09` behind in
    // somebody's notes for a save that never happened.
    let root = safe_recording_root(&vault_path, cfg.screen_capture_root())?;
    let dir = capture_dir(
        &root,
        recorded_date(&sidecar.recorded_at),
        cfg.screen_capture_date_folders,
    );
    // This is where the vault is first TOUCHED. Everything above refuses
    // without creating anything; everything below either saves or hands its
    // `created_dirs` to `rollback_export_dir`, so a save that does not
    // happen leaves a user's notes exactly as it found them.
    let created_dirs = prepare_export_dir(&vault_path, &dir)?;
    // Measured after creation, deliberately: a directory that does not exist
    // yet reports no free space at all, and an unmeasurable volume never
    // refuses. So this ONE refusal is ordered behind a vault mutation, and
    // it is the refusal that rolls it back.
    if let Err(e) = check_free_space(&timeline, &settings, &cfg, &[&staging_dir, &dir]) {
        rollback_export_dir(&created_dirs);
        return Err(e);
    }

    Ok(Prepared {
        created_dirs,
        temp: staging_dir.join(staging::export_part_file_name(base)),
        base: base.to_string(),
        staging: staging_dir,
        staged_mp4,
        dir,
        vault_id: sidecar.vault_id.clone(),
        vault_name: vault.name.clone(),
        sidecar,
        timeline,
        settings,
        ffmpeg: PathBuf::from(&tools.ffmpeg),
        create_note: cfg.screen_create_note,
        extra_frontmatter: cfg.screen_extra_frontmatter.clone(),
        body_template: cfg.screen_body_template.clone(),
    })
}

fn run_export(app: &AppHandle, p: &Prepared) -> Result<ExportOutcome, ExportFailure> {
    let cancel = cancel_flag(app).unwrap_or_else(|| std::sync::Arc::new(AtomicBool::new(false)));
    let base = p.base.clone();
    let mut on_progress = |percent: u64| emit_export_progress(app, &base, percent);
    let request = ExportRequest {
        ffmpeg: &p.ffmpeg,
        source: &p.staged_mp4,
        dest: &p.temp,
        timeline: &p.timeline,
        // The SIDECAR's duration, never a probed one: the editor built this
        // timeline against it, and ffprobe answers a frame or two
        // differently, which would make an untouched capture read as edited
        // and drop it to a full re-encode.
        source_duration_ms: p.sidecar.duration_ms,
        settings: p.settings.clone(),
    };
    match export(request, &cancel, &mut on_progress) {
        // No terminal emit here: `export` already guarantees a final 100,
        // and exactly one (`last_emitted != Some(100)`).
        Ok(outcome) => Ok(outcome),
        Err(ScreenError::Cancelled) => {
            remove_export_temp(&p.temp);
            Err(ExportFailure::Cancelled)
        }
        Err(e) => {
            remove_export_temp(&p.temp);
            Err(ExportFailure::Failed(e.to_string()))
        }
    }
}

/// The note's frontmatter, from the EXPORT's facts rather than the capture's.
///
/// `duration_secs` is the OUTPUT duration: a capture trimmed from ten
/// minutes to two must not claim ten. `width`/`height` are the exported
/// file's own pixels as ffprobe read them from the source, not the
/// sidecar's hand-editable copies.
fn note_meta(
    sidecar: &staging::StagedSidecar,
    vault_name: &str,
    settings: &EncodeSettings,
    output_duration_ms: u64,
    extra_frontmatter: Option<String>,
    body_template: Option<String>,
) -> ScreenNoteMeta {
    ScreenNoteMeta {
        recorded_at: sidecar.recorded_at.clone(),
        duration_secs: output_duration_ms / 1_000,
        vault_name: vault_name.to_string(),
        source: sidecar.source_title.clone(),
        input_devices: sidecar.inputs.clone(),
        width: settings.width,
        height: settings.height,
        extra_frontmatter,
        body_template,
    }
}

struct CommitInputs<'a> {
    temp: &'a Path,
    dir: &'a Path,
    base: &'a str,
    staging: &'a Path,
    /// `None` when the vault opted out of companion notes.
    note: Option<ScreenNoteMeta>,
}

#[derive(Debug)]
struct Committed {
    video: PathBuf,
    note: Option<PathBuf>,
    warning: Option<String>,
}

/// The vault write, in the one order that cannot lose a recording.
///
/// Video, then note, then — and only then — the staged capture. See the
/// module doc; the three orderings are pinned both structurally and by
/// `a_failed_commit_keeps_the_staged_capture_and_writes_no_note`.
fn commit_into_vault(inputs: CommitInputs<'_>) -> Result<Committed, String> {
    let (video, note_target) = commit_screen_capture(inputs.temp, inputs.dir, inputs.base)?;
    let mut note = None;
    let mut warning = None;
    if let Some(meta) = inputs.note {
        let file_name = video
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let content = render_screen_note(&meta, &file_name);
        match write_note_collision_safe(&note_target, &content) {
            Ok(path) => note = Some(path),
            // A WARNING, never a rollback: the video is irreplaceable and
            // already safely in the vault; the note is regenerable prose.
            Err(e) => {
                log::warn!(
                    "screen export: saved {} but could not write its note: {e}",
                    video.display()
                );
                warning = Some(
                    "The capture was saved, but its companion note could not be written."
                        .to_string(),
                );
            }
        }
    }
    remove_staged_capture(inputs.staging, inputs.base);
    Ok(Committed {
        video,
        note,
        warning,
    })
}

/// Delete the staged `.mp4` and its sidecar, AFTER the vault write landed.
///
/// The one removal site (a structural test pins that). Both failures are
/// warnings: the user's capture is in their vault by this point, so a
/// leftover staged file is litter rather than loss, and `screen_recovery`
/// will not delete it — but a duplicate in the resume list is a far smaller
/// harm than a removal that ran too early.
fn remove_staged_capture(staging_dir: &Path, base: &str) {
    for path in [
        staging_dir.join(staging::mp4_file_name(base)),
        staging_dir.join(staging::sidecar_file_name(base)),
    ] {
        if let Err(e) = std::fs::remove_file(&path) {
            log::warn!(
                "screen export: could not remove {} after saving: {e}",
                path.display()
            );
        }
    }
}

fn remove_export_temp(temp: &Path) {
    if temp.exists() {
        if let Err(e) = std::fs::remove_file(temp) {
            log::warn!(
                "screen export: could not remove the export temp {}: {e}",
                temp.display()
            );
        }
    }
}

/// The capture's own date, for the dated folder layout. A sidecar that can
/// no longer be parsed falls back to today rather than failing the save.
fn recorded_date(recorded_at: &str) -> NaiveDate {
    chrono::DateTime::parse_from_rfc3339(recorded_at)
        .map(|dt| dt.with_timezone(&Local).date_naive())
        .unwrap_or_else(|_| Local::now().date_naive())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn sidecar(base: &str) -> staging::StagedSidecar {
        staging::StagedSidecar {
            base: base.to_string(),
            vault_id: "v1".into(),
            source_title: "Figma".into(),
            source_kind: "screen".into(),
            inputs: vec!["Mic".into()],
            duration_ms: 600_000,
            paused_ms: 0,
            width: 1280,
            height: 720,
            recorded_at: "2026-09-20T14:32:00+02:00".into(),
            timeline: None,
            extra: serde_json::Map::new(),
        }
    }

    fn settings() -> EncodeSettings {
        EncodeSettings {
            width: 1920,
            height: 1080,
            fps: 30,
            quality: Default::default(),
            h264_encoder: "libx264".into(),
            has_audio: true,
        }
    }

    /// A staging directory holding a staged capture, and an export temp
    /// whose bytes are what a commit is supposed to move.
    fn staged(dir: &Path, base: &str, temp_bytes: Option<&[u8]>) -> PathBuf {
        fs::write(dir.join(staging::mp4_file_name(base)), b"staged").unwrap();
        fs::write(dir.join(staging::sidecar_file_name(base)), b"{}").unwrap();
        let temp = dir.join(staging::export_part_file_name(base));
        if let Some(bytes) = temp_bytes {
            fs::write(&temp, bytes).unwrap();
        }
        temp
    }

    fn meta() -> ScreenNoteMeta {
        note_meta(&sidecar("B"), "Vault", &settings(), 120_000, None, None)
    }

    const BASE: &str = "2026-09-20 1432 Demo";

    #[test]
    fn a_successful_commit_lands_the_video_the_note_and_then_clears_staging() {
        let stage = tempfile::tempdir().unwrap();
        let vault = tempfile::tempdir().unwrap();
        let temp = staged(stage.path(), BASE, Some(b"footage"));

        let out = commit_into_vault(CommitInputs {
            temp: &temp,
            dir: vault.path(),
            base: BASE,
            staging: stage.path(),
            note: Some(meta()),
        })
        .expect("the commit lands");

        assert_eq!(fs::read(&out.video).unwrap(), b"footage");
        assert_eq!(out.video, vault.path().join(format!("{BASE}.mp4")));
        assert_eq!(out.note, Some(vault.path().join(format!("{BASE}.md"))));
        assert_eq!(out.warning, None);
        assert!(!temp.exists(), "the export temp outlived the commit");
        // ...and ONLY NOW is the staged capture gone.
        assert!(!stage.path().join(staging::mp4_file_name(BASE)).exists());
        assert!(!stage.path().join(staging::sidecar_file_name(BASE)).exists());
    }

    // THE never-lose invariant, behaviourally. A commit that cannot move the
    // video must leave the user exactly where they started: with their
    // staged recording, and with no note in the vault pointing at a file
    // that is not there.
    #[test]
    fn a_failed_commit_keeps_the_staged_capture_and_writes_no_note() {
        let stage = tempfile::tempdir().unwrap();
        let vault = tempfile::tempdir().unwrap();
        // No temp on disk, so the move fails decisively (NotFound).
        let temp = staged(stage.path(), BASE, None);

        let err = commit_into_vault(CommitInputs {
            temp: &temp,
            dir: vault.path(),
            base: BASE,
            staging: stage.path(),
            note: Some(meta()),
        })
        .expect_err("a missing source cannot commit");
        assert!(err.contains("could not be moved"), "{err}");

        assert!(
            stage.path().join(staging::mp4_file_name(BASE)).is_file(),
            "the staged capture was deleted although nothing reached the vault"
        );
        assert!(
            stage
                .path()
                .join(staging::sidecar_file_name(BASE))
                .is_file(),
            "the staged sidecar was deleted although nothing reached the vault"
        );
        let stray: Vec<_> = fs::read_dir(vault.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert!(
            stray.is_empty(),
            "a failed commit left something in the vault: {stray:?}"
        );
    }

    // The note is named from where the VIDEO actually landed, so the pair
    // can never disagree about its own suffix.
    #[test]
    fn the_note_names_the_suffix_the_video_actually_took() {
        let stage = tempfile::tempdir().unwrap();
        let vault = tempfile::tempdir().unwrap();
        fs::write(vault.path().join(format!("{BASE}.mp4")), b"the user's own").unwrap();
        let temp = staged(stage.path(), BASE, Some(b"footage"));

        let out = commit_into_vault(CommitInputs {
            temp: &temp,
            dir: vault.path(),
            base: BASE,
            staging: stage.path(),
            note: Some(meta()),
        })
        .unwrap();

        assert_eq!(out.video, vault.path().join(format!("{BASE} (2).mp4")));
        assert_eq!(out.note, Some(vault.path().join(format!("{BASE} (2).md"))));
        let note = fs::read_to_string(out.note.unwrap()).unwrap();
        assert!(
            note.contains(&format!("{BASE} (2).mp4")),
            "the note embeds a name the video does not have: {note}"
        );
        // And the file that was already there is untouched.
        assert_eq!(
            fs::read(vault.path().join(format!("{BASE}.mp4"))).unwrap(),
            b"the user's own"
        );
    }

    #[test]
    fn a_vault_that_opted_out_of_notes_gets_only_the_video() {
        let stage = tempfile::tempdir().unwrap();
        let vault = tempfile::tempdir().unwrap();
        let temp = staged(stage.path(), BASE, Some(b"footage"));

        let out = commit_into_vault(CommitInputs {
            temp: &temp,
            dir: vault.path(),
            base: BASE,
            staging: stage.path(),
            note: None,
        })
        .unwrap();

        assert_eq!(out.note, None);
        assert!(!vault.path().join(format!("{BASE}.md")).exists());
        assert!(out.video.is_file());
    }

    // The note records what the EXPORT produced, not what was recorded. A
    // capture trimmed from ten minutes to two must not claim ten, and the
    // dimensions must be the file's own pixels rather than the sidecar's
    // hand-editable copies.
    #[test]
    fn the_note_records_the_output_duration_and_the_exported_dimensions() {
        let s = sidecar("B");
        let m = note_meta(&s, "Vault", &settings(), 120_000, None, None);
        assert_eq!(m.duration_secs, 120);
        assert_ne!(
            m.duration_secs,
            s.duration_ms / 1_000,
            "the note claimed the SOURCE duration"
        );
        assert_eq!((m.width, m.height), (1920, 1080));
        assert_ne!(
            (m.width, m.height),
            (s.width, s.height),
            "the note took its dimensions from the sidecar"
        );
    }

    #[test]
    fn the_dated_folder_uses_the_captures_own_date_and_falls_back_to_today() {
        assert_eq!(
            recorded_date("2026-09-20T14:32:00+02:00"),
            NaiveDate::from_ymd_opt(2026, 9, 20).unwrap()
        );
        assert_eq!(recorded_date("not a date"), Local::now().date_naive());
    }

    fn production_src() -> &'static str {
        include_str!("mod.rs")
            .split("#[cfg(test)]")
            .next()
            .expect("the production prefix")
    }

    /// The body of the one function that performs the vault write, sliced so
    /// an ordering assertion cannot be satisfied by an unrelated call
    /// somewhere else in the file — the trap that let one command's guard
    /// answer another's assertion in phase 4.
    fn commit_body() -> &'static str {
        let src = production_src();
        let start = src
            .find("fn commit_into_vault(inputs: CommitInputs<'_>)")
            .expect("commit_into_vault");
        let end = src[start..]
            .find("\n}\n")
            .map(|i| start + i)
            .expect("its closing brace");
        &src[start..end]
    }

    // The export temp's name must be MINTED by the shared helper, never
    // respelled here: `screen_recovery` recognises an abandoned temp by
    // exactly that shape, and a literal that drifted from it would simply
    // stop being swept -- silently, since nothing else ever reads the file.
    #[test]
    fn the_export_temp_is_named_by_the_shared_helper() {
        let src = production_src();
        assert_eq!(src.matches("export_part_file_name(").count(), 1);
        assert!(
            !src.contains("\".export\""),
            "the export infix is respelled here instead of imported"
        );
    }

    // The never-lose invariant, structurally. The behavioural test above is
    // the real gate; this one makes the ordering visible at the point a
    // reviewer would otherwise have to eyeball it.
    #[test]
    fn the_staged_capture_is_removed_only_after_the_vault_commit() {
        let body = commit_body();
        let commit = body
            .find("commit_screen_capture(")
            .expect("the vault commit is present");
        let remove = body
            .find("remove_staged_capture(")
            .expect("the staged removal is present");
        assert!(
            commit < remove,
            "the staged capture is removed at byte {remove}, before the vault commit at {commit}"
        );
        // Exactly one CALL site anywhere in the file (the definition is
        // written `fn remove_staged_capture(staging_dir:` and so does not
        // match this form).
        assert_eq!(
            production_src()
                .matches("remove_staged_capture(inputs")
                .count()
                + production_src().matches("remove_staged_capture(&").count(),
            1,
            "the staged capture must be removed from exactly one place"
        );
    }

    // Every way out of an export that is not a save rolls the directory
    // back: the space refusal in `prepare`, the `run_export` failure arm
    // (which is BOTH the user's Cancel and an ffmpeg failure) and the commit
    // failure arm. A behavioural test covers what a rollback does; only this
    // can see that a future fourth failure arm was given one.
    #[test]
    fn every_non_saving_exit_rolls_the_export_directory_back() {
        let src = production_src();
        assert_eq!(
            src.matches("rollback_export_dir(&").count(),
            3,
            "an exit was added or removed without its rollback"
        );
    }

    // Video first, note second — `capture::session::finalize`'s order. A
    // note written first and a video commit that then failed would leave a
    // note in the vault embedding a file that is not there.
    #[test]
    fn the_video_commits_before_the_note_is_written() {
        let body = commit_body();
        let video = body.find("commit_screen_capture(").expect("video commit");
        let note = body
            .find("write_note_collision_safe(")
            .expect("the note write lives in the same function as the commit");
        assert!(video < note, "the note is written before the video commits");
    }
}
