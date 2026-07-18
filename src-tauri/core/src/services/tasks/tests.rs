//! The task-service tests, split out of mod.rs for the Rust LOC cap —
//! same module position (`services::tasks::tests`), so every `super` path
//! is unchanged and the tests still sit beside the code they pin.

use super::super::test_support::fixture;
use super::*;

#[test]
fn add_list_and_toggle_tasks_through_the_service() {
    let dir = tempfile::tempdir().unwrap();
    let (paths, vault) = fixture(dir.path(), "MyVault");
    let created = add_task(
        &paths,
        "deadbeef01234567",
        "Buy milk",
        "2026-07-09",
        None,
        None,
        &[],
        None,
    )
    .unwrap();
    assert_eq!(created.title, "Buy milk");
    assert!(!created.done);
    assert!(vault.join("Tasks").is_dir());
    let listed = list_tasks(&paths, "deadbeef01234567");
    assert_eq!(listed.len(), 1);
    assert_eq!(count_open_tasks(&paths, "deadbeef01234567"), 1);
    let title = set_task_status(&paths, "deadbeef01234567", &created.path, "done").unwrap();
    assert_eq!(title, "Buy milk");
    assert_eq!(count_open_tasks(&paths, "deadbeef01234567"), 0);
}

#[test]
fn add_task_applies_the_vaults_configured_template() {
    // Task 11: add_task must thread the vault's task_extra_frontmatter/
    // task_body_template into create_task — a prior task left this as a
    // `None, None` placeholder pending this wiring.
    let dir = tempfile::tempdir().unwrap();
    let (paths, _vault) = fixture(dir.path(), "MyVault");
    std::fs::write(
        paths.config_json.as_ref().unwrap(),
        r#"{ "vaults": { "deadbeef01234567": {
            "taskExtraFrontmatter": "project: Alpha",
            "taskBodyTemplate": "- [ ] {{title}} by {{date}}"
        } } }"#,
    )
    .unwrap();
    let created = add_task(
        &paths,
        "deadbeef01234567",
        "Buy milk",
        "2026-07-09",
        None,
        None,
        &[],
        None,
    )
    .unwrap();
    let body = std::fs::read_to_string(&created.path).unwrap();
    assert!(
        body.contains("project: Alpha"),
        "configured extra frontmatter applied, got: {body}"
    );
    assert!(
        body.ends_with("- [ ] Buy milk by 2026-07-09\n"),
        "configured body template substituted, got: {body}"
    );
}

#[test]
fn add_task_writes_a_generated_id_when_enabled() {
    let dir = tempfile::tempdir().unwrap();
    let (paths, _vault) = fixture(dir.path(), "MyVault");
    std::fs::write(
        paths.config_json.as_ref().unwrap(),
        r#"{ "vaults": { "deadbeef01234567": { "taskIdEnabled": true, "taskIdProperty": "uid" } } }"#,
    )
    .unwrap();
    let created = add_task(
        &paths,
        "deadbeef01234567",
        "Buy milk",
        "2026-07-09",
        None,
        None,
        &[],
        None,
    )
    .unwrap();
    let body = std::fs::read_to_string(&created.path).unwrap();
    let line = body
        .lines()
        .find(|l| l.starts_with("uid: "))
        .expect("id line present");
    let id = line.trim_start_matches("uid: ");
    assert_eq!(id.len(), 8);
    assert!(id
        .chars()
        .all(|c| c.is_ascii_digit() || c.is_ascii_lowercase()));
}

#[test]
fn add_task_writes_no_id_when_disabled() {
    let dir = tempfile::tempdir().unwrap();
    let (paths, _vault) = fixture(dir.path(), "MyVault");
    let created = add_task(
        &paths,
        "deadbeef01234567",
        "Buy milk",
        "2026-07-09",
        None,
        None,
        &[],
        None,
    )
    .unwrap();
    assert!(!std::fs::read_to_string(&created.path)
        .unwrap()
        .contains("task-id"));
}

#[test]
fn add_task_skips_id_for_a_reserved_property() {
    // config.json is hand-editable: set_task_id_config rejects a reserved
    // property name, but a hand edit bypasses that gate. A reserved
    // property (here "status") must not make render_task emit a SECOND
    // structured `status:` line — is_valid_id_property must gate
    // generation here too, not just in the settings command.
    let dir = tempfile::tempdir().unwrap();
    let (paths, _vault) = fixture(dir.path(), "MyVault");
    std::fs::write(
        paths.config_json.as_ref().unwrap(),
        r#"{ "vaults": { "deadbeef01234567": { "taskIdEnabled": true, "taskIdProperty": "status" } } }"#,
    )
    .unwrap();
    let created = add_task(
        &paths,
        "deadbeef01234567",
        "Buy milk",
        "2026-07-09",
        None,
        None,
        &[],
        None,
    )
    .unwrap();
    let body = std::fs::read_to_string(&created.path).unwrap();
    assert_eq!(
        body.matches("status:").count(),
        1,
        "a reserved id property must not duplicate the status: line, got: {body}"
    );
}

#[test]
fn list_tasks_reads_the_generated_id_when_the_property_is_valid() {
    // Positive-case companion to the reserved-property regression below:
    // a valid, non-reserved configured property must round-trip through
    // list_tasks as TaskDto.id, same as what add_task stamped.
    let dir = tempfile::tempdir().unwrap();
    let (paths, _vault) = fixture(dir.path(), "MyVault");
    std::fs::write(
        paths.config_json.as_ref().unwrap(),
        r#"{ "vaults": { "deadbeef01234567": { "taskIdEnabled": true, "taskIdProperty": "uid" } } }"#,
    )
    .unwrap();
    let created = add_task(
        &paths,
        "deadbeef01234567",
        "Buy milk",
        "2026-07-09",
        None,
        None,
        &[],
        None,
    )
    .unwrap();
    let id = created
        .id
        .expect("add_task must stamp an id for a valid property");
    let listed = list_tasks(&paths, "deadbeef01234567");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id.as_deref(), Some(id.as_str()));
}

// Codex P2 (PR #59, services.rs:204): list_tasks computed the id property
// to read without validating it, so a config hand-edited to a RESERVED
// property (e.g. "status") made the scanner read that structured field's
// OWN value as the id — surfacing status "new" as TaskDto.id — even
// though add_task gates generation through tasks::id_property_for_
// generation and would never stamp an id under that property. The read
// must apply the exact same gate as the write.
#[test]
fn list_tasks_ignores_a_reserved_id_property_configured_by_hand() {
    let dir = tempfile::tempdir().unwrap();
    let (paths, _vault) = fixture(dir.path(), "MyVault");
    std::fs::write(
        paths.config_json.as_ref().unwrap(),
        r#"{ "vaults": { "deadbeef01234567": { "taskIdEnabled": true, "taskIdProperty": "status" } } }"#,
    )
    .unwrap();
    let created = add_task(
        &paths,
        "deadbeef01234567",
        "Buy milk",
        "2026-07-09",
        None,
        None,
        &[],
        None,
    )
    .unwrap();
    assert_eq!(
        created.id, None,
        "a reserved property must not be stamped (pre-existing add_task gate)"
    );
    let listed = list_tasks(&paths, "deadbeef01234567");
    assert_eq!(listed.len(), 1);
    assert_eq!(
        listed[0].id, None,
        "list_tasks must not surface the task's own status: value (\"new\") as its id"
    );
}

#[test]
fn count_open_tasks_excludes_open_tasks_in_archived_lists() {
    // The vault-row badge must agree with the DEFAULT Lists grouping,
    // which hides an archived list's open tasks: a vault whose only open
    // tasks sat in an archived list showed a nonzero badge over an
    // empty-looking view — the same phantom count the frontend's
    // visibleTasks fix removed one layer up (review, PR #59). The match
    // is case-insensitive and exact (a nested sub-list of an archived
    // list still renders, so it still counts).
    let dir = tempfile::tempdir().unwrap();
    let (paths, _vault) = fixture(dir.path(), "MyVault");
    std::fs::write(
        paths.config_json.as_ref().unwrap(),
        r#"{ "vaults": { "deadbeef01234567": { "archivedLists": ["Old"] } } }"#,
    )
    .unwrap();
    let mk = |title: &str, list: &str| {
        add_task(
            &paths,
            "deadbeef01234567",
            title,
            "2026-07-09",
            None,
            None,
            &[],
            Some(list),
        )
        .unwrap()
    };
    mk("Parked", "old"); // archived under a case variant → hidden → not counted
    mk("Rooted", ""); // No list → counted
    mk("Live one", "Live"); // visible list → counted
    mk("Nested", "Old/Sub"); // sub-list of an archived list still renders → counted
    let done = mk("Done parked", "old"); // done is out of the count regardless
    set_task_status(&paths, "deadbeef01234567", &done.path, "done").unwrap();
    assert_eq!(count_open_tasks(&paths, "deadbeef01234567"), 3);
}

#[test]
fn task_service_errors_mirror_the_command_layer() {
    let dir = tempfile::tempdir().unwrap();
    let (paths, _) = fixture(dir.path(), "MyVault");
    assert!(add_task(
        &paths,
        "deadbeef01234567",
        "   ",
        "2026-07-09",
        None,
        None,
        &[],
        None,
    )
    .is_err());
    // The spec requires MCP/IPC failures to carry the same user-facing
    // message the panel shows ("was it removed from Obsidian?"), not a
    // terse internal one that leaks only a raw hex id.
    let err = add_task(&paths, "unknown", "x", "2026-07-09", None, None, &[], None)
        .err()
        .expect("unknown vault must fail");
    assert!(err.contains("was it removed from Obsidian?"), "got: {err}");
    assert!(
        set_task_status(&paths, "deadbeef01234567", "whatever.md", "bogus")
            .unwrap_err()
            .contains("Unknown task status")
    );
    assert!(list_tasks(&paths, "unknown").is_empty());
}

#[test]
fn add_task_refuses_a_missing_vault_dir() {
    // A stale registry must not resurrect a deleted vault (same guard as
    // the IPC command).
    let dir = tempfile::tempdir().unwrap();
    let (paths, vault) = fixture(dir.path(), "MyVault");
    let vault_str = vault.to_string_lossy().into_owned();
    std::fs::remove_dir_all(&vault).unwrap();
    let err = add_task(
        &paths,
        "deadbeef01234567",
        "x",
        "2026-07-09",
        None,
        None,
        &[],
        None,
    )
    .err()
    .expect("missing vault dir must fail");
    // GAP-26 remainder: the user-facing message must not leak the
    // absolute vault path (it reaches the panel toast and MCP clients
    // verbatim) and must match the shell's own start_capture copy.
    assert_eq!(err, "Vault folder not found — was it moved or deleted?");
    assert!(!err.contains(&vault_str), "got: {err}");
}

#[test]
fn add_task_lands_in_the_picked_list() {
    let dir = tempfile::tempdir().unwrap();
    let (paths, vault) = fixture(dir.path(), "MyVault");
    let created = add_task(
        &paths,
        "deadbeef01234567",
        "Buy milk",
        "2026-07-09",
        None,
        None,
        &[],
        Some("Inbox"),
    )
    .unwrap();
    assert_eq!(created.list, "Inbox");
    assert!(vault.join("Tasks").join("Inbox").is_dir());
    assert!(std::path::Path::new(&created.path).starts_with(vault.join("Tasks").join("Inbox")));
    let listed = list_tasks(&paths, "deadbeef01234567");
    assert_eq!(listed[0].list, "Inbox");
}

#[cfg(unix)]
#[test]
fn add_task_rejects_a_list_symlinked_outside_the_tasks_root() {
    // A list folder that is a symlink under Tasks/ but resolves to ANOTHER
    // folder in the same vault passes vault-containment yet escapes the
    // tasks root. create_task must not write through it: the read-side
    // walkers (task_lists/list_tasks) skip such escaping folders, so the
    // add would silently succeed while the task vanishes from the Tasks
    // view. Validated against the tasks root, the add is rejected and no
    // task lands in the escape target (Codex, PR #53 re-review).
    let dir = tempfile::tempdir().unwrap();
    let (paths, vault) = fixture(dir.path(), "MyVault");
    let tasks = vault.join("Tasks");
    std::fs::create_dir_all(&tasks).unwrap();
    let elsewhere = vault.join("Elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    // Tasks/Work → <vault>/Elsewhere: inside the vault, outside the tasks root.
    std::os::unix::fs::symlink(&elsewhere, tasks.join("Work")).unwrap();
    let res = add_task(
        &paths,
        "deadbeef01234567",
        "Escapes",
        "2026-07-09",
        None,
        None,
        &[],
        Some("Work"),
    );
    assert!(
        res.is_err(),
        "a list escaping the tasks root must be rejected"
    );
    // No task file may land in the escape target.
    assert!(
        std::fs::read_dir(&elsewhere).unwrap().next().is_none(),
        "no task may be written outside the tasks root"
    );
}

#[cfg(unix)]
#[test]
fn add_task_rejects_a_nested_list_through_an_escaping_link_without_mkdir() {
    // Tasks/Link → <vault>/Elsewhere (a symlink inside the vault, outside
    // the tasks root). Adding to "Link/Sub" must be rejected BEFORE
    // create_dir_all can follow the link and mkdir "Sub" outside the tasks
    // root — the nearest existing ancestor is validated against the tasks
    // root pre-create, so no stray folder is created (vault is sacred; the
    // post-create check alone would reject the task but leave the mkdir'd
    // folder behind). (Codex, PR #53 re-review.)
    let dir = tempfile::tempdir().unwrap();
    let (paths, vault) = fixture(dir.path(), "MyVault");
    let tasks = vault.join("Tasks");
    std::fs::create_dir_all(&tasks).unwrap();
    let elsewhere = vault.join("Elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    std::os::unix::fs::symlink(&elsewhere, tasks.join("Link")).unwrap();
    let res = add_task(
        &paths,
        "deadbeef01234567",
        "Nested",
        "2026-07-09",
        None,
        None,
        &[],
        Some("Link/Sub"),
    );
    assert!(
        res.is_err(),
        "a nested list escaping the tasks root must be rejected"
    );
    assert!(
        !elsewhere.join("Sub").exists(),
        "no directory may be created outside the tasks root"
    );
}

#[cfg(unix)]
#[test]
fn add_task_with_an_escaping_default_list_degrades_to_the_root() {
    // A configured default that escapes the tasks root (here a symlink to a
    // sibling vault folder) must NOT break unpicked quick-adds — they
    // degrade to the tasks root, the read-lenient posture normalize_list_rel
    // already gives a lexically-unsafe default. An explicit pick of the
    // same list still errors (Codex, PR #53 re-review).
    let dir = tempfile::tempdir().unwrap();
    let (paths, vault) = fixture(dir.path(), "MyVault");
    std::fs::write(
        paths.config_json.as_ref().unwrap(),
        r#"{ "vaults": { "deadbeef01234567": { "defaultList": "Escape" } } }"#,
    )
    .unwrap();
    let tasks = vault.join("Tasks");
    std::fs::create_dir_all(&tasks).unwrap();
    let elsewhere = vault.join("Elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    std::os::unix::fs::symlink(&elsewhere, tasks.join("Escape")).unwrap();
    // Unpicked add (list: None) degrades to the root instead of erroring.
    let created = add_task(
        &paths,
        "deadbeef01234567",
        "Quick",
        "2026-07-09",
        None,
        None,
        &[],
        None,
    )
    .unwrap();
    assert_eq!(created.list, ""); // landed at the tasks root, not "Escape"
    assert!(std::path::Path::new(&created.path).starts_with(&tasks));
    // Nothing leaked through the link.
    assert!(elsewhere.read_dir().unwrap().next().is_none());
    // An explicit pick of the same escaping list still errors.
    assert!(add_task(
        &paths,
        "deadbeef01234567",
        "Boom",
        "2026-07-09",
        None,
        None,
        &[],
        Some("Escape")
    )
    .is_err());
}

#[test]
fn add_task_honors_the_config_default_list_and_explicit_root_overrides() {
    // None → the vault's configured defaultList (so MCP adds follow it
    // too); an explicit "" is the caller saying "the tasks root", which
    // must override the default rather than fall back to it.
    let dir = tempfile::tempdir().unwrap();
    let (paths, vault) = fixture(dir.path(), "MyVault");
    std::fs::write(
        paths.config_json.as_ref().unwrap(),
        r#"{ "vaults": { "deadbeef01234567": { "defaultList": "Inbox" } } }"#,
    )
    .unwrap();
    let defaulted = add_task(
        &paths,
        "deadbeef01234567",
        "A",
        "2026-07-09",
        None,
        None,
        &[],
        None,
    )
    .unwrap();
    assert_eq!(defaulted.list, "Inbox");
    assert!(vault.join("Tasks").join("Inbox").is_dir());
    let rooted = add_task(
        &paths,
        "deadbeef01234567",
        "B",
        "2026-07-09",
        None,
        None,
        &[],
        Some(""),
    )
    .unwrap();
    assert_eq!(rooted.list, "");
    assert!(std::path::Path::new(&rooted.path)
        .parent()
        .unwrap()
        .ends_with("Tasks"));
}

#[test]
fn add_task_rejects_an_escaping_list_but_degrades_a_bad_default() {
    // Explicit input is write-strict (an inline error); a hand-edited
    // config default is read-lenient (degrades to the root with a warn) —
    // a bad default must never block adding tasks. `.hidden` is the second
    // vector (Codex, PR #53): a dot-prefixed list would land the task in a
    // walk-skipped folder, so it is rejected/degraded exactly like `..`.
    let dir = tempfile::tempdir().unwrap();
    let (paths, vault) = fixture(dir.path(), "MyVault");
    for explicit in ["../escape", ".hidden", "Work/.hidden"] {
        assert!(
            add_task(
                &paths,
                "deadbeef01234567",
                "x",
                "2026-07-09",
                None,
                None,
                &[],
                Some(explicit),
            )
            .is_err(),
            "explicit list {explicit:?} must error"
        );
    }
    std::fs::write(
        paths.config_json.as_ref().unwrap(),
        r#"{ "vaults": { "deadbeef01234567": { "defaultList": ".hidden" } } }"#,
    )
    .unwrap();
    let created = add_task(
        &paths,
        "deadbeef01234567",
        "x",
        "2026-07-09",
        None,
        None,
        &[],
        None,
    )
    .unwrap();
    assert_eq!(created.list, "", "bad default degrades to the root");
    // And the task is really at the root, not a hidden folder.
    assert_eq!(
        std::path::Path::new(&created.path).parent().unwrap(),
        vault.join("Tasks")
    );
}
