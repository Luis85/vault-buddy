# Document Import: per-vault Images vs. Text-only — Design

- **Date:** 2026-07-16
- **Status:** Approved (design only — implementation is a separate step)
- **Source:** User request: "I want to be able to configure the documents
  intake to be able to import also images from the document or just the
  text, this shall be a vault setting." Brainstormed end-to-end before any
  code is written.

## Goal

Let each vault choose whether a document import (Pandoc `.docx`/`.odt`/`.rtf`
→ Markdown) brings the document's **images** along (today's behavior) or
produces a **text-only** note with the images dropped entirely. This is a
per-vault setting, sitting beside the existing per-vault Documents Folder
and "year/month folders" controls.

It is a small, additive increment on the existing document-import domain
(spec: `2026-07-10-document-import-pandoc-design.md`). It invents no new
vault-write path, no new IPC command, and no new config section — it adds
one boolean to the per-vault config and one branch in the Pandoc argument
vector, threaded through the surfaces that already carry
`document_date_folders`.

## The decision that shapes it: what "text only" does to images

Pandoc does **not** silently drop images when `--extract-media` is omitted.
For a binary container (docx/odt), it reads embedded images into its
internal mediabag and, at write time, emits image links pointing at a
`media/…` path. Without `--extract-media` those files are never written to
disk, so the note is left with **broken image links** — the opposite of a
clean "just the text" result.

So "text only" must actively **remove** the images, not merely skip
extraction. Two mechanisms were considered:

- **A — Pandoc Lua filter (chosen).** Pass `--lua-filter=strip-images.lua`
  (a small, app-authored filter that removes `Image` and `Figure` nodes
  from the parsed document) and drop `--extract-media`. This operates on
  Pandoc's AST, so it is correct by construction: no dangling links, and —
  because there is nothing to extract — no media folder. Pandoc ships a
  built-in Lua interpreter (HsLua), so no external Lua runtime is needed,
  and every Pandoc in our supported range (≥ 2.15, already required for
  `--sandbox`) supports `--lua-filter`.
- **B — Strip in Rust (rejected).** Run without `--extract-media`, then
  regex the `![…](…)` image markup out of Pandoc's generated Markdown
  before publishing. No Lua, but it re-implements what the AST already
  models and is fragile on real-world edge cases (brackets inside alt
  text, image attributes `{width=…}`, images inside tables/links). It also
  keeps a broken-links intermediate state that A never produces.

**Decision: A.** It matches the domain's correctness-first posture and
keeps the note clean without post-processing generated text. B is retained
only as a documented fallback (see Risks).

### Security posture is unchanged

`--sandbox` exists to contain the **untrusted document read** — a crafted
`.docx`/`.odt`/`.rtf` must not be able to exfiltrate local files or fetch
remote resources while Pandoc resolves media. That protection is provided
by the sandboxed *reader* and is unaffected here: the source document is
still read under `--sandbox`, and the Lua filter we add is **app-authored,
static, and performs no I/O** — it only deletes nodes from the AST. Pandoc's
sandbox does not restrict (nor is it bypassed by) a Lua filter's node
manipulation, and loading a command-line-specified filter file is not a
document-driven I/O operation. No untrusted input gains any new capability.

### The strip filter

A static constant, written into the per-import staging dir just before
Pandoc runs (only in text-only mode) and cleaned up with the rest of the
staging dir. Removing an inline `Image` or a block-level `Figure` is enough
to cover both older Pandoc (implicit figures are a `Para` holding a single
`Image`) and Pandoc 3.x (explicit `Figure` blocks); defining a handler for
an element a given Pandoc version doesn't produce is harmless (it never
matches).

```lua
-- Vault Buddy: "text only" document import — drop all images so the
-- imported note carries only text (no image links, no media folder).
-- App-authored and I/O-free: it manipulates the AST only, so it does not
-- affect --sandbox's protection of the untrusted document read.
function Image() return {} end
function Figure() return {} end
```

Returning an empty list removes the element. An image that stood alone in a
paragraph leaves an empty paragraph, which the GFM writer renders away; we
deliberately do **not** try to prune surrounding structure — minimal and
predictable beats clever.

## What changes, layer by layer

Everything below mirrors an existing field. The reference implementation to
copy in each file is `document_date_folders` (the per-vault dated-vs-flat
layout toggle) — same defaulting, same defensive parse, same
omit-when-default serialization, same preservation discipline.

### 1. Config (`src-tauri/core/src/vault_config.rs`)

- Add `document_extract_images: bool` to `VaultCaptureConfig`, **default
  `true`** (an absent setting keeps today's behavior — images imported).
- `vault_entry`: parse key `documentExtractImages`, `unwrap_or(true)` — one
  malformed value defaults only itself, like every other field.
- `serialize_vault_entry`: write `"documentExtractImages": false` **only
  when `false`**; omit when `true` so the hand-editable `config.json` stays
  minimal (the `document_date_folders` rule exactly).
- Extend the round-trip and default tests to cover the new field.

### 2. Pandoc arguments (`src-tauri/src/pandoc.rs`)

- `pandoc_args` gains an `extract_images: bool` parameter:
  - `true` → `--extract-media=<media_name>` (unchanged).
  - `false` → `--lua-filter=<STRIP_IMAGES_FILTER>` and **no**
    `--extract-media`.
  - Everything else (`-f`/`-t gfm`/`--sandbox`/`-o`/RTS heap cap) is
    unchanged and present in both modes.
- Add the strip-filter constant and its filename constant next to
  `pandoc_args`.

### 3. Conversion (`src-tauri/src/document_commands.rs`)

- `convert_blocking` reads `cfg.document_extract_images` and:
  - when `false`, writes the strip filter into `plan.work_dir` (relative
    name, cwd = work_dir, like every other Pandoc output) before spawning
    Pandoc;
  - passes the flag into `pandoc_args`.
- **No publish changes.** `publish` already writes only the note when the
  staging media dir is empty/absent, which is exactly the text-only case —
  so the media-folder path simply isn't taken. `reserve_basename` still
  reserves both names up front; reserving an unused media name is harmless
  and keeps one code path.
- `DocumentsConfigDto` gains `document_extract_images: bool`;
  `get_documents_config` reads it; `set_documents_config` accepts and
  persists it (read-modify-write already preserves the rest).

### 4. Capture-settings preservation (`src-tauri/src/capture_config_commands.rs`)

`set_capture_config` reconstructs the whole `VaultCaptureConfig` from the
capture DTO (which does **not** carry documents fields) plus values read
back under the lock. It must add
`document_extract_images: existing.document_extract_images`, exactly as it
already preserves `document_date_folders`, `documents_folder`,
`tasks_folder`, and the lists settings — otherwise saving recording
settings would silently reset a vault to images-on.

### 5. Frontend (`src/components/DocumentsConfigTab.vue`, `src/types.ts`)

- `DocumentsConfig` gains `documentExtractImages: boolean`.
- A third control under the Documents tab, structurally identical to the
  existing "Organize into year/month folders" toggle: label **"Import
  images"**, subtext *"Off = text only (no images, no media folder)"*,
  default on, wired to the same `set_documents_config` autosave call (the
  toggle saves immediately; nothing new about the save path).

## Naming

- Rust: `document_extract_images` · JSON/DTO/TS: `documentExtractImages`.
- UI label: "Import images" / subtext "Off = text only (no images, no media
  folder)".

Chosen to read naturally with the default-true state (on = import images)
and to sit alongside `documentDateFolders`. CONTEXT.md needs no new term —
this is a mode of the existing Document Import capability, not a new
concept.

## Behavioral guarantees

- **Default preserves today's behavior.** A vault with no setting, and every
  existing vault, imports images exactly as before.
- **New imports only.** The toggle changes what the *next* import produces;
  it never rewrites or re-converts already-imported notes (the
  `document_date_folders` precedent).
- **Text-only leaves nothing dangling.** No broken image links and no media
  folder — the note is text.
- **Never-clobber and containment are untouched.** This adds a Pandoc
  argument branch and a config bool; every path-safety, staging, publish,
  and recovery invariant of the document-import domain is unchanged.

## Testing (implementation phase, TDD — failing test first)

- **`vault_config.rs` (core):** `document_extract_images` defaults to
  `true`; parses `false`; omitted when `true` and written when `false`;
  survives a serialize→parse round trip. Fold into the existing
  `date_folder_toggles_default_true_and_round_trip` shape or add a sibling.
- **`pandoc.rs` (shell):** `pandoc_args(..., true)` contains
  `--extract-media=<media>` and no `--lua-filter`; `pandoc_args(..., false)`
  contains `--lua-filter=<name>` and **no** `--extract-media`; both keep
  `--sandbox`, `-t gfm`, and the RTS heap cap. A tiny assertion that the
  strip-filter constant references `Image`.
- **`documents-config-tab.test.ts` (Vitest):** the new toggle loads its
  value from `get_documents_config` and a change persists via
  `set_documents_config` carrying `documentExtractImages`.
- **Capture-save preservation:** a capture-settings save leaves
  `document_extract_images` intact (mirror the existing preservation
  coverage for the sibling documents/tasks fields).

## Risks / to verify during implementation

- **`--sandbox` + `--lua-filter` interaction.** This is the one assumption
  not testable in the design environment (no Pandoc present). Pandoc's
  sandbox restricts reader/writer I/O, not the loading or execution of a
  command-line Lua filter, so `--sandbox --lua-filter=strip-images.lua` is
  expected to strip images and still refuse untrusted-document I/O.
  Implementation must confirm this against a real Pandoc (a text-only
  conversion of a fixture doc with an embedded image → a note with no image
  and no media folder). If any supported Pandoc build rejects the
  combination, fall back to mechanism **B** (Rust-side image-markup strip)
  behind the same setting — the config, DTO, command, and UI surfaces are
  identical either way, so only the strip mechanism would change.

## Non-goals (this increment)

- Any third mode (e.g. "link images without copying", "images only"). The
  request is binary; a boolean is the right size.
- Preserving alt text/captions where an image had them (explicitly decided
  against: "drop images completely").
- Re-converting or migrating already-imported notes when the toggle flips.
- App-global (rather than per-vault) scope — the request is per-vault, and
  the setting belongs where its vault context already lives.
