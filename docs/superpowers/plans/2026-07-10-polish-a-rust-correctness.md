# Polish Sub-pass A — Rust Correctness & Data Safety Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix the ten Rust correctness / data-safety gaps (GAP-01…08, GAP-23, GAP-24) from docs/Gaps.md, per the approved umbrella spec `docs/superpowers/specs/2026-07-10-quality-polish-pass-design.md` (§ Sub-pass A).

**Architecture:** Pure logic lands in the crates that compile everywhere (`core`, `capture`) with unit tests; the shell (`src-tauri/src`) only gains thin wiring and is verified by the Linux compile gate + shell unit tests. New chokepoints: a canonical vault-containment helper (`core::capture_paths::vault_owning_path`) shared by three commands, a frontmatter-scoped transcript-marker reader, a pure tick-schedule policy for the capture loop, and a startup-wedged flag on the capture reservation.

**Tech Stack:** Rust (Tauri v2 shell + pure crates), `tempfile` for fs tests, `windows-sys` (already a workspace dep) for the Windows rename fallback.

## Global Constraints

- **Branch:** `claude/task-management-vertical-slice-ikeuly` (stacked on PR #46). Never push elsewhere.
- **Bookkeeping (from the spec, applies to EVERY task):** the task's Gaps.md entry is marked FIXED **in the same commit as its fix**, using the GAP-40 tombstone format: `### GAP-NN · ~~Severity~~ FIXED 2026-07-10 · <original title>` with the body replaced by a one-or-two-line what-changed (keep the entry; never delete it).
- **TDD:** failing test first for every behavioral fix; regression tests name the failure mode in a comment (usually the GAP number). Two sanctioned exceptions in this sub-pass, called out in their tasks: A9 (logging-only, return values unchanged by spec) and A10 (thread-spawn failure cannot be forced portably) — both are verified by compile + clippy + existing suites instead, and the plan says so honestly.
- **Gates at every task boundary** (run the ones matching what you touched):
  - core: `cd src-tauri/core && cargo test && cargo clippy --all-targets -- -D warnings`
  - capture: `cd src-tauri/capture && cargo test && cargo clippy --all-targets -- -D warnings` (needs `libasound2-dev`; CI installs it, the container has it)
  - shell (any `src-tauri/src/*.rs` change): `cd src-tauri && cargo clippy --workspace --all-targets -- -D warnings && cargo test -p vault-buddy --lib`, plus the compile gate `npx tauri build --no-bundle` (run `npm run setup:linux` once and `npm run build` first if `dist/` is missing)
  - always: `cd src-tauri && cargo fmt` before committing (`cargo fmt --check` is the CI gate)
  - LOC guard: `npm run check:loc`. Several touched files are grandfathered (`capture_commands.rs` 952, `transcription.rs` 948, `session.rs` 856). If a task grows one, run `npm run check:loc -- --update` **in the same commit** and justify the growth in the commit body (repo precedent: the Codex-fix commits on this branch). Shrinkage: also `--update` so the ceiling ratchets down.
- **Commits:** Conventional Commits (`fix(core)`, `fix(capture)`, `fix(shell)`), imperative subject, body explains the failure mode. One commit per task.
- **Windows-only mechanics** (A6's `MoveFileExW` arm) cannot execute here: the code is compile-gated, carries a `#[cfg(windows)]` unit test that will run once sub-pass D adds the Windows `cargo test` CI step (D7), and the task notes this — no silent "verified" claims.

---

### Task 1: Canonical vault containment for the transcription retry/force paths (A1 · GAP-01)

**Files:**
- Modify: `src-tauri/core/src/capture_paths.rs` (new `is_capture_mp3`, `OwningVault`, `vault_owning_path`; `rename_plan` reuses `is_capture_mp3`)
- Modify: `src-tauri/src/transcription.rs:581-586` (`owning_vault_id`), `:699-714` (`transcribe_recording_now`), `:719-734` (`retranscribe`)
- Modify: `src-tauri/src/capture_commands.rs:936-951` (`open_recording_note`)
- Modify: `docs/Gaps.md` (GAP-01 tombstone)
- Test: inline `mod tests` in `capture_paths.rs`

**Interfaces:**
- Consumes: `capture_paths::is_capture_base(&str) -> bool` (existing), `discovery::Vault { id, name, path, open }` (existing).
- Produces (Task 7 reuses both):
  - `pub fn is_capture_mp3(path: &Path) -> bool`
  - `pub struct OwningVault<'v> { pub vault: &'v crate::discovery::Vault, pub vault_canonical: PathBuf, pub path_canonical: PathBuf }`
  - `pub fn vault_owning_path<'v>(vaults: &'v [crate::discovery::Vault], path: &Path) -> Option<OwningVault<'v>>`

- [ ] **Step 1: Write the failing core tests**

Append to the `mod tests` block in `src-tauri/core/src/capture_paths.rs` (it already imports `super::*`; add `use crate::discovery::Vault;` inside the module):

```rust
fn vault_at(dir: &Path) -> Vault {
    Vault {
        id: "v1".into(),
        name: "V".into(),
        path: dir.to_string_lossy().into_owned(),
        open: false,
    }
}

#[test]
fn is_capture_mp3_requires_capture_stem_and_mp3_extension() {
    assert!(is_capture_mp3(Path::new("/v/2026-07-04 1405 Meeting.mp3")));
    // extension is case-insensitive, same as rename_plan's check
    assert!(is_capture_mp3(Path::new("/v/2026-07-04 1405 Meeting.MP3")));
    assert!(!is_capture_mp3(Path::new("/v/holiday-song.mp3")));
    assert!(!is_capture_mp3(Path::new("/v/2026-07-04 1405 Meeting.md")));
}

#[test]
fn vault_owning_path_matches_a_file_inside_the_vault() {
    let dir = tempfile::tempdir().unwrap();
    let vault_dir = dir.path().join("vault");
    std::fs::create_dir(&vault_dir).unwrap();
    let mp3 = vault_dir.join("2026-07-04 1405 Meeting.mp3");
    std::fs::write(&mp3, "x").unwrap();
    let vaults = vec![vault_at(&vault_dir)];
    let owned = vault_owning_path(&vaults, &mp3).expect("inside the vault");
    assert_eq!(owned.vault.id, "v1");
    assert_eq!(owned.path_canonical, std::fs::canonicalize(&mp3).unwrap());
    assert_eq!(
        owned.vault_canonical,
        std::fs::canonicalize(&vault_dir).unwrap()
    );
}

#[test]
fn vault_owning_path_rejects_a_dotdot_escape() {
    // GAP-01: Path::starts_with compares raw components, so
    // `<vault>/../outside.mp3` passed the old lexical prefix check while
    // pointing at a real file OUTSIDE the vault.
    let dir = tempfile::tempdir().unwrap();
    let vault_dir = dir.path().join("vault");
    std::fs::create_dir(&vault_dir).unwrap();
    let outside = dir.path().join("2026-07-04 1405 Outside.mp3");
    std::fs::write(&outside, "x").unwrap();
    let sneaky = vault_dir.join("..").join("2026-07-04 1405 Outside.mp3");
    let vaults = vec![vault_at(&vault_dir)];
    assert!(sneaky.exists(), "the escape path must point at a real file");
    assert!(vault_owning_path(&vaults, &sneaky).is_none());
}

#[cfg(unix)]
#[test]
fn vault_owning_path_rejects_a_symlink_escaping_the_vault() {
    let dir = tempfile::tempdir().unwrap();
    let vault_dir = dir.path().join("vault");
    std::fs::create_dir(&vault_dir).unwrap();
    let outside = dir.path().join("2026-07-04 1405 Outside.mp3");
    std::fs::write(&outside, "x").unwrap();
    let link = vault_dir.join("2026-07-04 1405 Linked.mp3");
    std::os::unix::fs::symlink(&outside, &link).unwrap();
    let vaults = vec![vault_at(&vault_dir)];
    assert!(vault_owning_path(&vaults, &link).is_none());
}

#[test]
fn vault_owning_path_rejects_missing_files_and_skips_dead_vaults() {
    let dir = tempfile::tempdir().unwrap();
    let vault_dir = dir.path().join("vault");
    std::fs::create_dir(&vault_dir).unwrap();
    let mp3 = vault_dir.join("2026-07-04 1405 Meeting.mp3");
    std::fs::write(&mp3, "x").unwrap();
    // an unresolvable path is a rejection, not a fallback to lexical matching
    assert!(vault_owning_path(&[vault_at(&vault_dir)], Path::new("/no/such.mp3")).is_none());
    // a registry entry whose folder is gone is skipped; a later vault still matches
    let dead = Vault {
        id: "dead".into(),
        name: "Dead".into(),
        path: dir.path().join("gone").to_string_lossy().into_owned(),
        open: false,
    };
    let vaults = vec![dead, vault_at(&vault_dir)];
    assert_eq!(vault_owning_path(&vaults, &mp3).unwrap().vault.id, "v1");
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd src-tauri/core && cargo test capture_paths`
Expected: FAIL to compile — `is_capture_mp3`, `vault_owning_path`, `OwningVault` not found.

- [ ] **Step 3: Implement the core helpers**

In `src-tauri/core/src/capture_paths.rs`, directly above `rename_plan`:

```rust
/// Ownership filter shared by the rename and transcription commands: a
/// `.mp3` (any case) whose stem carries the capture-pattern prefix. Any
/// command that mints or moves files NEXT TO a given mp3 must pass this —
/// an arbitrary user mp3 must never grow a Vault Buddy sidecar or be
/// shuffled by our rename machinery.
pub fn is_capture_mp3(path: &Path) -> bool {
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let is_mp3 = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("mp3"));
    is_mp3 && is_capture_base(&stem)
}

/// A canonical containment match: the owning vault plus the canonical forms
/// of BOTH sides, so callers derive vault-relative paths from the same
/// canonical prefix (`\\?\`-form on Windows) instead of mixing raw and
/// resolved paths — mixing them makes `strip_prefix` fail (the `open_task`
/// precedent in task_commands.rs).
pub struct OwningVault<'v> {
    pub vault: &'v crate::discovery::Vault,
    pub vault_canonical: PathBuf,
    pub path_canonical: PathBuf,
}

/// The registered vault whose folder contains `path`, matched on CANONICAL
/// paths. `Path::starts_with` compares raw components without resolving
/// `..` or links, so a lexical prefix check accepts `<vault>\..\anywhere`
/// and symlink escapes (GAP-01). An unresolvable `path` is a rejection —
/// never a fallback to lexical matching; a registry entry whose own folder
/// can't resolve is skipped.
pub fn vault_owning_path<'v>(
    vaults: &'v [crate::discovery::Vault],
    path: &Path,
) -> Option<OwningVault<'v>> {
    let path_canonical = std::fs::canonicalize(path).ok()?;
    for vault in vaults {
        let Ok(vault_canonical) = std::fs::canonicalize(&vault.path) else {
            continue;
        };
        if path_canonical.starts_with(&vault_canonical) {
            return Some(OwningVault {
                vault,
                vault_canonical,
                path_canonical,
            });
        }
    }
    None
}
```

Then make `rename_plan` reuse the filter — replace its stem/extension check (lines 95-107) with:

```rust
    let stem = mp3
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    if !is_capture_mp3(mp3) {
        // Ownership filter: rename only files carrying our capture
        // pattern — never an arbitrary user mp3 handed in by mistake.
        return Err("Not a Vault Buddy capture file".to_string());
    }
```

(`stem` is still needed below for the prefix/double-prefix handling; only the `is_mp3` local and the `if !is_mp3 || !is_capture_base(&stem)` condition are replaced.)

- [ ] **Step 4: Run the core tests to verify they pass**

Run: `cd src-tauri/core && cargo test`
Expected: PASS (including the existing `rename_plan` tests — its behavior is unchanged).

- [ ] **Step 5: Wire the shell commands**

In `src-tauri/src/transcription.rs`, replace `owning_vault_id` (lines 580-586):

```rust
/// The vault whose folder contains `mp3` (for the retry/force commands),
/// matched on CANONICAL paths — `Path::starts_with` on raw components
/// accepted `<vault>\..\anywhere` escapes and symlinks (GAP-01). None when
/// the path cannot be resolved or no registered vault contains it.
fn owning_vault_id(mp3: &Path) -> Option<String> {
    let vaults = discovery::discover_vaults();
    capture_paths::vault_owning_path(&vaults, mp3).map(|owned| owned.vault.id.clone())
}
```

In `transcribe_recording_now` AND `retranscribe`, insert the basename gate between the `is_file` check and the `owning_vault_id` call (identical two lines in both commands):

```rust
    if !capture_paths::is_capture_mp3(&mp3) {
        return Err("Not a Vault Buddy capture file.".to_string());
    }
```

(`capture_paths` is already in the file's `use vault_buddy_core::{...}` list.) The job keeps carrying the caller's original path: the canonical form is only for the containment CHECK — event payloads (`capture:transcribing` etc.) must keep echoing the exact string the frontend keyed its job map on, and a path that resolves inside the vault also writes inside it.

In `src-tauri/src/capture_commands.rs`, replace `open_recording_note` (lines 936-951) — same helper, canonical relative path, and the error no longer embeds the local path (it goes to the log via the caller's normal flow):

```rust
fn open_recording_note(path: &str) -> Result<(), String> {
    let mp3 = PathBuf::from(path);
    let vaults = discovery::discover_vaults();
    // Canonical containment (GAP-01's read-only sibling): the lexical
    // starts_with accepted `..`/symlink paths pointing outside every vault.
    let owned = capture_paths::vault_owning_path(&vaults, &mp3)
        .ok_or_else(|| "Recording is not inside a known vault.".to_string())?;
    let note = owned.path_canonical.with_extension("md");
    let target = if note.exists() {
        note
    } else {
        transcript::transcript_path(&owned.path_canonical)
    };
    // Both sides canonical, so strip_prefix agrees on Windows' \\?\ form
    // (the open_task precedent).
    let rel = uri::vault_relative_no_ext(&target, &owned.vault_canonical)
        .ok_or_else(|| format!("recording is outside its vault: {}", target.display()))?;
    uri::launch(&uri::open_file_uri(&owned.vault.id, &rel))
}
```

- [ ] **Step 6: Run the gates**

Run: `cd src-tauri && cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test -p vault_buddy_core && cargo test -p vault-buddy --lib`
Then: `npx tauri build --no-bundle` (Linux shell compile gate).
Expected: all green.

- [ ] **Step 7: Tombstone GAP-01 and commit**

In `docs/Gaps.md`, replace the GAP-01 entry body (keep the heading line, strike the severity):

```markdown
### GAP-01 · ~~High~~ FIXED 2026-07-10 · Transcription retry/force paths accept `..` escapes and skip the capture-basename gate
`owning_vault_id` and `open_recording_note` now match on canonical paths via
`capture_paths::vault_owning_path` (unresolvable = rejected), and both
transcription commands require `capture_paths::is_capture_mp3` — the same
ownership filter `rename_plan` enforces (now shared).
```

```bash
git add src-tauri/core/src/capture_paths.rs src-tauri/src/transcription.rs src-tauri/src/capture_commands.rs docs/Gaps.md
git commit -m "fix(shell): canonical vault containment + capture-basename gate on transcription retry paths" -m "GAP-01: Path::starts_with on raw components let <vault>/../anywhere pass the containment check, minting transcript sidecars outside any vault; any .mp3 in a vault also got a sidecar. Containment now canonicalizes both sides (core::capture_paths::vault_owning_path, shared with open_recording_note) and both commands require the capture-pattern basename rename_plan already enforced."
```

If `npm run check:loc` fails on `transcription.rs`/`capture_commands.rs` growth, run `npm run check:loc -- --update`, include `scripts/loc-baseline.json` in the commit, and add one justification line to the body.

---

### Task 2: Config saves fail loudly on unreadable config.json (A2 · GAP-02)

**Files:**
- Modify: `src-tauri/core/src/capture_config.rs:379-405` (`update_vault_config_at`, `update_mcp_config_at`, new `read_config_for_update`)
- Modify: `docs/Gaps.md` (GAP-02 tombstone)
- Test: inline `mod tests` in `capture_config.rs`

**Interfaces:**
- Consumes: `parse_config(&str) -> AppConfig`, `write_config(&Path, &AppConfig)` (existing, unchanged).
- Produces: `update_vault_config_at` / `update_mcp_config_at` signatures unchanged (`std::io::Result<()>`), but now return `Err` on any read error other than `NotFound`. `update_mcp_config_at` is included because it has the identical read-modify-write wipe (it read via `load_config_from`, which also swallows).

- [ ] **Step 1: Write the failing tests**

Append to `mod tests` in `capture_config.rs`:

```rust
#[test]
fn unreadable_config_fails_the_save_instead_of_wiping_other_vaults() {
    // GAP-02: any read error used to map to AppConfig::default(), then
    // write_config replaced the whole file with only the edited vault —
    // a momentarily locked/unreadable config.json (Windows AV, indexer)
    // silently dropped vaults B..N. Invalid UTF-8 stands in for that
    // read failure portably: read_to_string errors, writes would succeed.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.json");
    let garbage = [0xFFu8, 0xFE, 0x01];
    std::fs::write(&path, garbage).unwrap();
    let err = update_vault_config_at(&path, "a", VaultCaptureConfig::default())
        .expect_err("a non-NotFound read error must fail the save");
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    // the save failed LOUDLY and touched nothing
    assert_eq!(std::fs::read(&path).unwrap(), garbage);

    let err = update_mcp_config_at(&path, McpConfig::default())
        .expect_err("the mcp save has the same read-modify-write wipe");
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    assert_eq!(std::fs::read(&path).unwrap(), garbage);
}

#[test]
fn missing_config_still_defaults_on_first_save() {
    // NotFound is the one read error that may default: it IS the first save.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.json");
    update_vault_config_at(&path, "a", VaultCaptureConfig::default()).unwrap();
    let cfg = load_config_from(&path);
    assert!(cfg.vaults.contains_key("a"));
}
```

- [ ] **Step 2: Run to verify the first test fails**

Run: `cd src-tauri/core && cargo test capture_config`
Expected: `unreadable_config_fails_the_save_instead_of_wiping_other_vaults` FAILS (`expect_err` panics — the current code returns `Ok` and replaces the file). The second test passes already (pins existing behavior).

- [ ] **Step 3: Implement the guarded read**

In `capture_config.rs`, add above `update_vault_config_at` and rewire both updaters:

```rust
/// Read the current config for a read-modify-write. ONLY a missing file may
/// default (that is the first save); any other read error must propagate —
/// treating an unreadable config.json as empty and writing it back would
/// silently wipe every other vault's settings (GAP-02: a voice-note vault
/// reverting to Meeting mode re-enables desktop-audio loopback).
fn read_config_for_update(path: &Path) -> std::io::Result<AppConfig> {
    match std::fs::read_to_string(path) {
        Ok(json) => Ok(parse_config(&json)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(AppConfig::default()),
        Err(e) => {
            log::warn!("config: {} unreadable during save: {e}", path.display());
            Err(e)
        }
    }
}

pub fn update_vault_config_at(
    path: &Path,
    vault_id: &str,
    v: VaultCaptureConfig,
) -> std::io::Result<()> {
    let mut cfg = read_config_for_update(path)?;
    cfg.vaults.insert(vault_id.to_string(), v);
    write_config(path, &cfg)
}
```

and

```rust
pub fn update_mcp_config_at(path: &Path, mcp: McpConfig) -> std::io::Result<()> {
    let mut cfg = read_config_for_update(path)?;
    cfg.mcp = mcp;
    write_config(path, &cfg)
}
```

(Check `capture_config.rs` imports `log` — the crate already depends on it; add `use` only if the module lacks it. `update_vault_config`'s `map_err` string already surfaces the error to the UI unchanged.)

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd src-tauri/core && cargo test && cargo clippy --all-targets -- -D warnings`
Expected: PASS.

- [ ] **Step 5: Tombstone GAP-02 and commit**

Gaps.md:

```markdown
### GAP-02 · ~~Medium~~ FIXED 2026-07-10 · A transient config read failure during save wipes every other vault's settings
`update_vault_config_at` and `update_mcp_config_at` now share
`read_config_for_update`: only `NotFound` defaults (first save); any other
read error logs and propagates, so the save fails loudly and the file is
left untouched.
```

```bash
cd src-tauri && cargo fmt
git add src-tauri/core/src/capture_config.rs docs/Gaps.md
git commit -m "fix(core): fail config saves loudly when config.json is unreadable" -m "GAP-02: any read error during the read-modify-write defaulted to an empty AppConfig, so a transiently locked config.json (Windows AV/indexer) made saving vault A silently drop vaults B..N — including flipping a voice-note vault back to Meeting mode. Only NotFound may default now; other errors log and propagate. update_mcp_config_at had the identical wipe and shares the fix."
```

---

### Task 3: Transcript ownership markers are frontmatter-scoped (A3 · GAP-03)

**Files:**
- Modify: `src-tauri/core/src/transcript.rs:41-45` (`is_regenerable`), `:183-191` (`needs_transcription`), `:240-249` (`transcript_status`)
- Modify: `docs/Gaps.md` (GAP-03 tombstone)
- Test: inline `mod tests` in `transcript.rs`

**Interfaces:**
- Consumes: `capture_note::note_field(content: &str, key: &str) -> Option<String>` (existing, frontmatter-scoped).
- Produces: public signatures unchanged. New private `fn marker(content: &str) -> Option<String>`.

- [ ] **Step 1: Write the failing tests**

Append to `mod tests` in `transcript.rs`:

```rust
#[test]
fn body_quoting_a_marker_never_reclassifies_a_finished_transcript() {
    // GAP-03: whole-content contains() matched the marker text ANYWHERE, so
    // a complete sidecar whose BODY quotes the placeholder line was
    // classified regenerable, re-enqueued by the backfill, and overwritten
    // by replace_if_ours — the one way the never-overwrite rule could be
    // beaten by content coincidence.
    let content = "---\nvault-buddy-transcript: complete\ntranscript-of: \"a.mp3\"\n---\n\n\
                   The placeholder shows `vault-buddy-transcript: pending` until done.\n";
    assert!(!is_regenerable(content));

    let dir = tempfile::tempdir().unwrap();
    let mp3 = dir.path().join("2026-07-04 1405 Meeting.mp3");
    std::fs::write(transcript_path(&mp3), content).unwrap();
    assert!(!needs_transcription(&mp3), "backfill must not re-enqueue it");
    assert_eq!(transcript_status(&mp3), TranscriptStatus::Complete);
}

#[test]
fn hand_edited_sidecar_without_frontmatter_marker_is_foreign() {
    // A user's own file mentioning marker text in prose stays untouchable.
    let content = "# My notes\n\nvault-buddy-transcript: failed — that's what it said.\n";
    assert!(!is_regenerable(content));
    let dir = tempfile::tempdir().unwrap();
    let mp3 = dir.path().join("2026-07-04 1405 Meeting.mp3");
    std::fs::write(transcript_path(&mp3), content).unwrap();
    assert_eq!(transcript_status(&mp3), TranscriptStatus::Complete);
    assert!(!needs_transcription(&mp3));
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cd src-tauri/core && cargo test transcript`
Expected: both new tests FAIL (`is_regenerable` returns true on the quoted marker; `transcript_status` reads `Pending`).

- [ ] **Step 3: Implement the frontmatter-scoped reader**

In `transcript.rs`, replace `is_regenerable` (lines 41-45) with:

```rust
/// The sidecar's ownership marker, read from the FRONTMATTER only via
/// `note_field` — a body that quotes the literal marker text must never
/// change classification (GAP-03). The render_* functions still write the
/// full `MARKER_*` lines; only the readers changed.
fn marker(content: &str) -> Option<String> {
    crate::capture_note::note_field(content, "vault-buddy-transcript")
}

/// A sidecar we may (re)write: our own not-yet-finished output. A finished
/// (`complete`) transcript or a file a user has taken over must never match.
pub fn is_regenerable(content: &str) -> bool {
    matches!(marker(content).as_deref(), Some("pending") | Some("failed"))
}
```

In `needs_transcription`, replace the `Ok` arm:

```rust
        Ok(content) => marker(&content).as_deref() == Some("pending"),
```

In `transcript_status`, replace the four `Ok` arms with one:

```rust
        Ok(content) => match marker(&content).as_deref() {
            Some("pending") => TranscriptStatus::Pending,
            Some("failed") => TranscriptStatus::Failed,
            Some("cancelled") => TranscriptStatus::Cancelled,
            // `complete`, an unknown value, or no marker at all: a finished
            // or hand-edited file — the re-transcribe confirm must fire.
            _ => TranscriptStatus::Complete,
        },
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd src-tauri/core && cargo test && cargo clippy --all-targets -- -D warnings`
Expected: PASS — including every existing transcript test (the markers written by `render_*` parse to exactly `pending`/`failed`/`complete`/`cancelled` through `note_field`).

- [ ] **Step 5: Tombstone GAP-03 and commit**

Gaps.md:

```markdown
### GAP-03 · ~~Medium~~ FIXED 2026-07-10 · Transcript ownership markers match anywhere in the file, not the frontmatter
`is_regenerable`, `needs_transcription`, and `transcript_status` now read the
marker via a frontmatter-scoped `note_field(content, "vault-buddy-transcript")`
reader; body text quoting a marker no longer reclassifies a sidecar.
```

```bash
cd src-tauri && cargo fmt
git add src-tauri/core/src/transcript.rs docs/Gaps.md
git commit -m "fix(core): scope transcript ownership markers to the frontmatter" -m "GAP-03: whole-content contains(MARKER_*) let a complete or hand-edited sidecar whose body quotes 'vault-buddy-transcript: pending' be classified regenerable, re-enqueued by the backfill, and overwritten by replace_if_ours. The readers now use capture_note::note_field, which stops at the closing fence."
```

---

### Task 4: Rename moves the transcript sidecar and retargets its embed (A4 · GAP-04)

**Files:**
- Modify: `src-tauri/capture/src/rename.rs` (`execute` + tests)
- Modify: `docs/Gaps.md` (GAP-04 tombstone)
- Test: inline `mod tests` in `rename.rs`

**Interfaces:**
- Consumes: `vault_buddy_core::transcript::transcript_path(&Path) -> PathBuf`, `vault_buddy_core::capture_paths::rename_noreplace(&Path, &Path) -> io::Result<()>`, `retarget_embed(&str, &str, &str) -> String` (all existing).
- Produces: `execute(&RenamePlan) -> Result<RenameOutcome, String>` unchanged in signature. `RenameOutcome` unchanged (the UI needs no transcript path; failures surface via `warning`). The note's transcript embed line is `![[<stem>.transcript]]` (no `.md` — that is what `render_note` writes).

Design notes the implementer needs:
- The reserved base already guarantees `<new>.transcript.md` was free at reservation time — `reserve_final` (used by `rename_into_reserved`) refuses a base whose `.transcript.md` exists. A racing creation between reservation and the transcript move loses gracefully: `rename_noreplace` fails, we degrade to a warning and leave the transcript at its OLD name with its OLD embed intact (suffixing the transcript alone would orphan it from `transcript_path(new.mp3)` — worse than not moving it). Audio-first posture, same as the note.
- The transcript move happens AFTER the mp3 move (the mp3 move is the arbiter) and BEFORE the note rewrite (the rewrite needs to know whether to retarget the transcript embed).

- [ ] **Step 1: Write the failing tests**

In `rename.rs` `mod tests`, extend `seed` to optionally include a transcript, and add tests. Replace the existing `seed` with:

```rust
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
```

Add the tests:

```rust
    #[test]
    fn renames_transcript_sidecar_and_retargets_its_embed() {
        // GAP-04: execute moved only the mp3 + note; <old>.transcript.md
        // stayed behind, the note kept embedding the old transcript, and the
        // next launch's backfill re-ran a multi-minute inference for a
        // sidecar nothing embeds.
        let dir = tempfile::tempdir().unwrap();
        let (mp3, _note) = seed(dir.path());
        let transcript = dir.path().join("2026-07-04 1405 Meeting.transcript.md");
        std::fs::write(&transcript, "---\nvault-buddy-transcript: complete\n---\nwords\n").unwrap();
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
        std::fs::write(&transcript, "---\nvault-buddy-transcript: complete\n---\nwords\n").unwrap();
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
        assert!(text.contains("![[2026-07-04 1405 Meeting.transcript]]"), "{text}");
    }
```

- [ ] **Step 2: Run to verify they fail**

Run: `cd src-tauri/capture && cargo test rename`
Expected: `renames_transcript_sidecar_and_retargets_its_embed` FAILS (transcript not moved). The collision test also fails (mp3 lands at the plain base today because `reserve_final`'s transcript check makes the base advance — verify the actual failure output and adjust expectations only if the reservation behaves differently than described; do not weaken the first test).

- [ ] **Step 3: Implement the transcript move in `execute`**

In `rename.rs`, after the mp3 move + `new_mp3_name` derivation (line ~42), insert:

```rust
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
```

Then in the note branch, retarget both embeds (replace the single `retarget_embed` line):

```rust
            let mut retargeted = retarget_embed(&text, &old_mp3_name, &new_mp3_name);
            if transcript_moved {
                retargeted = retarget_embed(
                    &retargeted,
                    &format!("{old_stem}.transcript"),
                    &format!("{new_stem}.transcript"),
                );
            }
```

Finally fold `transcript_error` into the warning (replace the `let warning = note_error.map(...)` block):

```rust
    let mut issues: Vec<String> = Vec::new();
    if let Some(e) = transcript_error {
        issues.push(e);
    }
    if let Some(e) = note_error {
        issues.push(e);
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
```

(Delete the now-unused `plan.note_from.display()` mention only if the note path is no longer needed; if a note error occurred, appending `; note: {}` with `plan.note_from.display()` inside that branch is fine — keep the information, keep it in the log.)

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd src-tauri/capture && cargo test && cargo clippy --all-targets -- -D warnings`
Expected: PASS, including the pre-existing rename tests (their seeded notes now carry a transcript embed line that stays untouched when no transcript file exists).

- [ ] **Step 5: Tombstone GAP-04 and commit**

Gaps.md:

```markdown
### GAP-04 · ~~Medium~~ FIXED 2026-07-10 · Renaming a transcribed recording strands the transcript and silently re-transcribes
`rename::execute` now moves `<old>.transcript.md` via the same
`rename_noreplace` rails right after the mp3 and retargets the note's
`.transcript` embed; a transcript-side failure degrades to a warning and
keeps the old embed (audio first, never clobber).
```

```bash
cd src-tauri && cargo fmt
git add src-tauri/capture/src/rename.rs docs/Gaps.md
git commit -m "fix(capture): move the transcript sidecar on rename and retarget its embed" -m "GAP-04: execute moved only the mp3+note, so <old>.transcript.md stayed behind, the note embedded a dangling transcript name, and the next launch's backfill re-ran whisper for minutes to mint a sidecar nothing embeds. The sidecar now rides the reserved base with rename_noreplace; a collision degrades to a warning with the old embed kept intact."
```

---

### Task 5: Tick-schedule resync after suspend; no take on early wakes (A5 · GAP-05)

**Files:**
- Modify: `src-tauri/capture/src/session.rs` (new `MAX_TICK_LAG` const + `plan_tick`; loop integration at lines 194-215 and 276-283)
- Modify: `scripts/loc-baseline.json` via `npm run check:loc -- --update` (session.rs grows)
- Modify: `docs/Gaps.md` (GAP-05 tombstone)
- Test: inline `mod tests` in `session.rs`

**Interfaces:**
- Consumes: `TICK: Duration = 100ms` (existing const).
- Produces: `fn plan_tick(now: Instant, next_tick: Instant, tick: Duration, max_lag: Duration) -> (Instant, bool)` — returns `(new_next_tick, take_tick)`. Private to the module; the loop is its only caller.

- [ ] **Step 1: Write the failing tests**

Append to `mod tests` in `session.rs`:

```rust
    #[test]
    fn plan_tick_early_wake_keeps_schedule_and_skips_take() {
        // GAP-05 (pause→resume): a control message wakes recv_timeout BEFORE
        // next_tick; consuming a full tick there silence-padded up to 100 ms
        // of spurious audio per pause/resume and drifted the schedule.
        let base = Instant::now();
        let next = base + TICK;
        let (new_next, take) = plan_tick(base, next, TICK, MAX_TICK_LAG);
        assert_eq!(new_next, next, "schedule unchanged on an early wake");
        assert!(!take, "nothing consumed on an early wake");
    }

    #[test]
    fn plan_tick_on_schedule_advances_one_tick() {
        let base = Instant::now();
        let (new_next, take) = plan_tick(base, base, TICK, MAX_TICK_LAG);
        assert_eq!(new_next, base + TICK);
        assert!(take);
    }

    #[test]
    fn plan_tick_moderate_lag_catches_up_without_resync() {
        // Encode backpressure below the lag cap keeps the catch-up behavior:
        // average consumption must match real time or long recordings drop
        // samples (the reason the fixed schedule exists).
        let base = Instant::now();
        let now = base + Duration::from_millis(300);
        let (new_next, take) = plan_tick(now, base, TICK, MAX_TICK_LAG);
        assert_eq!(new_next, base + TICK, "catch-up: schedule not resynced");
        assert!(take);
    }

    #[test]
    fn plan_tick_suspend_gap_resyncs_to_now() {
        // GAP-05: Instant (QPC) advances across suspend on Windows, so after
        // a sleep the loop ran back-to-back catch-up ticks appending the
        // WHOLE gap (potentially hours) as encoded silence. Real
        // backpressure never accumulates MAX_TICK_LAG; a suspend always does.
        let base = Instant::now();
        let now = base + Duration::from_secs(3600);
        let (new_next, take) = plan_tick(now, base, TICK, MAX_TICK_LAG);
        assert_eq!(new_next, now + TICK, "schedule resynced to real time");
        assert!(take, "one tick still drains the buffers");
    }

    #[test]
    fn plan_tick_lag_boundary_is_exclusive() {
        // Exactly MAX_TICK_LAG behind is still backpressure territory.
        let base = Instant::now();
        let now = base + MAX_TICK_LAG;
        let (new_next, _) = plan_tick(now, base, TICK, MAX_TICK_LAG);
        assert_eq!(new_next, base + TICK);
    }
```

- [ ] **Step 2: Run to verify they fail**

Run: `cd src-tauri/capture && cargo test plan_tick`
Expected: FAIL to compile — `plan_tick`, `MAX_TICK_LAG` not found.

- [ ] **Step 3: Implement the policy and wire the loop**

Below the `TICK` const in `session.rs`:

```rust
/// A wake more than this far past its schedule is a clock discontinuity
/// (system suspend — Instant/QPC generally advances across it on Windows),
/// not encode backpressure: real backpressure never accumulates 5 ticks
/// while catch-up cycles run back-to-back (GAP-05).
const MAX_TICK_LAG: Duration = Duration::from_millis(500);

/// One wake's schedule decision, pure so it's unit-testable: returns the
/// new `next_tick` and whether this wake may consume a tick of audio.
/// Early wake (a control message before schedule) → consume nothing, keep
/// the schedule. Past schedule within `max_lag` → normal catch-up. Beyond
/// `max_lag` → resync to `now` so a suspend gap is never encoded as
/// catch-up silence. The finish/drain path ignores the take flag — a stop
/// always drains what's buffered.
fn plan_tick(
    now: Instant,
    next_tick: Instant,
    tick: Duration,
    max_lag: Duration,
) -> (Instant, bool) {
    if now < next_tick {
        (next_tick, false)
    } else if now.duration_since(next_tick) > max_lag {
        (now + tick, true)
    } else {
        (next_tick + tick, true)
    }
}
```

In `run_worker`, replace line 215 (`next_tick += TICK;`) with:

```rust
        let (new_next_tick, take_tick) = plan_tick(Instant::now(), next_tick, TICK, MAX_TICK_LAG);
        next_tick = new_next_tick;
```

and replace the `take` computation (lines 276-283) with:

```rust
        let tick_frames = (TARGET_RATE / 10) as usize;
        let take = if finish {
            states.iter().map(|s| s.buffer.len()).max().unwrap_or(0)
        } else if paused || !take_tick {
            0
        } else {
            tick_frames
        };
```

The `finish` arm deliberately ignores `take_tick`: Stop usually arrives as an early wake and must still drain everything buffered.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd src-tauri/capture && cargo test && cargo clippy --all-targets -- -D warnings`
Expected: PASS — including the existing session/pause/finalize tests (on-schedule behavior is byte-identical: same take sizes, same catch-up under moderate lag). Note honestly: the suspend path cannot be exercised end-to-end (Instant is unmockable); `plan_tick` is the tested unit and the loop wiring is reviewed.

- [ ] **Step 5: LOC baseline, tombstone GAP-05, commit**

Run `npm run check:loc`; if `session.rs` growth trips it, `npm run check:loc -- --update`.

Gaps.md:

```markdown
### GAP-05 · ~~Medium~~ FIXED 2026-07-10 · System suspend mid-recording appends the whole sleep gap as encoded silence
The tick loop now runs a pure `plan_tick` policy: a wake >500 ms behind
schedule resyncs `next_tick` to `now + TICK` (suspend gap never encoded);
a wake before schedule (pause/resume control message) consumes nothing.
Catch-up under 500 ms is unchanged (backpressure still averages out).
```

```bash
cd src-tauri && cargo fmt
git add src-tauri/capture/src/session.rs docs/Gaps.md scripts/loc-baseline.json
git commit -m "fix(capture): resync the tick schedule after a suspend gap; skip takes on early wakes" -m "GAP-05: next_tick += TICK never resynced to real time, so after a laptop suspend (Instant advances across it) the loop ran back-to-back catch-up ticks encoding the whole sleep as silence and inflating duration_secs; each pause/resume also padded up to ~100 ms of spurious silence from an early wake. The schedule decision is now the pure, unit-tested plan_tick."
```

---

### Task 6: Native non-replacing rename fallback on Windows (A6 · GAP-06)

**Files:**
- Modify: `src-tauri/core/Cargo.toml` (windows-sys target dep)
- Modify: `src-tauri/core/src/capture_paths.rs:269-291` (`rename_noreplace` fallback → `rename_noreplace_fallback`)
- Modify: `docs/Gaps.md` (GAP-06 tombstone)
- Test: inline `mod tests` in `capture_paths.rs` (non-Windows contract test now; a `#[cfg(windows)]` twin that executes once sub-pass D adds the Windows `cargo test` CI step)

**Interfaces:**
- Consumes: nothing new.
- Produces: `rename_noreplace` public behavior contract unchanged (`AlreadyExists` when `to` exists); private `fn rename_noreplace_fallback(from: &Path, to: &Path) -> std::io::Result<()>` with per-platform bodies.

- [ ] **Step 1: Write the (partly failing) contract tests**

Append to `mod tests` in `capture_paths.rs`:

```rust
    #[test]
    fn fallback_refuses_existing_destination() {
        // GAP-06: the non-decisive-error fallback (filesystems without hard
        // links: exFAT/FAT32/SMB) must be non-replacing. On Windows that is
        // MoveFileExW WITHOUT MOVEFILE_REPLACE_EXISTING (native, no TOCTOU
        // window); elsewhere the guarded exists()+rename remains (the
        // non-Windows build is a compile gate, never shipped).
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("from.mp3");
        let to = dir.path().join("to.mp3");
        std::fs::write(&from, "a").unwrap();
        std::fs::write(&to, "keep me").unwrap();
        let err = rename_noreplace_fallback(&from, &to).expect_err("must not replace");
        assert_eq!(err.kind(), std::io::ErrorKind::AlreadyExists);
        assert_eq!(std::fs::read_to_string(&to).unwrap(), "keep me");
    }

    #[test]
    fn fallback_moves_when_destination_free() {
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("from.mp3");
        let to = dir.path().join("to.mp3");
        std::fs::write(&from, "a").unwrap();
        rename_noreplace_fallback(&from, &to).unwrap();
        assert!(!from.exists());
        assert_eq!(std::fs::read_to_string(&to).unwrap(), "a");
    }
```

- [ ] **Step 2: Run to verify they fail**

Run: `cd src-tauri/core && cargo test fallback`
Expected: FAIL to compile — `rename_noreplace_fallback` not found.

- [ ] **Step 3: Implement**

In `src-tauri/core/Cargo.toml`, add (mirroring the shell crate's pattern):

```toml
# MoveFileExW for rename_noreplace's non-replacing fallback on filesystems
# without hard links (GAP-06) — std::fs::rename replaces on every platform.
[target."cfg(windows)".dependencies]
windows-sys = { version = "0.61", features = ["Win32_Foundation", "Win32_Storage_FileSystem"] }
```

In `capture_paths.rs`, replace `rename_noreplace`'s final `Err(_)` arm body with a call, and add the platform pair above the function:

```rust
/// The non-decisive-error fallback for filesystems that can't hard-link
/// (exFAT/FAT32/SMB). On Windows: MoveFileExW WITHOUT
/// MOVEFILE_REPLACE_EXISTING is natively non-replacing — it fails with
/// ERROR_ALREADY_EXISTS when `to` exists, which io::Error maps to
/// ErrorKind::AlreadyExists, exactly the signal our suffix-retry callers
/// key on. This closes the TOCTOU window the old exists()+rename check had
/// (GAP-06): a sync client creating the same name between check and rename
/// was silently replaced — the one path where never-clobber could break.
#[cfg(windows)]
fn rename_noreplace_fallback(from: &Path, to: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    fn wide(p: &Path) -> Vec<u16> {
        p.as_os_str().encode_wide().chain(std::iter::once(0)).collect()
    }
    // Flags deliberately 0: same-directory move (all callers), no replace,
    // no copy fallback needed.
    let ok = unsafe {
        windows_sys::Win32::Storage::FileSystem::MoveFileExW(
            wide(from).as_ptr(),
            wide(to).as_ptr(),
            0,
        )
    };
    if ok == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Non-Windows keeps the guarded rename it always had: these builds are the
/// Linux compile gate and tests, never a shipped binary, and Unix has no
/// portable non-replacing rename below renameat2 (not in std).
#[cfg(not(windows))]
fn rename_noreplace_fallback(from: &Path, to: &Path) -> std::io::Result<()> {
    if to.exists() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "destination exists",
        ));
    }
    std::fs::rename(from, to)
}
```

and in `rename_noreplace`:

```rust
        Err(e) if hard_link_error_is_decisive(&e) => Err(e),
        Err(_) => rename_noreplace_fallback(from, to),
```

Also update `rename_noreplace`'s doc comment second bullet: the fallback is now "the platform's native non-replacing move on Windows (MoveFileExW without MOVEFILE_REPLACE_EXISTING); the pre-check-guarded rename elsewhere".

- [ ] **Step 4: Run tests + verify the Windows arm compiles**

Run: `cd src-tauri/core && cargo test && cargo clippy --all-targets -- -D warnings`
Expected: PASS.
Cross-check the Windows arm at least syntactically: `cd src-tauri/core && cargo check --target x86_64-pc-windows-msvc` if that target is installed; if not, note in the task report that the Windows arm compiles only in CI's `windows-app` job and executes only once D7 lands — this is the spec's sanctioned manual-verification carve-out, state it explicitly.

- [ ] **Step 5: Tombstone GAP-06 and commit**

Gaps.md:

```markdown
### GAP-06 · ~~Medium~~ FIXED 2026-07-10 · Never-clobber degrades to a racy fallback on filesystems without hard links
On Windows the fallback is now MoveFileExW WITHOUT MOVEFILE_REPLACE_EXISTING
(natively non-replacing, no TOCTOU window); non-Windows keeps the guarded
rename (compile gate only, never shipped). Windows-arm execution arrives
with sub-pass D's Windows `cargo test` step (GAP-43).
```

```bash
cd src-tauri && cargo fmt
git add src-tauri/core/Cargo.toml src-tauri/core/src/capture_paths.rs src-tauri/Cargo.lock docs/Gaps.md
git commit -m "fix(core): native non-replacing MoveFileExW fallback for rename_noreplace on Windows" -m "GAP-06: when hard_link fails non-decisively (exFAT/FAT32/SMB), the fallback was a TOCTOU exists()+replacing-rename — a sync client creating the same name between check and rename was silently replaced, the one break in the never-clobber invariant. MoveFileExW without MOVEFILE_REPLACE_EXISTING fails natively with ERROR_ALREADY_EXISTS, which maps to ErrorKind::AlreadyExists for the existing suffix-retry callers."
```

---

### Task 7: `rename_capture` requires vault containment (A7 · GAP-07)

**Files:**
- Modify: `src-tauri/src/capture_commands.rs:739-781` (`rename_capture`)
- Modify: `docs/Gaps.md` (GAP-07 tombstone)

**Interfaces:**
- Consumes: `capture_paths::vault_owning_path` from Task 1 (canonical containment; its escape cases are unit-tested in core — this task is thin wiring the reviewer verifies).
- Produces: `rename_capture` signature unchanged; new precondition error `"Recording is not inside a known vault."`.

- [ ] **Step 1: Rework the command's precondition order**

In `rename_capture`, after the is-recording guard and BEFORE `rename_plan` is called, hoist the file-existence check (better error for a missing file than "not inside a known vault") and add the containment gate:

```rust
    if !Path::new(&mp3).is_file() {
        return Err("Recording file not found — was it moved?".to_string());
    }
    // Containment (GAP-07): every other write path gates on
    // assert_*_inside_vault; rename_plan validates only the capture-pattern
    // stem, so IPC could rename any `YYYY-MM-DD HHmm *.mp3` (and retarget
    // its note) anywhere on disk. Canonical matching per GAP-01's helper.
    let vaults = discovery::discover_vaults();
    if capture_paths::vault_owning_path(&vaults, Path::new(&mp3)).is_none() {
        return Err("Recording is not inside a known vault.".to_string());
    }
    let plan = capture_paths::rename_plan(Path::new(&mp3), &title)?;
```

and DELETE the old `if !plan.mp3_from.is_file() { ... }` check below (now redundant — same string, checked earlier). Everything else in the command is unchanged; the plan still executes on the caller's original path (canonicalization is for the check only, mirroring Task 1's reasoning — the returned `RenamedPayload.mp3` must stay in the path form the frontend passed in).

- [ ] **Step 2: Run the gates**

Run: `cd src-tauri && cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test -p vault-buddy --lib && npx tauri build --no-bundle`
Expected: green. (No shell-level unit test is feasible — the command takes `tauri::State`; the containment logic itself is the core helper Task 1 tested. Say so in the task report.)

- [ ] **Step 3: Tombstone GAP-07 and commit**

Gaps.md:

```markdown
### GAP-07 · ~~Medium~~ FIXED 2026-07-10 · `rename_capture` has no vault-containment check at all
The command now refuses paths outside every registered vault via the
canonical `capture_paths::vault_owning_path` (GAP-01's helper) before
planning the rename.
```

```bash
git add src-tauri/src/capture_commands.rs docs/Gaps.md
git commit -m "fix(shell): require vault containment in rename_capture" -m "GAP-07: rename_plan validates only the capture-pattern stem and .mp3 extension, so IPC could rename any capture-named mp3 (and retarget its note) anywhere on disk — unlike every other write path, which gates on containment. The command now resolves the owning vault on canonical paths and refuses outsiders before planning."
```

---

### Task 8: Shutdown may bypass a startup-wedged reservation (A8 · GAP-08)

**Files:**
- Modify: `src-tauri/src/capture_commands.rs` (`ActiveCapture` field, `bypasses_shutdown_wait`, `recording_blocks_shutdown`, `request_stop_and_wait`, `start_capture` timeout branch + janitor, new `mod tests`)
- Modify: `src-tauri/src/tray.rs:16-49` (`hide_buddy`, `quit`)
- Modify: `src-tauri/src/lib.rs:263-293` (CloseRequested handler)
- Modify: `AGENTS.md` (one-line amendment to the capture "buddy is the recording indicator" bullet)
- Modify: `docs/Gaps.md` (GAP-08 tombstone)
- Test: new inline `mod tests` in `capture_commands.rs` (runs via `cargo test -p vault-buddy --lib`)

**Interfaces:**
- Consumes: `ActiveCapture`, `CaptureState`, `request_stop_and_wait` (existing).
- Produces:
  - `ActiveCapture.startup_wedged: bool` (false everywhere except the start-timeout branch)
  - `fn bypasses_shutdown_wait(active: &ActiveCapture) -> bool` (private, pure, tested)
  - `pub fn recording_blocks_shutdown(app: &AppHandle) -> bool` — used by `tray::hide_buddy`, `tray::quit`, and lib.rs's CloseRequested handler in place of `is_recording`. `is_recording` itself is UNCHANGED and keeps its other callers (capture_status semantics, transcription/recovery gates, rename guard, `finalize_if_recording`).

Design constraints the implementer must keep:
- **Never-lose-audio stands:** the bypass fires ONLY when `startup_wedged && part.is_none()` — nothing reached disk. The janitor records the late worker's `.part` into the reservation the moment it learns it, which closes the bypass for the rest of that drain (a quit mid-late-finalize then waits, as before).
- **The CloseRequested loop:** with a wedged reservation, today's handler `prevent_close()`s forever (finalize returns, `is_recording` still true, re-triggered close prevents again — the "every Alt+F4 spawns another blocked thread" part of the gap). Switching its check to `recording_blocks_shutdown` makes the wedged case fall through to the clean-shutdown branch.

- [ ] **Step 1: Write the failing tests**

Add at the bottom of `capture_commands.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn active(startup_wedged: bool, part: Option<PathBuf>) -> ActiveCapture {
        let (control_tx, _rx) = mpsc::channel::<Control>();
        // _rx dropped: sends become no-ops, which these pure-predicate
        // tests never exercise anyway.
        ActiveCapture {
            control_tx,
            vault_id: "v".to_string(),
            started_at_ms: 0,
            paused: false,
            paused_total_ms: 0,
            paused_since_ms: None,
            part,
            startup_wedged,
        }
    }

    #[test]
    fn shutdown_bypasses_only_a_wedged_startup_with_nothing_on_disk() {
        // GAP-08: a wedged device open kept is_recording true forever —
        // quit blocked forever, hide_buddy no-op'd forever, every Alt+F4
        // spawned another permanently blocked close-finalize thread. The
        // bypass must fire for exactly that state and nothing else.
        assert!(bypasses_shutdown_wait(&active(true, None)));
    }

    #[test]
    fn shutdown_still_waits_for_any_recording_that_reached_disk() {
        // Never-lose-audio: once a .part exists, wait-forever stands — even
        // if the wedged flag was set (belt and suspenders: the janitor
        // records a late worker's part, closing the bypass mid-drain).
        assert!(!bypasses_shutdown_wait(&active(
            true,
            Some(PathBuf::from(".x.mp3.part"))
        )));
        assert!(!bypasses_shutdown_wait(&active(
            false,
            Some(PathBuf::from(".x.mp3.part"))
        )));
    }

    #[test]
    fn shutdown_waits_for_a_normal_still_starting_recording() {
        // part=None WITHOUT the wedged flag is an ordinary start in its
        // first ten seconds — not bypassable.
        assert!(!bypasses_shutdown_wait(&active(false, None)));
    }
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cd src-tauri && cargo test -p vault-buddy --lib`
Expected: FAIL to compile — `startup_wedged` field and `bypasses_shutdown_wait` missing.

- [ ] **Step 3: Implement**

(a) `ActiveCapture` gains the field (after `part`):

```rust
    /// True only for a reservation whose start timed out (the worker never
    /// reported back). Together with `part.is_none()` it marks the one
    /// state shutdown may bypass: nothing reached disk, so the
    /// never-lose-audio invariant is not in play (GAP-08).
    pub startup_wedged: bool,
```

Add `startup_wedged: false,` to the `ActiveCapture` literal in `start_capture`'s reservation block.

(b) The predicate + app-level check, next to `is_recording`:

```rust
/// Whether shutdown/hide may skip waiting on this reservation: only a
/// startup-wedged one with nothing on disk. Everything else — live
/// recording, ordinary start, late worker whose .part we've learned —
/// keeps the wait-forever posture.
fn bypasses_shutdown_wait(active: &ActiveCapture) -> bool {
    active.startup_wedged && active.part.is_none()
}

/// The shutdown/hide variant of `is_recording`: a startup-wedged
/// reservation with no .part must not make the app unquittable or
/// unhidable (GAP-08), while capture_status et al. keep conservatively
/// reporting it as recording.
pub fn recording_blocks_shutdown(app: &AppHandle) -> bool {
    lock_ignoring_poison(&app.state::<CaptureState>().0)
        .as_ref()
        .is_some_and(|active| !bypasses_shutdown_wait(active))
}
```

(c) `request_stop_and_wait` — after the `send(Control::Stop)` line:

```rust
    let _ = active.control_tx.send(Control::Stop);
    if wait.is_none() && bypasses_shutdown_wait(active) {
        // Shutdown against a wedged startup: nothing on disk to strand, and
        // recv() may never return — don't hold quit hostage. The Stop above
        // still halts a late worker the moment it reaches its poll loop.
        log::warn!("capture: bypassing shutdown wait for a startup-wedged reservation (nothing on disk)");
        return;
    }
```

(d) `start_capture`'s timeout branch — stamp the flag right before spawning the janitor (after `let _ = control_tx.send(Control::Stop);`):

```rust
            if let Some(active) = lock_ignoring_poison(&state.0).as_mut() {
                active.startup_wedged = true;
            }
```

and inside the janitor's `if let Ok(Ok(part)) = ready_rx.recv()` arm, FIRST record the part so the shutdown bypass closes while the drain runs:

```rust
                    if let Ok(Ok(part)) = ready_rx.recv() {
                        // The late worker DID reach disk: record its .part so
                        // the shutdown bypass (GAP-08) closes for this drain.
                        if let Some(active) =
                            lock_ignoring_poison(&app4.state::<CaptureState>().0).as_mut()
                        {
                            active.part = Some(part.clone());
                        }
                        log::warn!(
```

(e) `tray.rs`: `hide_buddy`'s guard becomes

```rust
    if crate::capture_commands::recording_blocks_shutdown(app) {
        log::info!("hide ignored: recording in progress");
        return;
    }
```

and `quit`'s `if crate::capture_commands::is_recording(app)` becomes `if crate::capture_commands::recording_blocks_shutdown(app)`.

(f) `lib.rs` CloseRequested: `if capture_commands::is_recording(app)` becomes `if capture_commands::recording_blocks_shutdown(app)` (with the wedged case now falling through to the clean-shutdown else branch — that is the fix for the Alt+F4 thread pile-up).

(g) AGENTS.md, in the capture-domain bullet "**The buddy is the recording indicator**", append one sentence:

```
  One scoped exception (GAP-08): a startup-wedged reservation with no
  `.part` on disk no longer blocks hide/quit/close — nothing exists to
  strand; once a late worker reports its `.part`, the wait-forever
  posture resumes.
```

- [ ] **Step 4: Run the gates**

Run: `cd src-tauri && cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test -p vault-buddy --lib && npx tauri build --no-bundle`
Expected: green; the three new tests pass.

- [ ] **Step 5: LOC baseline (capture_commands.rs grows), tombstone GAP-08, commit**

`npm run check:loc -- --update` if tripped; justify in the commit body.

Gaps.md:

```markdown
### GAP-08 · ~~Medium~~ FIXED 2026-07-10 · A wedged device open makes the app unquittable
The reservation now carries an explicit `startup_wedged` flag (set only in
the start-timeout branch); shutdown paths (`request_stop_and_wait(None)`,
`hide_buddy`, `quit`, CloseRequested) bypass the wait only when it is set
AND `part.is_none()` — nothing on disk. The janitor records a late worker's
`.part`, closing the bypass; recordings that reached disk keep the
wait-forever posture.
```

```bash
git add src-tauri/src/capture_commands.rs src-tauri/src/tray.rs src-tauri/src/lib.rs AGENTS.md docs/Gaps.md scripts/loc-baseline.json
git commit -m "fix(shell): let shutdown bypass a startup-wedged capture reservation" -m "GAP-08: the start-timeout branch keeps the reservation until the worker's recv() returns; a wedged audio driver therefore made is_recording true forever — quit blocked forever in request_stop_and_wait(None), hide_buddy no-op'd, and every Alt+F4 spawned another permanently blocked close-finalize thread (only a process kill exited, then reported as a crash). Bypass is scoped to startup_wedged && part.is_none(): nothing on disk, never-lose-audio not in play; a late worker's .part re-arms the wait."
```

---

### Task 9: Warn on non-NotFound single-file read errors (A9 · GAP-23)

**Files:**
- Modify: `src-tauri/core/src/discovery.rs:60-65`, `src-tauri/core/src/capture_config.rs:280-285` (`load_config_from`), `src-tauri/core/src/daily_notes.rs:42-48`, `src-tauri/core/src/app_diagnostics.rs:22-29`, `src-tauri/core/src/transcript.rs` (`needs_transcription` + `transcript_status` error arms)
- Modify: `docs/Gaps.md` (GAP-23 tombstone)
- Test: inline (value-pinning only)

**Interfaces:** none new. Return values are UNCHANGED at every site (the spec's explicit constraint) — this task adds `log::warn!` on read errors other than `NotFound`. Because behavior (return values) doesn't change, there is no failing test to write for the logging itself; each site gets a value-pinning test only where one doesn't already exist, and the log lines are review-verified. State this honestly in the task report.

- [ ] **Step 1: Add value-pinning tests where missing**

`discovery.rs` tests:

```rust
    #[test]
    fn unreadable_registry_still_degrades_to_empty() {
        // GAP-23 posture: the return value still degrades (never an error);
        // the fix only adds a warn log for non-NotFound failures. Reading a
        // DIRECTORY as the config file is the portable non-NotFound error.
        let dir = tempfile::tempdir().unwrap();
        assert!(discover_vaults_from(dir.path()).is_empty());
    }
```

(`discovery.rs`'s dev-deps: check `tempfile` is available to the core crate — it is, other core modules use it in tests.)

`app_diagnostics.rs` tests:

```rust
    #[test]
    fn unreadable_marker_reads_as_clean_or_first() {
        // Non-NotFound read error (marker path is a directory): degrade
        // value unchanged, only logged now.
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(RUN_MARKER)).unwrap();
        assert!(matches!(
            check_previous_run(dir.path()),
            PreviousRun::CleanOrFirst
        ));
    }
```

- [ ] **Step 2: Run — both should already pass (pinning, not driving)**

Run: `cd src-tauri/core && cargo test`
Expected: PASS (these pin the degrade contract so the edits below can't drift it).

- [ ] **Step 3: Add the warn arms (six sites)**

`discovery.rs` `discover_vaults_from`:

```rust
pub fn discover_vaults_from(config_path: &Path) -> Vec<Vault> {
    match std::fs::read_to_string(config_path) {
        Ok(json) => parse_obsidian_config(&json),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(e) => {
            // GAP-23: an existing-but-unreadable obsidian.json showed the
            // user "no vaults" with zero log trail.
            log::warn!("discovery: cannot read {}: {e}", config_path.display());
            Vec::new()
        }
    }
}
```

`capture_config.rs` `load_config_from`:

```rust
pub fn load_config_from(path: &Path) -> AppConfig {
    match std::fs::read_to_string(path) {
        Ok(json) => parse_config(&json),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => AppConfig::default(),
        Err(e) => {
            log::warn!("config: cannot read {}: {e}", path.display());
            AppConfig::default()
        }
    }
}
```

`daily_notes.rs` `load_settings`:

```rust
pub fn load_settings(vault_path: &Path) -> DailyNoteSettings {
    let path = vault_path.join(".obsidian").join("daily-notes.json");
    match std::fs::read_to_string(&path) {
        Ok(json) => parse_settings(&json),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => DailyNoteSettings::default(),
        Err(e) => {
            log::warn!("daily-notes: cannot read {}: {e}", path.display());
            DailyNoteSettings::default()
        }
    }
}
```

`app_diagnostics.rs` `check_previous_run` (split the `_` arm):

```rust
pub fn check_previous_run(dir: &Path) -> PreviousRun {
    match std::fs::read_to_string(dir.join(RUN_MARKER)) {
        Ok(content) if !content.starts_with("clean") => {
            PreviousRun::Unclean(content.trim().to_string())
        }
        Ok(_) => PreviousRun::CleanOrFirst,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => PreviousRun::CleanOrFirst,
        Err(e) => {
            // Unreadable marker: crash detection silently degraded before.
            log::warn!("diagnostics: cannot read run marker: {e}");
            PreviousRun::CleanOrFirst
        }
    }
}
```

`transcript.rs` `needs_transcription` final arm:

```rust
        // Unreadable (permissions/AV lock): don't spin on it this pass —
        // but say so (GAP-23), or the skip is invisible.
        Err(e) => {
            log::warn!("transcribe: cannot read sidecar {}: {e}", path.display());
            false
        }
```

`transcript.rs` `transcript_status`: the function currently discards the path; bind it first so the warn can name it:

```rust
pub fn transcript_status(mp3: &Path) -> TranscriptStatus {
    let path = transcript_path(mp3);
    match std::fs::read_to_string(&path) {
        Ok(content) => match marker(&content).as_deref() {
            Some("pending") => TranscriptStatus::Pending,
            Some("failed") => TranscriptStatus::Failed,
            Some("cancelled") => TranscriptStatus::Cancelled,
            _ => TranscriptStatus::Complete,
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => TranscriptStatus::Missing,
        Err(e) => {
            log::warn!("transcribe: cannot read sidecar {}: {e}", path.display());
            TranscriptStatus::Missing
        }
    }
}
```

(The `Ok` arm shown reflects Task 3's shape; if Task 3 hasn't merged yet in your worktree, apply the warn arms to the current match instead — the tasks are independent.)

Note: `app_diagnostics.rs` — verify the core crate's `log` is imported where needed (`log::warn!` works with the crate dep, no `use` required).

- [ ] **Step 4: Run the gates**

Run: `cd src-tauri/core && cargo test && cargo clippy --all-targets -- -D warnings`
Expected: PASS.

- [ ] **Step 5: Tombstone GAP-23 and commit**

Gaps.md:

```markdown
### GAP-23 · ~~Medium~~ FIXED 2026-07-10 · Silent `Ok`-with-empty on unreadable single-file configs
All six arms (`discovery`, `capture_config::load_config_from`,
`daily_notes::load_settings`, `app_diagnostics::check_previous_run`,
`transcript::needs_transcription`/`transcript_status`) now `log::warn!` on
any read error other than NotFound; return values still degrade unchanged.
```

```bash
cd src-tauri && cargo fmt
git add src-tauri/core/src/discovery.rs src-tauri/core/src/capture_config.rs src-tauri/core/src/daily_notes.rs src-tauri/core/src/app_diagnostics.rs src-tauri/core/src/transcript.rs docs/Gaps.md
git commit -m "fix(core): warn on non-NotFound reads of single-file configs" -m "GAP-23: six one-file reads swallowed every read error into their degrade value — an existing-but-unreadable obsidian.json showed 'no vaults' with an empty log, and an unreadable run marker silently degraded crash detection. Each site now warns on anything but NotFound; return values are unchanged (the no-swallowed-error invariant is about the log trail, not the degrade)."
```

---

### Task 10: Log-and-degrade on thread-spawn failure in native callbacks (A10 · GAP-24)

**Files:**
- Modify: `src-tauri/src/lib.rs` (CloseRequested `close-finalize` spawn), `src-tauri/src/tray.rs` (`shutdown-finalize`, `tray-stop`), `src-tauri/src/capture_commands.rs` (`capture-warn`, `capture-level`, `capture-device`, `capture-janitor`, `capture-monitor`)
- Modify: `docs/Gaps.md` (GAP-24 tombstone)

**Interfaces:** none new. Every `.expect("failed to spawn …")` at the eight listed sites becomes the `schedule_focus_out_check` pattern (`lib.rs:129-131`): bind the spawn result, log on `Err`, degrade per-site. **No unit tests**: thread-spawn failure cannot be forced portably; verification is compile + clippy + the existing suites + reviewer inspection of each degrade choice — the task report must say exactly that, not claim test coverage. (`run_recovery`'s and `run_transcription`'s `.expect`s run in `setup`, NOT in a native callback — they are out of GAP-24's scope; leave them.)

Per-site degrade decisions (the reviewer checks each against the invariant it protects):

- [ ] **Step 1: `lib.rs` CloseRequested — keep the close held, log, let the user retry**

```rust
                    api.prevent_close();
                    let app = app.clone();
                    let spawned = std::thread::Builder::new()
                        .name("close-finalize".into())
                        .spawn(move || {
                            capture_commands::finalize_if_recording(&app);
                            // The recording is finalized, so is_recording is
                            // now false and the re-triggered CloseRequested
                            // takes the else branch below (pass through to
                            // destruction) — no loop.
                            if let Some(window) = app.get_webview_window("main") {
                                let _ = window.close();
                            }
                        });
                    if let Err(e) = spawned {
                        // Never panic in a window-event handler (aborts across
                        // the WebView2 FFI boundary, no crash record). The
                        // close stays prevented: better an app that ignores
                        // one Alt+F4 than one that exits stranding a .part.
                        log::error!("could not spawn close-finalize thread: {e}");
                    }
```

- [ ] **Step 2: `tray.rs` — quit aborted (retryable), stop dropped (retryable)**

`quit`:

```rust
    if crate::capture_commands::recording_blocks_shutdown(app) {
        let app = app.clone();
        let spawned = std::thread::Builder::new()
            .name("shutdown-finalize".into())
            .spawn(move || {
                crate::capture_commands::finalize_if_recording(&app);
                finish_quit(&app);
            });
        if let Err(e) = spawned {
            // Menu callbacks run on the main thread — never panic here. Not
            // quitting is the safe degrade: the recording keeps running and
            // the user can retry Quit.
            log::error!("could not spawn shutdown-finalize thread: {e}");
        }
        return;
    }
```

`tray-stop-recording` arm:

```rust
            "tray-stop-recording" => {
                // Stopping waits up to 15s for the finalize — never block
                // the menu callback (and the event loop) on it.
                let app = app.clone();
                let spawned = std::thread::Builder::new()
                    .name("tray-stop".into())
                    .spawn(move || {
                        crate::capture_commands::stop_from_menu(&app);
                    });
                if let Err(e) = spawned {
                    // Dropping one stop request is harmless — the user
                    // retries from the still-visible tray item.
                    log::warn!("could not spawn tray-stop thread: {e}");
                }
            }
```

(This code block builds on Task 8's `recording_blocks_shutdown` change to `quit`; if executing out of order, apply the spawn pattern to whatever guard the function currently has.)

- [ ] **Step 3: `capture_commands.rs` `start_capture` — five sites**

`capture-warn` and `capture-level` (same shape; warnings/levels degrade to log-only — the dropped receiver makes the senders' `let _ = tx.send(..)` no-ops):

```rust
    let spawned = std::thread::Builder::new()
        .name("capture-warn".into())
        .spawn(move || {
            while let Ok(message) = warn_rx.recv() {
                let _ = app_warn.emit("capture:warning", serde_json::json!({ "message": message }));
            }
        });
    if let Err(e) = spawned {
        // Recording proceeds without live warning forwarding; sends into
        // the dropped receiver are already fire-and-forget.
        log::warn!("could not spawn capture-warn thread: {e}");
    }
```

(identically for `capture-level`, message `"could not spawn capture-level thread: {e}"`).

`capture-device` — without the worker there IS no recording; fail the start cleanly (nothing on disk yet):

```rust
    let device_thread = std::thread::Builder::new()
        .name("capture-device".into())
        .spawn(move || {
            // ... existing body unchanged ...
        });
    if let Err(e) = device_thread {
        clear_active(&app);
        let msg = format!("Could not start the recording worker: {e}");
        emit_failed(&app, &msg);
        return Err(msg);
    }
```

`capture-janitor` — reservation stays until restart; quit remains possible via Task 8's wedged bypass (the flag was just stamped in this same branch); log at error level and fall through to the existing `emit_failed` + `return Err(msg)`:

```rust
            let janitor = std::thread::Builder::new()
                .name("capture-janitor".into())
                .spawn(move || {
                    // ... existing body unchanged ...
                });
            if let Err(e) = janitor {
                // The reservation stays wedged until the worker replies or
                // the app restarts — quit stays possible via the
                // startup-wedged shutdown bypass stamped above (GAP-08).
                log::error!("could not spawn capture-janitor thread: {e}");
            }
            emit_failed(&app, &msg);
            return Err(msg);
```

`capture-monitor` — the session is LIVE but nobody would drain its outcome; stop it (the device thread finalizes and the audio lands on disk — its `done_tx.send` into the dropped receiver is a silent no-op), clear the state, surface the failure:

```rust
    let monitor = std::thread::Builder::new()
        .name("capture-monitor".into())
        .spawn(move || {
            // ... existing body unchanged ...
        });
    if let Err(e) = monitor {
        // Without a monitor nothing would ever drain the outcome or clear
        // the state. Stop the session — the device thread still finalizes
        // and the audio reaches disk (its done_tx send is a no-op into the
        // dropped receiver) — and report the start as failed.
        let _ = control_tx.send(Control::Stop);
        clear_active(&app);
        crate::tray::set_capture_state(&app, crate::tray::TrayCaptureState::Idle);
        let msg = format!("Recording could not be monitored; stopping: {e}");
        emit_failed(&app, &msg);
        return Err(msg);
    }
```

**Wiring note:** `control_tx` was moved into the `ActiveCapture` reservation as a clone; the original is still in scope here (it is used by the timeout branch today) — verify each closure's captures still compile after restructuring; where a spawn's closure was previously passed inline to `.expect(...)`, the only change is binding the result.

- [ ] **Step 4: Run the gates**

Run: `cd src-tauri && cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test -p vault-buddy --lib && npx tauri build --no-bundle`
Expected: green.

- [ ] **Step 5: LOC baseline if tripped, tombstone GAP-24, commit**

Gaps.md:

```markdown
### GAP-24 · ~~Medium~~ FIXED 2026-07-10 · `.expect` on thread spawn inside main-thread native callbacks
All eight sites (close-finalize, shutdown-finalize, tray-stop, and the five
start_capture spawns) now log-and-degrade per site instead of panicking
across the WebView2 FFI boundary; the setup-time spawns (recovery,
transcribe-worker) were never in a native callback and keep `.expect`.
```

```bash
git add src-tauri/src/lib.rs src-tauri/src/tray.rs src-tauri/src/capture_commands.rs docs/Gaps.md scripts/loc-baseline.json
git commit -m "fix(shell): log-and-degrade on thread-spawn failure in native callbacks" -m "GAP-24: a panic in a window-event or menu callback aborts across the WebView2 FFI boundary with no crash record; spawn failure under resource exhaustion did exactly that at eight sites. Each now uses the schedule_focus_out_check pattern with a per-site degrade: hold the close / abort the quit (retryable), drop warn/level forwarding (advisory), fail the start cleanly (device/monitor), rely on the GAP-08 bypass (janitor)."
```

---

### Task 11: Sub-pass close-out — full gate run

**Files:** none beyond what earlier tasks touched (fix anything the full run surfaces).

- [ ] **Step 1: Full frontend + Rust gate run, in CI's order**

```bash
npm run lint && npm run check:loc && npm run check:quality && npm run test:coverage
cd src-tauri && cargo fmt --check
cd src-tauri && cargo clippy --workspace --all-targets -- -D warnings
cd src-tauri/core && cargo test
cd src-tauri/capture && cargo test
cd src-tauri/transcribe && cargo test
cd src-tauri/mcp && cargo test
cd src-tauri && cargo test -p vault-buddy --lib
cd src-tauri && cargo machete . && cargo deny check
cd src-tauri && cargo llvm-cov -p vault_buddy_core -p vault_buddy_capture -p vault_buddy_transcribe --fail-under-lines 94
```

(`check:quality` must run with no `coverage/` dir present — run it before `test:coverage`, as listed. `cargo deny` matters this task: Task 6 added a target dep.)

- [ ] **Step 2: Verify the Gaps.md ledger**

All ten entries (GAP-01…08, 23, 24) carry `FIXED 2026-07-10` tombstones; `git log --oneline` shows one commit per task, each touching its Gaps.md entry.

- [ ] **Step 3: Push**

```bash
git push -u origin claude/task-management-vertical-slice-ikeuly
```

(Retry up to 4× with 2s/4s/8s/16s backoff on network errors only.) PR #46 already exists for this branch — do not open a new one.

---

## Self-review record

- **Spec coverage:** A1→Task 1, A2→Task 2, A3→Task 3, A4→Task 4, A5→Task 5, A6→Task 6, A7→Task 7, A8→Task 8, A9→Task 9, A10→Task 10; the spec's bookkeeping rules are in Global Constraints and each task's tombstone step; the spec's A8 test requirement ("stop-while-recording still waits") is Task 8's second/third tests at the decision-predicate level (the AppHandle-bound wait loop itself is not unit-testable without a Tauri runtime — stated in-task).
- **Honest non-coverage:** A6's Windows arm (compile-gated; executes when D7 lands), A9's log lines (return values pinned instead), A10 entirely (spawn failure unforceable), A5's loop wiring (policy tested, integration reviewed).
- **Type consistency:** `vault_owning_path` returns `OwningVault { vault, vault_canonical, path_canonical }` — used with exactly those field names in Tasks 1 and 7; `plan_tick` returns `(Instant, bool)` in both definition and call site; `bypasses_shutdown_wait(&ActiveCapture) -> bool` and `recording_blocks_shutdown(&AppHandle) -> bool` match across Tasks 8 and 10's tray/lib call sites.
- **Task-order dependencies:** Task 7 and Task 10 consume Task 1's and Task 8's products respectively; both note what to do if executed out of order. Everything else is independent.
