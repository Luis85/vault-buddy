# Recording Folders, Flat/Dated Layout & Vault Settings Regroup — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give Meeting and Voice Note recordings independent per-vault folders, add a per-domain flat-vs-dated layout toggle for recordings and imported documents, and regroup the Vault settings form into Recording / Tasks / Documents super-groups.

**Architecture:** Two structural splits land first (a new `core/src/vault_config.rs` and a new shell command module) purely to make LOC headroom in two at-cap files — zero behavior change. Then the UI regroup + `RecordingSettings.vue` extraction, then the per-mode folders (with a legacy `recordingFolder` read-time migration), then the layout toggle. The toggle changes only where NEW files are written; the recordings browser, transcription backfill, and both recovery sweepers are made **layout-agnostic** (scan flat root AND `YYYY/MM`), so existing files stay findable with no migration and no bulk-move.

**Tech Stack:** Rust (Tauri v2 shell + pure `core`/`capture` crates, `serde_json`, `chrono`), Vue 3 + Pinia + Tailwind 4, Vitest (happy-dom + `mockIPC`).

**Design spec:** `docs/superpowers/specs/2026-07-12-recording-folders-layout-and-vault-settings-design.md`

## Global Constraints

- **LOC caps (shrink-only baselines, `scripts/loc-baseline.json`):** frontend `src/**.{ts,vue}` = **500** nonblank; rust `src-tauri/**/src/**.rs` = **800** nonblank. Allowlisted files may shrink but never grow. When a change *shrinks* an allowlisted file, re-run `npm run check:loc -- --update` and commit the baseline in the same commit.
- **Config field defaults:** `meeting_folder` None → `"Meetings"`; `voice_note_folder` None → `"Voice Notes"`; `recording_date_folders` / `document_date_folders` default `true` (dated). Legacy `recordingFolder` seeds both folder fields at parse time; it is never written back.
- **Toggle serialization:** the two date-folder booleans are written to `config.json` **only when `false`** (parse-absent → `true`); the two folder keys are written **only when `Some`**. `recordingFolder` is never serialized.
- **No bulk-move:** flipping a toggle NEVER moves or rewrites existing files.
- **DTOs cross IPC as camelCase** (`#[serde(rename_all = "camelCase")]` ↔ TS camelCase).
- **TDD:** failing test first, named for the behavior; regression tests name the failure mode in a comment.
- **All gates green at every task boundary:** `npm test` + coverage floors, `npm run build` (vue-tsc), `npm run lint`, `npm run check:loc`, `cd src-tauri && cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` (per crate) and `cargo test` (core, capture, transcribe, mcp). Shell changes additionally compile-gate on Linux (`npm run setup:linux` once, then `npx tauri build --no-bundle`).
- **Vault-write safety is untouched:** the only writes remain the sanctioned capture/transcript/tasks/document-import paths; `rename_noreplace` + suffix retry + exclusive-create temps stay as-is. Recovery/scan ownership gates (`is_capture_base`, `NOTE_TMP_SUFFIX`+dot, owned-temp markers, canonical containment, no symlink follow) apply identically at the flat level.

---

# Phase 1 — Structural splits (LOC headroom, zero behavior change)

Pure moves + re-exports. Existing tests move with the code and keep passing — they ARE the safety net for these tasks.

## Task 1.1: Split `VaultCaptureConfig` into `core/src/vault_config.rs`

**Files:**
- Create: `src-tauri/core/src/vault_config.rs`
- Modify: `src-tauri/core/src/capture_config.rs`, `src-tauri/core/src/lib.rs` (add `mod vault_config;`)

**Interfaces:**
- Produces (in `vault_config`): `pub enum RecordingMode`, `pub struct VaultCaptureConfig` (+ its `impl`), `pub fn vault_entry(entry: &serde_json::Value) -> VaultCaptureConfig`, `pub fn serialize_vault_entry(v: &VaultCaptureConfig) -> serde_json::Map<String, serde_json::Value>`.
- Produces (re-exported from `capture_config`): `pub use crate::vault_config::{RecordingMode, VaultCaptureConfig};` — every existing `capture_config::RecordingMode` / `capture_config::VaultCaptureConfig` caller keeps compiling.

- [ ] **Step 1: Add the module declaration**

In `src-tauri/core/src/lib.rs`, add `mod vault_config;` beside the existing `mod capture_config;` (keep alphabetical if the file is ordered; otherwise place next to `capture_config`). If `capture_config` items are re-exported at crate root (`pub use capture_config::…`), leave those unchanged — they resolve through the re-export below.

- [ ] **Step 2: Move the types + parser into `vault_config.rs`**

Create `src-tauri/core/src/vault_config.rs`. Move, **byte-for-byte**, from `capture_config.rs`:
- the `RecordingMode` enum and its `impl` (label/uses_loopback/as_key/from_key),
- the `VaultCaptureConfig` struct, its `Default`, and its `impl` block (`effective_recording_folder`, `recording_roots`, `tasks_root`, `documents_root`),
- the `vault_entry` free function (currently private `fn vault_entry`) — make it `pub fn vault_entry`.

Add the necessary `use` lines at the top: `use std::path::Path;` is not needed here (no path IO); keep `use serde_json` references fully-qualified or `use serde_json::Value;` as the moved code requires.

- [ ] **Step 3: Extract the per-vault serializer**

The vault-entry serialization currently lives inline in `serialize_config`'s `for id in ids { … }` loop in `capture_config.rs` (the block that builds `entry` from `v.mode`, `v.recording_folder`, … through `v.list_order`). Move that block into `vault_config.rs` as:

```rust
/// Serialize ONE vault entry to the schema `vault_entry` reads. Optional
/// fields omitted so the hand-editable file stays minimal.
pub fn serialize_vault_entry(v: &VaultCaptureConfig) -> serde_json::Map<String, serde_json::Value> {
    use serde_json::{json, Map};
    let mut entry = Map::new();
    entry.insert("mode".to_string(), json!(v.mode.as_key()));
    if let Some(folder) = &v.recording_folder {
        entry.insert("recordingFolder".to_string(), json!(folder));
    }
    entry.insert("bitrateKbps".to_string(), json!(v.bitrate_kbps));
    entry.insert("createNote".to_string(), json!(v.create_note));
    if let Some(device) = &v.input_device {
        entry.insert("inputDevice".to_string(), json!(device));
    }
    if let Some(device) = &v.output_device {
        entry.insert("outputDevice".to_string(), json!(device));
    }
    entry.insert("transcribe".to_string(), json!(v.transcribe));
    entry.insert("transcriptionModel".to_string(), json!(v.transcription_model));
    if let Some(language) = &v.transcription_language {
        entry.insert("transcriptionLanguage".to_string(), json!(language));
    }
    entry.insert("transcriptTimestamps".to_string(), json!(v.transcript_timestamps));
    entry.insert("followUpTemplate".to_string(), json!(v.follow_up_template));
    if let Some(folder) = &v.tasks_folder {
        entry.insert("tasksFolder".to_string(), json!(folder));
    }
    if let Some(folder) = &v.documents_folder {
        entry.insert("documentsFolder".to_string(), json!(folder));
    }
    if let Some(list) = &v.default_list {
        entry.insert("defaultList".to_string(), json!(list));
    }
    if !v.list_order.is_empty() {
        entry.insert("listOrder".to_string(), json!(v.list_order));
    }
    entry
}
```

In `capture_config.rs`, replace the inline block with:

```rust
for id in ids {
    let entry = crate::vault_config::serialize_vault_entry(&cfg.vaults[id]);
    vaults.insert(id.clone(), serde_json::Value::Object(entry));
}
```

And in `parse_config`, replace the `vaults.insert(id.clone(), vault_entry(entry));` call with `vaults.insert(id.clone(), crate::vault_config::vault_entry(entry));`.

- [ ] **Step 4: Add the re-export and move the vault-specific tests**

At the top of `capture_config.rs` (near the existing `pub use crate::mcp_config::…` / `document_import_config::…` re-exports), add:

```rust
pub use crate::vault_config::{RecordingMode, VaultCaptureConfig};
```

Move the vault-config-specific `#[cfg(test)]` tests out of `capture_config.rs` into a `#[cfg(test)] mod tests` in `vault_config.rs`: the ones exercising `RecordingMode`, `effective_recording_folder`, `recording_roots`, `vault_entry` parsing/defaults, folder/device/transcription/tasks/lists field round-trips (e.g. `folder_defaults_follow_the_mode_but_config_overrides`, `recording_roots_are_the_custom_folder_or_both_defaults`, `mode_keys_round_trip`, `vault_entry_reads_lists_settings_defensively`, `config_round_trips_through_serialize_and_parse`). Leave the IO/mcp/documents tests (`update_vault_config_preserves_sibling_vaults`, `saving_a_vault_config_preserves_the_mcp_section`, `write_config_replaces_and_leaves_no_temp`, `documents_root_defaults_to_documents`, etc.) in `capture_config.rs` — they exercise the file IO that stays there. Tests that need both (`config_round_trips_through_serialize_and_parse`) can live in either; put it in `vault_config.rs` and use `crate::capture_config::{serialize_config, parse_config}` for the top-level helpers.

- [ ] **Step 5: Verify the move compiles and all tests pass unchanged**

Run: `cd src-tauri/core && cargo test`
Expected: PASS — same test count as before the move (nothing new; nothing lost).
Run: `cd src-tauri/core && cargo clippy --all-targets -- -D warnings && cargo fmt --check`
Expected: clean.

- [ ] **Step 6: Ratchet the LOC baseline down and commit**

Run: `npm run check:loc -- --update` (from repo root) — `capture_config.rs` shrank; commit the lowered baseline.

```bash
git add src-tauri/core/src/vault_config.rs src-tauri/core/src/capture_config.rs src-tauri/core/src/lib.rs scripts/loc-baseline.json
git commit -m "refactor(core): split VaultCaptureConfig into vault_config.rs

Move the per-vault config struct/enum/parse/serialize (and their tests)
into a new module, re-exported from capture_config, mirroring the
mcp_config/document_import_config split-outs. No behavior change; makes
headroom under the shrink-only LOC cap for the per-mode folders + layout
toggle."
```

## Task 1.2: Extract the capture-config commands into a shell module

**Files:**
- Create: `src-tauri/src/capture_config_commands.rs`
- Modify: `src-tauri/src/capture_commands.rs`, `src-tauri/src/lib.rs`

**Interfaces:**
- Produces: `pub struct CaptureConfigDto` (with `from_config`), `pub fn get_capture_config`, `pub fn set_capture_config` — same signatures/behavior, new home.
- Consumes: `crate::capture_commands::{ConfigWriteLock, BITRATES_KBPS, TRANSCRIPTION_MODELS}`, `crate::commands::find_vault`.

- [ ] **Step 1: Create the module and move the code**

Create `src-tauri/src/capture_config_commands.rs`. Move `CaptureConfigDto` (+ its `from_config` impl), `get_capture_config`, and `set_capture_config` out of `capture_commands.rs` verbatim. Add module-header `use` lines: `use std::path::Path; use tauri; use vault_buddy_core::{capture_config, capture_paths}; use crate::capture_commands::ConfigWriteLock;`. If `BITRATES_KBPS` / `TRANSCRIPTION_MODELS` are referenced only here now, move them too and update any other referrers to `crate::capture_config_commands::…`; otherwise keep them `pub` in `capture_commands` and reference `crate::capture_commands::{BITRATES_KBPS, TRANSCRIPTION_MODELS}`. Add `mod capture_config_commands;` in `lib.rs`.

- [ ] **Step 2: Repoint the command registrations**

In `lib.rs`'s `generate_handler!`, change `capture_commands::get_capture_config` → `capture_config_commands::get_capture_config` and `capture_commands::set_capture_config` → `capture_config_commands::set_capture_config`. Repoint any other in-crate callers (e.g. RecordMode has none in Rust; search `set_capture_config(` / `get_capture_config(` in `src-tauri/src/`).

- [ ] **Step 3: Compile-gate and shell test**

Run: `cd src-tauri && cargo fmt --check && cargo clippy -p vault-buddy --all-targets -- -D warnings`
Expected: clean (needs a built `../dist` + GUI libs; run `npm run build` and `npm run setup:linux` first if not already present).
Run: `cd src-tauri && cargo test -p vault-buddy --lib`
Expected: PASS.

- [ ] **Step 4: Ratchet LOC and commit**

Run: `npm run check:loc -- --update` — `capture_commands.rs` shrank.

```bash
git add src-tauri/src/capture_config_commands.rs src-tauri/src/capture_commands.rs src-tauri/src/lib.rs scripts/loc-baseline.json
git commit -m "refactor(shell): extract capture-config commands into their own module

Move CaptureConfigDto + get/set_capture_config out of the grandfathered
capture_commands.rs hotspot into capture_config_commands.rs (IPC surface
unchanged; only the defining module moves), making headroom for the
per-mode folder + layout-toggle DTO growth."
```

---

# Phase 2 — Regroup Vault settings (UI only)

## Task 2.1: Extract `RecordingSettings.vue`

**Files:**
- Create: `src/components/RecordingSettings.vue`
- Modify: `src/components/CaptureSettings.vue`
- Test: `tests/recording-settings.test.ts` (new)

**Interfaces:**
- Produces: `RecordingSettings.vue` — a controlled component. Props: `modelValue: RecordingSettingsValue`, `devices: AudioDevices`, `folderError: string | null`. Emits `update:modelValue`. `RecordingSettingsValue = { recordingFolder: string; bitrateKbps: number; createNote: boolean; followUpTemplate: boolean; inputDevice: string; outputDevice: string; transcribe: boolean; transcriptionModel: string; transcriptionLanguage: string; transcriptTimestamps: boolean }`.

> NOTE: Phase 2 keeps the single `recordingFolder` string (unchanged behavior). Phase 3 replaces it with `meetingFolder`/`voiceNoteFolder`. Doing the extraction first keeps the two changes reviewable in isolation.

- [ ] **Step 1: Write the failing component test**

Create `tests/recording-settings.test.ts`:

```ts
import { mount } from "@vue/test-utils";
import { describe, expect, it } from "vitest";

import RecordingSettings from "../src/components/RecordingSettings.vue";

const value = {
  recordingFolder: "Meetings",
  bitrateKbps: 128,
  createNote: true,
  followUpTemplate: true,
  inputDevice: "",
  outputDevice: "",
  transcribe: false,
  transcriptionModel: "small",
  transcriptionLanguage: "",
  transcriptTimestamps: true,
};
const devices = { inputs: [{ name: "USB Mic", isDefault: false }], outputs: [{ name: "Speakers", isDefault: true }] };

describe("RecordingSettings", () => {
  it("emits a merged update when the folder changes", async () => {
    const w = mount(RecordingSettings, { props: { modelValue: value, devices, folderError: null } });
    await w.get('[data-testid="folder-input"]').setValue("Inbox/Audio");
    const emits = w.emitted("update:modelValue");
    expect(emits).toBeTruthy();
    expect((emits!.at(-1)![0] as typeof value).recordingFolder).toBe("Inbox/Audio");
    // Untouched fields are preserved in the merge.
    expect((emits!.at(-1)![0] as typeof value).bitrateKbps).toBe(128);
  });

  it("shows the folder error", () => {
    const w = mount(RecordingSettings, { props: { modelValue: value, devices, folderError: "bad folder" } });
    expect(w.get('[data-testid="folder-error"]').text()).toContain("bad folder");
  });
});
```

- [ ] **Step 2: Run it to confirm it fails**

Run: `npx vitest run tests/recording-settings.test.ts`
Expected: FAIL — cannot resolve `RecordingSettings.vue`.

- [ ] **Step 3: Create `RecordingSettings.vue`**

Move the Recording / Companion note / Transcription / Audio devices markup + their computed helpers out of `CaptureSettings.vue`. Follow `TranscriptionSettings.vue`'s controlled-component pattern (a `patch()` that spread-merges onto `props.modelValue` and emits). Keep the existing `data-testid`s (`folder-input`, `folder-error`, `bitrate-select`, `note-toggle`, `follow-up-toggle`, `input-device-select`, `output-device-select`) so existing selectors keep working. Nest `<TranscriptionSettings v-model="transcriptionBundle" />` where `transcriptionBundle` is a computed get/set proxy onto the four transcription fields of `modelValue`. Bring the `withConfigured` / `inputMenuOptions` / `outputMenuOptions` / `bitrateOptions` computeds along; they read `props.devices` + `props.modelValue`.

Structure the template as the Recording super-group's inner cards (Folders → Audio → Companion note → Transcription). Example skeleton (fill each card with the moved markup):

```vue
<script setup lang="ts">
import { computed } from "vue";
import type { AudioDevices, AudioDevice } from "../types";
import SelectMenu from "./SelectMenu.vue";
import TranscriptionSettings from "./TranscriptionSettings.vue";

interface RecordingSettingsValue {
  recordingFolder: string; bitrateKbps: number; createNote: boolean; followUpTemplate: boolean;
  inputDevice: string; outputDevice: string;
  transcribe: boolean; transcriptionModel: string; transcriptionLanguage: string; transcriptTimestamps: boolean;
}
const props = defineProps<{ modelValue: RecordingSettingsValue; devices: AudioDevices; folderError: string | null }>();
const emit = defineEmits<{ "update:modelValue": [value: RecordingSettingsValue] }>();
function patch(change: Partial<RecordingSettingsValue>) { emit("update:modelValue", { ...props.modelValue, ...change }); }
const recordingFolder = computed({ get: () => props.modelValue.recordingFolder, set: (v: string) => patch({ recordingFolder: v }) });
const bitrateKbps = computed({ get: () => props.modelValue.bitrateKbps, set: (v: number) => patch({ bitrateKbps: v }) });
const createNote = computed({ get: () => props.modelValue.createNote, set: (v: boolean) => patch({ createNote: v }) });
const followUpTemplate = computed({ get: () => props.modelValue.followUpTemplate, set: (v: boolean) => patch({ followUpTemplate: v }) });
const inputDevice = computed({ get: () => props.modelValue.inputDevice, set: (v: string) => patch({ inputDevice: v }) });
const outputDevice = computed({ get: () => props.modelValue.outputDevice, set: (v: string) => patch({ outputDevice: v }) });
const transcriptionBundle = computed({
  get: () => ({ transcribe: props.modelValue.transcribe, transcriptionModel: props.modelValue.transcriptionModel, transcriptionLanguage: props.modelValue.transcriptionLanguage, transcriptTimestamps: props.modelValue.transcriptTimestamps }),
  set: (v: { transcribe: boolean; transcriptionModel: string; transcriptionLanguage: string; transcriptTimestamps: boolean }) => patch(v),
});
const BITRATES = [128, 160, 192];
const bitrateOptions = BITRATES.map((b) => ({ value: b, label: `${b} kbps` }));
function withConfigured(list: AudioDevice[], configured: string) {
  const options = list.map((d) => ({ value: d.name, label: d.name }));
  if (configured && !list.some((d) => d.name === configured)) options.push({ value: configured, label: `${configured} (not connected)` });
  return options;
}
const inputMenuOptions = computed(() => [{ value: "", label: "System default" }, ...withConfigured(props.devices.inputs, inputDevice.value)]);
const outputMenuOptions = computed(() => [{ value: "", label: "System default" }, ...withConfigured(props.devices.outputs, outputDevice.value)]);
</script>
```

Move the corresponding `<section>` template blocks from `CaptureSettings.vue` into this file's `<template>`, keeping their markup and testids. (The parent will wrap this in the "Recording" super-group header in Task 2.2.)

- [ ] **Step 4: Rewire `CaptureSettings.vue` to use it**

In `CaptureSettings.vue`, replace the four moved `<section>`s with a single `<RecordingSettings v-model="recordingBundle" :devices="devices" :folder-error="folderError" />`. Add a `recordingBundle` computed get/set that maps the individual refs (`recordingFolder`, `bitrateKbps`, `createNote`, `followUpTemplate`, `inputDevice`, `outputDevice`, `transcribe`, `transcriptionModel`, `transcriptionLanguage`, `transcriptTimestamps`) to/from the `RecordingSettingsValue` shape — same adapter idiom the existing `transcriptionSettings` computed uses. Delete the now-unused per-field template markup + the `withConfigured`/`*MenuOptions`/`bitrateOptions` computeds that moved. Keep `save()`, the folder-trim logic, and the optional tasks/documents folder handling unchanged.

- [ ] **Step 5: Run tests**

Run: `npx vitest run tests/recording-settings.test.ts tests/capture-settings.test.ts`
Expected: PASS (existing capture-settings selectors still resolve through the child).
Run: `npm run lint && npm run check:loc`
Expected: clean; `CaptureSettings.vue` now < 500 nonblank.

- [ ] **Step 6: Commit**

```bash
git add src/components/RecordingSettings.vue src/components/CaptureSettings.vue tests/recording-settings.test.ts scripts/loc-baseline.json
git commit -m "refactor(ui): extract RecordingSettings.vue from CaptureSettings

Controlled component (TranscriptionSettings pattern) holding the recording
folder/bitrate/note/transcription/devices markup, so the Vault settings
form drops under its LOC cap and the Recording super-group has one home."
```

## Task 2.2: Group the form into Recording / Tasks / Documents super-groups

**Files:**
- Modify: `src/components/CaptureSettings.vue`
- Test: `tests/capture-settings.test.ts`

- [ ] **Step 1: Write the failing test**

Add to `tests/capture-settings.test.ts` (inside the existing `describe`):

```ts
it("renders the three domain super-group headings", async () => {
  const w = await mountLoaded();
  const text = w.text();
  expect(text).toContain("Recording");
  expect(text).toContain("Tasks");
  expect(text).toContain("Documents");
});
```

- [ ] **Step 2: Run it**

Run: `npx vitest run tests/capture-settings.test.ts -t "super-group"`
Expected: FAIL if the headings aren't present as distinct group headers (today "Documents"/"Tasks" appear as sub-card headings; assert the grouping wrapper renders a single top-level header per domain — adjust the assertion to target `[data-testid="group-recording"]` etc. added below).

- [ ] **Step 3: Add the super-group wrappers**

Wrap the form body in three labeled groups. Give each a heading and a `data-testid` so the test is precise:

```vue
<section data-testid="group-recording">
  <h2 class="mb-1.5 text-xs font-semibold uppercase tracking-wide text-slate-400">Recording</h2>
  <div class="flex flex-col gap-3 rounded-xl border border-white/10 bg-white/5 p-2">
    <RecordingSettings v-model="recordingBundle" :devices="devices" :folder-error="folderError" />
  </div>
</section>
<section data-testid="group-tasks">
  <h2 class="mb-1.5 text-xs font-semibold uppercase tracking-wide text-slate-400">Tasks</h2>
  <div class="flex flex-col gap-3">
    <VaultFolderSetting v-model="tasksFolder" heading="Tasks folder" ... />
    <TaskListSettings :key="listsCardNonce" :vault-id="vaultId" />
  </div>
</section>
<section data-testid="group-documents">
  <h2 class="mb-1.5 text-xs font-semibold uppercase tracking-wide text-slate-400">Documents</h2>
  <div class="flex flex-col gap-3">
    <VaultFolderSetting v-model="documentsFolder" heading="Documents folder" ... />
  </div>
</section>
```

Because `VaultFolderSetting` renders its own `<h2>` heading, change its `heading` props here to the field label (`"Tasks folder"` / `"Documents folder"`) so the domain super-group header isn't duplicated — or pass a prop to suppress its internal heading. Simplest: set `heading="Tasks folder"` / `heading="Documents folder"` and let the group `<h2>` carry the domain name. `RecordingSettings` renders its inner cards without a top-level heading (the group wrapper supplies "Recording").

- [ ] **Step 4: Run the tests**

Run: `npx vitest run tests/capture-settings.test.ts`
Expected: PASS.
Run: `npm run lint && npm run check:loc && npm run build`
Expected: clean.

- [ ] **Step 5: Commit**

```bash
git add src/components/CaptureSettings.vue tests/capture-settings.test.ts
git commit -m "feat(ui): group Vault settings into Recording/Tasks/Documents

Regroup the single form under three domain super-group headers, one Save
button and save semantics unchanged."
```

---

# Phase 3 — Distinct Meeting / Voice Note folders

## Task 3.1: Per-mode folder model + migration (core)

**Files:**
- Modify: `src-tauri/core/src/vault_config.rs`

**Interfaces:**
- Produces: `VaultCaptureConfig { meeting_folder: Option<String>, voice_note_folder: Option<String>, … }` (replacing `recording_folder`); `pub fn folder_for(&self, mode: RecordingMode) -> &str`; `effective_recording_folder`/`recording_roots` retained by name.

- [ ] **Step 1: Write the failing tests**

In `vault_config.rs` tests, replace `folder_defaults_follow_the_mode_but_config_overrides` and `recording_roots_are_the_custom_folder_or_both_defaults` with:

```rust
#[test]
fn folder_for_is_per_mode_with_defaults_and_overrides() {
    let cfg = crate::capture_config::parse_config(
        r#"{ "vaults": {
            "a": {},
            "b": { "meetingFolder": "Mtgs" },
            "c": { "meetingFolder": "Mtgs", "voiceNoteFolder": "Notes" }
        } }"#,
    );
    let a = crate::capture_config::vault_config(&cfg, "a");
    assert_eq!(a.folder_for(RecordingMode::Meeting), "Meetings");
    assert_eq!(a.folder_for(RecordingMode::VoiceNote), "Voice Notes");
    let b = crate::capture_config::vault_config(&cfg, "b");
    assert_eq!(b.folder_for(RecordingMode::Meeting), "Mtgs");
    assert_eq!(b.folder_for(RecordingMode::VoiceNote), "Voice Notes"); // untouched → default
    let c = crate::capture_config::vault_config(&cfg, "c");
    assert_eq!(c.folder_for(RecordingMode::VoiceNote), "Notes");
}

#[test]
fn effective_folder_follows_the_active_mode() {
    let mut v = VaultCaptureConfig { meeting_folder: Some("M".into()), voice_note_folder: Some("V".into()), ..VaultCaptureConfig::default() };
    v.mode = RecordingMode::Meeting;
    assert_eq!(v.effective_recording_folder(), "M");
    v.mode = RecordingMode::VoiceNote;
    assert_eq!(v.effective_recording_folder(), "V");
}

#[test]
fn recording_roots_is_the_deduped_union_of_both_modes() {
    // none → both defaults
    let none = VaultCaptureConfig::default();
    assert_eq!(none.recording_roots(), vec!["Meetings", "Voice Notes"]);
    // both custom, distinct → both
    let both = VaultCaptureConfig { meeting_folder: Some("A".into()), voice_note_folder: Some("B".into()), ..VaultCaptureConfig::default() };
    assert_eq!(both.recording_roots(), vec!["A", "B"]);
    // both custom, same → deduped to one
    let same = VaultCaptureConfig { meeting_folder: Some("Audio".into()), voice_note_folder: Some("Audio".into()), ..VaultCaptureConfig::default() };
    assert_eq!(same.recording_roots(), vec!["Audio"]);
    // one custom → custom + the other default
    let one = VaultCaptureConfig { meeting_folder: Some("Audio".into()), ..VaultCaptureConfig::default() };
    assert_eq!(one.recording_roots(), vec!["Audio", "Voice Notes"]);
}

#[test]
fn legacy_recording_folder_seeds_both_and_retires_on_reserialize() {
    // A pre-split config with the unified key seeds BOTH modes (no data loss).
    let cfg = crate::capture_config::parse_config(r#"{ "vaults": { "v": { "recordingFolder": "Audio" } } }"#);
    let v = crate::capture_config::vault_config(&cfg, "v");
    assert_eq!(v.meeting_folder.as_deref(), Some("Audio"));
    assert_eq!(v.voice_note_folder.as_deref(), Some("Audio"));
    // Explicit new keys win over the legacy fallback, per field.
    let cfg2 = crate::capture_config::parse_config(r#"{ "vaults": { "v": { "recordingFolder": "Audio", "voiceNoteFolder": "Notes" } } }"#);
    let v2 = crate::capture_config::vault_config(&cfg2, "v");
    assert_eq!(v2.meeting_folder.as_deref(), Some("Audio")); // fell back
    assert_eq!(v2.voice_note_folder.as_deref(), Some("Notes")); // explicit
    // Re-serialize writes the two new keys, never the legacy one.
    let json = crate::capture_config::serialize_config(&cfg);
    assert!(json.contains("meetingFolder"));
    assert!(json.contains("voiceNoteFolder"));
    assert!(!json.contains("recordingFolder"));
}
```

- [ ] **Step 2: Run to confirm failure**

Run: `cd src-tauri/core && cargo test -p vault_buddy_core folder_for effective_folder recording_roots legacy_recording_folder`
Expected: FAIL to compile — no `meeting_folder`/`voice_note_folder`/`folder_for`.

- [ ] **Step 3: Update the struct, Default, and impl**

In `VaultCaptureConfig`, replace `pub recording_folder: Option<String>,` with:

```rust
pub meeting_folder: Option<String>,
pub voice_note_folder: Option<String>,
```

In `Default`, replace `recording_folder: None,` with `meeting_folder: None,` and `voice_note_folder: None,`. Replace the `effective_recording_folder` + `recording_roots` methods with:

```rust
/// The vault-relative folder for a given mode: the configured override, or
/// the mode default (the PRD gives meetings and voice notes distinct homes).
pub fn folder_for(&self, mode: RecordingMode) -> &str {
    match mode {
        RecordingMode::Meeting => self.meeting_folder.as_deref().unwrap_or("Meetings"),
        RecordingMode::VoiceNote => self.voice_note_folder.as_deref().unwrap_or("Voice Notes"),
    }
}

/// The folder the ACTIVE mode records into.
pub fn effective_recording_folder(&self) -> &str {
    self.folder_for(self.mode)
}

/// Every folder a vault's recordings may live in — the deduped union of both
/// modes' effective folders, so scans that must see EVERY recording (the
/// Recordings list, recovery, transcription backfill) cover exactly the
/// folders in use and no more.
pub fn recording_roots(&self) -> Vec<&str> {
    let m = self.folder_for(RecordingMode::Meeting);
    let v = self.folder_for(RecordingMode::VoiceNote);
    if m == v {
        vec![m]
    } else {
        vec![m, v]
    }
}
```

- [ ] **Step 4: Update `vault_entry` (parse + migration)**

In `vault_entry`, remove the `recording_folder` field parse and add (place a `let legacy = …` binding above the struct literal):

```rust
let legacy = entry.get("recordingFolder").and_then(|v| v.as_str());
```

and in the struct literal:

```rust
meeting_folder: entry.get("meetingFolder").and_then(|v| v.as_str()).or(legacy).map(str::to_string),
voice_note_folder: entry.get("voiceNoteFolder").and_then(|v| v.as_str()).or(legacy).map(str::to_string),
```

- [ ] **Step 5: Update `serialize_vault_entry`**

Replace the `if let Some(folder) = &v.recording_folder { entry.insert("recordingFolder"…) }` block with:

```rust
if let Some(folder) = &v.meeting_folder {
    entry.insert("meetingFolder".to_string(), json!(folder));
}
if let Some(folder) = &v.voice_note_folder {
    entry.insert("voiceNoteFolder".to_string(), json!(folder));
}
```

- [ ] **Step 6: Fix the remaining references and any test that constructs `recording_folder`**

Search the core crate for `recording_folder`: `cd src-tauri && rg -n "recording_folder" core/`. Update every struct-literal usage in tests (e.g. `config_round_trips_through_serialize_and_parse`, `serialize_omits_absent_optional_fields`) to set `meeting_folder`/`voice_note_folder` instead. Update `serialize_omits_absent_optional_fields` to assert `!json.contains("meetingFolder")` and `!json.contains("voiceNoteFolder")`.

- [ ] **Step 7: Run tests + clippy**

Run: `cd src-tauri/core && cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --check`
Expected: PASS + clean.

- [ ] **Step 8: Commit**

```bash
git add src-tauri/core/src/vault_config.rs
git commit -m "feat(core): per-mode recording folders with legacy migration

Split recording_folder into meeting_folder/voice_note_folder; folder_for
picks per mode, recording_roots is the deduped union. A legacy
recordingFolder seeds both fields at parse time and is never written back,
so no existing folder or recording is lost."
```

## Task 3.2: DTO + save validation (shell)

**Files:**
- Modify: `src-tauri/src/capture_config_commands.rs`
- Test: `src-tauri/src/capture_config_commands.rs` (inline) or exercise via `tests/` (frontend) — the DTO round-trip is covered by the frontend test in Task 3.3; add a Rust test only for the two-folder validation branch if practical (see step 1).

**Interfaces:**
- Produces: `CaptureConfigDto { meeting_folder: Option<String>, voice_note_folder: Option<String>, … }`.

- [ ] **Step 1: Update the DTO**

In `CaptureConfigDto`, replace `pub recording_folder: Option<String>,` with `pub meeting_folder: Option<String>,` and `pub voice_note_folder: Option<String>,`. In `from_config`, replace `recording_folder: v.recording_folder.clone(),` with `meeting_folder: v.meeting_folder.clone(),` and `voice_note_folder: v.voice_note_folder.clone(),`.

- [ ] **Step 2: Update `set_capture_config` validation + construction**

Replace the single-folder trim + validate block with a small helper applied to both:

```rust
fn clean_folder(raw: &Option<String>) -> Option<String> {
    raw.as_deref().map(str::trim).filter(|f| !f.is_empty()).map(str::to_string)
}
// … inside set_capture_config, after find_vault:
let meeting_folder = clean_folder(&cfg.meeting_folder);
let voice_note_folder = clean_folder(&cfg.voice_note_folder);
for folder in [&meeting_folder, &voice_note_folder].into_iter().flatten() {
    capture_paths::safe_recording_root(Path::new(&vault.path), folder)?;
}
```

In the `VaultCaptureConfig { … }` value built for the write, replace `recording_folder: folder,` with `meeting_folder,` and `voice_note_folder,`. Update the trailing `log::info!` to log both folders (`meeting={:?}, voice_note={:?}`).

- [ ] **Step 3: Compile-gate + shell test**

Run: `cd src-tauri && cargo clippy -p vault-buddy --all-targets -- -D warnings && cargo test -p vault-buddy --lib`
Expected: clean + PASS.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/capture_config_commands.rs
git commit -m "feat(shell): carry per-mode folders through the capture-config command

CaptureConfigDto splits recordingFolder into meetingFolder/voiceNoteFolder;
set_capture_config validates both against the vault before writing."
```

## Task 3.3: Frontend types + two folder inputs

**Files:**
- Modify: `src/types.ts`, `src/components/RecordingSettings.vue`, `src/components/CaptureSettings.vue`, `src/components/RecordMode.vue`
- Test: `tests/recording-settings.test.ts`, `tests/capture-settings.test.ts`, `tests/record-mode.test.ts`

- [ ] **Step 1: Write the failing test**

In `tests/recording-settings.test.ts`, replace the single-folder `value.recordingFolder` with `meetingFolder`/`voiceNoteFolder` and assert both inputs round-trip:

```ts
const value = {
  meetingFolder: "Meetings", voiceNoteFolder: "Voice Notes",
  bitrateKbps: 128, createNote: true, followUpTemplate: true, inputDevice: "", outputDevice: "",
  transcribe: false, transcriptionModel: "small", transcriptionLanguage: "", transcriptTimestamps: true,
};
it("emits per-mode folder updates", async () => {
  const w = mount(RecordingSettings, { props: { modelValue: value, devices, folderError: null } });
  await w.get('[data-testid="meeting-folder-input"]').setValue("Mtgs");
  await w.get('[data-testid="voice-note-folder-input"]').setValue("Notes");
  const last = w.emitted("update:modelValue")!.at(-1)![0] as typeof value;
  expect(last.voiceNoteFolder).toBe("Notes");
});
```

- [ ] **Step 2: Run to confirm failure**

Run: `npx vitest run tests/recording-settings.test.ts`
Expected: FAIL — no `meeting-folder-input` testid; `RecordingSettingsValue` lacks the fields.

- [ ] **Step 3: Update types**

In `src/types.ts`, in `CaptureConfig` replace `recordingFolder: string | null;` with:

```ts
meetingFolder: string | null;
voiceNoteFolder: string | null;
```

- [ ] **Step 4: Update `RecordingSettings.vue`**

Replace `recordingFolder` in `RecordingSettingsValue` with `meetingFolder: string` + `voiceNoteFolder: string`; replace the single folder computed + `<input>` with two computeds (`meetingFolder`, `voiceNoteFolder`) and two inputs (testids `meeting-folder-input` / `voice-note-folder-input`, placeholders `Meetings` / `Voice Notes`, labels "Meeting folder" / "Voice Note folder"). Keep ONE shared `folderError` line beneath the pair (testid stays `folder-error`).

- [ ] **Step 5: Update `CaptureSettings.vue`**

Replace the `recordingFolder` ref with `meetingFolder`/`voiceNoteFolder` refs (default `""`). In `onMounted`, set them from `cfg.meetingFolder ?? ""` / `cfg.voiceNoteFolder ?? ""`. In `save()`, send `meetingFolder: meetingFolder.value.trim() || null` and `voiceNoteFolder: voiceNoteFolder.value.trim() || null` (drop `recordingFolder`). Add both to the `watch([...])` list that resets the "Saved ✓" state. Map both into/out of the `recordingBundle` computed.

- [ ] **Step 6: Update `RecordMode.vue` + its test**

In `RecordMode.vue`'s default `config` seed, replace `recordingFolder: null,` with `meetingFolder: null,` + `voiceNoteFolder: null,`. In `tests/record-mode.test.ts`, replace every `recordingFolder: "Meetings"` in the mock config with `meetingFolder: "Meetings", voiceNoteFolder: "Voice Notes"`.

- [ ] **Step 7: Update `tests/capture-settings.test.ts` mock config**

Replace `recordingFolder: "Meetings"` in the shared `config` object with `meetingFolder: "Meetings", voiceNoteFolder: "Voice Notes"`. Update any test that references the single folder input (`folder-input`) to the new testids where applicable; keep the folder-error assertion.

- [ ] **Step 8: Run all affected tests + build**

Run: `npx vitest run tests/recording-settings.test.ts tests/capture-settings.test.ts tests/record-mode.test.ts`
Expected: PASS.
Run: `npm run build && npm run lint && npm run check:loc`
Expected: clean.

- [ ] **Step 9: Commit**

```bash
git add src/types.ts src/components/RecordingSettings.vue src/components/CaptureSettings.vue src/components/RecordMode.vue tests/
git commit -m "feat(ui): distinct Meeting and Voice Note folder inputs

Two folder fields (placeholders Meetings / Voice Notes) replace the single
recording-folder input across the settings form, record view, and types."
```

---

# Phase 4 — Flat vs. dated layout toggle (per-domain)

## Task 4.1: Toggle fields + `capture_dir` helper (core)

**Files:**
- Modify: `src-tauri/core/src/vault_config.rs`, `src-tauri/core/src/capture_paths.rs`

**Interfaces:**
- Produces: `VaultCaptureConfig { recording_date_folders: bool, document_date_folders: bool, … }`; `pub fn capture_paths::capture_dir(root: &Path, date: NaiveDate, dated: bool) -> PathBuf`.

- [ ] **Step 1: Write the failing tests**

In `vault_config.rs` tests:

```rust
#[test]
fn date_folder_toggles_default_true_and_round_trip() {
    let d = VaultCaptureConfig::default();
    assert!(d.recording_date_folders);
    assert!(d.document_date_folders);
    // Absent → true; present false parses.
    let cfg = crate::capture_config::parse_config(
        r#"{ "vaults": { "a": { "recordingDateFolders": false, "documentDateFolders": false } } }"#);
    let a = crate::capture_config::vault_config(&cfg, "a");
    assert!(!a.recording_date_folders);
    assert!(!a.document_date_folders);
    // Serialize omits when true, writes when false.
    let mut only_true = crate::capture_config::AppConfig::default();
    only_true.vaults.insert("t".into(), VaultCaptureConfig::default());
    let jt = crate::capture_config::serialize_config(&only_true);
    assert!(!jt.contains("recordingDateFolders"));
    assert!(!jt.contains("documentDateFolders"));
    let mut has_false = crate::capture_config::AppConfig::default();
    has_false.vaults.insert("f".into(), VaultCaptureConfig { recording_date_folders: false, document_date_folders: false, ..VaultCaptureConfig::default() });
    let jf = crate::capture_config::serialize_config(&has_false);
    assert!(jf.contains("\"recordingDateFolders\": false"));
    assert!(jf.contains("\"documentDateFolders\": false"));
}
```

In `capture_paths.rs` tests:

```rust
#[test]
fn capture_dir_is_dated_or_flat() {
    let root = Path::new("/v/Meetings");
    assert_eq!(capture_dir(root, date(), true), Path::new("/v/Meetings/2026/07"));
    assert_eq!(capture_dir(root, date(), false), Path::new("/v/Meetings"));
}
```

- [ ] **Step 2: Run to confirm failure**

Run: `cd src-tauri/core && cargo test -p vault_buddy_core date_folder_toggles capture_dir_is`
Expected: FAIL to compile.

- [ ] **Step 3: Add the fields + parse + serialize**

Struct: add `pub recording_date_folders: bool,` + `pub document_date_folders: bool,`. `Default`: both `true`. `vault_entry`:

```rust
recording_date_folders: entry.get("recordingDateFolders").and_then(|v| v.as_bool()).unwrap_or(true),
document_date_folders: entry.get("documentDateFolders").and_then(|v| v.as_bool()).unwrap_or(true),
```

`serialize_vault_entry` (append, before the return):

```rust
if !v.recording_date_folders {
    entry.insert("recordingDateFolders".to_string(), json!(false));
}
if !v.document_date_folders {
    entry.insert("documentDateFolders".to_string(), json!(false));
}
```

- [ ] **Step 4: Add `capture_dir`**

In `capture_paths.rs`, right after `dated_folder`:

```rust
/// The directory a capture (or import) writes into: the dated `<root>/YYYY/MM`
/// when `dated`, or the flat `root` itself when not. Scanners and recovery
/// look in BOTH layouts, so flipping this only changes where NEW files land.
pub fn capture_dir(root: &Path, date: NaiveDate, dated: bool) -> PathBuf {
    if dated {
        dated_folder(root, date)
    } else {
        root.to_path_buf()
    }
}
```

- [ ] **Step 5: Fix struct-literal test constructors**

`rg -n "VaultCaptureConfig \{" src-tauri/core` and add the two new bool fields wherever a full literal is built (they default via `..VaultCaptureConfig::default()` in most tests; only fully-spelled literals like `config_round_trips_through_serialize_and_parse` need the two fields added — set them to non-default `false`/`true` to exercise the round-trip).

- [ ] **Step 6: Run + commit**

Run: `cd src-tauri/core && cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --check`
Expected: PASS + clean.

```bash
git add src-tauri/core/src/vault_config.rs src-tauri/core/src/capture_paths.rs
git commit -m "feat(core): per-domain date-folder toggles + capture_dir helper

recording_date_folders/document_date_folders (default true, written only
when false); capture_dir(root, date, dated) picks the dated YYYY/MM dir or
the flat root."
```

## Task 4.2: Layout-agnostic recording scanners (core)

**Files:**
- Modify: `src-tauri/core/src/transcript.rs` (add shared `capture_mp3s`, use in `pending_transcriptions`), `src-tauri/core/src/recordings.rs` (use it)

**Interfaces:**
- Produces: `pub(crate) fn transcript::capture_mp3s(root: &Path) -> Vec<(PathBuf, String)>` — capture `.mp3`s (path + base) under `root` in BOTH the flat and dated layouts.

- [ ] **Step 1: Write the failing tests**

In `recordings.rs` tests, add a flat-layout case (reuse `write_recording` but write to the root itself; add a small helper or write directly):

```rust
#[test]
fn lists_recordings_from_both_flat_and_dated_layouts() {
    let root = tempfile::tempdir().unwrap();
    // dated
    write_recording(root.path(), "2026", "07", "2026-07-04 0900 Dated", Some("Meeting"));
    // flat: mp3 directly under the root
    std::fs::write(root.path().join("2026-07-04 1000 Flat.mp3"), b"id3").unwrap();
    let list = list_recordings(&[root.path().to_path_buf()]);
    let titles: Vec<_> = list.iter().map(|e| e.title.as_str()).collect();
    assert!(titles.contains(&"Flat"));
    assert!(titles.contains(&"Dated"));
}

#[test]
fn ignores_foreign_and_part_files_at_the_flat_root() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("holiday.mp3"), b"x").unwrap(); // not a capture base
    std::fs::write(root.path().join(".2026-07-04 1405 Live.mp3.part"), b"x").unwrap(); // in-progress
    std::fs::write(root.path().join("2026-07-04 1405 Real.mp3"), b"id3").unwrap();
    let list = list_recordings(&[root.path().to_path_buf()]);
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].title, "Real");
}
```

In `transcript.rs` tests, add:

```rust
#[test]
fn pending_transcriptions_finds_flat_layout_recordings() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("2026-07-04 1000 Flat.mp3"), b"id3").unwrap();
    let pending = pending_transcriptions(root.path());
    assert_eq!(pending.len(), 1);
    assert!(pending[0].ends_with("2026-07-04 1000 Flat.mp3"));
}
```

- [ ] **Step 2: Run to confirm failure**

Run: `cd src-tauri/core && cargo test -p vault_buddy_core lists_recordings_from_both pending_transcriptions_finds_flat ignores_foreign_and_part_files_at_the_flat_root`
Expected: FAIL — flat-level files are not scanned today.

- [ ] **Step 3: Add the shared `capture_mp3s` walker**

In `transcript.rs`, near `pending_transcriptions`:

```rust
/// Capture `.mp3` files (path + base name) under `root` in BOTH layouts:
/// directly in `root` (flat) and under `<root>/YYYY/MM` (dated). Capture-named
/// files only; never follows symlinks (dir_entries reads the dirent no-follow).
/// Shared by the recordings list and the transcription backfill so both agree
/// on where a recording can live regardless of the vault's date-folder setting.
pub(crate) fn capture_mp3s(root: &Path) -> Vec<(PathBuf, String)> {
    let mut out = Vec::new();
    let mut push_from = |dir: &Path| {
        for (path, ft, name) in dir_entries(dir) {
            if !ft.is_file() {
                continue;
            }
            if let Some(base) = name.strip_suffix(".mp3") {
                if is_capture_base(base) {
                    out.push((path, base.to_string()));
                }
            }
        }
    };
    push_from(root); // flat layout
    for (year, yft, yname) in dir_entries(root) {
        if !yft.is_dir() || !is_digit_dir(&yname, 4) {
            continue;
        }
        for (month, mft, mname) in dir_entries(&year) {
            if !mft.is_dir() || !is_digit_dir(&mname, 2) {
                continue;
            }
            push_from(&month); // dated layout
        }
    }
    out
}
```

Rewrite `pending_transcriptions` to use it:

```rust
pub fn pending_transcriptions(root: &Path) -> Vec<PathBuf> {
    capture_mp3s(root)
        .into_iter()
        .map(|(path, _base)| path)
        .filter(|path| needs_transcription(path))
        .collect()
}
```

- [ ] **Step 4: Rewrite `list_recordings` onto the shared walker**

In `recordings.rs`, replace the nested `dir_entries`/`is_digit_dir` walk in `list_recordings` with:

```rust
pub fn list_recordings(roots: &[PathBuf]) -> Vec<RecordingEntry> {
    let mut out = Vec::new();
    for root in roots {
        for (path, base) in crate::transcript::capture_mp3s(root) {
            out.push(entry_for(&path, &base));
        }
    }
    out.sort_by(|a, b| {
        b.recorded_at.cmp(&a.recorded_at).then_with(|| b.mp3_path.cmp(&a.mp3_path))
    });
    out
}
```

Update `recordings.rs`'s `use crate::transcript::{…}` to import `capture_mp3s` and drop now-unused `dir_entries`/`is_digit_dir` imports if they're no longer referenced (clippy will flag unused imports).

- [ ] **Step 5: Run + clippy + commit**

Run: `cd src-tauri/core && cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --check`
Expected: PASS + clean.

```bash
git add src-tauri/core/src/transcript.rs src-tauri/core/src/recordings.rs
git commit -m "feat(core): scan recordings in both flat and dated layouts

Share one capture_mp3s walker (flat root + YYYY/MM) between the recordings
list and the transcription backfill, so recordings are found regardless of
the vault's date-folder setting. Ownership/no-follow gates unchanged."
```

## Task 4.3: Layout-agnostic capture recovery (capture crate)

**Files:**
- Modify: `src-tauri/capture/src/recovery.rs`

- [ ] **Step 1: Write the failing test**

In `recovery.rs` tests, add (mirroring `walks_dated_subfolders_and_avoids_collisions` but at the flat root):

```rust
#[test]
fn recovers_a_flat_layout_orphan_part() {
    let root = tempfile::tempdir().unwrap();
    // A stale, framed .part directly under the root (flat layout).
    let part = root.path().join(".2026-07-04 1405 Flat.mp3.part");
    std::fs::write(&part, mp3_bytes()).unwrap();
    // Force staleness by using a zero window.
    let actions = recover_root(root.path(), "Work", Duration::from_secs(0), false, false);
    assert!(actions.iter().any(|a| matches!(a, RecoveryAction::Recovered { .. })));
    assert!(root.path().join("2026-07-04 1405 Flat (recovered).mp3").exists());
}

#[test]
fn a_foreign_part_at_the_flat_root_is_left_alone() {
    let root = tempfile::tempdir().unwrap();
    let foreign = root.path().join(".something.download.mp3.part");
    std::fs::write(&foreign, b"x").unwrap();
    recover_root(root.path(), "Work", Duration::from_secs(0), false, false);
    assert!(foreign.exists(), "foreign .part must survive");
}
```

- [ ] **Step 2: Run to confirm failure**

Run: `cd src-tauri/capture && cargo test recovers_a_flat_layout_orphan_part a_foreign_part_at_the_flat_root`
Expected: FAIL — `walk` only descends `YYYY/MM`. (Linux needs `libasound2-dev`; CI installs it.)

- [ ] **Step 3: Add the flat level to `walk`**

```rust
/// Vault Buddy writes under `<root>/YYYY/MM` (dated) OR directly in `<root>`
/// (flat), per the vault's date-folder setting — recovery sweeps BOTH so a
/// crash orphan is finalized regardless. It still descends no further (an
/// arbitrary user subfolder is never touched). The visit closure's ownership
/// gates (capture-base + owned-temp markers) keep foreign files safe at the
/// flat level.
fn walk(root: &Path, visit: &mut dyn FnMut(&Path)) {
    // Flat layout: our files directly under the root.
    for (file_path, file_ft, _) in dir_entries(root) {
        if file_ft.is_file() {
            visit(&file_path);
        }
    }
    // Dated layout: <root>/YYYY/MM.
    for (year_path, year_ft, year_name) in dir_entries(root) {
        if !year_ft.is_dir() || !is_digit_dir(&year_name, 4) {
            continue;
        }
        for (month_path, month_ft, month_name) in dir_entries(&year_path) {
            if !month_ft.is_dir() || !is_digit_dir(&month_name, 2) {
                continue;
            }
            for (file_path, file_ft, _) in dir_entries(&month_path) {
                if file_ft.is_file() {
                    visit(&file_path);
                }
            }
        }
    }
}
```

- [ ] **Step 4: Run + clippy + commit**

Run: `cd src-tauri/capture && cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --check`
Expected: PASS + clean.

```bash
git add src-tauri/capture/src/recovery.rs
git commit -m "feat(capture): recover orphaned parts in the flat layout too

walk() now sweeps capture files directly under the root (flat) as well as
<root>/YYYY/MM; ownership gates keep foreign files untouched."
```

## Task 4.4: Recording write branch (shell)

**Files:**
- Modify: `src-tauri/src/capture_commands.rs`

- [ ] **Step 1: Branch the write dir on the toggle**

At `capture_commands.rs:446`, replace:

```rust
let dir = capture_paths::dated_folder(&root, date);
```

with:

```rust
// Flat vs dated is a per-vault setting; the scanners/recovery look in both,
// so this only changes where THIS recording lands. cfg carries the (possibly
// per-recording-overridden) mode; the toggle is mode-independent.
let dir = capture_paths::capture_dir(&root, date, cfg.recording_date_folders);
```

The existing `create_dir_all(&dir)` + `assert_root_inside_vault(&vault_path2, &dir)` work for both layouts (for flat, `dir == root`, already the validated recording root).

- [ ] **Step 2: Compile-gate + shell test**

Run: `cd src-tauri && cargo clippy -p vault-buddy --all-targets -- -D warnings && cargo test -p vault-buddy --lib`
Expected: clean + PASS.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/capture_commands.rs
git commit -m "feat(shell): write recordings flat when the vault opts out of dated folders"
```

## Task 4.5: Document import write branch + flat recovery (core + shell)

**Files:**
- Modify: `src-tauri/core/src/document_import.rs` (flat staging sweep), `src-tauri/src/document_commands.rs` (write branch)

- [ ] **Step 1: Write the failing recovery test**

In `document_import.rs` tests, add a flat-staging case (find the existing `clean_stale_staging_at` tests and mirror them at the root level):

```rust
#[test]
fn sweeps_a_stale_staging_dir_in_the_flat_layout() {
    let root = tempfile::tempdir().unwrap();
    // A staging dir directly under the documents root (flat layout).
    let staging = root.path().join(".Doc.123-0.vault-buddy.tmp.import");
    std::fs::create_dir_all(&staging).unwrap();
    let sweep = clean_stale_staging_at(root.path(), std::time::SystemTime::now() + std::time::Duration::from_secs(3600), std::time::Duration::from_secs(600));
    assert_eq!(sweep.removed.len(), 1);
    assert!(!staging.exists());
}
```

(Use a `now` far in the future so the fresh staging dir counts as stale, matching how the existing dated tests force staleness.)

- [ ] **Step 2: Run to confirm failure**

Run: `cd src-tauri/core && cargo test -p vault_buddy_core sweeps_a_stale_staging_dir_in_the_flat_layout`
Expected: FAIL — only `YYYY/MM` is swept today.

- [ ] **Step 3: Sweep the flat level in `clean_stale_staging_at`**

Extract the per-directory staging sweep (the `for entry in read_dir(&month) { … }` body) into a closure `sweep_dir(dir: &Path, sweep: &mut StagingSweep)` and call it for the flat root and each dated month:

```rust
let sweep_dir = |dir: &Path, sweep: &mut StagingSweep| {
    let Ok(entries) = std::fs::read_dir(dir) else { return; };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if !is_import_staging_dir(&name) { continue; }
        let Ok(canon) = path.canonicalize() else { continue; };
        if canon != path || !canon.is_dir() { continue; }
        let stale = std::fs::symlink_metadata(&path)
            .and_then(|m| m.modified())
            .map(|mtime| match now.duration_since(mtime) { Ok(age) => age >= stale_after, Err(_) => false })
            .unwrap_or(false);
        if stale {
            match std::fs::remove_dir_all(&path) {
                Ok(()) => sweep.removed.push(path),
                Err(e) => { log::warn!("import-recovery: failed to remove {path:?}: {e}"); sweep.pending += 1; }
            }
        } else {
            sweep.pending += 1;
        }
    }
};
// Flat layout: staging dirs directly under the documents root.
sweep_dir(&canon_root, &mut sweep);
// Dated layout: Documents/<YYYY>/<MM>.
for year in contained_subdirs(&canon_root, is_year_dir) {
    for month in contained_subdirs(&year, is_month_dir) {
        sweep_dir(&month, &mut sweep);
    }
}
sweep
```

(`canon_root` is already canonicalized and asserted in-vault by the caller, so `canon == path` for a real in-place staging dir directly under it holds the same way it does under a canonical month.)

- [ ] **Step 4: Write branch in `convert_blocking`**

In `document_commands.rs::convert_blocking`, after computing `safe` and loading `cfg`, branch the target dir:

```rust
let dated = cfg.document_date_folders;
let dir = if dated {
    document_import::target_dir(vault_root, &documents_folder, year, month)
} else {
    safe.clone()
};
```

Then keep `assert_path_inside_vault(vault_root, &dir)?`, `create_dir_all(&dir)`, and the post-create `assert_path_inside_vault(vault_root, &dir)?`. Gate the `is_real_dated_dir` check on `dated` only (a flat `dir == safe` was already asserted in-vault above):

```rust
if dated && !document_import::is_real_dated_dir(&safe, &dir, year, month) {
    return Err("Import destination resolves through a link; use a real Documents folder.".into());
}
```

- [ ] **Step 5: Run tests + compile-gate**

Run: `cd src-tauri/core && cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --check`
Run: `cd src-tauri && cargo clippy -p vault-buddy --all-targets -- -D warnings`
Expected: PASS + clean.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/core/src/document_import.rs src-tauri/src/document_commands.rs
git commit -m "feat(documents): flat-layout import + recovery when the vault opts out

convert_document writes to the Documents root (no YYYY/MM) when
document_date_folders is off; the staging janitor sweeps orphans at the
flat root too. Containment + owned-in-place gates unchanged."
```

## Task 4.6: Frontend toggles (Recording + Documents)

**Files:**
- Modify: `src/types.ts`, `src/components/RecordingSettings.vue`, `src/components/CaptureSettings.vue`, `src/components/document_commands` DTO (`src-tauri/src/document_commands.rs`)
- Test: `tests/recording-settings.test.ts`, `tests/capture-settings.test.ts`

- [ ] **Step 1: Extend the documents config DTO/type + preserve rule (Rust)**

In `document_commands.rs`: add `pub document_date_folders: bool,` to `DocumentsConfigDto`; set it from `vault.document_date_folders` in `get_documents_config`. Change `set_documents_config`'s signature to also take `document_date_folders: bool` and set `v.document_date_folders = document_date_folders;` before the write (it already read-modify-writes the whole vault entry, so `recording_date_folders` and the rest are preserved). In `capture_config_commands.rs::set_capture_config`, add `recording_date_folders: cfg.recording_date_folders,` to the written value (from the DTO — add the field to `CaptureConfigDto` + `from_config` too) and `document_date_folders: existing.document_date_folders,` to the preserve list.

- [ ] **Step 2: Extend the TS types**

In `src/types.ts`: `CaptureConfig` gains `recordingDateFolders: boolean;`; `DocumentsConfig` gains `documentDateFolders: boolean;`. Add `recordingDateFolders` to `RecordingSettingsValue` (in the component).

- [ ] **Step 3: Write the failing frontend tests**

In `tests/recording-settings.test.ts`, assert the recording toggle round-trips:

```ts
it("emits the dated-folders toggle", async () => {
  const v = { ...value, recordingDateFolders: true };
  const w = mount(RecordingSettings, { props: { modelValue: v, devices, folderError: null } });
  await w.get('[data-testid="recording-date-folders-toggle"]').setValue(false);
  expect((w.emitted("update:modelValue")!.at(-1)![0] as typeof v).recordingDateFolders).toBe(false);
});
```

In `tests/capture-settings.test.ts`, extend the mock `config` with `recordingDateFolders: true`, add `documentDateFolders: true` to the `get_documents_config` mock return, and assert the Documents toggle appears (`[data-testid="document-date-folders-toggle"]`).

- [ ] **Step 4: Run to confirm failure**

Run: `npx vitest run tests/recording-settings.test.ts tests/capture-settings.test.ts`
Expected: FAIL — toggles/testids not present.

- [ ] **Step 5: Add the toggles**

In `RecordingSettings.vue`, add a `recordingDateFolders` computed proxy + a checkbox in the Folders card:

```vue
<div class="flex items-center justify-between">
  <label for="recording-date-folders" class="text-sm text-slate-200">
    Organize into year/month folders
    <span class="block text-xs text-slate-500">Off = one flat folder</span>
  </label>
  <input id="recording-date-folders" v-model="recordingDateFolders" data-testid="recording-date-folders-toggle" type="checkbox" class="h-4 w-4 accent-violet-500">
</div>
```

Wire `recordingDateFolders` in `CaptureSettings.vue`: add a ref (default `true`), load from `cfg.recordingDateFolders`, send it in `save()`'s `set_capture_config` payload, add it to the `recordingBundle` map and the "Saved ✓"-reset watch.

In `CaptureSettings.vue`'s Documents group, add a `documentDateFolders` ref (default `true`), load it from `get_documents_config`, render a matching checkbox (testid `document-date-folders-toggle`), and send it through the `set_documents_config` invoke (extend `useOptionalFolderField`'s documents save call, or add the bool as a second arg to the `set_documents_config` invoke). Add it to the "Saved ✓"-reset watch.

- [ ] **Step 6: Run tests + build + full suite**

Run: `npx vitest run tests/recording-settings.test.ts tests/capture-settings.test.ts`
Expected: PASS.
Run: `npm test && npm run build && npm run lint && npm run check:loc`
Expected: all green.
Run: `cd src-tauri && cargo test -p vault-buddy --lib && cargo clippy -p vault-buddy --all-targets -- -D warnings`
Expected: PASS + clean.

- [ ] **Step 7: Commit**

```bash
git add src/types.ts src/components/RecordingSettings.vue src/components/CaptureSettings.vue src-tauri/src/document_commands.rs src-tauri/src/capture_config_commands.rs tests/
git commit -m "feat(ui): per-domain year/month folder toggles in Vault settings

Recording toggle (capture-config save) and Documents toggle
(set_documents_config); set_capture_config preserves document_date_folders."
```

---

# Docs & final verification

## Task D: Documentation + full gate sweep

**Files:**
- Modify: `AGENTS.md`, `docs/DEVELOPMENT.md`, `CONTEXT.md`, `docs/Gaps.md` (if an edge surfaced)

- [ ] **Step 1: AGENTS.md**

- Capture domain section: `recording_folder` → `meeting_folder`/`voice_note_folder`; note `folder_for`/`recording_roots` (deduped union) and the legacy migration; document the flat/dated toggle (`recording_date_folders`) and that the recordings list, transcription backfill, and recovery are layout-agnostic (scan flat root + `YYYY/MM`), with no bulk-move.
- Document-import domain section: `document_date_folders` toggle + flat staging recovery.
- The IPC table: update the "Defined in" column for `get_capture_config`/`set_capture_config` (now `capture_config_commands.rs`); `set_documents_config` now also carries the documents date-folder toggle.
- "What compiles where" / repository map: add `core/src/vault_config.rs` and `src-tauri/src/capture_config_commands.rs`.

- [ ] **Step 2: docs/DEVELOPMENT.md**

Update the `config.json` reference: remove `recordingFolder`; add `meetingFolder`/`voiceNoteFolder` (optional; omit → `Meetings`/`Voice Notes`; note the legacy `recordingFolder` still reads as a fallback), `recordingDateFolders`/`documentDateFolders` (optional; omit → `true`; written only when `false`).

- [ ] **Step 3: CONTEXT.md**

Add **Dated layout** and **Flat layout** to the ubiquitous-language glossary (definitions from the spec's "Domain vocabulary" section).

- [ ] **Step 4: Full gate sweep**

Run, from repo root:

```bash
npm run lint && npm run check:loc && npm run check:quality && npm run test:coverage && npm run build
cd src-tauri && cargo fmt --check
cd src-tauri/core && cargo clippy --all-targets -- -D warnings && cargo test
cd ../capture && cargo clippy --all-targets -- -D warnings && cargo test
cd ../transcribe && cargo clippy --all-targets -- -D warnings && cargo test
cd ../mcp && cargo clippy --all-targets -- -D warnings && cargo test
cd .. && cargo test -p vault-buddy --lib
```

Expected: all green. If `check:loc`/coverage improved, `--update` and include the baseline in this commit.

- [ ] **Step 5: Commit**

```bash
git add AGENTS.md docs/DEVELOPMENT.md CONTEXT.md docs/Gaps.md scripts/loc-baseline.json vite.config.ts
git commit -m "docs: per-mode folders, flat/dated layout, and settings regroup

Update AGENTS.md (capture + document-import domains, IPC table, repo map),
the config.json reference, and CONTEXT.md (dated/flat layout terms)."
```

---

## Self-review notes (spec coverage)

- **Distinct folders** → Tasks 3.1–3.3 (model + DTO + UI), migration in 3.1.
- **Per-domain layout toggle** → Task 4.1 (fields + helper), 4.4/4.5 (write branches), 4.2/4.3/4.5 (layout-agnostic read+recovery), 4.6 (UI).
- **Regroup** → Tasks 2.1–2.2.
- **LOC-forced splits** → Tasks 1.1–1.2.
- **No bulk-move** → honored by design (write-only branch; layout-agnostic scanners); no task moves files.
- **Docs + terms** → Task D.
- **Type consistency:** `meetingFolder`/`voiceNoteFolder`/`recordingDateFolders`/`documentDateFolders` are the single camelCase spellings used across `config.json`, the Rust DTOs, and `types.ts`; `folder_for`/`recording_roots`/`capture_dir`/`capture_mp3s` keep one spelling each throughout.
