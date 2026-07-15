# Tasks Feature Improvements Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make Lists the first/default task grouping, make drag-and-drop the default sort, add a discoverable "New list" control in the Lists view, and add a per-vault opt-in that stamps a short random ID onto tasks under a configurable frontmatter property.

**Architecture:** Backend-first, core-crate-first. New per-vault config fields (`task_id_enabled`, `task_id_property`) flow through the existing `services::add_task` create path (ID written at creation) and a new `ensure_absent` parameter on the surgical writer (`update_task_fields` stamps a missing ID on edit/reorder, never overwrites). A new independent settings command persists the two fields. The frontend flips two defaults, reorders the grouping row, and adds a settings card + a Lists-view create control — reusing the existing `useTaskLists.createList` fan-out.

**Tech Stack:** Rust (Tauri v2 shell + pure `core` crate), `getrandom` for the CSPRNG, Vue 3 + Pinia + Tailwind, Vitest (happy-dom + `mockIPC`).

## Global Constraints

- **Spec:** `docs/superpowers/specs/2026-07-15-tasks-lists-first-drag-default-and-task-ids-design.md`. Every task implements part of it.
- **TDD:** failing test first, then minimal code. Regression tests name the failure mode in a comment.
- **Commits:** Conventional Commits with a repo scope (`feat(tasks)`, `fix(ui)`, `docs(tasks)`, `test(core)`, …), imperative subject, body explains the *why*. Git identity is already `Claude <noreply@anthropic.com>`; the session appends `Co-Authored-By:`/`Claude-Session:` trailers at commit time.
- **ID format:** 8 characters of base36 (`0-9a-z`) from `getrandom`. Never scanned/sequential.
- **ID property default:** `"task-id"`. Reserved keys forbidden: `type`, `status`, `title`, `created`, `due`, `priority`, `tags`, `tag`, `order`.
- **Never overwrite an existing ID**; stamping only inserts an *absent* property. Status toggles/archive never stamp.
- **Vault is sacred:** no bulk backfill, no moving/rewriting existing files. All changes additive and default-off / default-preserving.
- **Rust core tests:** `cd src-tauri/core && cargo test`. Rust fmt: `cd src-tauri && cargo fmt`. Frontend: `npx vitest run tests/<file>.test.ts`.
- **Sort persistence:** changing the default sort must NOT change a view that already has a stored preference.

## File structure

Created:
- `src-tauri/core/src/tasks/id.rs` — `new_task_id()` + `is_valid_id_property()`.

Modified (Rust):
- `src-tauri/core/src/vault_config.rs` — two config fields + resolver + parse/serialize + tests.
- `src-tauri/core/Cargo.toml` — add `getrandom`.
- `src-tauri/core/src/tasks/mod.rs` — export the new `id` module fns.
- `src-tauri/core/src/tasks/disk.rs` — `render_task`/`create_task` ID param; `update_task_fields` `ensure_absent` param.
- `src-tauri/core/src/services.rs` — `add_task` writes the ID at creation.
- `src-tauri/src/capture_config_commands.rs` — preserve the two fields in `set_capture_config`.
- `src-tauri/src/task_commands.rs` — `TasksConfigDto` fields, `set_task_id_config`, `update_task` stamps.
- `src-tauri/src/lib.rs` — register `set_task_id_config`.

Modified (frontend):
- `src/utils/taskSort.ts` — default sort → `manual`.
- `src/components/TaskViewControls.vue` — Lists-first order; New-list control.
- `src/components/Tasks.vue` — default grouping → `lists`; wire the New-list control.
- `src/components/TasksConfigTab.vue` — Task IDs settings card.
- `src/types.ts` — `TasksConfig` gains `taskIdEnabled`/`taskIdProperty`.
- Tests: `tests/task-sort.test.ts`, `tests/tasks.test.ts`, `tests/tasks-config-tab.test.ts`, `tests/helpers/taskMount.ts`.

Docs (final task): `AGENTS.md`, `CONTEXT.md`, `docs/Gaps.md`, baselines.

---

### Task 1: Core config fields (`task_id_enabled`, `task_id_property`)

**Files:**
- Modify: `src-tauri/core/src/vault_config.rs` (struct ~50-86, `Default` ~88-111, `impl` ~126-164, `vault_entry` ~170-267, `serialize_vault_entry` ~271-321, test ~539-559)
- Modify: `src-tauri/src/capture_config_commands.rs:108-138`
- Test: `src-tauri/core/src/vault_config.rs` (tests module)

**Interfaces:**
- Produces: `VaultCaptureConfig.task_id_enabled: bool`, `VaultCaptureConfig.task_id_property: Option<String>`, `VaultCaptureConfig::task_id_property_name(&self) -> &str`. JSON keys `taskIdEnabled`, `taskIdProperty`.

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module in `src-tauri/core/src/vault_config.rs`:

```rust
    #[test]
    fn task_id_settings_default_off_and_resolve_property() {
        let v = VaultCaptureConfig::default();
        assert!(!v.task_id_enabled);
        assert_eq!(v.task_id_property, None);
        assert_eq!(v.task_id_property_name(), "task-id"); // None → default
        let custom = VaultCaptureConfig {
            task_id_property: Some("uid".into()),
            ..VaultCaptureConfig::default()
        };
        assert_eq!(custom.task_id_property_name(), "uid");
    }

    #[test]
    fn task_id_settings_parse_defensively_and_round_trip() {
        // Trimmed on read; one malformed field defaults only itself.
        let cfg = parse_config(
            r#"{ "vaults": { "a": { "taskIdEnabled": true, "taskIdProperty": "  uid  " } } }"#,
        );
        let a = vault_config(&cfg, "a");
        assert!(a.task_id_enabled);
        assert_eq!(a.task_id_property.as_deref(), Some("uid"));
        let bad = parse_config(
            r#"{ "vaults": { "a": { "taskIdEnabled": "yes", "taskIdProperty": 7, "mode": "voice-note" } } }"#,
        );
        let a = vault_config(&bad, "a");
        assert!(!a.task_id_enabled);
        assert_eq!(a.task_id_property, None);
        assert_eq!(a.mode, RecordingMode::VoiceNote);
        // Round-trip preserves; serialize omits defaults.
        let mut c = AppConfig::default();
        c.vaults.insert(
            "a".into(),
            VaultCaptureConfig {
                task_id_enabled: true,
                task_id_property: Some("uid".into()),
                ..VaultCaptureConfig::default()
            },
        );
        assert_eq!(parse_config(&serialize_config(&c)).vaults, c.vaults);
        let mut d = AppConfig::default();
        d.vaults.insert("b".into(), VaultCaptureConfig::default());
        let jd = serialize_config(&d);
        assert!(!jd.contains("taskIdEnabled"));
        assert!(!jd.contains("taskIdProperty"));
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd src-tauri/core && cargo test task_id_settings`
Expected: FAIL — `no field task_id_enabled`, `no method task_id_property_name`.

- [ ] **Step 3: Add the struct fields, default, and resolver**

In `src-tauri/core/src/vault_config.rs`, add to `struct VaultCaptureConfig` (immediately after the `list_order: Vec<String>,` field, ~line 78):

```rust
    /// Whether NEW/edited tasks get a generated ID written under
    /// `task_id_property` (opt-in, default false).
    pub task_id_enabled: bool,
    /// Frontmatter property the generated task ID is written under.
    /// None → the default "task-id".
    pub task_id_property: Option<String>,
```

Add to the `Default` impl (after `list_order: Vec::new(),`, ~line 106):

```rust
            task_id_enabled: false,
            task_id_property: None,
```

Add to `impl VaultCaptureConfig` (after `tasks_root()`, ~line 158):

```rust
    /// The effective frontmatter property for generated task IDs
    /// (empty/None → the default "task-id").
    pub fn task_id_property_name(&self) -> &str {
        self.task_id_property
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("task-id")
    }
```

- [ ] **Step 4: Parse and serialize the fields**

In `vault_entry`, add before `recording_date_folders:` (~line 258):

```rust
        task_id_enabled: entry
            .get("taskIdEnabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(defaults.task_id_enabled),
        task_id_property: entry
            .get("taskIdProperty")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
```

In `serialize_vault_entry`, add before the `recording_date_folders` block (~line 314):

```rust
    if v.task_id_enabled {
        entry.insert("taskIdEnabled".to_string(), json!(true));
    }
    if let Some(prop) = &v.task_id_property {
        entry.insert("taskIdProperty".to_string(), json!(prop));
    }
```

- [ ] **Step 5: Fix the full-field round-trip test + shell construct**

In `src-tauri/core/src/vault_config.rs`, the test `config_round_trips_through_serialize_and_parse` (~line 539) constructs every field explicitly. Add after `list_order: vec![...],`:

```rust
                task_id_enabled: true,
                task_id_property: Some("uid".to_string()),
```

In `src-tauri/src/capture_config_commands.rs`, `set_capture_config` builds a full `VaultCaptureConfig`. Add after the `list_order: existing.list_order,` line (~line 129):

```rust
        // The Task ID settings own their command (set_task_id_config); a
        // capture save must never reset them (same read-inside-the-lock
        // preserve as default_list/list_order above).
        task_id_enabled: existing.task_id_enabled,
        task_id_property: existing.task_id_property,
```

- [ ] **Step 6: Run tests to verify they pass**

Run: `cd src-tauri/core && cargo test task_id_settings && cargo test config_round_trips`
Expected: PASS. Then `cargo test` (whole core crate) PASS.

- [ ] **Step 7: Format and commit**

```bash
cd src-tauri && cargo fmt
git add src-tauri/core/src/vault_config.rs src-tauri/src/capture_config_commands.rs
git commit -m "feat(tasks): add per-vault task-id config fields (enabled + property)"
```

---

### Task 2: ID generation + property validation (`tasks/id.rs`)

**Files:**
- Create: `src-tauri/core/src/tasks/id.rs`
- Modify: `src-tauri/core/src/tasks/mod.rs`
- Modify: `src-tauri/core/Cargo.toml`

**Interfaces:**
- Produces: `tasks::new_task_id() -> String` (8 base36 chars), `tasks::is_valid_id_property(name: &str) -> bool`.

- [ ] **Step 1: Add the `getrandom` dependency**

In `src-tauri/core/Cargo.toml`, under `[dependencies]` (already-present `getrandom = "0.3"` in the `mcp` crate is the version to match):

```toml
getrandom = "0.3"
```

- [ ] **Step 2: Write the failing tests (in the new file)**

Create `src-tauri/core/src/tasks/id.rs`:

```rust
//! Task ID generation + property-name validation. IDs are short random
//! handles (opt-in per vault) written under a configurable frontmatter
//! property, giving tasks a stable identifier for Dataview/links without a
//! vault scan or a cross-device sequential collision.

/// Reserved frontmatter keys the ID property must never collide with — the
/// structured task fields the surgical writer and reader own. Using one as
/// the ID property would let the ID writer clobber a real field.
const RESERVED_TASK_KEYS: &[&str] = &[
    "type", "status", "title", "created", "due", "priority", "tags", "tag", "order",
];

/// A short random task ID: 8 base36 characters (`0-9a-z`) from the OS CSPRNG.
pub fn new_task_id() -> String {
    const ALPHABET: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut bytes = [0u8; 8];
    // getrandom only fails on a broken OS RNG; a loud panic is correct here
    // (mirrors mcp::token::generate_token).
    getrandom::fill(&mut bytes).expect("OS RNG unavailable");
    bytes
        .iter()
        .map(|b| ALPHABET[*b as usize % 36] as char)
        .collect()
}

/// True iff `name` is a safe frontmatter key for the ID property: non-empty,
/// `[A-Za-z0-9_-]` only, and not a reserved structured task key.
pub fn is_valid_id_property(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        && !RESERVED_TASK_KEYS.contains(&name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_task_id_is_8_base36_chars_and_unique() {
        let a = new_task_id();
        assert_eq!(a.len(), 8);
        assert!(a
            .chars()
            .all(|c| c.is_ascii_digit() || c.is_ascii_lowercase()));
        // Weak uniqueness over a 36^8 space — a collision in 1000 draws is
        // effectively impossible; this pins that the source is actually random.
        let mut seen = std::collections::HashSet::new();
        for _ in 0..1000 {
            assert!(seen.insert(new_task_id()));
        }
    }

    #[test]
    fn is_valid_id_property_charset_and_reserved() {
        assert!(is_valid_id_property("task-id"));
        assert!(is_valid_id_property("uid_2"));
        assert!(!is_valid_id_property("")); // empty
        assert!(!is_valid_id_property("task id")); // space
        assert!(!is_valid_id_property("task:id")); // colon
        for reserved in [
            "type", "status", "title", "created", "due", "priority", "tags", "tag", "order",
        ] {
            assert!(!is_valid_id_property(reserved), "{reserved} must be rejected");
        }
    }
}
```

- [ ] **Step 3: Export from the tasks module**

In `src-tauri/core/src/tasks/mod.rs`, add `mod id;` (after `mod doc;`) and add the export:

```rust
pub use id::{is_valid_id_property, new_task_id};
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd src-tauri/core && cargo test id::tests`
Expected: PASS (2 tests).

Then verify no unused dep: `cd src-tauri && cargo machete .` → `getrandom` used by `vault_buddy_core`.

- [ ] **Step 5: Format and commit**

```bash
cd src-tauri && cargo fmt
git add src-tauri/core/src/tasks/id.rs src-tauri/core/src/tasks/mod.rs src-tauri/core/Cargo.toml src-tauri/Cargo.lock
git commit -m "feat(tasks): short random task-id generator and property validator"
```

---

### Task 3: `render_task` / `create_task` write the ID at creation

**Files:**
- Modify: `src-tauri/core/src/tasks/disk.rs` (`render_task` ~49-72, `create_task` ~79-93, all test call sites)

**Interfaces:**
- Consumes: nothing new.
- Produces: `render_task(title, created, due, priority, tags, task_id: Option<(&str, &str)>) -> String`; `create_task(root, title, today, due, priority, tags, task_id: Option<(&str, &str)>) -> io::Result<PathBuf>`. `task_id` is `Some((property, id))`.

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module in `src-tauri/core/src/tasks/disk.rs`:

```rust
    #[test]
    fn render_writes_the_id_property_after_created_when_present() {
        let doc = render_task("A", "2026-07-09", None, None, &[], Some(("task-id", "k3n7p2qz")));
        assert!(doc.contains("created: 2026-07-09\ntask-id: k3n7p2qz\n"));
        // Absent → byte-identical to the pre-id output (no id line).
        let plain = render_task("A", "2026-07-09", None, None, &[], None);
        assert!(!plain.contains("task-id"));
        assert_eq!(
            plain,
            "---\ntype: Task\nstatus: new\ntitle: \"A\"\ncreated: 2026-07-09\n---\n\n"
        );
    }

    #[test]
    fn create_task_writes_the_id_property() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Tasks");
        let p = create_task(&root, "Buy milk", "2026-07-08", None, None, &[], Some(("task-id", "abcd1234")))
            .unwrap();
        assert!(std::fs::read_to_string(&p).unwrap().contains("task-id: abcd1234\n"));
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd src-tauri/core && cargo test -p vault_buddy_core tasks::disk 2>&1 | head`
Expected: FAIL — `render_task` takes 5 args, not 6 (compile error).

- [ ] **Step 3: Add the `task_id` parameter**

In `render_task`, change the signature and prepend the id line to `extra`:

```rust
pub fn render_task(
    title: &str,
    created: &str,
    due: Option<&str>,
    priority: Option<&str>,
    tags: &[String],
    task_id: Option<(&str, &str)>,
) -> String {
    let mut extra = String::new();
    // The generated ID (when enabled) sits right after `created`, before the
    // widened fields. The value is charset-safe base36; the property was
    // validated on save, so neither needs YAML quoting.
    if let Some((prop, id)) = task_id {
        extra.push_str(&format!("{prop}: {id}\n"));
    }
    if let Some(d) = due {
        extra.push_str(&format!("due: {d}\n"));
    }
    if let Some(p) = priority {
        extra.push_str(&format!("priority: {p}\n"));
    }
    if !tags.is_empty() {
        extra.push_str(&format!("tags: [{}]\n", tags.join(", ")));
    }
    format!(
        "---\ntype: Task\nstatus: new\ntitle: {}\ncreated: {created}\n{extra}---\n\n",
        yaml_quote(title)
    )
}
```

In `create_task`, add the param and pass it through:

```rust
pub fn create_task(
    root: &Path,
    title: &str,
    today: &str,
    due: Option<&str>,
    priority: Option<&str>,
    tags: &[String],
    task_id: Option<(&str, &str)>,
) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(root)?;
    let target = root.join(format!("{}.md", task_basename(title, today)));
    crate::capture_note::write_note_collision_safe(
        &target,
        &render_task(title, today, due, priority, tags, task_id),
    )
}
```

- [ ] **Step 4: Update every existing call site to pass `None`**

The compiler now flags every existing `render_task(...)`/`create_task(...)` call. Append `, None` as the final argument to each. These are all in the `disk.rs` tests module: `set_task_status_writes_an_arbitrary_status`, `render_writes_type_task_status_new_quoted_title`, `render_quotes_a_colon_title`, `render_quotes_and_escapes_special_title`, `create_task_writes_file_and_never_clobbers` (two calls), `set_task_status_writes_and_rejects_escape`, `render_includes_due_and_priority_only_when_present` (two calls), `render_includes_flow_tags_only_when_present` (two calls). Example edit:

```rust
        let p = create_task(&root, "Buy milk", "2026-07-08", None, None, &[], None).unwrap();
```

(`services.rs::add_task`'s `create_task` call is updated in Task 5, not here — the workspace still compiles because Task 5's edit is the only non-`None` site and Task 5 changes it in the same crate; to keep this task self-contained, temporarily append `, None` to the `services.rs::add_task` call site too, then Task 5 replaces it.)

- [ ] **Step 5: Run tests to verify they pass**

Run: `cd src-tauri/core && cargo test`
Expected: PASS (whole core crate compiles and passes).

- [ ] **Step 6: Format and commit**

```bash
cd src-tauri && cargo fmt
git add src-tauri/core/src/tasks/disk.rs src-tauri/core/src/services.rs
git commit -m "feat(tasks): render_task/create_task write an optional id property"
```

---

### Task 4: `update_task_fields` stamps an absent key (`ensure_absent`)

**Files:**
- Modify: `src-tauri/core/src/tasks/disk.rs` (`update_task_fields` ~101-120, `set_task_status` ~123-125)
- Modify: `src-tauri/src/task_commands.rs:376` (pass `&[]` to keep it compiling — Task 6 wires the real value)

**Interfaces:**
- Produces: `update_task_fields(root, path, updates: &[(&str, Option<&str>)], ensure_absent: &[(&str, &str)]) -> Result<(), String>` — each `ensure_absent` key is written only when the key is absent from the file; a present value is never overwritten.

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module in `src-tauri/core/src/tasks/disk.rs`:

```rust
    #[test]
    fn update_task_fields_stamps_an_absent_ensure_key_but_never_overwrites() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Tasks");
        let p = create_task(&root, "A", "2026-07-08", None, None, &[], None).unwrap();
        // Absent → stamped alongside the edit.
        update_task_fields(&root, &p, &[("status", Some("done"))], &[("task-id", "abcd1234")]).unwrap();
        let body = std::fs::read_to_string(&p).unwrap();
        assert!(body.contains("status: done\n"));
        assert!(body.contains("task-id: abcd1234\n"));
        // Present → never overwritten (a second stamp with a new id is a no-op).
        update_task_fields(&root, &p, &[], &[("task-id", "zzzz9999")]).unwrap();
        assert!(std::fs::read_to_string(&p).unwrap().contains("task-id: abcd1234\n"));
    }

    #[test]
    fn set_task_status_does_not_stamp_any_id() {
        // A checkbox toggle is not an "edit": set_task_status passes no
        // ensure keys, so toggling never adds an id.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Tasks");
        let p = create_task(&root, "A", "2026-07-08", None, None, &[], None).unwrap();
        set_task_status(&root, &p, "done").unwrap();
        assert!(!std::fs::read_to_string(&p).unwrap().contains("task-id"));
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd src-tauri/core && cargo test -p vault_buddy_core tasks::disk 2>&1 | head`
Expected: FAIL — `update_task_fields` takes 3 args, not 4.

- [ ] **Step 3: Add the `ensure_absent` parameter**

In `src-tauri/core/src/tasks/disk.rs`, change `update_task_fields`:

```rust
pub fn update_task_fields(
    root: &Path,
    path: &Path,
    updates: &[(&str, Option<&str>)],
    ensure_absent: &[(&str, &str)],
) -> Result<(), String> {
    let canon_root =
        std::fs::canonicalize(root).map_err(|e| format!("Cannot resolve tasks folder: {e}"))?;
    let canon_path =
        std::fs::canonicalize(path).map_err(|e| format!("Cannot resolve task file: {e}"))?;
    if !canon_path.starts_with(&canon_root) {
        return Err("Task file is outside the vault's tasks folder".to_string());
    }
    let content =
        std::fs::read_to_string(&canon_path).map_err(|e| format!("Cannot read task: {e}"))?;
    // Stamp-if-absent keys (the generated task ID): included in the write only
    // when the property is not already present, so an existing/hand-authored
    // value is never overwritten and IDs stay stable.
    let mut effective: Vec<(&str, Option<&str>)> = updates.to_vec();
    for (key, val) in ensure_absent {
        if super::parse::scalar_field(&content, key).is_none() {
            effective.push((key, Some(val)));
        }
    }
    let updated = set_fields(&content, &effective).ok_or(
        "Task frontmatter could not be updated (not a type: Task document, or its frontmatter is malformed)",
    )?;
    crate::capture_note::write_atomic_replacing(&canon_path, &updated)
        .map_err(|e| format!("Cannot save task: {e}"))
}
```

In `set_task_status` (same file), pass no ensure keys:

```rust
pub fn set_task_status(root: &Path, path: &Path, new_status: &str) -> Result<(), String> {
    update_task_fields(root, path, &[("status", Some(new_status))], &[])
}
```

- [ ] **Step 4: Keep the shell caller compiling**

In `src-tauri/src/task_commands.rs`, the `update_task` command calls `tasks::update_task_fields(&root, Path::new(&path), &refs)` (~line 376). Append `, &[]` for now (Task 6 replaces it with the real ensure):

```rust
        tasks::update_task_fields(&root, Path::new(&path), &refs, &[])
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cd src-tauri/core && cargo test`
Expected: PASS (whole core crate).

- [ ] **Step 6: Format and commit**

```bash
cd src-tauri && cargo fmt
git add src-tauri/core/src/tasks/disk.rs src-tauri/src/task_commands.rs
git commit -m "feat(tasks): update_task_fields stamps an absent ensure key without overwriting"
```

---

### Task 5: `services::add_task` writes the ID at creation

**Files:**
- Modify: `src-tauri/core/src/services.rs` (`add_task` ~213-318, tests module)

**Interfaces:**
- Consumes: `VaultCaptureConfig.task_id_enabled`, `task_id_property_name()`, `tasks::new_task_id()`, `tasks::create_task(..., task_id)`.

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module in `src-tauri/core/src/services.rs`:

```rust
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
            &paths, "deadbeef01234567", "Buy milk", "2026-07-09", None, None, &[], None,
        )
        .unwrap();
        let body = std::fs::read_to_string(&created.path).unwrap();
        let line = body.lines().find(|l| l.starts_with("uid: ")).expect("id line present");
        let id = line.trim_start_matches("uid: ");
        assert_eq!(id.len(), 8);
        assert!(id.chars().all(|c| c.is_ascii_digit() || c.is_ascii_lowercase()));
    }

    #[test]
    fn add_task_writes_no_id_when_disabled() {
        let dir = tempfile::tempdir().unwrap();
        let (paths, _vault) = fixture(dir.path(), "MyVault");
        let created = add_task(
            &paths, "deadbeef01234567", "Buy milk", "2026-07-09", None, None, &[], None,
        )
        .unwrap();
        assert!(!std::fs::read_to_string(&created.path).unwrap().contains("task-id"));
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd src-tauri/core && cargo test add_task_writes 2>&1 | head`
Expected: FAIL — the id line is absent (or a compile error if the `create_task` call still passes `None` from Task 3's temporary edit).

- [ ] **Step 3: Generate and thread the ID**

In `src-tauri/core/src/services.rs`, in `add_task`, replace the `create_task` call (~line 304) with:

```rust
    // Generate a task ID when the vault opted in — written into the initial
    // file content by render_task. Borrows are valid for the create call:
    // the property name borrows `cfg`, the id borrows `generated_id`.
    let generated_id = cfg.task_id_enabled.then(tasks::new_task_id);
    let task_id = generated_id
        .as_deref()
        .map(|id| (cfg.task_id_property_name(), id));
    let path = tasks::create_task(&target_root, title, today, due, priority, tags, task_id)
        .map_err(|e| format!("Could not create task: {e}"))?;
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd src-tauri/core && cargo test`
Expected: PASS (whole core crate; the existing `add_task` tests still pass — they don't enable the setting).

- [ ] **Step 5: Format and commit**

```bash
cd src-tauri && cargo fmt
git add src-tauri/core/src/services.rs
git commit -m "feat(tasks): add_task writes a generated id when the vault opts in"
```

---

### Task 6: Shell — settings command, DTO fields, edit-time stamp

**Files:**
- Modify: `src-tauri/src/task_commands.rs` (`TasksConfigDto` ~8-16, `get_tasks_config` ~21-29, add `set_task_id_config`, `update_task` closure ~369-379)
- Modify: `src-tauri/src/lib.rs` (register `set_task_id_config`, ~line 441)

**Interfaces:**
- Produces IPC: `set_task_id_config(id: String, enabled: bool, property: Option<String>) -> Result<(), String>`; `get_tasks_config` returns `taskIdEnabled: bool` and `taskIdProperty: String` (the resolved name).

- [ ] **Step 1: Extend the DTO and `get_tasks_config`**

In `src-tauri/src/task_commands.rs`, add to `TasksConfigDto`:

```rust
    /// Whether generated task IDs are enabled for this vault.
    pub task_id_enabled: bool,
    /// The RESOLVED id property name (default "task-id" when unset) — the UI
    /// shows it as the placeholder/current value.
    pub task_id_property: String,
```

Update `get_tasks_config` (call the resolver before moving the owned fields):

```rust
#[tauri::command]
pub fn get_tasks_config(id: String) -> TasksConfigDto {
    let cfg = capture_config::vault_config(&capture_config::load_config(), &id);
    let task_id_property = cfg.task_id_property_name().to_string();
    TasksConfigDto {
        task_id_enabled: cfg.task_id_enabled,
        task_id_property,
        tasks_folder: cfg.tasks_folder,
        default_list: cfg.default_list,
        list_order: cfg.list_order,
    }
}
```

- [ ] **Step 2: Add the `set_task_id_config` command**

In `src-tauri/src/task_commands.rs`, after `set_task_lists_config` (~line 96):

```rust
/// Persist the vault's Task ID settings (enable + frontmatter property),
/// preserving every other per-vault field via the same read-modify-write
/// under ConfigWriteLock. Write-strict on the property: empty → the default
/// (stored as None); an invalid or reserved name is an inline error. Its own
/// command — the independent field-save pattern of set_task_lists_config.
///
/// ASYNC (GAP-22 class): the config write is fsync'd file I/O.
#[tauri::command]
pub async fn set_task_id_config(
    lock: tauri::State<'_, ConfigWriteLock>,
    id: String,
    enabled: bool,
    property: Option<String>,
) -> Result<(), String> {
    crate::commands::find_vault(&id)?;
    let property = match property.as_deref().map(str::trim) {
        None | Some("") => None,
        Some(p) if tasks::is_valid_id_property(p) => Some(p.to_string()),
        Some(p) => {
            return Err(format!(
                "Invalid ID property name (letters, digits, - and _ only; not a reserved task field): {p}"
            ))
        }
    };
    let _guard = lock_ignoring_poison(&lock.0);
    let mut value = capture_config::vault_config(&capture_config::load_config(), &id);
    value.task_id_enabled = enabled;
    value.task_id_property = property;
    capture_config::update_vault_config(&id, value)
}
```

- [ ] **Step 3: Stamp on edit in `update_task`**

In `src-tauri/src/task_commands.rs`, in the `update_task` command's `spawn_blocking` closure, replace the body (the block from `let (vault_path, root) = tasks_root_for(&id)?;` through the `update_task_fields` call) with:

```rust
    tauri::async_runtime::spawn_blocking(move || {
        let (vault_path, root) = tasks_root_for(&id)?;
        if root.exists() {
            capture_paths::assert_root_inside_vault(&vault_path, &root)?;
        }
        let refs: Vec<(&str, Option<&str>)> =
            updates.iter().map(|(k, v)| (*k, v.as_deref())).collect();
        // Stamp a generated ID when the vault opted in and the task lacks one
        // (update_task_fields only writes an ensure key that is absent). Any
        // update_task write — a field edit OR an order-only reorder — stamps.
        let cfg = capture_config::vault_config(&capture_config::load_config(), &id);
        let generated_id = cfg.task_id_enabled.then(tasks::new_task_id);
        let ensure: Vec<(&str, &str)> = match &generated_id {
            Some(idv) => vec![(cfg.task_id_property_name(), idv.as_str())],
            None => Vec::new(),
        };
        tasks::update_task_fields(&root, Path::new(&path), &refs, &ensure)
    })
    .await
    .map_err(|e| format!("update_task: task failed: {e}"))?
```

- [ ] **Step 4: Register the command**

In `src-tauri/src/lib.rs`, add to the `generate_handler!` list after `task_commands::set_task_lists_config,` (~line 441):

```rust
            task_commands::set_task_id_config,
```

- [ ] **Step 5: Verify it compiles + core still green**

Run: `cd src-tauri/core && cargo test` → PASS (core mechanism unchanged).
The shell crate compiles under the Linux gate (see AGENTS.md § What compiles where): `npm run setup:linux` once, then `cd src-tauri && cargo clippy -p vault-buddy --all-targets -- -D warnings`. If the GUI libs aren't installed in this environment, rely on the `linux-app` CI job for the shell compile/clippy and confirm `cargo test -p vault_buddy_core` locally.

- [ ] **Step 6: Format and commit**

```bash
cd src-tauri && cargo fmt
git add src-tauri/src/task_commands.rs src-tauri/src/lib.rs
git commit -m "feat(tasks): set_task_id_config command and edit-time id stamping"
```

---

### Task 7: Frontend — Lists-first grouping + drag-and-drop default sort

**Files:**
- Modify: `src/utils/taskSort.ts:107`
- Modify: `src/components/TaskViewControls.vue:24-28`
- Modify: `src/components/Tasks.vue:267`
- Test: `tests/task-sort.test.ts`, `tests/tasks.test.ts`

**Interfaces:**
- Produces: `DEFAULT_PREF = { key: "manual", dir: "asc" }`; default grouping `"lists"`; grouping button order `Lists, Dates, Tags`.

- [ ] **Step 1: Write/adjust the failing tests**

In `tests/task-sort.test.ts`, add a new test and update the existing default expectation (line ~120):

```ts
  it("an unconfigured view defaults to manual sort (drag-and-drop is standard)", () => {
    expect(loadSortPref("brand-new-view")).toEqual({ key: "manual", dir: "asc" });
  });
```

Change the existing assertion `expect(loadSortPref("vault-2")).toEqual({ key: "default", dir: "asc" });` to:

```ts
    expect(loadSortPref("vault-2")).toEqual({ key: "manual", dir: "asc" });
```

In `tests/tasks.test.ts`, replace the test `grouping defaults to dates and the toggle switches back` (~line 887) with:

```ts
  it("grouping defaults to lists and the toggle switches to dates", async () => {
    const { wrapper } = mountView({
      list_tasks: () => [
        { path: "C:/v/Tasks/a.md", title: "Tagged", status: "new", created: "2026-07-08", done: false, due: null, priority: null, tags: ["work"], list: "", order: null },
      ],
    });
    await flushPromises();
    // Lists mode by default: a root task shows the "No list" section header.
    expect(wrapper.get('[data-testid="task-grouping-lists"]').attributes("aria-checked")).toBe("true");
    expect(wrapper.findAll('[data-testid="task-bucket-header"]').length).toBeGreaterThan(0);
    // Switching to dates: an undated list shows no headers.
    await wrapper.get('[data-testid="task-grouping-dates"]').trigger("click");
    expect(wrapper.findAll('[data-testid="task-bucket-header"]')).toHaveLength(0);
  });
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `npx vitest run tests/task-sort.test.ts tests/tasks.test.ts`
Expected: FAIL — default pref is `default`; default grouping is `dates`.

- [ ] **Step 3: Flip the defaults**

In `src/utils/taskSort.ts` (~line 107):

```ts
const DEFAULT_PREF: TaskSortPref = { key: "manual", dir: "asc" };
```

In `src/components/Tasks.vue` (~line 267):

```ts
const grouping = ref<"dates" | "tags" | "lists">("lists");
```

In `src/components/TaskViewControls.vue`, reorder `GROUPINGS` (~line 24):

```ts
const GROUPINGS = [
  { key: "lists", label: "Lists" },
  { key: "dates", label: "Dates" },
  { key: "tags", label: "Tags" },
] as const;
```

- [ ] **Step 4: Run tests + the full suite to catch fallout**

Run: `npx vitest run tests/task-sort.test.ts tests/tasks.test.ts`
Expected: PASS.

Run the WHOLE suite: `npm test`. The default-sort flip makes `reorderView` true on open (grips render) and Lists the default grouping. Fix any test that assumed the old defaults — e.g. a test asserting no drag handles by default, or dates-grouping headers on open. Update each to the new default (do not weaken an assertion to hide a real regression). Expected after fixes: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/utils/taskSort.ts src/components/Tasks.vue src/components/TaskViewControls.vue tests/task-sort.test.ts tests/tasks.test.ts
git commit -m "feat(ui): lists-first grouping and drag-and-drop as the default task sort"
```

---

### Task 8: Frontend — "New list" control in the Lists view

**Files:**
- Modify: `src/components/TaskViewControls.vue` (props/emits + inline create UI)
- Modify: `src/components/Tasks.vue` (pass props, wire the emit)
- Test: `tests/tasks.test.ts`

**Interfaces:**
- Consumes: `useTaskLists.createList(name) => Promise<string | null>`, `creatingList` (already returned by the composable and used by `Tasks.vue`).
- Produces: `TaskViewControls` props `isAggregate: boolean`, `creatingList: boolean`; emit `(e: "create-list", name: string)`. Testids `task-newlist`, `task-newlist-input`, `task-newlist-confirm`, `task-newlist-cancel`.

- [ ] **Step 1: Write the failing test**

Add to `tests/tasks.test.ts`:

```ts
  it("creates a new list from the Lists view controls and shows the empty section", async () => {
    const created: string[] = [];
    const { wrapper } = mountView({
      list_task_lists: () => [],
      create_task_list: (args) => {
        const name = (args as { name: string }).name;
        created.push(name);
        return name; // the landed list name
      },
    });
    await flushPromises();
    // Lists grouping is the default → the New list button is visible.
    await wrapper.get('[data-testid="task-newlist"]').trigger("click");
    await wrapper.get('[data-testid="task-newlist-input"]').setValue("Inbox");
    await wrapper.get('[data-testid="task-newlist-confirm"]').trigger("click");
    await flushPromises();
    expect(created).toEqual(["Inbox"]);
    const headers = wrapper.findAll('[data-testid="task-bucket-header"]').map((h) => h.text());
    expect(headers).toContain("Inbox");
  });
```

- [ ] **Step 2: Run test to verify it fails**

Run: `npx vitest run tests/tasks.test.ts -t "creates a new list"`
Expected: FAIL — `[data-testid="task-newlist"]` not found.

- [ ] **Step 3: Add the create control to `TaskViewControls.vue`**

Extend the props and emits:

```ts
defineProps<{
  grouping: "dates" | "tags" | "lists";
  sortPref: TaskSortPref;
  isAggregate: boolean;
  creatingList: boolean;
}>();
defineEmits<{
  (e: "update:grouping", value: "dates" | "tags" | "lists"): void;
  (e: "setSortKey", key: SortKey): void;
  (e: "flipSortDir"): void;
  (e: "createList", name: string): void;
}>();
```

Add local state + handlers to the `<script setup>` (after `GROUPINGS`), importing `ref`:

```ts
import { ref } from "vue";

// Inline "New list" create — shown only in per-vault Lists grouping (the
// aggregate has no single target vault). Mirrors TaskListPicker's create UX:
// IME-guarded Enter, Escape stops propagation so it doesn't close the panel.
const newMode = ref(false);
const newName = ref("");
function openNew() {
  newMode.value = true;
  newName.value = "";
}
function confirmNew() {
  const name = newName.value.trim();
  if (!name || props.creatingList) return;
  emit("createList", name);
  newMode.value = false;
  newName.value = "";
}
function onNewEnter(e: KeyboardEvent) {
  if (e.isComposing) return;
  e.preventDefault();
  confirmNew();
}
function onNewEscape(e: KeyboardEvent) {
  if (e.isComposing) return;
  e.stopPropagation();
  newMode.value = false;
}
```

(Change `defineProps`/`defineEmits` to capture `props`/`emit`: `const props = defineProps<...>()` and `const emit = defineEmits<...>()`.)

Add the markup to the template, after the grouping `<div role="radiogroup">` block and before the `<div class="ml-auto ...">` sort block:

```html
    <div
      v-if="grouping === 'lists' && !isAggregate"
      class="flex items-center gap-1"
    >
      <button
        v-if="!newMode"
        type="button"
        data-testid="task-newlist"
        aria-label="New list"
        title="New list"
        class="cursor-pointer rounded-lg border border-white/10 bg-white/5 px-1.5 py-0.5 text-[10px] text-slate-300 transition-colors hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400"
        @click="openNew"
      >
        ＋ List
      </button>
      <template v-else>
        <input
          v-model="newName"
          data-testid="task-newlist-input"
          type="text"
          placeholder="List name"
          aria-label="New list name"
          class="min-w-0 flex-1 rounded-lg border border-white/10 bg-white/5 px-2 py-0.5 text-[10px] text-slate-100 placeholder:text-slate-500 focus:border-violet-400 focus:outline-none"
          @keydown.enter="onNewEnter"
          @keydown.esc="onNewEscape"
        >
        <button
          type="button"
          data-testid="task-newlist-confirm"
          :disabled="creatingList || newName.trim() === ''"
          aria-label="Create list"
          title="Create list"
          class="shrink-0 cursor-pointer rounded-lg border border-white/10 bg-white/5 px-1.5 py-0.5 text-[10px] text-slate-300 transition-colors hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400 disabled:cursor-default disabled:opacity-40"
          @click="confirmNew"
        >
          ✓
        </button>
        <button
          type="button"
          data-testid="task-newlist-cancel"
          aria-label="Cancel new list"
          title="Cancel"
          class="shrink-0 cursor-pointer rounded-lg border border-white/10 bg-white/5 px-1.5 py-0.5 text-[10px] text-slate-300 transition-colors hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400"
          @click="newMode = false"
        >
          ✕
        </button>
      </template>
    </div>
```

- [ ] **Step 4: Wire it in `Tasks.vue`**

Pass the new props and handle the emit. Update the `<TaskViewControls>` usage (~line 451):

```html
    <TaskViewControls
      v-if="!loading && !loadError && (tasks.length > 0 || hasDisplayableLists)"
      :grouping="grouping"
      :sort-pref="sortPref"
      :is-aggregate="isAggregate"
      :creating-list="creatingList"
      @update:grouping="grouping = $event"
      @set-sort-key="setSortKey"
      @flip-sort-dir="flipSortDir"
      @create-list="onControlsCreateList"
    />
```

Add the handler in `<script setup>` (near `onCreateList`, ~line 83):

```ts
// The Lists-view "New list" control: create + cache in this vault's lists so
// the empty section appears immediately (createList is composerVaultId ??
// vaultId scoped — in per-vault mode that is this vault). Failures are toasted
// by the composable.
async function onControlsCreateList(name: string) {
  await createList(name);
}
```

- [ ] **Step 5: Run test to verify it passes**

Run: `npx vitest run tests/tasks.test.ts -t "creates a new list"`
Expected: PASS. Then `npx vitest run tests/tasks.test.ts` full file PASS.

- [ ] **Step 6: Commit**

```bash
git add src/components/TaskViewControls.vue src/components/Tasks.vue tests/tasks.test.ts
git commit -m "feat(ui): add a New list control to the tasks Lists view"
```

---

### Task 9: Frontend — Task IDs settings card

**Files:**
- Modify: `src/types.ts` (`TasksConfig` ~183-189)
- Modify: `src/components/TasksConfigTab.vue`
- Modify: `tests/helpers/taskMount.ts` (get_tasks_config mocks) and `tests/tasks-config-tab.test.ts`

**Interfaces:**
- Consumes IPC: `get_tasks_config` (now returns `taskIdEnabled`/`taskIdProperty`), `set_task_id_config(id, enabled, property)`.

- [ ] **Step 1: Extend the type**

In `src/types.ts`, add to `interface TasksConfig`:

```ts
  /** Whether generated task IDs are enabled for this vault. */
  taskIdEnabled: boolean;
  /** The resolved id property name (default "task-id"). */
  taskIdProperty: string;
```

- [ ] **Step 2: Write the failing test**

In `tests/tasks-config-tab.test.ts`, extend `mountTab`'s `get_tasks_config` default and add a test. First update the default return (so it carries the new fields):

```ts
    if (cmd === "get_tasks_config")
      return opts.onGet
        ? opts.onGet()
        : { tasksFolder: opts.tasksFolder ?? null, defaultList: null, listOrder: [], taskIdEnabled: false, taskIdProperty: "task-id" };
```

Add a `set_task_id_config` capture branch to the `mockIPC`:

```ts
    if (cmd === "set_task_id_config") return opts.onSetId?.(args) ?? null;
```

Add `onSetId?: (a: unknown) => unknown;` to the `mountTab` opts type. Then the test:

```ts
  it("enabling task ids and setting a property saves via set_task_id_config", async () => {
    const saved: unknown[] = [];
    const { wrapper } = mountTab({ onSetId: (a) => (saved.push(a), null) });
    await flushPromises();
    await wrapper.get('[data-testid="task-id-enabled"]').setValue(true);
    await flushPromises();
    await wrapper.get('[data-testid="task-id-property"]').setValue("uid");
    await wrapper.get('[data-testid="task-id-property"]').trigger("blur");
    await flushPromises();
    expect(saved).toContainEqual({ id: "v1", enabled: true, property: "uid" });
  });
```

(If the tab reads `taskIdProperty` into a ref only when non-default, ensure the input starts empty and shows `task-id` as a placeholder — see Step 3.)

- [ ] **Step 3: Run test to verify it fails**

Run: `npx vitest run tests/tasks-config-tab.test.ts -t "enabling task ids"`
Expected: FAIL — `[data-testid="task-id-enabled"]` not found.

- [ ] **Step 4: Add the Task IDs card to `TasksConfigTab.vue`**

Add state + autosave in `<script setup>` (after the folder autosave):

```ts
const taskIdEnabled = ref(false);
// Empty means "use the default"; the default name is shown as a placeholder.
const taskIdProperty = ref("");

const idAutosave = useAutosave(
  async () => {
    await invoke("set_task_id_config", {
      id: props.vaultId,
      enabled: taskIdEnabled.value,
      property: taskIdProperty.value.trim() || null,
    });
  },
  { label: "task ids" },
);
```

Load them in `onMounted`'s callback (extend the existing `load<TasksConfig>` callback body):

```ts
    taskIdEnabled.value = cfg.taskIdEnabled ?? false;
    // Show the resolved name only when the user set a non-default one, so the
    // placeholder communicates the default without pre-filling it.
    taskIdProperty.value = cfg.taskIdProperty && cfg.taskIdProperty !== "task-id" ? cfg.taskIdProperty : "";
```

Add change handlers:

```ts
function onIdEnabledChange(value: boolean) {
  taskIdEnabled.value = value;
  idAutosave.saveNow();
}
function onIdPropertyInput(value: string) {
  taskIdProperty.value = value;
  idAutosave.schedule();
}
```

Add the markup inside the `<template>`'s non-loading branch (after `TaskListSettings`/the pending message), guarded like the rest by `v-if="!loadError"`:

```html
      <section v-if="!loadError">
        <h2 class="mb-1.5 text-xs font-semibold uppercase tracking-wide text-slate-400">
          Task IDs
        </h2>
        <div class="rounded-xl border border-white/10 bg-white/5 p-2">
          <div class="flex items-center justify-between gap-2">
            <label
              for="task-id-enabled"
              class="text-sm text-slate-200"
            >
              Generate an ID for each task
              <span class="block text-xs text-slate-500">Written to new tasks and stamped on the next edit</span>
            </label>
            <input
              id="task-id-enabled"
              data-testid="task-id-enabled"
              type="checkbox"
              class="h-4 w-4 accent-violet-500"
              :checked="taskIdEnabled"
              @change="onIdEnabledChange(($event.target as HTMLInputElement).checked)"
            >
          </div>
          <div
            v-if="taskIdEnabled"
            class="mt-2"
          >
            <label
              for="task-id-property"
              class="mb-1 block text-sm text-slate-200"
            >
              Property name
              <span class="block text-xs text-slate-500">Frontmatter key the ID is saved under</span>
            </label>
            <input
              id="task-id-property"
              data-testid="task-id-property"
              type="text"
              placeholder="task-id"
              class="w-full rounded-lg border border-white/10 bg-white/5 px-2 py-1 text-sm text-slate-100 placeholder:text-slate-500 focus:border-violet-400 focus:outline-none"
              :value="taskIdProperty"
              @input="onIdPropertyInput(($event.target as HTMLInputElement).value)"
              @blur="idAutosave.flush()"
            >
            <p
              v-if="idAutosave.error.value"
              data-testid="task-id-error"
              class="mt-1 text-xs text-red-300"
            >
              {{ idAutosave.error.value }}
            </p>
          </div>
        </div>
      </section>
```

- [ ] **Step 5: Update the shared task-view mocks**

In `tests/helpers/taskMount.ts`, add the two fields to every `get_tasks_config` return (three occurrences) so the shape matches `TasksConfig`:

```ts
    if (cmd === "get_tasks_config") return { tasksFolder: null, defaultList: null, listOrder: [], taskIdEnabled: false, taskIdProperty: "task-id" };
```

- [ ] **Step 6: Run tests to verify they pass**

Run: `npx vitest run tests/tasks-config-tab.test.ts`
Expected: PASS. Then `npx vitest run tests/tasks.test.ts tests/tasks-lists.test.ts` still PASS.

- [ ] **Step 7: Commit**

```bash
git add src/types.ts src/components/TasksConfigTab.vue tests/tasks-config-tab.test.ts tests/helpers/taskMount.ts
git commit -m "feat(ui): task ids settings card (enable + property) in Vault settings"
```

---

### Task 10: Docs, baselines, and full gate run

**Files:**
- Modify: `AGENTS.md`, `CONTEXT.md`, `docs/Gaps.md`
- Modify (if the gate reports drift): `scripts/loc-baseline.json`, `scripts/quality-baseline.json`, `vite.config.ts` coverage floors

- [ ] **Step 1: Update AGENTS.md (tasks domain)**

In the tasks-domain section, document: (a) the default grouping is now Lists and the default sort is Manual; (b) the Lists-view "New list" control (per-vault); (c) the per-vault Task ID setting — `task_id_enabled`/`task_id_property` in `config.json`, `set_task_id_config` command, ID written at create by `services::add_task` and stamped-if-absent on `update_task` via `update_task_fields`'s `ensure_absent` (never overwrites; status toggles don't stamp). Add `set_task_id_config` to the IPC surface table and bump the command count.

- [ ] **Step 2: Update CONTEXT.md**

Add **Task ID** to the ubiquitous language: a generated, stable frontmatter identifier for a Task (short random base36), distinct from the file path and the manual `order` rank; opt-in per vault, written under a configurable property (default `task-id`).

- [ ] **Step 3: Update docs/Gaps.md**

Record the accepted residuals: changing the ID property name later leaves prior-property IDs in place (by design, never overwritten); aggregate-mode list creation is intentionally omitted this slice; a reorder that materializes ranks across a section may stamp several IDs at once.

- [ ] **Step 4: Run every gate and ratchet baselines**

```bash
npm run lint && npm run check:loc && npm run check:quality && npm test
cd src-tauri && cargo fmt --check && cd core && cargo clippy --all-targets -- -D warnings && cargo test
```

If `check:loc` fails because a modified file grew, re-run with `--update` (`node scripts/loc-baseline.mjs --update` per the repo's LOC guard usage) and commit the baseline. Same for `check:quality` (`--update`) and any `vite.config.ts` coverage floor that rose. Loosening a baseline downward is NOT allowed — only ratchet where the change legitimately added lines. Run `npm run test:coverage` LAST (it needs no pre-existing `coverage/` dir for `check:quality`).

- [ ] **Step 5: Commit**

```bash
git add AGENTS.md CONTEXT.md docs/Gaps.md scripts/loc-baseline.json scripts/quality-baseline.json vite.config.ts
git commit -m "docs(tasks): document lists-first, drag-default, and task ids; ratchet baselines"
```

---

## Self-Review

**Spec coverage:**
- Goal 1 (Lists first + open on Lists) → Task 7. ✓
- Goal 2 (easy list creation) → Task 8. ✓
- Goal 3 (drag-and-drop default sort) → Task 7. ✓
- Goal 4 (per-vault Task IDs) → Tasks 1, 2, 3, 4, 5, 6, 9. ✓
- Spec D.1 config → Task 1; D.2 generation → Task 2; D.3 create write → Tasks 3+5; D.3 stamp-on-edit → Tasks 4+6; D.4 command + UI → Tasks 6+9. ✓
- Reserved-key/charset validation → Tasks 2 (core) + 6 (command). ✓
- Testing/docs/baselines → each task's tests + Task 10. ✓

**Placeholder scan:** No TBD/TODO; every code step shows exact code; every test step shows the assertion. The RNG crate is pinned (`getrandom = "0.3"`). The one non-code instruction (Task 7 Step 4 "fix any test that assumed the old defaults") names the mechanism (grips-on-open, lists-default) and the discipline (don't weaken assertions) rather than leaving it open.

**Type consistency:** `task_id_enabled: bool` / `task_id_property: Option<String>` (Rust) ↔ `taskIdEnabled`/`taskIdProperty` (JSON/TS DTO, resolved `String` on read) are used consistently across Tasks 1, 6, 9. `update_task_fields(..., ensure_absent: &[(&str, &str)])` and `create_task(..., task_id: Option<(&str, &str)>)` signatures match between definition (Tasks 3, 4) and callers (Tasks 5, 6). `new_task_id`/`is_valid_id_property` names are stable across Tasks 2, 5, 6. `createList` reused (not renamed) in Task 8.
