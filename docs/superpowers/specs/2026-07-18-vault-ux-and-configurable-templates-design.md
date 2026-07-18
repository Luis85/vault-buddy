# Vault UX polish + per-vault configurable templates — design

Date: 2026-07-18
Status: accepted (user request: "when using the right-click menu option to
import a document the vault list should be searchable/filterable. Vaults
should also be favoritable, favorites should be on top of the list of
not-open vaults or on top of open vaults. the organize into year/month
folders options for documents or recordings should be off by default. the
task-id settings should be the second option in the tasks settings. Tasks,
Documents, Recording Notes should also have configurable templates on a
per vault basis where the user can configure frontmatter and body content")

## Problem

Five independent asks, four small UX/defaults changes plus one substantial
feature:

1. **Import picker isn't filterable.** The buddy-menu "Import document…"
   vault-first flow (`ImportVaultPicker.vue`) renders a flat, unfiltered,
   unordered list of every vault. The main vault list already offers a
   name/path filter above 5 vaults; the picker should match.
2. **No vault favorites.** The vault list groups "Open now" then "Other
   vaults", alphabetical within each. A user with many vaults has no way to
   pin the few they use most to the top.
3. **Dated folders default ON.** `recording_date_folders` and
   `document_date_folders` default to `true`, so new recordings/documents
   land under `YYYY/MM`. The user wants a flat folder by default.
4. **Task-ID card is third.** In the per-vault Tasks settings tab the order
   is Tasks-folder → Task-lists → Task-IDs; the user wants Task-IDs second.
5. **Generated files are hardcoded.** The three sanctioned generated-note
   write paths — recording companion Note (`render_note`), Task
   (`render_task`), imported Document (`render_frontmatter` + Pandoc body) —
   emit fixed frontmatter and body as Rust format strings. The user wants
   to configure the frontmatter and body content of each, per vault.

## Design

All rendering logic stays in the pure `core` crate (testable on Linux).
The four small items land first; templates (#5) lands as a phased,
TDD'd sequence sharing one render helper.

### 1. Import picker filter (`ImportVaultPicker.vue`)

Replicate the `ActionPanel.vue` filter pattern locally in the picker (it is
not yet extracted into a shared composable, so duplicating the ~10 lines is
consistent with the current codebase — a shared composable is optional
cleanup, not required):

- A local `filter` ref, `FILTER_THRESHOLD = 5`, and a `filtered` computed
  over the vault list (case-insensitive `name`/`path` substring).
- A `type="search"` input rendered only when the list length exceeds the
  threshold, `aria-label`/placeholder "Filter vaults…", with the same
  `onFilterEscape` (IME-guarded: first Escape clears, second bubbles to
  close — `event.stopPropagation()` only when it clears; GAP-31).
- The `v-for` iterates `filtered` instead of the raw list.
- Reset the filter when the panel is re-shown (watch `store.shownNonce`,
  the existing mechanism) so a stale query can't strand the picker.

The picker renders the same list the main vault list does (see §2), so the
favorites-first ordering applies here for free. Filtering runs on top of
that ordering. The filter input sits above the vault-first mode's header
list only (the converting/checking/blocked/empty gates are unaffected —
they short-circuit `viewState` before the list renders).

### 2. Favorite vaults

**Storage — frontend `localStorage`.** Favorites are pure panel-list
ordering state Rust never needs (recording/MCP/import all address vaults by
id regardless of favorite status), so they follow the `recentSearches.ts` /
`perViewStore.ts` precedent rather than `config.json` (which would force a
`Vault` DTO change and joining `config.json` into the `list_vaults` read
path). New util `src/utils/favoriteVaults.ts`: a defensive JSON array of
vault ids under key `vault-buddy:favorite-vaults`, with `load()`,
`isFavorite(id)`, `toggle(id)`, mirroring `recentSearches.ts`'s
load/push/clear shape and `logWarning`-on-failure posture.

**Store surface.** The `vaults` store gains a reactive `favorites`
(`Set<string>` or `string[]`) hydrated from the util on init, plus a
`toggleFavorite(id)` action that updates the set and persists. It flows to
the list exactly like `taskCounts` does today: store field → `ActionPanel`
→ `VaultList` prop.

**Placement — dedicated top section.** `VaultList.vue`'s `groups` computed
gains a `★ Favorites` group **pinned above** `Open now` and `Other vaults`:

- A vault that is favorited appears **once**, in the Favorites group,
  regardless of its open state (no duplication into "Open now").
- A favorited vault that is currently open shows a small "open" dot on its
  row so the open signal isn't lost.
- Remaining (non-favorite) vaults keep today's exact grouping: "Open now"
  for open ones, "Other vaults" for the rest, alphabetical within each.
- With no favorites, the list is byte-for-byte what it is today (the
  Favorites group renders nothing and the existing flat/"Open now" logic is
  untouched).

**Toggle UI.** A star toggle button is added to the per-row button cluster
in `VaultList.vue` (modeled on the existing open/daily-note/tasks/capture/
settings buttons), `aria-pressed` reflecting favorite state, calling
`store.toggleFavorite(vault.id)`. Clicking it re-sorts the row into/out of
the Favorites group reactively. The star lives on the main list rows only;
the import picker inherits favorites-first ordering but does not show the
toggle (keeps the picker single-purpose).

**Cross-window note.** Favorites surface only in the panel webview, so the
`storage`-event sync used for buddy/panel settings is unnecessary here.
(If a future surface needs it, the existing `useSettingsStorageSync`
mechanism is the path.)

### 3. Date folders off by default (`vault_config.rs`)

Flip both `recording_date_folders` and `document_date_folders` to `false`.
Three coupled spots must change together or the minimal-config round-trip
breaks:

- **Default impl** (`impl Default for VaultCaptureConfig`): `true` → `false`.
- **Parse fallback** (`vault_entry`): `.unwrap_or(true)` → `.unwrap_or(false)`
  for both `recordingDateFolders` and `documentDateFolders`.
- **Serialize omit-when-default** (`serialize_vault_entry`): currently writes
  the key only when `false`; invert to write only when `true` (so a user who
  turns the toggle back ON is persisted, not silently dropped and re-parsed
  as OFF).

Update the one test that asserts the old default,
`date_folder_toggles_default_true_and_round_trip` (rename + invert the two
default assertions and the serialize omit/write expectations). Flip the
cosmetic pre-load seed values in `DocumentsConfigTab.vue`,
`RecordingConfigTab.vue`, `RecordMode.vue` (overwritten on mount, so purely
to avoid a wrong-state flash) and the `documentDateFolders ?? true` mock
default in `tests/documents-config-tab.test.ts`.

**Behavior change, acknowledged:** any vault whose `config.json` doesn't set
these keys explicitly (the common case) will land *new* recordings/documents
flat instead of dated from this release on. Existing files are never moved
(flipping the layout toggle has always been new-writes-only), and both
layouts continue to be found on read/recovery, so nothing is lost or
orphaned — only the default target for new writes changes.

### 4. Task-ID card second (`TasksConfigTab.vue`)

Pure template reorder: move the `<TaskIdSettings>` block to immediately
after `<VaultFolderSetting>` (Tasks folder) and before `<TaskListSettings>`
(Task lists). Preserve the two `v-if`/`v-else` adjacency pairs — insert
after `VaultFolderSetting` (keeps its `v-else` adjacent to the loadError
`v-if`) and before `TaskListSettings` (keeps its `v-if` adjacent to its
pending-folder `v-else`). No `<script>` change; no test asserts card order
(they locate by `data-testid`). Resulting order: Tasks folder → **Task IDs**
→ Task lists.

### 5. Additive per-vault templates (Tasks / Documents / Recording Notes)

**Model — additive, invariant-safe.** Per vault, per type, the user gets
two optional free-text fields: **Extra frontmatter** and **Body template**,
both supporting `{{placeholder}}` substitution. The app **always** emits the
managed identity frontmatter and the structural body; the user's content is
composed around it. Empty fields reproduce today's output byte-for-byte, so
nothing changes until a user opts in.

#### 5a. Shared helper — `core::template`

A new module in the core crate:

- `substitute(template: &str, vars: &[(&str, &str)]) -> String` — replaces
  `{{key}}` tokens (whitespace inside braces tolerated, e.g. `{{ title }}`);
  an unknown token renders as the empty string (the available tokens are
  documented in the UI helper text, so a typo yields visibly-missing text
  rather than a literal `{{typo}}` polluting the vault). Case-sensitive keys.
- `sanitize_extra_frontmatter(text: &str, reserved: &[&str]) -> String` —
  takes the (already-substituted) extra-frontmatter text and returns lines
  safe to inject before the closing `---`: drops blank lines, **rejects any
  line that is a `---` fence** (can't break out of frontmatter), and drops
  any line whose key (the token before the first `:`) is in `reserved`
  (managed keys the app owns). Non-`key: value` lines that aren't list
  continuations are dropped defensively. Returns the surviving lines joined
  with newlines (each newline-terminated), or empty.

Both are unit-tested on Linux and reused by all three renderers.

#### 5b. Composition per type

| Type | Managed frontmatter (always, app-owned) | Structural body (always) | User body template placement | Placeholders |
|---|---|---|---|---|
| **Recording Note** | recorded, duration, paused, vault, type, inputs, event, created-by | `![[audio.mp3]]` at top; `## Transcript` + `![[stem.transcript]]` at bottom when transcribing | the middle section, between the two embeds | `recordedAt`, `duration`, `vault`, `type`, `date` |
| **Task** | type, status, title, created, task-id, due, priority, tags, order | none (tasks have no structural body) | the entire task body (after the closing fence) | `title`, `date`, `due`, `priority` |
| **Document** | type, tags, source, imported, format, created-by | Pandoc-converted content, including its media links | wraps the content via a `{{content}}` token (Pandoc output appended if the token is absent, so content is never lost) | `source`, `format`, `name`, `date`, `content` |

For each renderer, the sequence is: emit managed frontmatter (exact current
order) → inject sanitized extra frontmatter → close fence → emit the body
per the "placement" column with structural pieces guaranteed.

**Note (`render_note`, `capture_note.rs`).** Managed frontmatter unchanged.
Extra frontmatter injected after the managed keys, immediately before the
closing `---`. Body: audio embed (always, top) → **body template** (the
middle section; when empty, fall back to the existing
`follow_up_template`-gated `## Follow-up` scaffold so existing configs are
untouched; when non-empty it *replaces* that scaffold) → transcript embed
(always, bottom, when transcribing). The rename `retarget_embed` still
rewrites only the
`![[…mp3]]` / `.transcript` embed lines, which are app-emitted and therefore
always present in the expected shape; `note_field` still reads the managed
`type:` scalar. A user's body template cannot remove or move the embeds.

**Task (`render_task` / `create_task`, `tasks/disk.rs`).** Managed
frontmatter unchanged (`type: Task`, `status`, `title`, `created`,
task-id, due, priority, tags). Extra frontmatter injected before the
closing `---`, its keys sanitized against the reserved task-key set
(`type`/`status`/`title`/`created`/`due`/`priority`/`tags`/`tag`/`order`
plus the vault's configured task-id property) — the same disjointness the
task-id property validation already enforces, so the surgical field writer
(`set_fields`) is never confused by a user key. Body: the **body template**
(substituted) becomes the task document body (today it is empty). `is_task`
still only requires `type: Task` + a closed fence, and `set_fields`
preserves unknown keys and the body byte-for-byte, so extra frontmatter and
a body survive status toggles and inline edits.

**Document (`render_frontmatter` + `publish_inner`, `document_import.rs`).**
Managed frontmatter unchanged. Extra frontmatter injected before the closing
`---`. Body: the **body template** with `{{content}}` replaced by the Pandoc
GFM output (the current behavior equals the default template `{{content}}`);
if the template omits `{{content}}`, the Pandoc output is appended so it is
never dropped. Because the Pandoc media links live inside `{{content}}`,
they are preserved as long as the content is present. `publish`/
`publish_inner` still writes the assembled string through the never-clobber
atomic writer; the reserved media-sibling folder logic is unchanged.

#### 5c. Config plumbing (`VaultCaptureConfig`)

Six new `Option<String>` fields, blank→None (the `transcription_vocabulary`
pattern — trimmed, empty filtered to `None`):

```
note_extra_frontmatter / note_body_template
task_extra_frontmatter / task_body_template
document_extra_frontmatter / document_body_template
```

- Add to the struct + `Default` (all `None`).
- Add per-field defensive reads to `vault_entry` (camelCase keys:
  `noteExtraFrontmatter`, `noteBodyTemplate`, `taskExtraFrontmatter`,
  `taskBodyTemplate`, `documentExtraFrontmatter`, `documentBodyTemplate`).
- Add omit-when-`None` writes to `serialize_vault_entry`.
- **Add every field to the preserve-lists in `config_merge.rs`** for the
  surfaces that don't own it (`merge_capture_owned` /
  `merge_documents_owned`) — the GAP-60 class: the Note fields are owned by
  the recording surface, the Task fields by the tasks surface, the Document
  fields by the documents surface, so each must be preserved by the *other*
  merges or a save from another tab resets them. Covered by the existing
  merge tests.

Thread the resolved templates into the renderers:

- Note: `SessionParams` gains the note template strings (as `NoteMeta`
  fields), set by `capture_commands.rs` from the vault config, exactly as
  `follow_up` is threaded today.
- Task: `services::add_task` passes the task template strings into
  `create_task` / `render_task` (positional args, alongside the existing
  `task_id` tuple).
- Document: `document_commands.rs` reads the vault's document template from
  config and passes it to `render_frontmatter` / `publish`.

#### 5d. IPC + frontend

Extend the three existing config DTOs and their get/set commands (no new
commands): `CaptureConfigDto` (Note fields), `TasksConfigDto` (Task fields),
`DocumentsConfigDto` (Document fields), each with the blank→None
normalization the setter already does for `transcription_vocabulary`. Mirror
into the TS `CaptureConfig` / tasks / documents config types and the form
models.

UI — a pair of textareas ("Extra frontmatter" + "Body template") per type in
the domain's existing settings tab:

- **Note** → `RecordingSettings.vue`, under the Companion note section
  (shown when `createNote` is on), wired through `RecordingConfigTab.vue`
  (and `RecordMode.vue`, the second capture-config surface).
- **Task** → a new card in `TasksConfigTab.vue` (its own presentational
  component, mirroring `TaskIdSettings.vue`'s prop/emit shape; parent owns
  load + autosave).
- **Document** → `DocumentsConfigTab.vue`.

Each textarea pair carries helper text listing the available placeholders
for that type and a one-line note that identity fields and embeds are always
added automatically. All six are free-text ⇒ their keys are added to each
surface's `TEXT_KEYS` set so edits debounce instead of firing a synchronous
Rust command per keystroke.

## Alternatives considered

- **Full raw single-blob templates with silent re-injection** (user edits
  the whole file; the app re-inserts any missing required field/embed):
  rejected — silent injection surprises, and letting the user's frontmatter
  block define managed keys risks confusing the surgical task field-writer
  and `note_field`. The additive two-box model makes "app-owned vs
  user-owned" explicit.
- **Structured field-editor (no free text):** rejected — the user asked to
  configure frontmatter *and* body content; a fixed-palette form is less
  than that.
- **Favorites in `config.json`:** rejected for the default — it buys
  hand-editability at the cost of a `Vault` DTO change and joining config
  into `list_vaults`, for state Rust never consumes. `localStorage` matches
  the `recentSearches`/`perViewStore` precedent for panel-only ordering.
- **Favorites float within existing groups / only atop "Other vaults":**
  offered; the user chose a dedicated top section (cleanly "on top" of both).
- **One shared template field for all three types:** rejected — the three
  write paths differ structurally (Note has embeds, Task has no body,
  Document wraps Pandoc output), so per-type placeholders and composition
  are clearer than one leaky abstraction.

## Testing

**Rust (core, Linux):**
- `core::template`: `substitute` (known/unknown/whitespaced tokens), and
  `sanitize_extra_frontmatter` (drops fence lines, reserved keys, blanks;
  keeps valid `key: value` and list continuations).
- Each renderer, TDD: empty templates reproduce **byte-identical** current
  output (regression-guarding today's tests); placeholders substitute;
  extra frontmatter injects after the managed block; reserved/managed keys
  are dropped; a `---` in extra frontmatter can't break the fence; embeds
  (Note) / `{{content}}` and media links (Document) are always present; a
  Task with extra frontmatter + body still passes `is_task` and survives a
  `set_fields` status toggle unchanged.
- `vault_config.rs`: the flipped date-folder default test; config
  round-trip + `config_merge` preservation of the six new fields.

**Frontend (Vitest, happy-dom + mockIPC):**
- Import picker: filter shows only above threshold, filters name+path,
  Escape clears then closes, resets on re-show, favorites sort first.
- Favorites: `favoriteVaults` util load/toggle/persist; store hydration +
  `toggleFavorite`; `VaultList` renders the Favorites group above the
  others, once per favorite, with the open dot; empty-favorites list is
  unchanged.
- Templates: each config surface loads the template fields from
  `get_*_config` and saves them via `set_*_config` (debounced through
  `TEXT_KEYS`), including blank→cleared.

**Shell/Rust compile gate:** `npx tauri build --no-bundle` +
`cargo fmt --check`; CI `windows-app` is the behavior gate for the capture
threading changes.

## Out of scope

- Extracting the vault filter into a shared composable (optional cleanup).
- Migrating existing recordings/documents to the new flat default (writes
  have always been new-only; existing files stay put).
- Template variables beyond those listed per type; conditionals/loops in
  templates (plain `{{token}}` substitution only).
- Favorites sync across windows or machines, and a favorites star in the
  import picker (ordering only there).
- Retiring the `follow_up_template` toggle (kept; it governs the Note body
  template's default when the body template is empty).
