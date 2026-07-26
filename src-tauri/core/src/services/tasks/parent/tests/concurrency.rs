//! Concurrency/race regression tests for the parent-assignment write path,
//! split out of `tests/mod.rs` for the Rust LOC cap — these three tests are
//! grouped here on THEME, not just size: each one exists specifically to
//! catch a race landing in the narrow window between a phase-1 (lock-free)
//! check and this path's own `config_write_lock()` acquisition, which is
//! exactly the class of defect a single-threaded test can never observe
//! (see each test's own doc comment for its specific race). `use super::*`
//! brings in every fixture helper `tests/mod.rs` already defines — same
//! module tree, so nothing here needed new visibility.

use super::*;

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

#[test]
fn the_under_lock_recheck_refuses_a_cycle_a_concurrent_write_would_otherwise_create() {
    // Fix 4 (final whole-branch review, task report): the under-lock
    // re-check inside `set_task_parent`'s own `resolve_parent_for_write` call
    // (the closure above, which reads `tasks::parent_index_for_validation`)
    // exists ONLY to catch a concurrent write landing between phase 1's
    // (lock-free) validation and THIS call's own lock acquisition — so it
    // must be validated against the VALIDATION index, never the DISPLAY
    // index (`parent_index`, which drops a pre-existing on-disk cycle's
    // edges so the UI can render both rows parentless). The existing
    // regression coverage (`refuses_a_cycle_routed_through_an_uppercase_md_
    // task` and friends) only ever exercises phase 1's lock-free check,
    // which reads the SAME data before and after — a mutation of the
    // RECHECK's own index selection can never be observed there, since
    // nothing changes the graph mid-call in a single-threaded test. Only a
    // genuine concurrent write, landing in the narrow window this recheck
    // exists to close, can tell the two indices apart here.
    //
    // Setup: X and Y hand-author a mutual cycle (X's parent is Y, Y's parent
    // is X) — exactly like a pre-existing on-disk cycle a user's own
    // frontmatter can create. Z carries its own id and, initially, no
    // parent. Two concurrent calls race: thread A assigns "Z's parent = X",
    // thread B assigns "Y's parent = Z". Both PASS phase 1 (at that moment Z
    // has no parent yet, so neither assignment closes a cycle on the graph
    // either call's own lock-free scan sees). Whichever call wins the lock
    // commits first; the LOSER's under-lock recheck then reads the FRESH,
    // just-committed graph:
    //   - If A (Z's parent = X) commits first: B's recheck now sees X<->Y
    //     (unchanged) plus the just-landed Z->X. Assigning Y's parent = Z
    //     would close X -> Y -> Z -> X — a REAL new cycle. The VALIDATION
    //     index (retaining every edge) correctly refuses it. The DISPLAY
    //     index drops X<->Y's edges (they sit on a pre-existing on-disk
    //     cycle) but leaves Z->X untouched (Z is not itself cyclic) — so it
    //     wrongly reports NO cycle, and B's write lands, corrupting the vault.
    //   - If B (Y's parent = Z) commits first, it BREAKS the X<->Y cycle (Y's
    //     parent-id changes from x to z), so A's subsequent recheck sees a
    //     plain chain (X -> Y -> Z, no loop) — both indices agree there and
    //     correctly refuse A's write. This ordering never distinguishes the
    //     two indices, which is why the test is a PROPERTY over many
    //     iterations (Barrier-synchronized, like the sibling `concurrent_
    //     capture_save_and_parent_assignment_never_desync_task_id_enabled`
    //     race above) rather than a single deterministic interleaving: it
    //     asserts the specific 3-cycle X -> Y -> Z -> X can NEVER exist
    //     afterward, in ANY iteration, regardless of which side won the lock.
    for _ in 0..60 {
        let dir = tempfile::tempdir().unwrap();
        let (paths, vault) = fixture_with_ids_enabled(dir.path(), &[]);
        let root = tasks_root(&paths, &vault);
        write(
            &root,
            "x.md",
            "---\ntype: Task\nstatus: new\ntitle: \"X\"\ntask-id: x\nparent-id: y\n---\n",
        );
        write(
            &root,
            "y.md",
            "---\ntype: Task\nstatus: new\ntitle: \"Y\"\ntask-id: y\nparent-id: x\n---\n",
        );
        write(
            &root,
            "z.md",
            "---\ntype: Task\nstatus: new\ntitle: \"Z\"\ntask-id: z\n---\n",
        );
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
                set_task_parent(&paths, &vault, &z, Some(&x))
            })
        };
        let thread_b = {
            let paths = paths.clone();
            let vault = vault.clone();
            let (y, z) = (y.clone(), z.clone());
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                set_task_parent(&paths, &vault, &y, Some(&z))
            })
        };
        let _ = thread_a.join().unwrap();
        let _ = thread_b.join().unwrap();

        // The invariant: whichever call won the race, the specific cycle
        // X -> Y -> Z -> X must never exist on disk afterward.
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
