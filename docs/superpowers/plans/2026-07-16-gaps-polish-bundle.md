# GAP Polish Bundle Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Land five independent fixes from `docs/Gaps.md` (plus the too-old-Pandoc cache edge) as separate TDD commits, appending to PR #62.

**Architecture:** Each fix is self-contained. Two are pure-core (`vault_config.rs`, `daily_notes.rs`, `document_import.rs`), one is a one-line frontend store guard, one is a small frontend state change, and one (the import queue) spans Rust + the Pinia store + the picker component.

**Tech Stack:** Rust (`vault_buddy_core` + Tauri shell), Vue 3 + Pinia, Vitest.

**Spec:** `docs/superpowers/specs/2026-07-16-gaps-polish-bundle-design.md`

## Global Constraints

- **Fixed-gap hygiene:** when a numbered gap is fixed, delete its entry from `docs/Gaps.md` in the same commit, and the regression test names the failure mode (repo convention).
- **Delete-path safety (GAP-54):** any new removal must be canonical-contained, real-in-place only (`canonicalize()==path`, never follow a symlink/junction), and gated exactly like the existing staging sweep.
- **Config preserve-vs-write:** `set_capture_config` owns mode/folders/bitrate/devices/transcription/`recording_date_folders`; `set_documents_config` owns `documents_folder`/`document_date_folders`/`document_extract_images`; every other field is preserved from the existing config.
- **Daily-note tokens:** only `YYYY`/`MM`/`DD` + `[literal]` escapes are supported; any other letter run still falls back to the default format.
- **Shell build prerequisite:** the shell crate + its tests need `npm run setup:linux` (done) and a built `../dist` (`npm run build`). Core-only tasks use `cargo test -p vault_buddy_core`; frontend tasks use `npx vitest`.
- **Conventional Commits**, ending with the two trailers this repo requires. No model identifier in any committed artifact.

---

### Task 1: GAP-60 — testable config preserve-vs-write helpers

**Files:**
- Modify: `src-tauri/core/src/vault_config.rs` (add two helpers + tests)
- Modify: `src-tauri/core/src/capture_config.rs` (re-export the helpers)
- Modify: `src-tauri/src/capture_config_commands.rs` (`set_capture_config` uses `merge_capture_owned`)
- Modify: `src-tauri/src/document_commands.rs` (`set_documents_config` uses `merge_documents_owned`)
- Modify: `docs/Gaps.md` (delete GAP-60)

**Interfaces:**
- Produces: `merge_capture_owned(existing: &VaultCaptureConfig, incoming: VaultCaptureConfig) -> VaultCaptureConfig` and `merge_documents_owned(existing: &VaultCaptureConfig, documents_folder: Option<String>, document_date_folders: bool, document_extract_images: bool) -> VaultCaptureConfig`.

- [ ] **Step 1: Write the failing tests.** In `src-tauri/core/src/vault_config.rs`, inside `mod tests`, add:

```rust
    #[test]
    fn merge_capture_owned_writes_owned_and_preserves_the_rest() {
        // existing carries distinctive values for every NON-capture-owned field.
        let existing = VaultCaptureConfig {
            tasks_folder: Some("Inbox/Tasks".into()),
            documents_folder: Some("Inbox/Docs".into()),
            default_list: Some("Inbox".into()),
            list_order: vec!["Inbox".into(), "Next".into()],
            document_date_folders: false,
            document_extract_images: false,
            ..VaultCaptureConfig::default()
        };
        // incoming carries the capture-owned fields (non-owned left at defaults).
        let incoming = VaultCaptureConfig {
            mode: RecordingMode::VoiceNote,
            bitrate_kbps: 192,
            recording_date_folders: false,
            ..VaultCaptureConfig::default()
        };
        let merged = merge_capture_owned(&existing, incoming);
        // owned fields come from incoming
        assert_eq!(merged.mode, RecordingMode::VoiceNote);
        assert_eq!(merged.bitrate_kbps, 192);
        assert!(!merged.recording_date_folders);
        // every non-owned field is preserved from existing (a transposed
        // document/recording date-folder pair would fail here)
        assert_eq!(merged.tasks_folder.as_deref(), Some("Inbox/Tasks"));
        assert_eq!(merged.documents_folder.as_deref(), Some("Inbox/Docs"));
        assert_eq!(merged.default_list.as_deref(), Some("Inbox"));
        assert_eq!(merged.list_order, vec!["Inbox", "Next"]);
        assert!(!merged.document_date_folders);
        assert!(!merged.document_extract_images);
    }

    #[test]
    fn merge_documents_owned_writes_owned_and_preserves_the_rest() {
        let existing = VaultCaptureConfig {
            mode: RecordingMode::VoiceNote,
            recording_date_folders: false,
            tasks_folder: Some("T".into()),
            ..VaultCaptureConfig::default()
        };
        let merged = merge_documents_owned(&existing, Some("Docs".into()), false, false);
        // owned
        assert_eq!(merged.documents_folder.as_deref(), Some("Docs"));
        assert!(!merged.document_date_folders);
        assert!(!merged.document_extract_images);
        // preserved (would break if set_documents_config touched them)
        assert_eq!(merged.mode, RecordingMode::VoiceNote);
        assert!(!merged.recording_date_folders);
        assert_eq!(merged.tasks_folder.as_deref(), Some("T"));
    }
```

- [ ] **Step 2: Run to verify failure.**

Run: `cd src-tauri/core && cargo test merge_capture_owned merge_documents_owned`
Expected: compile error — `merge_capture_owned` / `merge_documents_owned` not found.

- [ ] **Step 3: Add the helpers.** In `src-tauri/core/src/vault_config.rs`, immediately after the `serialize_vault_entry` function (before `#[cfg(test)]`), add:

```rust
/// Merge the fields `set_capture_config` OWNS from `incoming` onto `existing`,
/// preserving every field another settings command owns
/// (`set_documents_config`: documents_folder/document_date_folders/
/// document_extract_images; `set_tasks_config`: tasks_folder;
/// `set_task_lists_config`: default_list/list_order). The preserved fields are
/// listed explicitly and everything else comes from `incoming` via `..`, so a
/// capture save can never transpose an owned field with a preserved one
/// (GAP-60). Pure, so the split is unit-tested in the core crate.
pub fn merge_capture_owned(
    existing: &VaultCaptureConfig,
    incoming: VaultCaptureConfig,
) -> VaultCaptureConfig {
    VaultCaptureConfig {
        tasks_folder: existing.tasks_folder.clone(),
        documents_folder: existing.documents_folder.clone(),
        default_list: existing.default_list.clone(),
        list_order: existing.list_order.clone(),
        document_date_folders: existing.document_date_folders,
        document_extract_images: existing.document_extract_images,
        ..incoming
    }
}

/// The `set_documents_config` counterpart: owns exactly documents_folder,
/// document_date_folders, document_extract_images; every other field is
/// preserved from `existing` via `..` (GAP-60).
pub fn merge_documents_owned(
    existing: &VaultCaptureConfig,
    documents_folder: Option<String>,
    document_date_folders: bool,
    document_extract_images: bool,
) -> VaultCaptureConfig {
    VaultCaptureConfig {
        documents_folder,
        document_date_folders,
        document_extract_images,
        ..existing.clone()
    }
}
```

- [ ] **Step 4: Run to verify pass.**

Run: `cd src-tauri/core && cargo test merge_capture_owned merge_documents_owned`
Expected: both PASS.

- [ ] **Step 5: Re-export from `capture_config`.** In `src-tauri/core/src/capture_config.rs`, change the vault_config re-export line:

```rust
pub use crate::vault_config::{RecordingMode, VaultCaptureConfig};
```
to:
```rust
pub use crate::vault_config::{
    merge_capture_owned, merge_documents_owned, RecordingMode, VaultCaptureConfig,
};
```

- [ ] **Step 6: Wire `set_capture_config`.** In `src-tauri/src/capture_config_commands.rs`, replace the `let value = capture_config::VaultCaptureConfig { … };` struct-literal block (the one ending with `document_extract_images: existing.document_extract_images,`) with:

```rust
    // Build ONLY the capture-owned fields; merge_capture_owned preserves every
    // field another settings command owns (GAP-60), so a save here can't reset
    // the documents/tasks/lists settings, and the recording/document date-folder
    // pair can't be transposed.
    let incoming = capture_config::VaultCaptureConfig {
        mode,
        meeting_folder,
        voice_note_folder,
        bitrate_kbps: cfg.bitrate_kbps,
        create_note: cfg.create_note,
        input_device: cfg.input_device.clone().filter(|d| !d.is_empty()),
        output_device: cfg.output_device.clone().filter(|d| !d.is_empty()),
        transcribe: cfg.transcribe,
        transcription_model: cfg.transcription_model.clone(),
        transcription_language: cfg.transcription_language.clone().filter(|l| !l.is_empty()),
        transcript_timestamps: cfg.transcript_timestamps,
        follow_up_template: cfg.follow_up_template,
        recording_date_folders: cfg.recording_date_folders,
        ..capture_config::VaultCaptureConfig::default()
    };
    let value = capture_config::merge_capture_owned(&existing, incoming);
```

(The following `let result = capture_config::update_vault_config(&id, value.clone()); … log::info! …` block is unchanged — `value` still has all fields.)

- [ ] **Step 7: Wire `set_documents_config`.** In `src-tauri/src/document_commands.rs`, replace the in-place mutation:

```rust
    let _guard = lock_ignoring_poison(&lock.0);
    let mut v = capture_config::vault_config(&capture_config::load_config(), &id);
    v.documents_folder = folder;
    v.document_date_folders = document_date_folders;
    v.document_extract_images = document_extract_images;
    capture_config::update_vault_config(&id, v)
```
with:
```rust
    let _guard = lock_ignoring_poison(&lock.0);
    let existing = capture_config::vault_config(&capture_config::load_config(), &id);
    let value = capture_config::merge_documents_owned(
        &existing,
        folder,
        document_date_folders,
        document_extract_images,
    );
    capture_config::update_vault_config(&id, value)
```

- [ ] **Step 8: Delete GAP-60 from `docs/Gaps.md`.** Remove the entire `### GAP-60 · Low · …` entry block (from its heading through the blank line before `## 2. Main-thread responsiveness (shell)`).

- [ ] **Step 9: Build + test core and shell.**

Run: `cd src-tauri/core && cargo test && cargo clippy --all-targets -- -D warnings && cd .. && cargo fmt --check && cargo test -p vault-buddy --lib && cargo clippy -p vault-buddy --all-targets -- -D warnings`
Expected: all pass; the shell compiles with the new helper calls.

- [ ] **Step 10: Commit.**

```bash
git add src-tauri/core/src/vault_config.rs src-tauri/core/src/capture_config.rs src-tauri/src/capture_config_commands.rs src-tauri/src/document_commands.rs docs/Gaps.md
git commit  # subject: refactor(core): extract + test the config preserve-vs-write merge (GAP-60)
```

---

### Task 2: Too-old Pandoc keeps re-probing

**Files:**
- Modify: `src/stores/pandoc.ts` (cache-hit guard)
- Test: `tests/pandoc-store.test.ts`

**Interfaces:**
- Consumes: `usePandocStore` (existing).

- [ ] **Step 1: Write the failing test.** In `tests/pandoc-store.test.ts`, add inside the `describe`:

```ts
  it("re-probes while Pandoc is installed but too old for the sandbox", async () => {
    let calls = 0;
    mockIPC((cmd) => {
      if (cmd === "detect_pandoc") {
        calls += 1;
        return {
          installed: true,
          version: "pandoc 2.14",
          path: "pandoc",
          sandboxSupported: false,
          configuredPath: null,
        };
      }
    });
    const store = usePandocStore();
    await store.ensureDetected();
    await store.ensureDetected();
    expect(calls).toBe(2); // too old is not a usable cache hit → re-probe
  });
```

- [ ] **Step 2: Run to verify failure.**

Run: `npx vitest run tests/pandoc-store.test.ts -t "too old"`
Expected: FAIL — `expect(calls).toBe(2)` reads 1 (a too-old status currently caches).

- [ ] **Step 3: Tighten the cache-hit guard.** In `src/stores/pandoc.ts`, in `ensureDetected`, change:

```ts
      if (this.status?.installed) return;
```
to:
```ts
      // Cache only a USABLE result: an "installed but too old (<2.15)" Pandoc
      // keeps re-probing so an update is picked up on the next open (like a
      // not-installed status), rather than staying stale until a settings Recheck.
      if (this.status?.installed && this.status.sandboxSupported) return;
```

- [ ] **Step 4: Run to verify pass.**

Run: `npx vitest run tests/pandoc-store.test.ts`
Expected: all PASS (5 tests).

- [ ] **Step 5: Lint + commit.**

```bash
npm run lint
git add src/stores/pandoc.ts tests/pandoc-store.test.ts
git commit  # subject: fix(ui): re-probe a too-old Pandoc instead of caching it
```

---

### Task 3: GAP-09 — daily-note `[literal]` escapes

**Files:**
- Modify: `src-tauri/core/src/daily_notes.rs` (`substitute_tokens` + tests)
- Modify: `docs/Gaps.md` (delete GAP-09)

**Interfaces:** none (internal to `daily_notes.rs`).

- [ ] **Step 1: Write the failing tests.** In `src-tauri/core/src/daily_notes.rs`, inside `mod tests`, add:

```rust
    #[test]
    fn bracket_literals_render_verbatim() {
        // moment's [literal] escape: bracketed text is emitted as-is (brackets
        // stripped), so a common Obsidian format like "YYYY-MM-DD [Daily]" no
        // longer falls back to the default and misnames the note.
        assert_eq!(render_format("YYYY-MM-DD [Daily]", date()), "2026-07-03 Daily");
        assert_eq!(render_format("[Week of] YYYY-MM-DD", date()), "Week of 2026-07-03");
        // a literal containing characters that would otherwise be tokens
        assert_eq!(render_format("YYYY [at] MM", date()), "2026 at 07");
    }

    #[test]
    fn unterminated_bracket_falls_back_to_default() {
        // No closing ']': unparseable → default format, never a half-built path.
        assert_eq!(render_format("YYYY-MM-DD [Daily", date()), "2026-07-03");
    }
```

- [ ] **Step 2: Run to verify failure.**

Run: `cd src-tauri/core && cargo test bracket_literals unterminated_bracket`
Expected: FAIL — `bracket_literals_render_verbatim` gets `2026-07-03` (current fallback) instead of the verbatim literal.

- [ ] **Step 3: Handle `[` in `substitute_tokens`.** In `src-tauri/core/src/daily_notes.rs`, replace the `while i < chars.len() { … }` loop body in `substitute_tokens` so the first branch handles brackets:

```rust
    while i < chars.len() {
        if chars[i] == '[' {
            // moment `[literal]` escape: emit the bracketed text verbatim
            // (brackets stripped), skipping the token rule inside. An
            // unterminated `[` is unparseable — fall back to the default format
            // rather than risk a misnamed note.
            let start = i + 1;
            let mut j = start;
            while j < chars.len() && chars[j] != ']' {
                j += 1;
            }
            if j >= chars.len() {
                return None; // no closing ']'
            }
            out.extend(&chars[start..j]);
            i = j + 1; // past the ']'
        } else if chars[i].is_ascii_alphabetic() {
            let start = i;
            while i < chars.len() && chars[i].is_ascii_alphabetic() {
                i += 1;
            }
            let run: String = chars[start..i].iter().collect();
            match run.as_str() {
                "YYYY" => out.push_str(&date.format("%Y").to_string()),
                "MM" => out.push_str(&date.format("%m").to_string()),
                "DD" => out.push_str(&date.format("%d").to_string()),
                _ => return None,
            }
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
```

- [ ] **Step 4: Run to verify pass.**

Run: `cd src-tauri/core && cargo test daily_notes`
Expected: all daily_notes tests PASS (including the existing `unsupported_tokens_fall_back_to_default` and `repeated_token_runs_fall_back…`).

- [ ] **Step 5: Delete GAP-09 from `docs/Gaps.md`** (the entire `### GAP-09 · Low · …` entry block).

- [ ] **Step 6: Lint + commit.**

```bash
cd src-tauri && cargo fmt --check && cd ..
git add src-tauri/core/src/daily_notes.rs docs/Gaps.md
git commit  # subject: fix(core): support [literal] escapes in daily-note formats (GAP-09)
```

---

### Task 4: GAP-55 — full FIFO import queue

**Files:**
- Modify: `src-tauri/src/document_commands.rs` (`DocumentImportPending`, `begin_document_import`, `take_pending_import`)
- Modify: `src/stores/vaults.ts` (queue state + actions + refresh guard)
- Modify: `src/components/ImportVaultPicker.vue` (head + queued count + dequeue)
- Test: `tests/vaults-store.test.ts`, `tests/import-vault-picker.test.ts`, `tests/action-panel.test.ts`
- Modify: `docs/Gaps.md` (delete GAP-55)

**Interfaces:**
- Produces (store): `pendingImports: string[]`; `enqueueImports(paths: string[])`; `dequeueImport(path: string)`. Removes `pendingImportPath` and `openImportPicker`.
- Produces (Rust): `take_pending_import -> Vec<String>` (was `Option<String>`); `begin_document_import` pushes onto a `VecDeque`.

- [ ] **Step 1: Write the failing test (the queue behavior).** In `tests/import-vault-picker.test.ts`, add inside the `describe` (uses the existing `installed()`/`sampleVaults` helpers and `basename`-free assertions):

```ts
  it("processes a queue of dropped documents one at a time (GAP-55)", async () => {
    const convertSources: string[] = [];
    mockIPC((cmd, args) => {
      if (cmd === "detect_pandoc") return installed();
      if (cmd === "convert_document") {
        convertSources.push((args as { sourcePath: string }).sourcePath);
        return "Documents/2026/07/note.md";
      }
    });
    const store = useVaultsStore();
    store.vaults = sampleVaults;
    store.enqueueImports(["/a.docx", "/b.docx", "/c.docx"]);
    const wrapper = mount(ImportVaultPicker);
    await flushPromises();
    // Head shown with a "+2 more queued" indicator.
    expect(wrapper.text()).toContain("a.docx");
    expect(wrapper.get('[data-testid="import-picker-queued"]').text()).toContain("2");
    // Pick a vault for each queued doc in turn.
    await wrapper.findAll('[data-testid="import-picker-vault"]')[0].trigger("click");
    await flushPromises();
    expect(wrapper.text()).toContain("b.docx");
    await wrapper.findAll('[data-testid="import-picker-vault"]')[0].trigger("click");
    await flushPromises();
    expect(wrapper.text()).toContain("c.docx");
    await wrapper.findAll('[data-testid="import-picker-vault"]')[0].trigger("click");
    await flushPromises();
    // All three converted, in order; queue drained → back to the list.
    expect(convertSources).toEqual(["/a.docx", "/b.docx", "/c.docx"]);
    expect(store.view).toBe("list");
    expect(store.pendingImports).toEqual([]);
  });
```

- [ ] **Step 2: Run to verify failure.**

Run: `npx vitest run tests/import-vault-picker.test.ts -t "processes a queue"`
Expected: FAIL — `store.enqueueImports` doesn't exist / `import-picker-queued` not found.

- [ ] **Step 3: Rust — queue the stash.** In `src-tauri/src/document_commands.rs`:

Change the struct:
```rust
#[derive(Default)]
pub struct DocumentImportPending(pub std::sync::Mutex<Option<String>>);
```
to:
```rust
#[derive(Default)]
pub struct DocumentImportPending(pub std::sync::Mutex<std::collections::VecDeque<String>>);
```

Change `begin_document_import`'s stash write:
```rust
        *lock_ignoring_poison(&state.0) = Some(path);
```
to:
```rust
        // Queue the drop (FIFO) instead of overwriting: two documents dropped in
        // quick succession must both survive to be imported (GAP-55).
        lock_ignoring_poison(&state.0).push_back(path);
```

Replace `take_pending_import`:
```rust
#[tauri::command]
pub fn take_pending_import(app: tauri::AppHandle) -> Option<String> {
    let state = app.state::<DocumentImportPending>();
    let mut guard = lock_ignoring_poison(&state.0);
    guard.take()
}
```
with:
```rust
#[tauri::command]
pub fn take_pending_import(app: tauri::AppHandle) -> Vec<String> {
    let state = app.state::<DocumentImportPending>();
    let mut guard = lock_ignoring_poison(&state.0);
    guard.drain(..).collect()
}
```

Also update the doc-comment above `take_pending_import` to say it drains the whole queue and returns every pending path (`Vec<String>`, empty when none).

- [ ] **Step 4: Store — queue state + actions.** In `src/stores/vaults.ts`:

4a. State: replace
```ts
    pendingImportPath: null as string | null,
```
with
```ts
    // The FIFO queue of dropped-document paths awaiting a vault pick; the head
    // (index 0) is the document the picker is currently showing (GAP-55).
    pendingImports: [] as string[],
```

4b. `refresh()`: replace the `take_pending_import` drain + branch:
```ts
      const dropped = await invoke<string | null>(
        "take_pending_import",
      ).catch(() => null);
      if (dropped) {
        this.openImportPicker(dropped);
        // A drop supersedes any armed one-shot view (e.g. the startup update
        // check's "settings"): clear it so a LATER panel-shown refresh doesn't
        // consume it stale and navigate away after the import returns to list.
        this.pendingView = null;
        this.pendingCaptureVaultId = null;
      } else if (this.pendingView) {
        this.view = this.pendingView;
        this.captureSettingsVaultId = this.pendingCaptureVaultId;
        this.pendingView = null;
        this.pendingCaptureVaultId = null;
      } else {
        this.showList();
      }
```
with:
```ts
      const dropped = await invoke<string[]>("take_pending_import").catch(
        () => [] as string[],
      );
      if (dropped.length) {
        this.enqueueImports(dropped);
        // A drop supersedes any armed one-shot view (e.g. the startup update
        // check's "settings"): clear it so a LATER panel-shown refresh doesn't
        // consume it stale and navigate away after the import returns to list.
        this.pendingView = null;
        this.pendingCaptureVaultId = null;
      } else if (this.view === "importPicker" && this.pendingImports.length) {
        // An un-picked import queue survives a spurious empty-drain refresh:
        // near-simultaneous drops each fire panel-shown → refresh, and a later
        // refresh can drain empty; leaving the picker as-is keeps the queue the
        // first refresh built (and keeps an un-picked single drop, too).
      } else if (this.pendingView) {
        this.view = this.pendingView;
        this.captureSettingsVaultId = this.pendingCaptureVaultId;
        this.pendingView = null;
        this.pendingCaptureVaultId = null;
      } else {
        this.showList();
      }
```

4c. `showList()`: replace `this.pendingImportPath = null;` with `this.pendingImports = [];`.

4d. Replace the `openImportPicker` action:
```ts
    // Rust-owned buddy drop lands here (via `refresh`'s take_pending_import
    // consume). back() needs no case: it falls through to the final else →
    // showList, which also clears pendingImportPath.
    openImportPicker(path: string) {
      this.view = "importPicker";
      this.pendingImportPath = path;
    },
```
with:
```ts
    // Append drained buddy-drop paths to the FIFO queue and show the picker.
    // back() needs no case: it falls through to the final else → showList,
    // which also clears the queue.
    enqueueImports(paths: string[]) {
      this.view = "importPicker";
      this.pendingImports.push(...paths);
    },
    // Remove a just-converted document (the head) from the queue; when none
    // remain, leave the picker for the list. Matches by value so a
    // mid-conversion append can't remove the wrong entry.
    dequeueImport(path: string) {
      const idx = this.pendingImports.indexOf(path);
      if (idx !== -1) this.pendingImports.splice(idx, 1);
      if (this.pendingImports.length === 0) this.showList();
    },
```

- [ ] **Step 5: Picker — head + queued count + dequeue.** In `src/components/ImportVaultPicker.vue`:

5a. `sourceName`: replace `const path = store.pendingImportPath;` with `const path = store.pendingImports[0];`.

5b. Add a queued-count computed after `sourceName`:
```ts
const queuedMore = computed(() => Math.max(0, store.pendingImports.length - 1));
```

5c. `pick()`: replace the body's snapshot + `showList` with head-convert + `dequeueImport`:
```ts
async function pick(vaultId: string) {
  // Convert the head of the queue; a mid-conversion drop appends to the tail
  // (GAP-55), so afterward we drop the head and either advance to the next
  // queued document or return to the list.
  const source = store.pendingImports[0];
  if (busyVaultId.value || !source) return;
  busyVaultId.value = vaultId;
  try {
    const notePath = await invoke<string>("convert_document", {
      id: vaultId,
      sourcePath: source,
    });
    notifications.notify("success", `Imported ${basename(notePath)}`, {
      action: {
        label: "Open in Obsidian",
        run: () => invoke("open_imported_document", { id: vaultId, path: notePath }),
      },
    });
    store.dequeueImport(source);
  } catch (e) {
    // Stay on the picker (queue head unchanged) so the user can retry a
    // different vault for this same document.
    logWarning(`import picker: convert_document failed: ${String(e)}`);
    notifications.error(`Couldn't import document: ${String(e)}`);
  } finally {
    busyVaultId.value = null;
  }
}
```

5d. In the `<template>`, in the `Import <sourceName> into which vault?` paragraph, add the queued indicator right after `into which vault?`:
```html
      into which vault?
      <span
        v-if="queuedMore > 0"
        data-testid="import-picker-queued"
        class="text-slate-500"
      >(+{{ queuedMore }} more queued)</span>
```

5e. Update the two stale comments referencing `pendingImportPath` (the module comment at the top and the snapshot comment in `pick`) to say `pendingImports` / "the queue head".

- [ ] **Step 6: Update the store tests.** In `tests/vaults-store.test.ts`:

6a. The "openImportPicker sets the pending path and view" test → rename and use the queue:
```ts
  it("enqueueImports appends to the queue and shows the picker", () => {
    const store = useVaultsStore();
    store.enqueueImports(["C:/x/Report.docx"]);
    expect(store.view).toBe("importPicker");
    expect(store.pendingImports).toEqual(["C:/x/Report.docx"]);
  });
```

6b. The "showList clears the pending import path" test:
```ts
  it("showList clears the import queue", () => {
    const store = useVaultsStore();
    store.enqueueImports(["C:/x/Report.docx"]);
    store.showList();
    expect(store.view).toBe("list");
    expect(store.pendingImports).toEqual([]);
  });
```

6c. "refresh routes to the import picker…": change the mock to return an array and the assertion:
```ts
      if (cmd === "take_pending_import") return ["C:/x/Report.docx"];
```
```ts
    expect(store.pendingImports).toEqual(["C:/x/Report.docx"]);
```

6d. "a winning drop clears an armed pendingView…": the mock's `pending` becomes an array, and the second-refresh assertion changes — an un-picked drop now stays on the picker (the empty-drain guard), which still proves the stale `settings` view was NOT consumed:
```ts
    let pending: string[] = ["C:/x/Report.docx"];
```
```ts
      if (cmd === "take_pending_import") return pending;
```
```ts
    // The drop cleared the armed "settings" request; a later empty refresh
    // keeps the un-picked import on the picker (never consumes stale settings).
    pending = [];
    await store.refresh();
    expect(store.view).toBe("importPicker");
```

6e. "refresh falls back to the vault list when there is no pending import": mock returns `[]`, assertion uses the queue:
```ts
      if (cmd === "take_pending_import") return [];
```
```ts
    expect(store.pendingImports).toEqual([]);
```

- [ ] **Step 7: Update the other suites' import references.** In `tests/import-vault-picker.test.ts` and `tests/action-panel.test.ts`, replace every `store.openImportPicker(X)` with `store.enqueueImports([X])`, and every `expect(store.pendingImportPath).toBeNull()` with `expect(store.pendingImports).toEqual([])` (and any `.toBe("…")` read with `expect(store.pendingImports[0]).toBe("…")`). Grep to confirm none remain:

Run: `grep -rn "openImportPicker\|pendingImportPath" src tests`
Expected: no matches.

- [ ] **Step 8: Run the affected suites.**

Run: `npx vitest run tests/import-vault-picker.test.ts tests/vaults-store.test.ts tests/action-panel.test.ts`
Expected: all PASS (including the new queue test).

- [ ] **Step 9: Build the shell + typecheck/lint.**

Run: `cd src-tauri && cargo test -p vault-buddy --lib && cargo fmt --check && cd .. && npm run build && npm run lint`
Expected: shell compiles/tests pass; typecheck + lint clean.

- [ ] **Step 10: Delete GAP-55 from `docs/Gaps.md`** (the entire `### GAP-55 · Low (mitigated) · …` entry).

- [ ] **Step 11: Commit.**

```bash
git add src-tauri/src/document_commands.rs src/stores/vaults.ts src/components/ImportVaultPicker.vue tests/vaults-store.test.ts tests/import-vault-picker.test.ts tests/action-panel.test.ts docs/Gaps.md
git commit  # subject: feat(document-import): queue multiple dropped documents (GAP-55)
```

---

### Task 5: GAP-54 — sweep the crash-orphan media folder

**Files:**
- Modify: `src-tauri/core/src/document_import.rs` (basename extraction + orphan-media removal in the janitor + tests)
- Modify: `docs/Gaps.md` (delete GAP-54)

**Interfaces:** none (internal to `document_import.rs`).

- [ ] **Step 1: Write the failing tests.** In `src-tauri/core/src/document_import.rs`, inside `mod tests`, add:

```rust
    #[test]
    fn janitor_removes_crash_orphan_media_folder_with_no_note() {
        use std::time::{Duration, SystemTime};
        let tmp = tempfile::tempdir().unwrap();
        let month = tmp.path().join("Documents/2026/07");
        std::fs::create_dir_all(&month).unwrap();
        // The crash window: published media + surviving staging dir, but no note.
        let orphan_media = month.join("2026-07-10 Report");
        std::fs::create_dir_all(&orphan_media).unwrap();
        std::fs::write(orphan_media.join("image1.png"), b"PNG").unwrap();
        let staging = month.join(".2026-07-10 Report.123-4.vault-buddy.tmp.import");
        std::fs::create_dir_all(&staging).unwrap();

        let now = SystemTime::now() + Duration::from_secs(3600);
        clean_stale_staging_at(&tmp.path().join("Documents"), now, Duration::from_secs(60));
        assert!(!staging.exists(), "staging dir swept");
        assert!(!orphan_media.exists(), "orphan media swept too (GAP-54)");
    }

    #[test]
    fn janitor_keeps_media_folder_that_has_a_sibling_note() {
        use std::time::{Duration, SystemTime};
        let tmp = tempfile::tempdir().unwrap();
        let month = tmp.path().join("Documents/2026/07");
        std::fs::create_dir_all(&month).unwrap();
        // A normal published import (note + media) plus a stale staging dir for
        // the SAME basename must NOT delete the real media.
        let media = month.join("2026-07-10 Report");
        std::fs::create_dir_all(&media).unwrap();
        std::fs::write(media.join("image1.png"), b"PNG").unwrap();
        std::fs::write(month.join("2026-07-10 Report.md"), "real note").unwrap();
        let staging = month.join(".2026-07-10 Report.9-9.vault-buddy.tmp.import");
        std::fs::create_dir_all(&staging).unwrap();

        let now = SystemTime::now() + Duration::from_secs(3600);
        clean_stale_staging_at(&tmp.path().join("Documents"), now, Duration::from_secs(60));
        assert!(media.join("image1.png").exists(), "real media kept (has a note)");
        assert!(month.join("2026-07-10 Report.md").exists());
    }

    #[test]
    fn orphan_media_basename_extracts_dotted_stems() {
        // The unique token pid-seq has no '.', so the last '.' before it
        // separates it from a basename that itself contains dots.
        assert_eq!(
            orphan_media_basename(".2026-07-10 Report v1.2.123-4.vault-buddy.tmp.import"),
            Some("2026-07-10 Report v1.2".to_string())
        );
        assert_eq!(
            orphan_media_basename(".x.1-1.vault-buddy.tmp.import"),
            Some("x".to_string())
        );
        assert_eq!(orphan_media_basename("not-a-staging-dir"), None);
    }

    #[cfg(unix)]
    #[test]
    fn janitor_skips_symlinked_media_named_like_an_orphan() {
        use std::time::{Duration, SystemTime};
        let tmp = tempfile::tempdir().unwrap();
        let month = tmp.path().join("Documents/2026/07");
        std::fs::create_dir_all(&month).unwrap();
        let real = month.join("real-data");
        std::fs::create_dir_all(&real).unwrap();
        std::fs::write(real.join("keep.md"), "precious").unwrap();
        std::os::unix::fs::symlink(&real, month.join("2026-07-10 Report")).unwrap();
        let staging = month.join(".2026-07-10 Report.1-1.vault-buddy.tmp.import");
        std::fs::create_dir_all(&staging).unwrap();

        let now = SystemTime::now() + Duration::from_secs(3600);
        clean_stale_staging_at(&tmp.path().join("Documents"), now, Duration::from_secs(60));
        assert!(real.join("keep.md").exists(), "symlink target untouched");
        assert!(month.join("2026-07-10 Report").exists(), "the link itself left alone");
    }
```

- [ ] **Step 2: Run to verify failure.**

Run: `cd src-tauri/core && cargo test janitor_removes_crash_orphan orphan_media_basename`
Expected: compile error — `orphan_media_basename` not found (and the removal tests fail).

- [ ] **Step 3: Add the two helpers.** In `src-tauri/core/src/document_import.rs`, immediately above `pub fn clean_stale_staging_at(`, add:

```rust
/// Extract the import basename from a staging dir name
/// `.<basename>.<pid>-<seq>.vault-buddy.tmp.import`. The unique token
/// `<pid>-<seq>` has no `.`, so the last `.` before it separates it from the
/// basename even when the basename itself contains dots. `None` for a name that
/// isn't a staging dir.
fn orphan_media_basename(staging_name: &str) -> Option<String> {
    let inner = staging_name.strip_prefix('.')?.strip_suffix(STAGING_MARKER)?;
    let (base, _unique) = inner.rsplit_once('.')?;
    (!base.is_empty()).then(|| base.to_string())
}

/// Remove a crash-orphaned `<dir>/<basename>/` media folder (GAP-54) — ONLY
/// when it has no sibling `<basename>.md` note (proving it's an orphan, not a
/// normal import), it is a REAL in-place directory (`canonicalize() == path` —
/// rejects a symlink/junction, never followed), and it stays inside `dir`
/// (already canonical-contained by the caller). Pushes the removed path onto
/// `removed` for logging.
fn remove_orphan_media(dir: &Path, basename: &str, removed: &mut Vec<PathBuf>) {
    let media = dir.join(basename);
    if dir.join(format!("{basename}.md")).exists() {
        return; // a real published import (note + media) — not our orphan
    }
    let Ok(canon) = media.canonicalize() else {
        return; // missing → nothing to sweep
    };
    if canon != media || !canon.is_dir() {
        return; // a file or a symlink/junction — never follow it
    }
    match std::fs::remove_dir_all(&media) {
        Ok(()) => removed.push(media),
        Err(e) => log::warn!("import-recovery: failed to remove orphan media {media:?}: {e}"),
    }
}
```

- [ ] **Step 4: Call the orphan sweep from the staging removal.** In `clean_stale_staging_at`, inside the `sweep_dir` closure, in the `if stale { … }` block, add the orphan-media removal immediately **before** the `match std::fs::remove_dir_all(&path) { … }`:

```rust
            if stale {
                // Also remove a crash-orphaned media folder published without its
                // note (GAP-54): the staging dir name gives us the basename, so a
                // `<dir>/<basename>/` with no sibling `<basename>.md` is provably
                // ours. Guarded (no-note + real-in-place + contained) in
                // remove_orphan_media.
                if let Some(base) = orphan_media_basename(&name) {
                    remove_orphan_media(dir, &base, &mut sweep.removed);
                }
                match std::fs::remove_dir_all(&path) {
                    Ok(()) => sweep.removed.push(path),
                    Err(e) => {
                        log::warn!("import-recovery: failed to remove {path:?}: {e}");
                        sweep.pending += 1;
                    }
                }
            } else {
                sweep.pending += 1; // fresh orphan → caller reschedules
            }
```

(This replaces the existing `if stale { match … } else { … }` block — only the two-line orphan call + comment are new.)

- [ ] **Step 5: Run to verify pass.**

Run: `cd src-tauri/core && cargo test document_import`
Expected: all `document_import` tests PASS (the four new ones plus the existing janitor tests).

- [ ] **Step 6: Lint.**

Run: `cd src-tauri/core && cargo clippy --all-targets -- -D warnings && cd .. && cargo fmt --check`
Expected: clean.

- [ ] **Step 7: Delete GAP-54 from `docs/Gaps.md`** (the entire `### GAP-54 · Low · …` entry block).

- [ ] **Step 8: Commit.**

```bash
git add src-tauri/core/src/document_import.rs docs/Gaps.md
git commit  # subject: fix(document-import): sweep crash-orphaned media folders (GAP-54)
```

---

### Task 6: Full verification + push

**Files:** none (verification + delivery).

- [ ] **Step 1: Full frontend suite.**

Run: `npm run lint && npm run build && npm test`
Expected: ESLint clean, typecheck+build ok, full Vitest suite passes.

- [ ] **Step 2: Full Rust suite (core + shell).**

Run: `cd src-tauri && cargo fmt --check && (cd core && cargo test && cargo clippy --all-targets -- -D warnings) && cargo test -p vault-buddy --lib && cargo clippy -p vault-buddy --all-targets -- -D warnings && cd ..`
Expected: all green.

- [ ] **Step 3: Push (appends to PR #62).**

```bash
git push -u origin claude/document-intake-image-text-config-lhrmsk
```
Expected: fast-forward push; PR #62 updates with the five new commits. (No new PR — the branch already has an open PR.)

---

## Self-Review

**Spec coverage:**
- GAP-60 helpers + wiring + tests → Task 1. ✓
- Too-old Pandoc guard + test → Task 2. ✓
- GAP-09 `[literal]` + unterminated fallback + tests → Task 3. ✓
- GAP-55 Rust queue + store queue + empty-drain guard + picker + tests → Task 4. ✓
- GAP-54 basename extraction + guarded orphan removal + tests → Task 5. ✓
- Gaps.md entries deleted with each fix; full verification + push → Tasks 1/3/4/5 + Task 6. ✓

**Placeholder scan:** No TBD/TODO; every code step shows complete code. The mechanical test-rename in Task 4 Step 7 gives the exact old→new transform and a grep gate.

**Type consistency:** `merge_capture_owned` / `merge_documents_owned` signatures match Task 1's definition and both call sites. `pendingImports: string[]` / `enqueueImports(paths: string[])` / `dequeueImport(path: string)` are used identically across the store, picker, and tests; `take_pending_import` returns `Vec<String>` (Rust) ↔ `string[]` (store) consistently. `orphan_media_basename` / `remove_orphan_media` signatures match their definitions and the janitor call.
