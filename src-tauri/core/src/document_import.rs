//! Document Import: convert .docx/.odt/.rtf to a vault note via Pandoc.
//! Pure filename/frontmatter/path/staging logic; the shell drives Pandoc.
//! Fifth sanctioned vault write — same never-clobber discipline as the
//! capture note. Spec:
//! docs/superpowers/specs/2026-07-10-document-import-pandoc-design.md

use crate::capture_note::{write_note_atomic, yaml_quote, NOTE_TMP_SUFFIX};
use crate::capture_paths::candidate;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocFormat {
    Docx,
    Odt,
    Rtf,
}

impl DocFormat {
    /// Extension is authoritative (Obsidian/search treat extensions the same
    /// way). Case-insensitive so `.DOCX` from Windows still maps.
    pub fn from_extension(ext: &str) -> Option<DocFormat> {
        match ext.to_ascii_lowercase().as_str() {
            "docx" => Some(DocFormat::Docx),
            "odt" => Some(DocFormat::Odt),
            "rtf" => Some(DocFormat::Rtf),
            _ => None,
        }
    }

    /// The Pandoc `-f <reader>` value.
    pub fn reader(&self) -> &'static str {
        match self {
            DocFormat::Docx => "docx",
            DocFormat::Odt => "odt",
            DocFormat::Rtf => "rtf",
        }
    }

    /// Value written to the note's `format:` frontmatter field.
    pub fn label(&self) -> &'static str {
        self.reader()
    }
}

/// `YYYY-MM-DD <Original Name>` (no extension). `today` supplied by the shell
/// so the core stays clock-free.
pub fn document_basename(original_stem: &str, today: &str) -> String {
    format!("{today} {original_stem}")
}

pub struct DocMeta {
    /// The original file's absolute path (provenance).
    pub source_path: String,
    /// Import date, `YYYY-MM-DD`.
    pub imported: String,
    pub format: DocFormat,
}

/// The `type: Document` frontmatter block (no body — Pandoc's markdown is
/// prepended by the shell after this). Every string value quoted via
/// `yaml_quote`, so a Windows source path can't emit an invalid YAML escape.
pub fn render_frontmatter(meta: &DocMeta) -> String {
    format!(
        "---\ntype: Document\ntags: [vault-buddy-import]\nsource: {}\nimported: {}\nformat: {}\ncreated-by: Vault Buddy\n---\n\n",
        yaml_quote(&meta.source_path),
        yaml_quote(&meta.imported),
        yaml_quote(meta.format.label()),
    )
}

/// `<vault>/<documents_folder>/<YYYY>/<MM>`.
pub fn target_dir(vault_path: &Path, documents_folder: &str, year: &str, month: &str) -> PathBuf {
    vault_path.join(documents_folder).join(year).join(month)
}

/// Resolve a collision-free basename BEFORE Pandoc runs: walk the ` (N)`
/// suffix scheme until BOTH `<name>.md` and the `<name>/` media folder are
/// free, and use that one name for both. Up-front (not at write time) because
/// Pandoc bakes the media-folder name into image links as it converts — a
/// publish-time re-suffix would leave the note pointing at the wrong folder.
pub fn reserve_basename(target_dir: &Path, basename: &str) -> String {
    for attempt in 1u32.. {
        let name = candidate(basename, attempt);
        let note_free = !target_dir.join(format!("{name}.md")).exists();
        let media_free = !target_dir.join(&name).exists();
        if note_free && media_free {
            return name;
        }
    }
    unreachable!("suffix search always terminates")
}

pub struct StagePlan {
    pub work_dir: PathBuf,
    pub media_name: String,
    pub note_name: String,
}

/// Dot-prefixed temp working dir under `target_dir` (same volume → atomic
/// publish rename; dot-dir → excluded from every vault_walk scan and
/// recovery). `unique` (a per-invocation token from the shell) keeps two
/// imports to the same date from colliding on the temp dir. Media/note names
/// are the FINAL names, so Pandoc's relative-to-note image links stay correct
/// after the publish move.
pub fn plan_staging(target_dir: &Path, basename: &str, unique: &str) -> StagePlan {
    let work_dir = target_dir.join(format!(".{basename}.{unique}{NOTE_TMP_SUFFIX}.import"));
    StagePlan {
        work_dir,
        media_name: basename.to_string(),
        note_name: format!("{basename}.md"),
    }
}

pub fn cleanup_staging(work_dir: &Path) {
    let _ = std::fs::remove_dir_all(work_dir);
}

/// The owned staging-dir marker. Matched by the janitor so it removes ONLY
/// our own crash-orphaned temp dirs, never another tool's dot-directory.
/// MUST equal `<NOTE_TMP_SUFFIX>.import` — the tail `plan_staging` mints — or
/// the janitor stops recognizing (and cleaning) the dirs the importer creates.
/// `staging_marker_matches_plan_staging_output` locks that coupling.
const STAGING_MARKER: &str = ".vault-buddy.tmp.import";

pub fn is_import_staging_dir(name: &str) -> bool {
    name.starts_with('.') && name.ends_with(STAGING_MARKER)
}

/// Outcome of one janitor sweep.
#[derive(Debug, Default, PartialEq)]
pub struct StagingSweep {
    /// Stale orphan staging dirs removed this pass (for logging).
    pub removed: Vec<PathBuf>,
    /// Staging dirs seen that were too FRESH to remove yet. >0 means the
    /// shell janitor must reschedule — a crash-then-immediate-restart leaves
    /// an orphan younger than the staleness window (Codex review).
    pub pending: usize,
}

/// A `YYYY` year folder: exactly four ASCII digits. The janitor descends only
/// into these (see `clean_stale_staging_at`), so a non-dated folder is skipped.
fn is_year_dir(name: &str) -> bool {
    name.len() == 4 && name.bytes().all(|b| b.is_ascii_digit())
}

/// A `MM` month folder: exactly two ASCII digits.
fn is_month_dir(name: &str) -> bool {
    name.len() == 2 && name.bytes().all(|b| b.is_ascii_digit())
}

/// Startup janitor: remove crash-orphaned import staging dirs under a vault's
/// Documents folder (walking its `YYYY/MM` subtree — that's where staging
/// dirs live). Staleness-gated with an injected `now` so a clock jump giving
/// a live dir a future mtime can't make it look stale (mirrors capture's
/// `is_stale_at`). Returns what was removed AND how many fresh orphans remain
/// (so the caller can reschedule).
///
/// **Canonical containment at every level** (Codex review): a symlinked OR
/// junctioned dated subfolder (`Documents/2026`, `2026/07`) must never let the
/// sweep descend or `remove_dir_all` outside the vault. `is_symlink()` alone is
/// insufficient — a Windows directory junction is a reparse point, NOT a
/// symlink. So each descended dir is `canonicalize()`d and required to stay
/// under the canonicalized `documents_root`; anything that fails to canonicalize
/// or escapes is skipped. The caller (the shell janitor) additionally
/// canonical-checks `documents_root` is inside the vault before calling.
///
/// **Delete only an owned in-place staging dir, never a link's target** (Codex
/// review): a stale entry named like a staging dir but which is actually a
/// symlink/junction would, if followed, make `remove_dir_all` erase whatever it
/// points at — real vault data still *inside* the vault, so the containment
/// check wouldn't catch it. An owned staging dir is a real directory we created,
/// so it canonicalizes to itself; the leaf is deleted only when `canon == path`
/// and the ORIGINAL entry path is removed (never a resolved target).
pub fn clean_stale_staging_at(
    documents_root: &Path,
    now: std::time::SystemTime,
    stale_after: std::time::Duration,
) -> StagingSweep {
    let mut sweep = StagingSweep::default();
    let Ok(canon_root) = documents_root.canonicalize() else {
        return sweep; // no Documents folder yet → nothing to do
    };
    // Real subdir whose NAME passes `name_ok` and whose canonical path stays
    // under canon_root — resolves BOTH symlinks and Windows junctions, so
    // neither can redirect the walk out. The name gate keeps the sweep to the
    // owned `YYYY/MM` layout: the importer only ever stages under dated folders,
    // so an ordinary `Documents/Projects/Client` two-level path must NOT be
    // treated as year/month and have a like-named dot dir swept (Codex review) —
    // the same dated-layout-only discipline capture recovery uses. Returns the
    // CANONICAL path so a real in-place child below canonicalizes to itself
    // (that self-equality is how the leaf tells an owned staging dir apart from a
    // symlink/junction named like one).
    let contained_subdirs = |dir: &Path, name_ok: fn(&str) -> bool| -> Vec<PathBuf> {
        std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.file_name().to_str().is_some_and(name_ok))
            .filter_map(|e| e.path().canonicalize().ok())
            .filter(|c| c.is_dir() && c.starts_with(&canon_root))
            .collect()
    };
    // Documents/<YYYY>/<MM>/.<name>.<unique>.vault-buddy.tmp.import
    for year in contained_subdirs(&canon_root, is_year_dir) {
        for month in contained_subdirs(&year, is_month_dir) {
            let Ok(entries) = std::fs::read_dir(&month) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path(); // <month_canon>/<name>
                let name = entry.file_name().to_string_lossy().into_owned();
                if !is_import_staging_dir(&name) {
                    continue;
                }
                // Delete ONLY a REAL, owned, in-place staging directory — never
                // the target a symlink/junction resolves to (Codex review). An
                // owned staging dir is one we created as a real directory, so
                // it canonicalizes to ITSELF (its parent `month` is already
                // canonical). A symlink/junction named like a staging dir
                // canonicalizes to a DIFFERENT path — the containment check
                // alone would still pass for an in-vault target, so
                // remove_dir_all on the resolved target would erase real vault
                // data. The `canon == path` self-equality test rejects it, and
                // we remove `path` (the entry in place), not any resolved
                // target.
                let Ok(canon) = path.canonicalize() else {
                    continue;
                };
                if canon != path || !canon.is_dir() {
                    continue; // symlink/junction/reparse redirect → skip
                }
                // Staleness: age from mtime, guarding a future mtime (clock jump).
                // symlink_metadata (no-follow) is correct here — path is a real
                // dir, but never resolve a link for the age check either.
                let stale = std::fs::symlink_metadata(&path)
                    .and_then(|m| m.modified())
                    .map(|mtime| match now.duration_since(mtime) {
                        Ok(age) => age >= stale_after,
                        Err(_) => false, // mtime in the future → treat as fresh
                    })
                    .unwrap_or(false);
                if stale {
                    if std::fs::remove_dir_all(&path).is_ok() {
                        sweep.removed.push(path);
                    }
                } else {
                    sweep.pending += 1; // fresh orphan → caller reschedules
                }
            }
        }
    }
    sweep
}

/// Publish a completed staging dir into `target_dir`. Prepends `frontmatter`
/// to the staged note, then moves the media dir (if non-empty) then the note,
/// both at the EXACT names reserved up front (no re-suffixing — the suffix was
/// already resolved by `reserve_basename` and Pandoc pinned the links to it).
/// The note write is non-replacing; on failure the already-published media dir
/// is rolled back. Always cleans the work dir before returning.
pub fn publish(plan: &StagePlan, target_dir: &Path, frontmatter: &str) -> std::io::Result<PathBuf> {
    let result = publish_inner(plan, target_dir, frontmatter);
    cleanup_staging(&plan.work_dir);
    result
}

fn publish_inner(
    plan: &StagePlan,
    target_dir: &Path,
    frontmatter: &str,
) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(target_dir)?;
    let staged_note = plan.work_dir.join(&plan.note_name);
    let body = std::fs::read_to_string(&staged_note)?;
    let full = format!("{frontmatter}{body}");

    // Media first, so the note never resolves to missing images. Only when
    // Pandoc actually extracted something.
    let staged_media = plan.work_dir.join(&plan.media_name);
    let media_has_files = staged_media
        .read_dir()
        .map(|mut it| it.next().is_some())
        .unwrap_or(false);
    let mut published_media: Option<PathBuf> = None;
    if media_has_files {
        let dest = target_dir.join(&plan.media_name);
        // The basename was reserved (note + media dir both free) up front, so
        // dest should be free; a directory here means the name was claimed
        // AFTER reservation — refuse rather than merge/clobber.
        if dest.exists() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                "media directory already exists at destination",
            ));
        }
        std::fs::rename(&staged_media, &dest)?;
        published_media = Some(dest);
        // KNOWN LIMITATION (docs/Gaps.md): if the process is killed / loses
        // power in the ~two-rename window between here and the note write
        // below, the media folder is already published but no note exists, and
        // the startup janitor only sweeps `.vault-buddy.tmp.import` staging
        // dirs — not this published-but-unreferenced media folder. The result
        // is a stray media folder (our OWN extracted files — no user data
        // loss) that a later same-name import suffixes around (` (2)`).
        // Accepted: a crash-atomic fix would need two-phase commit across two
        // filesystem objects (unavailable) or a permanent per-import marker
        // file in every media folder; disproportionate to a microsecond window
        // whose worst case is a cosmetic leftover folder.
    }

    // Write the note at the EXACT reserved name (non-replacing). NOT
    // write_note_collision_safe — re-suffixing here would break Pandoc's
    // baked-in media links. If the name was claimed after reservation the
    // write fails; roll the already-published media dir back so a failed
    // import never leaves an orphaned media folder (Codex review).
    let note_path = target_dir.join(&plan.note_name);
    match write_note_atomic(&note_path, &full) {
        Ok(()) => Ok(note_path),
        Err(e) => {
            if let Some(media) = published_media {
                let _ = std::fs::remove_dir_all(&media);
            }
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn format_from_extension_is_case_insensitive_and_bounded() {
        assert_eq!(DocFormat::from_extension("docx"), Some(DocFormat::Docx));
        assert_eq!(DocFormat::from_extension("DOCX"), Some(DocFormat::Docx));
        assert_eq!(DocFormat::from_extension("odt"), Some(DocFormat::Odt));
        assert_eq!(DocFormat::from_extension("rtf"), Some(DocFormat::Rtf));
        assert_eq!(DocFormat::from_extension("pdf"), None);
        assert_eq!(DocFormat::Docx.reader(), "docx");
    }

    #[test]
    fn basename_is_date_prefixed_original_name() {
        assert_eq!(
            document_basename("Quarterly Report", "2026-07-10"),
            "2026-07-10 Quarterly Report"
        );
    }

    #[test]
    fn frontmatter_quotes_windows_source_path() {
        let meta = DocMeta {
            source_path: r"C:\Users\me\Quarterly Report.docx".into(),
            imported: "2026-07-10".into(),
            format: DocFormat::Docx,
        };
        let fm = render_frontmatter(&meta);
        assert!(fm.starts_with("---\n"));
        assert!(fm.contains("type: Document\n"));
        assert!(fm.contains("tags: [vault-buddy-import]\n"));
        // yaml_quote doubled the backslashes — no raw backslash escape in the scalar.
        assert!(fm.contains(r#"source: "C:\\Users\\me\\Quarterly Report.docx""#));
        // Every string value goes through yaml_quote — even closed-set ones — so
        // they land quoted (invariant: no bare scalars for string fields).
        assert!(fm.contains(r#"imported: "2026-07-10""#));
        assert!(fm.contains(r#"format: "docx""#));
        assert!(fm.trim_end().ends_with("---"));
    }

    #[test]
    fn target_dir_is_documents_folder_dated() {
        let d = target_dir(Path::new("/vault"), "Documents", "2026", "07");
        assert_eq!(d, Path::new("/vault/Documents/2026/07"));
    }

    #[test]
    fn reserve_basename_avoids_both_note_and_media_collisions() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("Documents/2026/07");
        std::fs::create_dir_all(&target).unwrap();
        // base free → returned as-is
        assert_eq!(
            reserve_basename(&target, "2026-07-10 Report"),
            "2026-07-10 Report"
        );
        // a prior note claims the base → next suffix (capture_paths::candidate
        // numbers the first collision (2), matching the capture/tasks scheme)
        std::fs::write(target.join("2026-07-10 Report.md"), "x").unwrap();
        assert_eq!(
            reserve_basename(&target, "2026-07-10 Report"),
            "2026-07-10 Report (2)"
        );
        // a prior MEDIA FOLDER (no note) also forces a suffix — both must be free
        std::fs::create_dir_all(target.join("2026-07-10 Photo")).unwrap();
        assert_eq!(
            reserve_basename(&target, "2026-07-10 Photo"),
            "2026-07-10 Photo (2)"
        );
    }

    #[test]
    fn plan_staging_uses_dot_prefixed_in_vault_workdir() {
        let plan = plan_staging(
            Path::new("/vault/Documents/2026/07"),
            "2026-07-10 Report",
            "t1",
        );
        assert!(plan.work_dir.starts_with("/vault/Documents/2026/07"));
        assert!(plan
            .work_dir
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with('.'));
        // the unique token keeps two same-date imports from sharing a temp dir
        let other = plan_staging(
            Path::new("/vault/Documents/2026/07"),
            "2026-07-10 Report",
            "t2",
        );
        assert_ne!(plan.work_dir, other.work_dir);
        assert_eq!(plan.media_name, "2026-07-10 Report");
        assert_eq!(plan.note_name, "2026-07-10 Report.md");
    }

    // The whole recovery path depends on the janitor recognizing the dirs the
    // importer creates. `plan_staging` mints the name and `is_import_staging_dir`
    // matches it, but the two derive the tail independently — this ties them so
    // changing NOTE_TMP_SUFFIX can't silently orphan every future staging dir.
    #[test]
    fn staging_marker_matches_plan_staging_output() {
        // Derivation lock: the const is the suffix the producer actually appends.
        assert_eq!(STAGING_MARKER, format!("{NOTE_TMP_SUFFIX}.import"));
        // Producer → matcher round-trip: whatever plan_staging mints is matched.
        let plan = plan_staging(
            Path::new("/vault/Documents/2026/07"),
            "2026-07-10 Report",
            "u1",
        );
        let name = plan
            .work_dir
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        assert!(
            is_import_staging_dir(&name),
            "janitor must recognize plan_staging's own output: {name}"
        );
    }

    #[test]
    fn publish_moves_note_and_media_and_prepends_frontmatter() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("Documents/2026/07");
        std::fs::create_dir_all(&target).unwrap();
        let plan = plan_staging(&target, "2026-07-10 Report", "u");
        std::fs::create_dir_all(&plan.work_dir).unwrap();
        // Simulate a Pandoc run: note body + a media dir with one file.
        std::fs::write(
            plan.work_dir.join(&plan.note_name),
            "# Body\n\n![img](2026-07-10 Report/image1.png)\n",
        )
        .unwrap();
        let media = plan.work_dir.join(&plan.media_name);
        std::fs::create_dir_all(&media).unwrap();
        std::fs::write(media.join("image1.png"), b"PNG").unwrap();

        let note = publish(&plan, &target, "---\ntype: Document\n---\n\n").unwrap();
        let published = std::fs::read_to_string(&note).unwrap();
        assert!(published.starts_with("---\ntype: Document\n---\n\n# Body"));
        // media dir landed beside the note, same name → link still resolves
        assert!(target.join("2026-07-10 Report/image1.png").exists());
        // work dir cleaned up
        assert!(!plan.work_dir.exists());
    }

    #[test]
    fn publish_without_media_writes_only_the_note() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("Documents/2026/07");
        std::fs::create_dir_all(&target).unwrap();
        let plan = plan_staging(&target, "2026-07-10 Note", "u");
        std::fs::create_dir_all(&plan.work_dir).unwrap();
        std::fs::write(plan.work_dir.join(&plan.note_name), "# Body\n").unwrap();

        let note = publish(&plan, &target, "---\ntype: Document\n---\n\n").unwrap();
        assert!(note.exists());
        // no media subfolder created when there were no images
        assert!(!target.join("2026-07-10 Note").exists());
    }

    #[test]
    fn publish_rolls_back_media_if_note_commit_fails() {
        // The reserved note name is claimed AFTER reservation (the residual
        // post-reservation race): publish must NOT re-suffix (that would break the
        // media links) — it fails and rolls the already-published media dir back.
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("Documents/2026/07");
        std::fs::create_dir_all(&target).unwrap();
        let plan = plan_staging(&target, "2026-07-10 Doc", "u");
        std::fs::create_dir_all(&plan.work_dir).unwrap();
        std::fs::write(plan.work_dir.join(&plan.note_name), "# Body\n").unwrap();
        let media = plan.work_dir.join(&plan.media_name);
        std::fs::create_dir_all(&media).unwrap();
        std::fs::write(media.join("image1.png"), b"PNG").unwrap();
        // Claim the exact reserved note name so the non-replacing write fails.
        std::fs::write(target.join("2026-07-10 Doc.md"), "SOMEONE ELSE").unwrap();

        let result = publish(&plan, &target, "---\n---\n\n");
        assert!(result.is_err());
        // original untouched (never clobbered)
        assert_eq!(
            std::fs::read_to_string(target.join("2026-07-10 Doc.md")).unwrap(),
            "SOMEONE ELSE"
        );
        // media rolled back — no orphaned sibling folder
        assert!(!target.join("2026-07-10 Doc").exists());
    }

    #[test]
    fn janitor_removes_stale_orphan_staging_dirs_only() {
        use std::time::{Duration, SystemTime};
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("Documents");
        let month = root.join("2026/07");
        std::fs::create_dir_all(&month).unwrap();
        // an orphaned staging dir + a real note + an unrelated dot-dir
        let orphan = month.join(".2026-07-10 Doc.123-4.vault-buddy.tmp.import");
        std::fs::create_dir_all(&orphan).unwrap();
        std::fs::write(orphan.join("partial.md"), "x").unwrap();
        std::fs::write(month.join("2026-07-10 Real.md"), "keep").unwrap();
        let foreign = month.join(".obsidian-cache");
        std::fs::create_dir_all(&foreign).unwrap();

        // now = far future so the orphan is definitely stale
        let now = SystemTime::now() + Duration::from_secs(3600);
        let sweep = clean_stale_staging_at(&root, now, Duration::from_secs(60));
        assert_eq!(sweep.removed.len(), 1);
        assert_eq!(sweep.pending, 0);
        assert!(!orphan.exists()); // owned orphan gone
        assert!(month.join("2026-07-10 Real.md").exists()); // real note kept
        assert!(foreign.exists()); // foreign dot-dir untouched
    }

    #[test]
    fn janitor_ignores_staging_dirs_outside_the_dated_layout() {
        use std::time::{Duration, SystemTime};
        // The importer only stages under Documents/<YYYY>/<MM>. A like-named dot
        // dir in an ordinary two-level path (Documents/Projects/Client) must NOT
        // be treated as year/month and swept.
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("Documents");
        let dated = root.join("2026/07");
        let plain = root.join("Projects/Client");
        std::fs::create_dir_all(&dated).unwrap();
        std::fs::create_dir_all(&plain).unwrap();
        let owned = dated.join(".2026-07-10 Doc.1-2.vault-buddy.tmp.import");
        let outside = plain.join(".x.1-2.vault-buddy.tmp.import");
        std::fs::create_dir_all(&owned).unwrap();
        std::fs::create_dir_all(&outside).unwrap();

        let now = SystemTime::now() + Duration::from_secs(3600);
        let sweep = clean_stale_staging_at(&root, now, Duration::from_secs(60));
        assert_eq!(sweep.removed.len(), 1);
        assert!(!owned.exists()); // dated orphan swept
        assert!(outside.exists()); // non-dated path left alone
    }

    #[test]
    fn janitor_keeps_fresh_staging_dirs() {
        use std::time::{Duration, SystemTime};
        let tmp = tempfile::tempdir().unwrap();
        let month = tmp.path().join("Documents/2026/07");
        std::fs::create_dir_all(&month).unwrap();
        let fresh = month.join(".2026-07-10 Doc.9-9.vault-buddy.tmp.import");
        std::fs::create_dir_all(&fresh).unwrap();
        // now ≈ creation time → not yet stale
        let sweep = clean_stale_staging_at(
            &tmp.path().join("Documents"),
            SystemTime::now(),
            Duration::from_secs(600),
        );
        assert!(sweep.removed.is_empty());
        assert_eq!(sweep.pending, 1); // fresh orphan seen → caller must reschedule
        assert!(fresh.exists());
    }

    // A stale entry NAMED like a staging dir but which is actually a symlink to
    // real vault data must be skipped, and its target left intact — deleting
    // the resolved target would erase vault data even though it's in-vault
    // (Codex review). Unix-only: symlink creation needs the std::os::unix API.
    #[cfg(unix)]
    #[test]
    fn janitor_skips_symlink_named_like_staging_and_keeps_its_target() {
        use std::time::{Duration, SystemTime};
        let tmp = tempfile::tempdir().unwrap();
        let month = tmp.path().join("Documents/2026/07");
        std::fs::create_dir_all(&month).unwrap();
        // Real vault data the symlink points at.
        let real = month.join("Important Notes");
        std::fs::create_dir_all(&real).unwrap();
        std::fs::write(real.join("keep.md"), "precious").unwrap();
        // A symlink whose NAME matches the staging marker, pointing at the data.
        let link = month.join(".evil.9-9.vault-buddy.tmp.import");
        std::os::unix::fs::symlink(&real, &link).unwrap();

        // now = far future so it would be "stale" if the janitor deleted it.
        let sweep = clean_stale_staging_at(
            &tmp.path().join("Documents"),
            SystemTime::now() + Duration::from_secs(3600),
            Duration::from_secs(60),
        );
        assert!(sweep.removed.is_empty()); // nothing deleted
        assert!(real.join("keep.md").exists()); // target data intact
        assert!(link.exists()); // the link itself left alone too
    }

    #[test]
    fn publish_writes_at_the_exact_reserved_name() {
        // publish does NOT suffix — the reserved basename is final so Pandoc's
        // baked-in media links stay valid. A free target writes at the exact name.
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("Documents/2026/07");
        std::fs::create_dir_all(&target).unwrap();
        let plan = plan_staging(&target, "2026-07-10 Note", "u");
        std::fs::create_dir_all(&plan.work_dir).unwrap();
        std::fs::write(plan.work_dir.join(&plan.note_name), "NEW\n").unwrap();

        let note = publish(&plan, &target, "---\n---\n\n").unwrap();
        assert_eq!(note, target.join("2026-07-10 Note.md"));
        assert!(std::fs::read_to_string(&note).unwrap().contains("NEW"));
    }
}
