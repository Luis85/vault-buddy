# Document Import: per-vault Images vs. Text-only — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a per-vault setting that makes a document import either bring the source's images along (today's behavior) or produce a text-only note with images dropped.

**Architecture:** One new boolean on the per-vault `VaultCaptureConfig` (`document_extract_images`, default `true`), threaded through the same surfaces that already carry `document_date_folders`: config parse/serialize, the `get_documents_config`/`set_documents_config` IPC pair, the capture-save preservation, the `DocumentsConfigTab.vue` toggle, and — the only behavioral branch — the Pandoc argument vector, where text-only swaps `--extract-media` for an app-authored `--lua-filter` that strips `Image`/`Figure` nodes.

**Tech Stack:** Rust (Tauri v2 shell + pure `vault_buddy_core` crate), Vue 3 + Pinia + Tailwind frontend, Vitest (happy-dom + mockIPC), Pandoc (user-installed) driven as a subprocess.

**Spec:** `docs/superpowers/specs/2026-07-16-document-import-image-text-toggle-design.md`

## Global Constraints

- **Default `true`** everywhere — an absent/omitted setting means "extract images", so existing vaults and a fresh vault behave exactly as today.
- **Per-field defensive parse:** a malformed `documentExtractImages` value defaults only itself (`unwrap_or(true)`), never rejects the file — the whole-file rule for the hand-edited `config.json`.
- **Omit-when-default serialize:** write `"documentExtractImages": false` only when `false`; never when `true` — keep `config.json` minimal (the `document_date_folders` rule).
- **New imports only:** flipping the toggle changes what the next import produces; it never rewrites or re-converts existing notes.
- **Naming (verbatim):** Rust `document_extract_images`; JSON/DTO/TS `documentExtractImages`; UI label `Import images`, subtext `Off = text only (no images, no media folder)`.
- **`--sandbox` stays:** both modes keep `--sandbox`; the strip filter is app-authored and I/O-free, so the untrusted-document read stays sandboxed.
- **Conventional Commits**, imperative subject; body explains the *why*/failure mode. End commit messages with the two trailers this repo requires (`Co-Authored-By:` and `Claude-Session:`).
- **Shell-crate build prerequisite (one-time):** the shell crate (`src-tauri/src/*`, incl. `pandoc.rs`) only compiles/tests on Linux after `npm run setup:linux` (GTK/WebView/tray libs) **and** a built frontend (`npm run build`, producing `../dist`). Run these once before Tasks 2–3. Core-only (Task 1) and frontend (Task 4) need neither.
- **Do not** put the model identifier `claude-opus-4-8` in any committed artifact.

---

### Task 1: Core config field `document_extract_images`

**Files:**
- Modify: `src-tauri/core/src/vault_config.rs` (struct + `Default` + `vault_entry` + `serialize_vault_entry`, and the full-literal round-trip test)
- Test: `src-tauri/core/src/vault_config.rs` (its inline `#[cfg(test)] mod tests`)

**Interfaces:**
- Produces: `VaultCaptureConfig.document_extract_images: bool` — default `true`, parsed from JSON key `documentExtractImages`, serialized only when `false`. Consumed by Tasks 2 and 3.

- [ ] **Step 1: Write the failing test.** In `src-tauri/core/src/vault_config.rs`, inside `mod tests`, add:

```rust
    #[test]
    fn document_extract_images_defaults_true_and_round_trips() {
        // Default is true (images imported) — today's behavior is preserved
        // when the setting is absent.
        assert!(VaultCaptureConfig::default().document_extract_images);
        // Absent → true; an explicit false parses.
        let cfg = crate::capture_config::parse_config(
            r#"{ "vaults": { "a": {}, "b": { "documentExtractImages": false } } }"#,
        );
        assert!(crate::capture_config::vault_config(&cfg, "a").document_extract_images);
        assert!(!crate::capture_config::vault_config(&cfg, "b").document_extract_images);
        // Serialize omits when true (default), writes the key only when false.
        let mut only_true = crate::capture_config::AppConfig::default();
        only_true
            .vaults
            .insert("t".into(), VaultCaptureConfig::default());
        assert!(!crate::capture_config::serialize_config(&only_true).contains("documentExtractImages"));
        let mut has_false = crate::capture_config::AppConfig::default();
        has_false.vaults.insert(
            "f".into(),
            VaultCaptureConfig {
                document_extract_images: false,
                ..VaultCaptureConfig::default()
            },
        );
        let jf = crate::capture_config::serialize_config(&has_false);
        assert!(jf.contains("\"documentExtractImages\": false"));
        // And it survives a full serialize → parse round trip.
        let round = crate::capture_config::parse_config(&jf);
        assert!(!crate::capture_config::vault_config(&round, "f").document_extract_images);
    }
```

- [ ] **Step 2: Run the test to verify it fails.**

Run: `cd src-tauri/core && cargo test document_extract_images_defaults_true_and_round_trips`
Expected: **compile error** — `no field document_extract_images on type VaultCaptureConfig` (the field doesn't exist yet). This is the failing state.

- [ ] **Step 3: Add the struct field.** In `pub struct VaultCaptureConfig`, immediately after the `document_date_folders: bool,` field (the last field), add:

```rust
    /// Whether a document import extracts the source's images into a media
    /// folder beside the note (true, the default) or drops them for a
    /// text-only note (false). Like the date-folder toggles, flipping this
    /// only changes what NEW imports produce — existing notes are untouched.
    pub document_extract_images: bool,
```

- [ ] **Step 4: Add the default.** In `impl Default for VaultCaptureConfig`, immediately after `document_date_folders: true,` add:

```rust
            document_extract_images: true,
```

- [ ] **Step 5: Parse it.** In `pub fn vault_entry(...)`, immediately after the `document_date_folders: entry.get("documentDateFolders")...unwrap_or(true),` block (the last field in the returned struct literal), add:

```rust
        document_extract_images: entry
            .get("documentExtractImages")
            .and_then(|v| v.as_bool())
            .unwrap_or(true),
```

- [ ] **Step 6: Serialize it.** In `pub fn serialize_vault_entry(...)`, immediately after the existing `if !v.document_date_folders { ... }` block, add:

```rust
    if !v.document_extract_images {
        entry.insert("documentExtractImages".to_string(), json!(false));
    }
```

- [ ] **Step 7: Fix the full-literal round-trip test.** The test `config_round_trips_through_serialize_and_parse` builds a complete `VaultCaptureConfig { ... }` literal, which now won't compile (missing field). In that literal, immediately after the `document_date_folders: false,` line, add:

```rust
                document_extract_images: false,
```

- [ ] **Step 8: Run the test to verify it passes.**

Run: `cd src-tauri/core && cargo test document_extract_images_defaults_true_and_round_trips`
Expected: **PASS** (1 passed).

- [ ] **Step 9: Run the whole core crate + lint to check nothing regressed.**

Run: `cd src-tauri/core && cargo test && cargo clippy --all-targets -- -D warnings && cd .. && cargo fmt --check`
Expected: all tests pass, clippy clean, fmt reports no diffs.
Note: the *shell* crate won't compile until Task 2 adds the field to `set_capture_config`'s literal — that's Task 2's first step, by design. Core is self-contained here.

- [ ] **Step 10: Commit.**

```bash
git add src-tauri/core/src/vault_config.rs
git commit  # subject: feat(core): add per-vault documentExtractImages config field
```

---

### Task 2: Persist the setting through the documents-config IPC

**Files:**
- Modify: `src-tauri/src/capture_config_commands.rs` (`set_capture_config` — preserve the field; fixes the shell compile break)
- Modify: `src-tauri/src/document_commands.rs` (`DocumentsConfigDto`, `get_documents_config`, `set_documents_config`)

**Interfaces:**
- Consumes: `VaultCaptureConfig.document_extract_images` (Task 1).
- Produces: `DocumentsConfigDto.document_extract_images: bool`; `get_documents_config` returns it; `set_documents_config(lock, id, documents_folder, document_date_folders, document_extract_images: bool)` persists it. Consumed by Task 4.

- [ ] **Step 1: Preserve the field on a capture save.** In `src-tauri/src/capture_config_commands.rs`, in `set_capture_config`, the constructed `capture_config::VaultCaptureConfig { ... }` literal (ends with `document_date_folders: existing.document_date_folders,`) now won't compile. Immediately after that line add:

```rust
        // Same rule as the sibling documents fields: the image/text-only
        // toggle is owned by set_documents_config, so a capture save must
        // preserve it (read inside the lock) — never reset a vault to
        // images-on.
        document_extract_images: existing.document_extract_images,
```

- [ ] **Step 2: Add the DTO field.** In `src-tauri/src/document_commands.rs`, in `pub struct DocumentsConfigDto`, immediately after the `pub document_date_folders: bool,` field, add:

```rust
    /// Whether a document import extracts images into a media folder (true) or
    /// produces a text-only note with images dropped (false) — the Documents
    /// settings surface for `VaultCaptureConfig::document_extract_images`.
    pub document_extract_images: bool,
```

- [ ] **Step 3: Return it from `get_documents_config`.** In `get_documents_config`, in the returned `DocumentsConfigDto { ... }` literal, immediately after `document_date_folders: vault.document_date_folders,` add:

```rust
        document_extract_images: vault.document_extract_images,
```

- [ ] **Step 4: Accept + persist it in `set_documents_config`.** Change the signature to add the parameter (after `document_date_folders: bool,`):

```rust
pub fn set_documents_config(
    lock: tauri::State<ConfigWriteLock>,
    id: String,
    documents_folder: Option<String>,
    document_date_folders: bool,
    document_extract_images: bool,
) -> Result<(), String> {
```

Then, in the function body, immediately after `v.document_date_folders = document_date_folders;` add:

```rust
    v.document_extract_images = document_extract_images;
```

- [ ] **Step 5: Build the shell crate + run its tests.** (Requires the one-time `npm run setup:linux` + `npm run build` from Global Constraints.)

Run: `cd src-tauri && cargo test -p vault-buddy --lib`
Expected: **compiles and passes** — the previous compile break in `set_capture_config` and the new DTO literal both resolve. (These two commands have no dedicated unit test; the frontend test in Task 4 exercises the contract shape. The gate here is a clean compile + no regression in existing shell tests.)

- [ ] **Step 6: Lint + format.**

Run: `cd src-tauri && cargo clippy -p vault-buddy --all-targets -- -D warnings && cargo fmt --check`
Expected: clippy clean, fmt no diffs.

- [ ] **Step 7: Commit.**

```bash
git add src-tauri/src/capture_config_commands.rs src-tauri/src/document_commands.rs
git commit  # subject: feat(shell): persist documentExtractImages through documents config IPC
```

---

### Task 3: Conversion honors text-only via an image-strip Lua filter

**Files:**
- Modify: `src-tauri/src/pandoc.rs` (add the filter constants; add an `extract_images` parameter to `pandoc_args`; update its unit test; add a text-only test)
- Modify: `src-tauri/src/document_commands.rs` (`convert_blocking` — write the filter when text-only, pass the flag)

**Interfaces:**
- Consumes: `VaultCaptureConfig.document_extract_images` (Task 1).
- Produces: `pandoc_args(reader: &str, media_name: &str, note_name: &str, extract_images: bool) -> Vec<String>`; `pub(crate) const pandoc::STRIP_IMAGES_FILTER: &str`; `pub(crate) const pandoc::STRIP_IMAGES_LUA: &str`.

- [ ] **Step 1: Write the failing test.** In `src-tauri/src/pandoc.rs`, inside `mod tests`, add a new test for text-only mode:

```rust
    #[test]
    fn text_only_args_strip_images_and_skip_extract_media() {
        // extract_images = false: the strip filter replaces --extract-media, so
        // Pandoc never writes a media folder and the note ends up text-only.
        let args = pandoc_args("docx", "2026-07-10 Report", "2026-07-10 Report.md", false);
        assert!(args
            .iter()
            .any(|a| a == &format!("--lua-filter={STRIP_IMAGES_FILTER}")));
        assert!(!args.iter().any(|a| a.starts_with("--extract-media")));
        // Still sandboxed, GFM, and heap-capped in text-only mode.
        assert!(args.iter().any(|a| a == "--sandbox"));
        assert!(args.windows(2).any(|w| w == ["-t", "gfm"]));
        assert!(args.join(" ").contains("+RTS -M512M -RTS"));
        // The filter body actually removes images.
        assert!(STRIP_IMAGES_LUA.contains("function Image()"));
    }
```

- [ ] **Step 2: Update the existing `pandoc_args` test for the new signature.** Replace the body of `pandoc_args_are_sandboxed_relative_and_heap_capped` so it passes `true` and asserts images mode does not add the filter:

```rust
    #[test]
    fn pandoc_args_are_sandboxed_relative_and_heap_capped() {
        let args = pandoc_args("docx", "2026-07-10 Report", "2026-07-10 Report.md", true);
        // reader
        assert!(args.windows(2).any(|w| w == ["-f", "docx"]));
        assert!(args.windows(2).any(|w| w == ["-t", "gfm"]));
        // sandbox always present
        assert!(args.iter().any(|a| a == "--sandbox"));
        // relative extract-media + output (no temp path baked in)
        assert!(args
            .iter()
            .any(|a| a == "--extract-media=2026-07-10 Report"));
        assert!(args.windows(2).any(|w| w == ["-o", "2026-07-10 Report.md"]));
        // heap cap
        let joined = args.join(" ");
        assert!(joined.contains("+RTS -M512M -RTS"));
        // images mode does NOT add the strip filter
        assert!(!args.iter().any(|a| a.starts_with("--lua-filter")));
    }
```

- [ ] **Step 3: Run the tests to verify they fail.**

Run: `cd src-tauri && cargo test -p vault-buddy --lib pandoc_args_are_sandboxed_relative_and_heap_capped text_only_args_strip_images_and_skip_extract_media`
Expected: **compile error** — `pandoc_args` takes 3 args, not 4, and `STRIP_IMAGES_FILTER`/`STRIP_IMAGES_LUA` are undefined.

- [ ] **Step 4: Add the filter constants.** In `src-tauri/src/pandoc.rs`, immediately above `pub(crate) fn pandoc_args`, add:

```rust
/// Filename of the "text only" image-strip Lua filter. `convert_blocking`
/// writes it into the per-import staging dir and passes it to Pandoc relative
/// to Pandoc's cwd (= that staging dir). A plain (non-dot) name is fine: it
/// lives inside the already-hidden, already-cleaned staging dir.
pub(crate) const STRIP_IMAGES_FILTER: &str = "strip-images.lua";

/// The image-strip Lua filter body. App-authored and I/O-free: it only
/// deletes Image/Figure nodes from the parsed document, so it does NOT weaken
/// `--sandbox`'s protection of the untrusted document read. Handles both older
/// Pandoc (an implicit figure is a Para holding one Image — its inline Image is
/// dropped) and Pandoc 3.x (an explicit Figure block); a handler for an element
/// a given Pandoc version never produces simply never fires.
pub(crate) const STRIP_IMAGES_LUA: &str = "\
-- Vault Buddy: \"text only\" document import — drop all images so the
-- imported note carries only text (no image links, no media folder).
function Image() return {} end
function Figure() return {} end
";
```

- [ ] **Step 5: Add the `extract_images` parameter to `pandoc_args`.** Replace the whole `pandoc_args` function with:

```rust
/// Pandoc argument vector (program excluded). Source is added by the caller as
/// an absolute path; every OUTPUT here is relative (Pandoc runs with cwd =
/// work dir) so rewritten image links stay valid after publish.
///
/// `extract_images` picks the media behavior: true extracts embedded/linked
/// media into the reserved sibling folder (`--extract-media`, the default);
/// false strips all images via the app-authored `--lua-filter` and creates NO
/// media folder — the per-vault "text only" mode. `--sandbox` and the heap cap
/// are present either way.
pub(crate) fn pandoc_args(
    reader: &str,
    media_name: &str,
    note_name: &str,
    extract_images: bool,
) -> Vec<String> {
    let mut args = vec![
        "-f".into(),
        reader.into(),
        "-t".into(),
        "gfm".into(),
        "--sandbox".into(),
    ];
    if extract_images {
        args.push(format!("--extract-media={media_name}"));
    } else {
        // Text only: strip images instead of extracting them. Without
        // --extract-media no media folder is created; the filter drops the
        // links so the note has no dangling image references.
        args.push(format!("--lua-filter={STRIP_IMAGES_FILTER}"));
    }
    args.extend([
        "-o".into(),
        note_name.into(),
        // GHC RTS heap cap: a timeout bounds time, not memory; a crafted doc
        // could OOM before it fires. Pandoc dies with a memory error instead.
        "+RTS".into(),
        "-M512M".into(),
        "-RTS".into(),
    ]);
    args
}
```

- [ ] **Step 6: Run the two tests to verify they pass.**

Run: `cd src-tauri && cargo test -p vault-buddy --lib pandoc_args_are_sandboxed_relative_and_heap_capped text_only_args_strip_images_and_skip_extract_media`
Expected: **both PASS**. (The real call site in `convert_blocking` is still on the old 3-arg signature, so the crate as a whole won't build yet — the next step fixes it. If you prefer a green build between every step, do Step 7 before re-running the full suite.)

- [ ] **Step 7: Thread the flag through `convert_blocking`.** In `src-tauri/src/document_commands.rs`:

First, extend the `use crate::pandoc::{...}` import to include the two constants:

```rust
use crate::pandoc::{
    pandoc_args, pandoc_command, resolve_working_pandoc, run_capturing, sandbox_supported, Capture,
    CONVERT_TIMEOUT, STRIP_IMAGES_FILTER, STRIP_IMAGES_LUA,
};
```

Then, in `convert_blocking`, find the line `let args = pandoc_args(format.reader(), &plan.media_name, &plan.note_name);` and replace it with:

```rust
    // Images vs. text-only is per-vault. When off, write the app-authored strip
    // filter into the staging dir (cleaned up with everything else) and pass it
    // to Pandoc instead of --extract-media, so the note ends up text-only with
    // no media folder. The filter path is relative — Pandoc's cwd is work_dir.
    let extract_images = cfg.document_extract_images;
    if !extract_images {
        std::fs::write(plan.work_dir.join(STRIP_IMAGES_FILTER), STRIP_IMAGES_LUA)
            .map_err(|e| format!("Could not prepare import: {e}"))?;
    }
    let args = pandoc_args(
        format.reader(),
        &plan.media_name,
        &plan.note_name,
        extract_images,
    );
```

(No publish changes are needed: `publish` already writes only the note when the staged media dir is empty/absent, which is exactly the text-only case.)

- [ ] **Step 8: Build + test the whole shell crate.**

Run: `cd src-tauri && cargo test -p vault-buddy --lib`
Expected: **compiles and all pass** — including both updated pandoc tests.

- [ ] **Step 9: (Best-effort) real-Pandoc check.** This is the one thing the unit tests can't cover. If a real Pandoc ≥ 2.15 is available (`pandoc --version`), confirm the `--sandbox --lua-filter` combination behaves:

```bash
# text-only: expect out.md with NO image and NO media/ folder created
printf '%s' 'placeholder' >/dev/null   # (use a real fixture .docx with an embedded image)
pandoc <fixture-with-image>.docx -f docx -t gfm --sandbox \
  --lua-filter=<path-to>/strip-images.lua -o out.md +RTS -M512M -RTS
# verify: out.md contains no "![" image markup, and no media folder was written
```

Expected: succeeds, `out.md` has no image markup, no media folder. If any supported Pandoc build rejects `--sandbox` + `--lua-filter`, stop and switch to the spec's fallback (mechanism B: strip the `![…](…)` markup in Rust after conversion) — the config/DTO/command/UI surfaces are unchanged; only this task's mechanism changes. If Pandoc isn't installed here, note it and leave this for the reviewer/CI environment.

- [ ] **Step 10: Lint + format, then commit.**

```bash
cd src-tauri && cargo clippy -p vault-buddy --all-targets -- -D warnings && cargo fmt --check && cd ..
git add src-tauri/src/pandoc.rs src-tauri/src/document_commands.rs
git commit  # subject: feat(document-import): honor text-only mode via image-strip Lua filter
```

---

### Task 4: Frontend — "Import images" toggle

**Files:**
- Modify: `src/types.ts` (`DocumentsConfig`)
- Modify: `src/components/DocumentsConfigTab.vue`
- Test: `tests/documents-config-tab.test.ts`
- Modify: `tests/capture-settings.test.ts` (its `get_documents_config` mock)

**Interfaces:**
- Consumes: `DocumentsConfig.documentExtractImages` from `get_documents_config`; `set_documents_config`'s `documentExtractImages` arg (Task 2).
- Produces: a `data-testid="document-extract-images-toggle"` checkbox that autosaves immediately.

- [ ] **Step 1: Write the failing tests.** In `tests/documents-config-tab.test.ts`:

First, extend the `mountTab` options type and default `get` response so the mock carries the new field. Change the `opts` type to include `documentExtractImages?: boolean;` (after `documentDateFolders?: boolean;`), and change the default `get_documents_config` return object to:

```ts
        : {
            documentsFolder: opts.documentsFolder ?? null,
            documentDateFolders: opts.documentDateFolders ?? true,
            documentExtractImages: opts.documentExtractImages ?? true,
          };
```

Then update the two existing `toEqual(...)` assertions on `set_documents_config` args to include the new field. In "debounces a folder edit and saves both fields after 600ms":

```ts
    expect(calls.find((c) => c.cmd === "set_documents_config")?.args).toEqual({
      id: "v1",
      documentsFolder: "Imported",
      documentDateFolders: false,
      documentExtractImages: true,
    });
```

In "saves the toggle immediately (no debounce)":

```ts
    expect(calls.find((c) => c.cmd === "set_documents_config")?.args).toEqual({
      id: "v1",
      documentsFolder: "Docs",
      documentDateFolders: false,
      documentExtractImages: true,
    });
```

Then add two new tests inside the `describe`:

```ts
  it("loads the images toggle from disk", async () => {
    const { wrapper } = mountTab({ documentExtractImages: false });
    await flushPromises();
    expect(
      wrapper.get<HTMLInputElement>('[data-testid="document-extract-images-toggle"]').element.checked,
    ).toBe(false);
  });

  it("saves the images toggle immediately when turned off", async () => {
    const { wrapper, calls } = mountTab({ documentsFolder: "Docs", documentExtractImages: true });
    await flushPromises();
    await wrapper.get('[data-testid="document-extract-images-toggle"]').setValue(false);
    await flushPromises();
    expect(calls.find((c) => c.cmd === "set_documents_config")?.args).toEqual({
      id: "v1",
      documentsFolder: "Docs",
      documentDateFolders: true,
      documentExtractImages: false,
    });
  });
```

- [ ] **Step 2: Run the tests to verify they fail.**

Run: `npx vitest run tests/documents-config-tab.test.ts`
Expected: **FAIL** — the new toggle element doesn't exist, and the updated `toEqual` assertions don't match (the component doesn't send `documentExtractImages` yet).

- [ ] **Step 3: Add the type field.** In `src/types.ts`, in `interface DocumentsConfig`, immediately after the `documentDateFolders: boolean;` field, add:

```ts
  /** Whether a document import extracts images into a media folder (true) or
   * produces a text-only note with images dropped (false). Default true. */
  documentExtractImages: boolean;
```

- [ ] **Step 4: Add the ref + load + save wiring in `DocumentsConfigTab.vue`.**

Add the ref after `const documentDateFolders = ref(true);`:

```ts
const documentExtractImages = ref(true);
```

Add the field to the `set_documents_config` invoke inside `useAutosave` (after `documentDateFolders: documentDateFolders.value,`):

```ts
      documentExtractImages: documentExtractImages.value,
```

Set it on load — in the `onMounted` `load` callback, after `documentDateFolders.value = cfg.documentDateFolders;`:

```ts
    documentExtractImages.value = cfg.documentExtractImages;
```

Add a toggle handler after `onToggle`:

```ts
function onExtractImagesToggle(event: Event) {
  documentExtractImages.value = (event.target as HTMLInputElement).checked;
  autosave.saveNow();
}
```

- [ ] **Step 5: Add the toggle markup.** In the `<template>`, immediately after the closing `</div>` of the existing `document-date-folders` toggle block (still inside the `<template v-else>`), add:

```html
      <div class="flex items-center justify-between rounded-xl border border-white/10 bg-white/5 p-2">
        <label
          for="document-extract-images"
          class="text-sm text-slate-200"
        >
          Import images
          <span class="block text-xs text-slate-500">Off = text only (no images, no media folder)</span>
        </label>
        <input
          id="document-extract-images"
          data-testid="document-extract-images-toggle"
          type="checkbox"
          class="h-4 w-4 accent-violet-500"
          :checked="documentExtractImages"
          @change="onExtractImagesToggle"
        >
      </div>
```

- [ ] **Step 6: Update the sibling `capture-settings.test.ts` mock.** In `tests/capture-settings.test.ts`, change the `get_documents_config` mock line so it returns a complete config:

```ts
    if (cmd === "get_documents_config") return { documentsFolder: null, documentDateFolders: true, documentExtractImages: true };
```

- [ ] **Step 7: Run the tests to verify they pass.**

Run: `npx vitest run tests/documents-config-tab.test.ts tests/capture-settings.test.ts`
Expected: **all PASS** (both files).

- [ ] **Step 8: Typecheck + lint.**

Run: `npm run build && npm run lint`
Expected: `vue-tsc` typecheck + production build succeed; ESLint clean.

- [ ] **Step 9: Commit.**

```bash
git add src/types.ts src/components/DocumentsConfigTab.vue tests/documents-config-tab.test.ts tests/capture-settings.test.ts
git commit  # subject: feat(ui): add Import images toggle to Documents settings
```

---

### Task 5: Documentation + full verification sweep

**Files:**
- Modify: `AGENTS.md` (the document-import domain section + the `set_documents_config` row of the IPC table)
- Modify: `docs/DEVELOPMENT.md` (per-vault config field reference, if it enumerates the documents fields)

**Interfaces:** none (docs + gates only).

- [ ] **Step 1: Update AGENTS.md — IPC table.** Find the `document_commands.rs` row's `set_documents_config` entry (it reads "now also carries the `document_date_folders` layout toggle"). Extend it to mention the new toggle, e.g. change that clause to: "now also carries the `document_date_folders` layout toggle and the `document_extract_images` images/text-only toggle".

- [ ] **Step 2: Update AGENTS.md — document-import domain section.** In "The document-import domain" section, add a short invariant bullet after the "Flat vs. dated layout" bullet, describing the new per-vault mode. Suggested text:

```markdown
- **Images vs. text-only (`document_extract_images`).** A per-vault toggle
  (default `true` = extract images, today's behavior). When off, the
  conversion swaps `--extract-media` for an app-authored `--lua-filter`
  (`pandoc::STRIP_IMAGES_LUA`, written into the staging dir) that drops
  `Image`/`Figure` nodes, so the note is text-only with no media folder — no
  dangling links. `--sandbox` is unaffected (the filter is app-authored and
  I/O-free). Same parse/serialize/preserve discipline as the date-folder
  toggles; flipping it changes only what NEW imports produce.
```

- [ ] **Step 3: Update docs/DEVELOPMENT.md config reference (if present).** Grep the file for `documentDateFolders`: `grep -n documentDateFolders docs/DEVELOPMENT.md`. If it documents the per-vault config keys, add a `documentExtractImages` row/line mirroring the `documentDateFolders` entry (default `true`; `false` = import text only). If the grep returns nothing, skip this step.

- [ ] **Step 4: Full verification sweep.** Run every gate the change touches:

```bash
# Frontend
npm run lint && npm run build && npm test
# Rust core
cd src-tauri/core && cargo test && cargo clippy --all-targets -- -D warnings && cd ../..
# Rust shell (needs the one-time npm run setup:linux + npm run build already done)
cd src-tauri && cargo fmt --check && cargo clippy -p vault-buddy --all-targets -- -D warnings && cargo test -p vault-buddy --lib && cd ..
```

Expected: all green — ESLint clean, typecheck+build ok, full Vitest suite passes, core + shell Rust tests pass, clippy clean, fmt no diffs.

- [ ] **Step 5: Commit.**

```bash
git add AGENTS.md docs/DEVELOPMENT.md
git commit  # subject: docs: document the images vs text-only import toggle
```

---

## Self-Review

**Spec coverage:**
- Per-vault setting, default images-on → Task 1 (field + default), Global Constraints. ✓
- Text-only drops images via app-authored Lua filter, no `--extract-media` → Task 3. ✓
- `--sandbox` retained, filter I/O-free → Task 3 (both modes keep `--sandbox`; filter constant comment). ✓
- No publish changes (publish already skips absent media) → Task 3 Step 7 note. ✓
- DTO + get/set_documents_config carry it → Task 2. ✓
- `set_capture_config` preserves it → Task 2 Step 1. ✓
- Frontend toggle + `DocumentsConfig` type → Task 4. ✓
- Tests: config round-trip/default (Task 1), `pandoc_args` both modes (Task 3), Vitest toggle load/save (Task 4), capture-save preservation (covered by the core round-trip in Task 1 + the compile-enforced preservation line in Task 2). ✓
- New-imports-only, naming, defensive parse, omit-when-default → Global Constraints, applied in Tasks 1/4. ✓
- Risk: `--sandbox` + `--lua-filter` verification → Task 3 Step 9 (best-effort) + fallback to mechanism B. ✓
- Docs kept current → Task 5. ✓

**Placeholder scan:** No TBD/TODO; every code step shows complete code and exact commands. The one intentionally-conditional step (Task 5 Step 3, DEVELOPMENT.md) is gated on a grep and says to skip if absent. ✓

**Type consistency:** `document_extract_images` (Rust field, DTO field, `set_documents_config` param, `convert_blocking` local) / `documentExtractImages` (JSON key, TS `DocumentsConfig`, Vue ref via `cfg.documentExtractImages`, IPC arg) / `documentExtractImages` (test mock + assertions) — consistent across all tasks. `STRIP_IMAGES_FILTER` / `STRIP_IMAGES_LUA` used identically in `pandoc.rs` (defined) and `document_commands.rs` (imported). `pandoc_args`'s new 4-arg signature is updated at both call sites (the test in Task 3 Step 2, the real one in Task 3 Step 7). ✓
