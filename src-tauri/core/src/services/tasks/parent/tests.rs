//! The parent-assignment write path tests, split out of `mod.rs` for the
//! Rust LOC cap — same module position (`services::tasks::parent::tests`),
//! so every `super` path is unchanged and the tests still sit beside the
//! code they pin (the `services::tasks::tests` precedent).

use super::super::move_task_to_list;
use super::*;
use crate::services::test_support::fixture;

const VAULT: &str = "deadbeef01234567";

/// The existing services harness (`test_support::fixture`: registry +
/// config.json in a tempdir) plus the vault's Task ID setting and any task
/// files the case needs. The caller owns the tempdir, exactly like every
/// other test in this crate's services suite.
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
        write(
            &root,
            f,
            &format!("---\ntype: Task\nstatus: new\ntitle: \"{title}\"\n---\n"),
        );
    }
    (paths, VAULT.to_string())
}

fn fixture_with_ids_disabled(dir: &Path, files: &[&str]) -> (ServicePaths, String) {
    fixture_with_ids(dir, false, files)
}

fn fixture_with_ids_enabled(dir: &Path, files: &[&str]) -> (ServicePaths, String) {
    fixture_with_ids(dir, true, files)
}

/// The vault's tasks root, resolved the way production does (so a vault
/// with a nested `tasksFolder` needs no second derivation here).
fn tasks_root(paths: &ServicePaths, vault: &str) -> PathBuf {
    tasks_root_for(paths, vault).unwrap().1
}

fn config_for(paths: &ServicePaths, vault: &str) -> VaultCaptureConfig {
    capture_config::vault_config(&app_config(paths), vault)
}

fn write(root: &Path, rel: &str, content: &str) -> PathBuf {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(&p, content).unwrap();
    p
}

#[test]
fn resolve_parent_for_write_refuses_when_the_tasks_root_moved_mid_flight() {
    // The post-lock re-check (design spec §2) already refused a
    // task_id_enabled/property change committed between phase 1 and the
    // lock; it did NOT compare `tasks_root()` — so a concurrent
    // set_tasks_config moving the vault's tasksFolder in that same window
    // passed the re-check, and phase 3 stamped/wrote under ctx.root, the
    // STALE folder. Every later list_tasks/open_task resolves the NEW
    // folder, so the hierarchy the user just created is invisible
    // immediately. Constructed directly (rather than raced with a real
    // thread) for a deterministic repro: hand `resolve_parent_for_write` a
    // ctx captured from tasksFolder = "A", then mutate config.json to "B"
    // before calling it — exactly what a set_tasks_config landing in the
    // phase-1-to-lock window produces.
    let dir = tempfile::tempdir().unwrap();
    let (paths, vault_path) = fixture(dir.path(), "MyVault");
    std::fs::write(
        paths.config_json.as_ref().unwrap(),
        format!(r#"{{ "vaults": {{ "{VAULT}": {{ "tasksFolder": "A" }} }} }}"#),
    )
    .unwrap();
    let phase1_cfg = config_for(&paths, VAULT);
    let root_a = tasks_root(&paths, VAULT);
    let parent = write(
        &root_a,
        "p.md",
        "---\ntype: Task\nstatus: new\ntitle: \"P\"\n---\n",
    );
    let child = write(
        &root_a,
        "c.md",
        "---\ntype: Task\nstatus: new\ntitle: \"C\"\n---\n",
    );

    // The race: an (async) set_tasks_config commits a DIFFERENT tasks
    // folder before the lock's re-check runs.
    std::fs::write(
        paths.config_json.as_ref().unwrap(),
        format!(r#"{{ "vaults": {{ "{VAULT}": {{ "tasksFolder": "B" }} }} }}"#),
    )
    .unwrap();

    let ctx = ParentWriteCtx {
        paths: &paths,
        vault_id: VAULT,
        vault_path: &vault_path,
        root: &root_a,
        prop: "task-id",
        phase1_cfg: &phase1_cfg,
    };
    // Matched manually rather than `.unwrap_err()`: `ResolvedParent` (the Ok
    // payload) derives no `Debug`, and adding one purely for this assertion
    // would be a production-code change unrelated to the bug being fixed.
    let err = match resolve_parent_for_write(&ctx, &parent, &child, || Ok(false), |_| Ok(())) {
        Err(e) => e,
        Ok(_) => panic!("expected an error when the tasks root moved mid-flight"),
    };
    assert!(err.contains("changed"), "got {err}");

    // Nothing was written under the stale root...
    assert!(!std::fs::read_to_string(&parent)
        .unwrap()
        .contains("task-id:"));
    assert!(!std::fs::read_to_string(&child)
        .unwrap()
        .contains("parent-id:"));
    // ...the new folder was never touched either (phase 3 never ran)...
    assert!(!vault_path.join("B").exists());
    // ...and the doomed attempt never flipped Task IDs on for the vault.
    assert!(!config_for(&paths, VAULT).task_id_enabled);
}

#[test]
fn rejects_a_self_parent_without_enabling_ids_or_stamping() {
    // Phase separation: validation precedes EVERY side effect.
    let dir = tempfile::tempdir().unwrap();
    let (paths, vault) = fixture_with_ids_disabled(dir.path(), &["a.md"]);
    let p = tasks_root(&paths, &vault).join("a.md");
    let before = std::fs::read_to_string(&p).unwrap();
    assert!(set_task_parent(&paths, &vault, &p, Some(&p)).is_err());
    assert_eq!(std::fs::read_to_string(&p).unwrap(), before); // no stamp
    assert!(!config_for(&paths, &vault).task_id_enabled); // still disabled
}

#[test]
fn refuses_an_unassignable_parent_id_without_enabling_ids() {
    // Phase separation, the sub-case phase 1 didn't see: the parent's
    // `task-id` is a flow map (a synced external id from some other
    // system), which `ensure_id` must never clobber — phase 3a's
    // update_task_fields correctly returns None for it. The bug was
    // ordering: phase 2 enabled Task IDs BEFORE phase 3a discovered the
    // parent was unassignable, so a rejected assignment still left the
    // vault's ID setting flipped on with no stamp and no disclosure. The
    // second assertion below is the one that must catch it.
    let dir = tempfile::tempdir().unwrap();
    let (paths, vault) = fixture_with_ids_disabled(dir.path(), &["c.md"]);
    let root = tasks_root(&paths, &vault);
    let parent = write(
        &root,
        "p.md",
        "---\ntype: Task\nstatus: new\ntitle: \"P\"\ntask-id: {source: jira}\n---\n",
    );
    let err = set_task_parent(&paths, &vault, &root.join("c.md"), Some(&parent));
    assert!(err.is_err(), "an unassignable parent id must be refused");
    assert!(
        !config_for(&paths, &vault).task_id_enabled,
        "a refused assignment must not leave Task IDs switched on for the vault"
    );
}

#[test]
fn refuses_a_cycle_through_an_id_less_prospective_parent() {
    // REGRESSION (design spec §3): P has no id but already names C as its
    // parent. The path-keyed graph must see P->C and refuse; an id-keyed one
    // would skip the check and create a P<->C cycle on the next write.
    let dir = tempfile::tempdir().unwrap();
    let (paths, vault) = fixture_with_ids_enabled(dir.path(), &[]);
    let root = tasks_root(&paths, &vault);
    write(
        &root,
        "p.md",
        "---\ntype: Task\nstatus: new\ntitle: \"P\"\nparent-id: c\n---\n",
    );
    write(
        &root,
        "c.md",
        "---\ntype: Task\nstatus: new\ntitle: \"C\"\ntask-id: c\n---\n",
    );
    assert!(set_task_parent(&paths, &vault, &root.join("c.md"), Some(&root.join("p.md"))).is_err());
    // And nothing was stamped onto the id-less parent by the failed attempt.
    assert!(!std::fs::read_to_string(root.join("p.md"))
        .unwrap()
        .contains("task-id:"));
}

#[test]
fn refuses_a_cycle_using_dormant_ids_while_generation_is_disabled() {
    // Hand-authored ids exist even with the feature off; the ordinary
    // list_tasks walk suppresses them, so validation must read the property
    // unconditionally or this passes vacuously and creates a real cycle.
    let dir = tempfile::tempdir().unwrap();
    let (paths, vault) = fixture_with_ids_disabled(dir.path(), &[]);
    let root = tasks_root(&paths, &vault);
    write(
        &root,
        "a.md",
        "---\ntype: Task\nstatus: new\ntitle: \"A\"\ntask-id: a\nparent-id: b\n---\n",
    );
    write(
        &root,
        "b.md",
        "---\ntype: Task\nstatus: new\ntitle: \"B\"\ntask-id: b\n---\n",
    );
    // A already points at B; making A the parent of B closes the loop.
    let err = set_task_parent(&paths, &vault, &root.join("b.md"), Some(&root.join("a.md")));
    assert!(err.is_err(), "a cycle through dormant ids must be refused");
}

#[test]
fn refuses_an_ambiguous_parent_id() {
    let dir = tempfile::tempdir().unwrap();
    let (paths, vault) = fixture_with_ids_enabled(dir.path(), &[]);
    let root = tasks_root(&paths, &vault);
    write(
        &root,
        "p1.md",
        "---\ntype: Task\nstatus: new\ntitle: \"P\"\ntask-id: dup\n---\n",
    );
    write(
        &root,
        "p2.md",
        "---\ntype: Task\nstatus: new\ntitle: \"P2\"\ntask-id: dup\n---\n",
    );
    write(
        &root,
        "c.md",
        "---\ntype: Task\nstatus: new\ntitle: \"C\"\n---\n",
    );
    assert!(set_task_parent(
        &paths,
        &vault,
        &root.join("c.md"),
        Some(&root.join("p1.md"))
    )
    .is_err());
}

#[test]
fn enables_ids_stamps_both_and_writes_a_resolvable_pair() {
    // The bootstrap: IDs off (the default), so no id is surfaced anywhere —
    // the parent is named by PATH and the service does the rest.
    let dir = tempfile::tempdir().unwrap();
    let (paths, vault) = fixture_with_ids_disabled(dir.path(), &["p.md", "c.md"]);
    let root = tasks_root(&paths, &vault);
    let out =
        set_task_parent(&paths, &vault, &root.join("c.md"), Some(&root.join("p.md"))).unwrap();
    let pid = out.parent_id.unwrap();
    assert!(!pid.is_empty());
    assert!(out.ids_enabled, "this call turned Task IDs on");
    assert!(config_for(&paths, &vault).task_id_enabled); // auto-enabled
    assert!(std::fs::read_to_string(root.join("p.md"))
        .unwrap()
        .contains(&format!("task-id: {pid}")));
    let child = std::fs::read_to_string(root.join("c.md")).unwrap();
    assert!(child.contains(&format!("parent-id: {pid}")));
    assert!(child.contains("parent: \"[[")); // a link was written
}

/// A vault whose parent link is a MARKDOWN FALLBACK (the parent sits in a
/// List folder carrying a wikilink metacharacter), with the child nested
/// under `child_list`. Returns (paths, vault id, tasks root, child path).
fn fixture_with_a_fallback_link(
    dir: &Path,
    tasks_folder: &str,
    child_list: &str,
) -> (ServicePaths, String, PathBuf, PathBuf) {
    let (paths, _vault) = fixture(dir, "MyVault");
    std::fs::write(
        paths.config_json.as_ref().unwrap(),
        format!(
            r#"{{ "vaults": {{ "{VAULT}": {{ "taskIdEnabled": true, "tasksFolder": "{tasks_folder}" }} }} }}"#
        ),
    )
    .unwrap();
    let vault_id = VAULT.to_string();
    let root = tasks_root(&paths, &vault_id);
    std::fs::create_dir_all(&root).unwrap();
    // `Project#1` is a legal List folder; `#` has no wikilink escape, so
    // the composer must fall back to a percent-encoded markdown link —
    // the only form a move can stale.
    let parent = write(
        &root,
        "Project#1/p.md",
        "---\ntype: Task\nstatus: new\ntitle: \"P\"\n---\n",
    );
    let child = write(
        &root,
        &format!("{child_list}/c.md"),
        "---\ntype: Task\nstatus: new\ntitle: \"C\"\n---\n",
    );
    set_task_parent(&paths, &vault_id, &child, Some(&parent)).unwrap();
    (paths, vault_id, root, child)
}

#[test]
fn moving_a_child_recomposes_its_link_under_a_nested_tasks_folder() {
    // tasks root = <vault>/Notes/Tasks, so vault_root != tasks_root.parent().
    // Getting this wrong silently drops a path segment from every link.
    let dir = tempfile::tempdir().unwrap();
    let (paths, vault, _root, child) =
        fixture_with_a_fallback_link(dir.path(), "Notes/Tasks", "Deep/Sub");
    let landed = move_task_to_list(&paths, &vault, &child.to_string_lossy(), "").unwrap();
    let out = std::fs::read_to_string(&landed.path).unwrap();
    // Vault-relative, `Notes/` included: deriving the vault root as
    // tasks_root.parent() would emit `../../Tasks/Project%231/p.md`.
    assert!(
        out.contains("](../../Notes/Tasks/Project%231/p.md)"),
        "link must be vault-relative, got {out}"
    );
}

#[test]
fn moving_a_child_recomposes_its_own_fallback_link() {
    // Child moves Tasks/Deep/Sub -> Tasks, so the ../ depth changes.
    let dir = tempfile::tempdir().unwrap();
    let (paths, vault, _root, child) =
        fixture_with_a_fallback_link(dir.path(), "Tasks", "Deep/Sub");
    let before = std::fs::read_to_string(&child).unwrap();
    assert!(
        before.contains("](../../../Tasks/Project%231/p.md)"),
        "the pre-move link is three levels deep, got {before}"
    );
    let landed = move_task_to_list(&paths, &vault, &child.to_string_lossy(), "").unwrap();
    let out = std::fs::read_to_string(&landed.path).unwrap();
    // One `../` now: the child sits at <vault>/Tasks/c.md, and a markdown
    // destination resolves from the note's OWN directory.
    assert!(out.contains("](../Tasks/Project%231/p.md)"), "got {out}");
}

#[test]
fn moving_a_child_with_an_unchanged_link_writes_nothing_extra() {
    // A wikilink is vault-relative, so a move cannot stale it — no rewrite.
    let dir = tempfile::tempdir().unwrap();
    let (paths, vault) = fixture_with_ids_enabled(dir.path(), &[]);
    let root = tasks_root(&paths, &vault);
    let parent = write(
        &root,
        "Plain/p.md",
        "---\ntype: Task\nstatus: new\ntitle: \"P\"\n---\n",
    );
    let child = write(
        &root,
        "Work/c.md",
        "---\ntype: Task\nstatus: new\ntitle: \"C\"\n---\n",
    );
    set_task_parent(&paths, &vault, &child, Some(&parent)).unwrap();
    let before = std::fs::read_to_string(&child).unwrap();
    assert!(before.contains("parent: \"[[Tasks/Plain/p]]\""), "{before}");
    let landed = move_task_to_list(&paths, &vault, &child.to_string_lossy(), "Home").unwrap();
    assert_eq!(std::fs::read_to_string(&landed.path).unwrap(), before);
}

#[test]
fn clearing_removes_both_keys() {
    let dir = tempfile::tempdir().unwrap();
    let (paths, vault) = fixture_with_ids_enabled(dir.path(), &["p.md", "c.md"]);
    let root = tasks_root(&paths, &vault);
    set_task_parent(&paths, &vault, &root.join("c.md"), Some(&root.join("p.md"))).unwrap();
    set_task_parent(&paths, &vault, &root.join("c.md"), None).unwrap();
    let child = std::fs::read_to_string(root.join("c.md")).unwrap();
    assert!(!child.contains("parent-id"));
    assert!(!child.contains("parent:"));
}

/// TASK 6b regression pin. The defect: config.json read-modify-writes were
/// serialized by TWO mutexes that did not exclude each other — this core
/// `config_write_lock()`, taken here, and a separate shell-only
/// `ConfigWriteLock` the IPC settings commands took instead. A capture
/// settings save could read `task_id_enabled: false`, race this function's
/// enable, and write `false` back over it via `config_merge::
/// merge_capture_owned`'s `task_id_enabled: existing.task_id_enabled` —
/// while the child it raced already carried a stamped `parent-id`,
/// orphaning the reference the instant it was created.
///
/// HONESTY NOTE (can't-go-red, by design): both threads below take
/// `config_write_lock()` — the ONE lock the shell now takes at every
/// config-write site after the fix. That is deliberate, not an oversight:
/// core has only ever had this one lock; the second mutex was a shell
/// (`src-tauri/src`) type built on `tauri::State`/`AppHandle`, which
/// cannot be constructed or invoked from a `core`-crate unit test — there
/// is no way to reach the actual pre-fix code path from here. So this
/// test passes identically before and after the fix; it does not catch
/// today's bug, it PINS the invariant so a future core write path that
/// forgets to take this lock reopens the same race. The fix itself is
/// structural, not something a core test can observe: `capture_commands::
/// ConfigWriteLock` no longer exists anywhere in the compiled shell crate
/// (see the task report), so there is no second lock left to pick by
/// mistake. The task report also documents a manual, uncommitted
/// experiment confirming this harness DOES fail reliably when thread B is
/// changed to skip the lock — proof the apparatus below is sensitive to
/// the class of bug being fixed, even though it cannot reach the specific
/// pre-fix shell code.
#[test]
fn concurrent_capture_save_and_parent_assignment_never_desync_task_id_enabled() {
    // Iterated (not asserted once): both threads share one lock, so every
    // ordering the OS scheduler produces must converge on a consistent
    // state — this stress-tests that claim across many orderings rather
    // than trusting a single lucky interleaving, and would also surface a
    // hang/deadlock if the lock were ever made reentrant-unsafe.
    for _ in 0..50 {
        let dir = tempfile::tempdir().unwrap();
        let (paths, vault) = fixture_with_ids_disabled(dir.path(), &["p.md", "c.md"]);
        let root = tasks_root(&paths, &vault);
        let parent = root.join("p.md");
        let child = root.join("c.md");
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));

        let thread_a = {
            let paths = paths.clone();
            let vault = vault.clone();
            let (parent, child) = (parent.clone(), child.clone());
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                set_task_parent(&paths, &vault, &child, Some(&parent))
            })
        };

        let thread_b = {
            let paths = paths.clone();
            let vault = vault.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || -> Result<(), String> {
                barrier.wait();
                // Mirrors set_capture_config exactly: read, change a
                // capture-owned field, merge (which preserves
                // task_id_enabled — config_merge.rs's clobbering line),
                // write — all under the ONE process-wide lock, with the
                // read INSIDE it so a concurrent writer's commit is never
                // read as stale (the same rule set_capture_config's own
                // doc comment states).
                let _guard = capture_config::config_write_lock();
                let existing = capture_config::vault_config(&app_config(&paths), &vault);
                let incoming = VaultCaptureConfig {
                    bitrate_kbps: 192,
                    ..VaultCaptureConfig::default()
                };
                let merged = capture_config::merge_capture_owned(&existing, incoming);
                capture_config::update_vault_config_at(
                    paths.config_json.as_ref().unwrap(),
                    &vault,
                    merged,
                )
                .map_err(|e| e.to_string())
            })
        };

        // Assert on thread A's Result explicitly, naming the invariant, rather
        // than a bare `.unwrap()`: when the lock discipline this test guards
        // breaks, the two threads' unsynchronized config.json writes collide
        // and set_task_parent comes back Err (an incidental temp-file race,
        // e.g. "No such file or directory") well before the desync assertion
        // below ever runs — a plain `.unwrap()` would panic on that Err with
        // the raw IO message, burying the actual invariant that broke.
        let a_result = thread_a.join().unwrap();
        let b_result = thread_b.join().unwrap();
        a_result.expect(
            "thread A's set_task_parent failed instead of losing the race cleanly to \
             the desync assertion below — both threads must serialize through the ONE \
             config_write_lock(), so a write racing outside it is the invariant this \
             test exists to catch, surfacing here as config.json read-modify-write \
             corruption rather than a clean, well-ordered outcome",
        );
        b_result.unwrap();

        let enabled = config_for(&paths, &vault).task_id_enabled;
        let child_has_parent_id = std::fs::read_to_string(&child)
            .unwrap()
            .contains("parent-id:");
        assert_eq!(
            child_has_parent_id, enabled,
            "interleaving a capture save with a parent assignment left the \
             vault inconsistent: child parent-id present={child_has_parent_id} \
             but vault task_id_enabled={enabled} — a child must never carry a \
             parent-id in a vault whose Task IDs are off"
        );
    }
}
