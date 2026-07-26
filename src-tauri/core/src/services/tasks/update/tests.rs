//! `update.rs`'s tests, split out for the Rust LOC cap (Fix 2, PR #78 —
//! mirrors the `tasks/disk.rs`+`disk/tests.rs` and
//! `services::tasks::parent` mod.rs/tests/mod.rs precedent) — same module
//! position (`services::tasks::update::tests`), so every `super` path below
//! is unchanged and the tests still sit beside the code they pin.

use super::*;
use crate::services::test_support::fixture;

const VAULT: &str = "deadbeef01234567";

/// Registry + config.json in a tempdir, the vault's Task ID setting, and
/// any task files the case needs — the `parent/tests.rs` fixture shape,
/// reproduced here rather than shared: each services test module owns its
/// own minimal fixture (the established convention — see `id_config.rs`'s
/// near-identical local copy).
fn fixture_with_ids(dir: &Path, enabled: bool, files: &[&str]) -> (ServicePaths, String) {
    let (paths, vault) = fixture(dir, "MyVault");
    if enabled {
        std::fs::write(
            paths.config_json.as_ref().unwrap(),
            format!(r#"{{ "vaults": {{ "{VAULT}": {{ "taskIdEnabled": true }} }} }}"#),
        )
        .unwrap();
    }
    let root = vault.join("Tasks");
    std::fs::create_dir_all(&root).unwrap();
    for f in files {
        let title = f.trim_end_matches(".md");
        std::fs::write(
            root.join(f),
            format!("---\ntype: Task\nstatus: new\ntitle: \"{title}\"\n---\n"),
        )
        .unwrap();
    }
    (paths, VAULT.to_string())
}

fn fixture_with_ids_disabled(dir: &Path, files: &[&str]) -> (ServicePaths, String) {
    fixture_with_ids(dir, false, files)
}

fn fixture_with_ids_enabled(dir: &Path, files: &[&str]) -> (ServicePaths, String) {
    fixture_with_ids(dir, true, files)
}

fn tasks_root(paths: &ServicePaths, vault: &str) -> PathBuf {
    tasks_root_for(paths, vault).unwrap().1
}

#[test]
fn a_combined_patch_with_an_invalid_parent_writes_nothing() {
    // Title + a self-parent in one call: validation runs first, so the
    // title must NOT be committed (Codex P2, PR #77). (Brief deviation:
    // the plan's literal test called a nonexistent `apply_task_patch` —
    // the shell cannot be unit-tested (tasks_root_for reads the real
    // %APPDATA% config) and the write half now lives here, per the
    // controller amendment. Same invariant, exercised against this
    // function directly with the `updates` slice the shell would build.)
    let dir = tempfile::tempdir().unwrap();
    let (paths, vault) = fixture_with_ids_enabled(dir.path(), &["a.md"]);
    let root = tasks_root(&paths, &vault);
    let p = root.join("a.md");
    let before = std::fs::read_to_string(&p).unwrap();
    let quoted = crate::capture_note::yaml_quote("Renamed");
    let updates: Vec<(&str, Option<&str>)> = vec![("title", Some(quoted.as_str()))];
    let result = update_task(&paths, &vault, &p, &updates, ParentOp::Set(p.clone()));
    assert!(result.is_err());
    assert_eq!(std::fs::read_to_string(&p).unwrap(), before); // title untouched
}

#[test]
fn a_plain_field_patch_is_unaffected_by_the_parent_machinery() {
    // ParentOp::Keep must reproduce the exact pre-Task-7 behavior: only
    // the field write runs, and the id reflects it.
    let dir = tempfile::tempdir().unwrap();
    let (paths, vault) = fixture_with_ids_enabled(dir.path(), &["a.md"]);
    let root = tasks_root(&paths, &vault);
    let p = root.join("a.md");
    let quoted = crate::capture_note::yaml_quote("Renamed");
    let updates: Vec<(&str, Option<&str>)> = vec![("title", Some(quoted.as_str()))];
    let result = update_task(&paths, &vault, &p, &updates, ParentOp::Keep).unwrap();
    assert!(
        result.id.is_some(),
        "an id-enabled vault stamps on any edit"
    );
    assert_eq!(result.parent_id, None);
    assert_eq!(result.parent_link, None);
    assert!(!result.ids_enabled);
    assert!(std::fs::read_to_string(&p)
        .unwrap()
        .contains("title: \"Renamed\""));
}

#[test]
fn a_parent_only_set_bootstraps_ids_and_reports_the_childs_own_id() {
    // No ordinary fields — the parent-only patch the Parent picker's
    // Change control sends. `id` must reflect the child's own stamp even
    // though step 2 (the field write) never ran (Codex P2, PR #77 class:
    // a stale pre-enable snapshot would otherwise under-report it).
    let dir = tempfile::tempdir().unwrap();
    let (paths, vault) = fixture_with_ids_disabled(dir.path(), &["p.md", "c.md"]);
    let root = tasks_root(&paths, &vault);
    let child = root.join("c.md");
    let result = update_task(
        &paths,
        &vault,
        &child,
        &[],
        ParentOp::Set(root.join("p.md")),
    )
    .unwrap();
    assert!(result.ids_enabled, "this call turned Task IDs on");
    let pid = result.parent_id.expect("a parent id was written");
    assert!(!pid.is_empty());
    // `parent_link` is the RAW composed link (compose_parent_link's own
    // output, e.g. `[[Tasks/p]]`) — YAML-quoting is a detail of the WRITE
    // (the closure below wraps it via yaml_scalar::yaml_quote before it
    // reaches disk), not part of what this field reports back.
    assert!(result.parent_link.unwrap().starts_with("[["));
    let cid = result.id.expect("the child's own id is reported");
    assert!(std::fs::read_to_string(&child)
        .unwrap()
        .contains(&format!("parent-id: {pid}")));
    assert!(std::fs::read_to_string(&child)
        .unwrap()
        .contains(&format!("task-id: {cid}")));
}

#[test]
fn a_parent_set_mirrors_an_implicitly_typed_id_bare_so_types_agree() {
    // review, PR #77: `task-id: 123` is UNQUOTED, so YAML resolves
    // it as the NUMBER 123 — not the string "123" the old decode-then-
    // requote pipeline assumed. That pipeline decoded the parent's id to
    // the Rust string "123" and re-derived a YAML form from THAT decoded
    // string alone (a charset/keyword heuristic), losing the fact that
    // the source was never quoted, and wrote `parent-id: "123"` — a
    // STRING — so an equality-based Dataview query between the two
    // properties stopped matching.
    //
    // The assertion below is about the RAW on-disk text the child
    // receives, not a decode-and-compare round trip:
    // `parent_id_field(&child) == Some("123")` would ALSO pass under the
    // OLD, buggy `parent-id: "123"` output (both decode to the identical
    // Rust string "123"), which is exactly what let a type-mismatching
    // bug hide behind a passing round-trip test once already.
    let dir = tempfile::tempdir().unwrap();
    let (paths, vault) = fixture_with_ids_disabled(dir.path(), &["c.md"]);
    let root = tasks_root(&paths, &vault);
    let parent = root.join("p.md");
    std::fs::write(
        &parent,
        "---\ntype: Task\nstatus: new\ntitle: \"P\"\ntask-id: 123\n---\n",
    )
    .unwrap();
    let child = root.join("c.md");
    update_task(&paths, &vault, &child, &[], ParentOp::Set(parent)).unwrap();
    let out = std::fs::read_to_string(&child).unwrap();
    assert!(
        out.contains("parent-id: 123\n"),
        "must mirror the parent's own unquoted, number-typed token, got: {out}"
    );
    assert!(
        !out.contains("parent-id: \"123\""),
        "must not retype the number as a string: {out}"
    );
}

#[test]
fn a_parent_set_mirrors_a_tag_decorated_id_verbatim() {
    // `task-id: !!str 123` forces the SOURCE to resolve as the STRING
    // "123" via an explicit YAML tag. The old pipeline's strict decoder
    // does not understand tags — it treats the whole thing as opaque
    // plain-scalar text — so it decoded to the literal Rust string
    // "!!str 123" (tag syntax included) and quoted THAT for the child:
    // `parent-id: "!!str 123"`, a string whose CONTENT is tag syntax,
    // resolving to neither "123" nor anything the parent's own value
    // equals. Mirroring the raw text lets the child's copy resolve
    // through the identical tag.
    let dir = tempfile::tempdir().unwrap();
    let (paths, vault) = fixture_with_ids_disabled(dir.path(), &["c.md"]);
    let root = tasks_root(&paths, &vault);
    let parent = root.join("p.md");
    std::fs::write(
        &parent,
        "---\ntype: Task\nstatus: new\ntitle: \"P\"\ntask-id: !!str 123\n---\n",
    )
    .unwrap();
    let child = root.join("c.md");
    update_task(&paths, &vault, &child, &[], ParentOp::Set(parent)).unwrap();
    let out = std::fs::read_to_string(&child).unwrap();
    assert!(
        out.contains("parent-id: !!str 123\n"),
        "must mirror the parent's tag-decorated token verbatim, got: {out}"
    );
}

#[test]
fn a_parent_set_strips_the_anchor_but_mirrors_the_value() {
    // `&stable abc` NAMES the node "stable" so a `*stable` alias
    // elsewhere in the SAME document can reference it. Copying the
    // annotation verbatim into the child's own frontmatter would define
    // a SECOND anchor of that name there. The value itself, `abc`, is
    // what a reference may legitimately copy — decoding the anchor away
    // and mirroring `abc` resolves identically to the parent's own value
    // (both the string "abc").
    let dir = tempfile::tempdir().unwrap();
    let (paths, vault) = fixture_with_ids_disabled(dir.path(), &["c.md"]);
    let root = tasks_root(&paths, &vault);
    let parent = root.join("p.md");
    std::fs::write(
        &parent,
        "---\ntype: Task\nstatus: new\ntitle: \"P\"\ntask-id: &stable abc\n---\n",
    )
    .unwrap();
    let child = root.join("c.md");
    update_task(&paths, &vault, &child, &[], ParentOp::Set(parent)).unwrap();
    let out = std::fs::read_to_string(&child).unwrap();
    assert!(
        out.contains("parent-id: abc\n"),
        "must strip the anchor annotation and mirror only its value, got: {out}"
    );
    assert!(
        !out.contains("&stable"),
        "must never define a second anchor of the same name: {out}"
    );
}

#[test]
fn a_parent_set_strips_a_trailing_comment_before_mirroring() {
    // Trap check: `raw_scalar_field` (capture_note.rs) does NOT strip a
    // trailing inline comment on the raw text it returns — it only trims
    // whitespace. Mirroring that raw text VERBATIM would copy the
    // parent's own edit-history comment into the child's
    // machine-managed `parent-id` line.
    let dir = tempfile::tempdir().unwrap();
    let (paths, vault) = fixture_with_ids_disabled(dir.path(), &["c.md"]);
    let root = tasks_root(&paths, &vault);
    let parent = root.join("p.md");
    std::fs::write(
        &parent,
        "---\ntype: Task\nstatus: new\ntitle: \"P\"\ntask-id: 123 # was xyz\n---\n",
    )
    .unwrap();
    let child = root.join("c.md");
    update_task(&paths, &vault, &child, &[], ParentOp::Set(parent)).unwrap();
    let out = std::fs::read_to_string(&child).unwrap();
    assert!(
        out.contains("parent-id: 123\n"),
        "must strip the trailing comment before mirroring, got: {out}"
    );
    assert!(
        !out.contains("was xyz"),
        "the parent's own comment must never leak into the child: {out}"
    );
}

#[test]
fn a_parent_set_replaces_an_existing_differently_cased_line_instead_of_duplicating_it() {
    // Fix 2 (final whole-branch review, task report): `update_task`'s own
    // SET branch (update.rs, not `set_task_parent`) needs the identical
    // on-disk-casing fix — `update_task_fields`/`set_fields` matches a key
    // case-SENSITIVELY, so writing the canonical lowercase `parent-id`
    // onto a child that already carries `Parent-Id:` would insert a
    // case-mismatched DUPLICATE rather than replacing the stale line.
    let dir = tempfile::tempdir().unwrap();
    let (paths, vault) = fixture_with_ids_disabled(dir.path(), &["p.md"]);
    let root = tasks_root(&paths, &vault);
    let parent = root.join("p.md");
    let child = root.join("c.md");
    std::fs::write(
        &child,
        "---\ntype: Task\nstatus: new\ntitle: \"C\"\nParent-Id: old99999\n---\n",
    )
    .unwrap();
    update_task(&paths, &vault, &child, &[], ParentOp::Set(parent)).unwrap();
    let out = std::fs::read_to_string(&child).unwrap();
    let id_lines: Vec<&str> = out
        .lines()
        .filter(|l| l.to_ascii_lowercase().starts_with("parent-id:"))
        .collect();
    assert_eq!(
        id_lines.len(),
        1,
        "must replace the existing line, not insert a case-mismatched duplicate: got {out}"
    );
    assert!(
        id_lines[0].starts_with("Parent-Id:"),
        "must preserve the file's own on-disk casing, got {out}"
    );
    assert!(
        !id_lines[0].contains("old99999"),
        "the stale value must be replaced, got {out}"
    );
}

#[test]
fn a_parent_only_clear_removes_both_keys_and_reports_no_parent() {
    let dir = tempfile::tempdir().unwrap();
    let (paths, vault) = fixture_with_ids_enabled(dir.path(), &["p.md", "c.md"]);
    let root = tasks_root(&paths, &vault);
    let child = root.join("c.md");
    let set_result = update_task(
        &paths,
        &vault,
        &child,
        &[],
        ParentOp::Set(root.join("p.md")),
    )
    .unwrap();
    // Review finding 3: the vault (fixture_with_ids_enabled) ALREADY had
    // Task IDs on before this Set, so idsEnabled must read false here —
    // this is the only place in the suite a Set on an already-enabled
    // vault checks that field at all (the sibling bootstrap test,
    // `a_parent_only_set_bootstraps_ids_and_reports_the_childs_own_id`,
    // only covers the ids-were-OFF/turned-on arm). A hardcoded
    // `resolved.ids_enabled` -> `true` at update.rs:177 left every one
    // of the 615 core tests green without this assertion — a false
    // "Task IDs were turned on for this vault" disclosure on every
    // parent assignment in an already-enabled vault.
    assert!(
        !set_result.ids_enabled,
        "ids were already on before this call, not turned on by it"
    );
    assert!(std::fs::read_to_string(&child)
        .unwrap()
        .contains("parent-id"));

    let result = update_task(&paths, &vault, &child, &[], ParentOp::Clear).unwrap();
    assert_eq!(result.parent_id, None);
    assert_eq!(result.parent_link, None);
    assert!(!result.ids_enabled);
    let after = std::fs::read_to_string(&child).unwrap();
    assert!(!after.contains("parent-id"));
    assert!(!after.contains("parent:"));
}

#[test]
fn a_parent_clear_removes_a_differently_cased_existing_line_not_a_lowercase_no_op() {
    // The clear-branch counterpart to the SET-branch casing fix above:
    // `update_task`'s own `ParentOp::Clear` arm must target whatever
    // casing is ACTUALLY on disk, or the stale hand-authored line
    // survives untouched while the app believes it cleared the
    // relationship.
    let dir = tempfile::tempdir().unwrap();
    let (paths, vault) = fixture_with_ids_enabled(dir.path(), &[]);
    let root = tasks_root(&paths, &vault);
    let child = root.join("c.md");
    std::fs::write(
        &child,
        "---\ntype: Task\nstatus: new\ntitle: \"C\"\nParent-Id: x\nParent: \"[[p]]\"\n---\n",
    )
    .unwrap();
    let result = update_task(&paths, &vault, &child, &[], ParentOp::Clear).unwrap();
    assert_eq!(result.parent_id, None);
    assert_eq!(result.parent_link, None);
    let out = std::fs::read_to_string(&child).unwrap();
    assert!(
        !out.to_ascii_lowercase().contains("parent-id:"),
        "the differently-cased parent-id line must be removed, got {out}"
    );
    assert!(
        !out.to_ascii_lowercase().contains("\nparent:"),
        "the differently-cased parent line must be removed, got {out}"
    );
}

#[cfg(unix)]
#[test]
// `TaskWriteResult` (the Ok payload) deliberately derives no `Debug` — adding
// one purely so `.expect_err()` can print a discarded Ok value would be a
// production-code change unrelated to whatever this test is pinning (the
// `resolve_parent_for_write_refuses_when_the_tasks_root_moved_mid_flight`
// test in `parent/tests/concurrency.rs` documents the identical reasoning
// for `ResolvedParent`), so this stays the `.err().expect()` form clippy's
// `err_expect` lint would otherwise rewrite into something that can't compile.
#[allow(clippy::err_expect)]
fn a_parent_stamp_failure_after_a_committed_field_write_reports_fields_saved() {
    // Review finding 2: `fields_saved` (computed at update.rs:103, right
    // above) had no test driving a REAL step-3 I/O failure after a REAL
    // step-2 commit — `parent_write_error_names_the_fields_saved_state`
    // below only unit-tests the formatting helper directly, so
    // mutating `let fields_saved = !updates.is_empty();` to
    // `let fields_saved = false;` left every one of the 615 core tests
    // green (this file's own report documents that mutation run).
    //
    // Constructed by making the PARENT's own list folder read+execute
    // only: phase 1 (`validate_parent_assignment`) only ever READS —
    // canonicalize, `list_tasks_structural`'s walk, the unassignable
    // forecast — so it passes. The CHILD lives directly in the
    // (writable) tasks root, so step 2's ordinary field write (the
    // title) commits. Only THEN does phase 3a's `ensure_id` try to
    // stamp the parent — which must CREATE a temp file beside it via
    // `write_atomic_replacing` — and that fails with EACCES, so the
    // combined call reports a partial success instead of a clean one.
    //
    // Root bypasses DAC (this sandbox runs every test as root), so a
    // write-probe into the locked directory decides whether to
    // self-skip — the loud, restore-before-assert idiom
    // `services/tasks/id_config.rs`'s chmod tests use
    // (id_config.rs:355-382), adapted to probe a WRITE (what this test
    // denies) rather than a read, matching how `tasks/lists/relocate.rs`
    // varies the very same idiom for its own write-denial tests. CI's
    // rust-core job runs unprivileged and exercises the real assertions
    // below; independently verified for this task by re-running under
    // `setpriv --reuid=65534 --regid=65534 --clear-groups` (see the task
    // report for that output).
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let (paths, vault) = fixture_with_ids_disabled(dir.path(), &["a.md"]);
    let root = tasks_root(&paths, &vault);
    let locked_dir = root.join("Locked");
    std::fs::create_dir_all(&locked_dir).unwrap();
    let parent = locked_dir.join("p.md");
    std::fs::write(&parent, "---\ntype: Task\nstatus: new\ntitle: \"P\"\n---\n").unwrap();
    std::fs::set_permissions(&locked_dir, std::fs::Permissions::from_mode(0o555)).unwrap();

    let probe = locked_dir.join(".probe");
    let bypassed = std::fs::write(&probe, b"x").is_ok();
    if bypassed {
        let _ = std::fs::remove_file(&probe);
        std::fs::set_permissions(&locked_dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        eprintln!(
            "SKIPPED a_parent_stamp_failure_after_a_committed_field_write_reports_fields_saved: \
                 running as root, chmod 555 does not deny directory writes here"
        );
        return;
    }

    let child = root.join("a.md");
    let quoted = crate::capture_note::yaml_quote("Renamed");
    let updates: Vec<(&str, Option<&str>)> = vec![("title", Some(quoted.as_str()))];
    let outcome = update_task(&paths, &vault, &child, &updates, ParentOp::Set(parent));

    // Restore before asserting so the tempdir can clean up either way.
    std::fs::set_permissions(&locked_dir, std::fs::Permissions::from_mode(0o755)).unwrap();

    let err = outcome
        .err()
        .expect("the parent stamp must fail under a read-only list folder");
    assert!(
        err.starts_with("Saved fields, but couldn't set the parent:"),
        "got {err}"
    );
    assert!(err.contains("Permission denied"), "got {err}");
    assert!(
        std::fs::read_to_string(&child)
            .unwrap()
            .contains("title: \"Renamed\""),
        "the committed field write must survive the later parent failure"
    );
}

#[test]
// See the identical justification on `a_parent_stamp_failure_after_a_
// committed_field_write_reports_fields_saved` above: `TaskWriteResult`
// deliberately derives no `Debug`.
#[allow(clippy::err_expect)]
fn a_vanished_parent_refuses_at_validation_and_never_claims_fields_saved() {
    // A parent that vanished between load and write fails phase 1
    // (canonical_task_in_root can't resolve it) — BEFORE the field write
    // runs — so this is a validation failure, not the step-3 partial-
    // state case `parent_write_error` exists for; asserted here so that
    // distinction stays pinned (a regression that let this reach step 3
    // instead would silently start committing fields ahead of a doomed
    // parent write).
    let dir = tempfile::tempdir().unwrap();
    let (paths, vault) = fixture_with_ids_enabled(dir.path(), &["a.md", "p.md"]);
    let root = tasks_root(&paths, &vault);
    let p = root.join("a.md");
    let parent = root.join("p.md");
    let before = std::fs::read_to_string(&p).unwrap();
    let quoted = crate::capture_note::yaml_quote("Renamed");
    let updates: Vec<(&str, Option<&str>)> = vec![("title", Some(quoted.as_str()))];
    std::fs::remove_file(&parent).unwrap();
    let err = update_task(&paths, &vault, &p, &updates, ParentOp::Set(parent))
        .err()
        .expect("a vanished parent must fail the write");
    assert!(!err.starts_with("Saved fields"), "{err}");
    assert_eq!(std::fs::read_to_string(&p).unwrap(), before); // title untouched
}

#[test]
fn parent_write_error_names_the_fields_saved_state() {
    // The step-3 partial-state wording directly: a parent write that
    // fails AFTER the ordinary fields already committed must not claim
    // total failure (Codex P2, PR #77) — but a parent-ONLY patch (no
    // fields in `updates` at all) that fails must not falsely claim
    // fields were saved either. Exercising this through a real end-to-end
    // I/O failure at step 3 specifically (validation already having
    // passed) would need a genuine race — `resolve_parent_for_write`'s
    // own mid-flight races are already pinned directly in
    // `parent/tests.rs` — so the conditional wording itself, the thing
    // Task 7 actually adds, is pinned here instead.
    assert_eq!(
        parent_write_error(true, "boom".to_string()),
        "Saved fields, but couldn't set the parent: boom"
    );
    assert_eq!(
        parent_write_error(false, "boom".to_string()),
        "Couldn't set the parent: boom"
    );
}

#[test]
fn the_under_lock_recheck_refuses_a_cycle_a_concurrent_write_would_otherwise_create() {
    // Fix 4 (final whole-branch review, task report): `update_task`'s own
    // ParentOp::Set call site is a SEPARATE entry point from
    // `set_task_parent`'s (`services::tasks::parent::mod.rs`) — at the time
    // this test was written, this module built its own inline
    // `resolve_parent_for_write` closure rather than delegating to that
    // one, so the sibling regression pinned in `parent/tests.rs` covered
    // ONLY that other call site. Fix 2 (PR #78) later replaced this
    // module's inline closure with a one-line delegation to the shared
    // `parent::recheck_set_or_update` — but this test stays: it now pins
    // that the DELEGATION ITSELF is wired up (a regression that reverted
    // `update.rs`'s call site back to a stub, dropping the shared
    // function entirely, was confirmed BY THIS TEST during Fix 2's own
    // mutation testing — see `recheck_set_or_update`'s own doc comment),
    // not that a hand-copied closure's internals stay correct. See that
    // sibling test's own doc comment for the full mechanics (a
    // pre-existing hand-authored X<->Y cycle, Z with no parent yet, and a
    // race between "Z's parent = X" and "Y's parent = Z") — reproduced
    // here against `update_task` instead of `set_task_parent`, since the
    // two are still separate call sites even though they now share the
    // recheck logic itself.
    for _ in 0..60 {
        let dir = tempfile::tempdir().unwrap();
        let (paths, vault) = fixture_with_ids_enabled(dir.path(), &[]);
        let root = tasks_root(&paths, &vault);
        std::fs::write(
            root.join("x.md"),
            "---\ntype: Task\nstatus: new\ntitle: \"X\"\ntask-id: x\nparent-id: y\n---\n",
        )
        .unwrap();
        std::fs::write(
            root.join("y.md"),
            "---\ntype: Task\nstatus: new\ntitle: \"Y\"\ntask-id: y\nparent-id: x\n---\n",
        )
        .unwrap();
        std::fs::write(
            root.join("z.md"),
            "---\ntype: Task\nstatus: new\ntitle: \"Z\"\ntask-id: z\n---\n",
        )
        .unwrap();
        let x = root.join("x.md");
        let y = root.join("y.md");
        let z = root.join("z.md");
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));

        let thread_a = {
            let paths = paths.clone();
            let vault = vault.clone();
            let (z, x) = (z.clone(), x.clone());
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                update_task(&paths, &vault, &z, &[], ParentOp::Set(x))
            })
        };
        let thread_b = {
            let paths = paths.clone();
            let vault = vault.clone();
            let (y, z) = (y.clone(), z.clone());
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                update_task(&paths, &vault, &y, &[], ParentOp::Set(z))
            })
        };
        let _ = thread_a.join().unwrap();
        let _ = thread_b.join().unwrap();

        let x_parent = tasks::parent_id_field(&std::fs::read_to_string(&x).unwrap());
        let y_parent = tasks::parent_id_field(&std::fs::read_to_string(&y).unwrap());
        let z_parent = tasks::parent_id_field(&std::fs::read_to_string(&z).unwrap());
        let closed_the_cycle = x_parent.as_deref() == Some("y")
            && y_parent.as_deref() == Some("z")
            && z_parent.as_deref() == Some("x");
        assert!(
            !closed_the_cycle,
            "a concurrent pair of parent assignments closed a real cycle \
                 X -> Y -> Z -> X: x.parent={x_parent:?} y.parent={y_parent:?} \
                 z.parent={z_parent:?}"
        );
    }
}

#[test]
// See the identical justification above: `TaskWriteResult` deliberately
// derives no `Debug`.
#[allow(clippy::err_expect)]
fn a_parent_set_refuses_an_archived_parent_end_to_end() {
    // Fix 2 (whole-branch review, PR #78): `update_task`'s ParentOp::Set
    // closure used to hand-copy `set_task_parent`'s under-lock recheck
    // logic; it now delegates to the shared `parent::recheck_set_or_
    // update` instead (see that function's own doc comment). This IS the
    // actual IPC-wired path the Parent picker's Change/Set-parent
    // control uses (`task_commands.rs::update_task` ->
    // `services::update_task`), so a fix applied only to
    // `set_task_parent`/`add_subtask` (`parent/mod.rs`) would leave the
    // real user-facing race open — but there was no test at all, through
    // THIS entry point, confirming an archived parent is refused, even
    // via phase 1 (`validate_parent_assignment`, which independently
    // checks the same thing before ever reaching the closure below it).
    //
    // HONESTY NOTE: an UP-FRONT archived parent (as constructed here) is
    // refused by phase 1 alone, whether or not the under-lock recheck
    // this test is nominally about even runs — reverting update.rs's
    // delegation to `recheck_set_or_update` back to a bare `Ok(()))`
    // does NOT turn this test red (verified by mutation). That specific
    // recheck's OWN correctness is pinned precisely, with zero timing
    // dependence, by `recheck_set_or_update_refuses_a_parent_archived_
    // mid_flight` in `parent::tests::concurrency` — the exact function
    // this module's closure now delegates to, not a hand-rolled copy.
    // A genuine mid-flight race through update_task specifically would
    // need a real concurrent write (like the cycle test above), which
    // this investigation found unreliable to calibrate for this specific
    // invariant (see that module's doc comment for the full reasoning).
    // This test's job is narrower and still real: confirm the ordinary,
    // whole-function behavior via the one path users and MCP actually
    // call, so a future refactor that drops EITHER guard (phase 1 or the
    // delegated recheck) is still caught by something.
    let dir = tempfile::tempdir().unwrap();
    let (paths, vault) = fixture_with_ids_enabled(dir.path(), &["c.md"]);
    let root = tasks_root(&paths, &vault);
    let parent = root.join("p.md");
    std::fs::write(
        &parent,
        "---\ntype: Task\nstatus: archived\ntitle: \"P\"\n---\n",
    )
    .unwrap();
    let child = root.join("c.md");
    let err = update_task(&paths, &vault, &child, &[], ParentOp::Set(parent))
        .err()
        .expect("must refuse a parent archived on disk");
    assert!(err.contains("archived"), "got {err}");
}
