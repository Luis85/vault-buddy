# Tutorial Editor — P00 Integration Decision Record

**Status:** Accepted as the P00 deliverable (solution architect). Binds P01–P13.
**Date:** 2026-09-21.
**Inspected tree:** branch `claude/vault-buddy-improvement-polish-95f5bc` (= PR #79 head: screen-capture phases 1–6, the region indicator, the 2026-09-21 polish pass).
**Concept bundle:** `docs/concepts/vault-buddy-editor/` — `START-HERE.md`, `docs/PRODUCT-SPEC.md`, `docs/FEATURE-CATALOG.md` + `contracts/feature-catalog.json` (50 F-IDs), `docs/ARCHITECTURE-AND-STACK.md`, `docs/DATA-MODEL.md`, `docs/IPC-CONTRACTS.md`, `docs/NATIVE-MEDIA.md`, `docs/PERSISTENCE-AND-SECURITY.md`, `docs/SCREENS-AND-INTERACTIONS.md`, `docs/ONBOARDING.md` + `contracts/onboarding.{steps,chapters}.json`, `docs/IMPLEMENTATION-PLAN.md` (P00–P13), `docs/ACCEPTANCE-SCENARIOS.md` (A01–A30), `contracts/workspace.schema.json`, `contracts/reference-workspace.example.json`, `implementation-starter/`.
**Plan:** `docs/superpowers/plans/2026-09-21-tutorial-editor.md`.

The concept bundle was written against commit `3f330a9` (2026-09-19). Several of its "the repo has no X" statements are already stale — the editor window, the asset-protocol scope and a tested ffmpeg export all exist now. This record reconciles the bundle with the tree **as it is**, decides every conflict explicitly, and maps every F-ID to an owning module. Where this record departs from the bundle it says so, names the affected F-IDs and states the replacement acceptance evidence, as `PRODUCT-SPEC.md` § Acceptance authority requires.

---

## 1. Inventory of the current code (what exists, what is reused)

| Area | Where it lives today | Facts that matter to the editor | Decision |
|---|---|---|---|
| Editor window | `src-tauri/tauri.conf.json` window `editor` (960×640, min 720×480, decorated, resizable, not on top, `skipTaskbar:false`); `tray::ALL_WINDOW_LABELS` has it, `COMPANION_LABELS` and `POSITION_DENYLIST` treat it specially; `capture_exclusion::EXCLUDED_LABELS` excludes it (GAP-166) | Hidden-and-reused, never destroyed until `finish_quit`; its webview mounts ONCE per process; `window_close.rs` turns its X into `prevent_close()+hide()` | **Reuse the window label `editor`.** Resize defaults to 1280×820, min 960×640 (the bundle's compact target). No new window. |
| Opening the editor | `editor_commands.rs`: `open_capture_editor(base)` (sync, stashes base, `emit_to("editor","editor:open")` BEFORE `show()`), `take_editor_request()` (one-shot drain), `is_safe_base` | The stash/drain split exists because a sync command must not touch disk; `editor:open` exists because the webview mounts once | **Keep both unchanged.** The editor's *load* step changes from `load_staged_capture` to the new `editor_open_staged`. |
| Staged capture | `screen/src/staging.rs`: `StagedSidecar {base,vault_id,source_title,source_kind,inputs,duration_ms,paused_ms,width,height,recorded_at,timeline:Option<Value>, #[flatten] extra}`, `write_sidecar` (atomic replacing, containment-checked), `read_sidecar` (degrading) | The sidecar — not `screen:stopped` — carries `vault_id`; `extra` round-trips unknown keys, so a new key can be added without breaking older builds | **Reuse.** The sidecar is the destination authority for F-01. A new `editorProjectId` key is written through `extra` (the pin, R6). |
| Sequential edit algebra | `core/src/timeline.rs` (`Segment`, `Timeline::{whole,split_at,delete,reorder,output_duration_ms,to_source_ms,is_untouched}`), `screen/src/select.rs` (`plan`, `PlanSpan`), shared fixture `tests/fixtures/timeline-cases.json` read by Rust (`include_str!`) and `tests/timelineFixtures.test.ts` | Size-guarded in both languages (GAP-136); it is a single-source sequential model | **Keep as the migration input and the remux fast-path oracle.** It is NOT extended into the multi-track model (the catalog's compatibility obligation says so explicitly). |
| Phase-4 editor frontend | `src/roots/EditorRoot.vue` (386), `useEditorTimeline.ts`, `useEditorSelection.ts`, `useEditorExport.ts`, `components/editor/{CapturePreview,TimelineStrip,ExportBar}.vue`, `utils/timelineGeometry.ts` | Local TS algebra persisted per edit via `save_capture_timeline`; installs no store | **Evolve the root, replace the internals** (R4). `EditorRoot.vue` stays the root for label `editor`; its body becomes the new shell. The phase-4 composables/components are retired in Task 59 after every capability they carried is covered. |
| Export (the ninth vault write) | `export_commands.rs` (`export_and_save_capture`, `cancel_export`, `ExportState`, 5 events), `export_worker/{mod.rs,vault_dir.rs}` (prepare → ffmpeg → `commit_into_vault` → delete staged LAST; `prepare_export_dir`/`rollback_export_dir`), `screen/src/{ffmpeg_args,export,disk}.rs`, `ffmpeg.rs`/`external_tool.rs` (resolve, probe, `CREATE_NO_WINDOW`), `core::screen_capture_paths` (pairwise `.mp4`+`.md` reservation, `rename_noreplace` + ` (N)`), `core::screen_note` | User-installed ffmpeg, never bundled; pure argv builders; CI installs ffmpeg and `screen/tests/export_roundtrip.rs` does a real round trip; `export::run` owns spawn/progress/cancel/kill/delete-truncated-output | **Extend this route for rendering** (R1). Reuse `external_tool`, `ffmpeg::resolve_working_ffmpeg`/`probe_source`, `export.rs`'s runner, `disk::free_bytes`, `vault_dir`'s create/rollback, `screen_capture_paths`' reservation rails. `export_and_save_capture` itself is retired in Task 59, replaced by Render + Publish. |
| Asset protocol | `tauri.conf.json` `assetProtocol.scope = ["$APPLOCALDATA/screen-captures/*"]`, CSP `media-src 'self' asset: http://asset.localhost`, pinned EXACTLY by `tray.rs`'s `the_asset_protocol_scope_is_pinned_to_the_staging_directory_alone` | A security boundary; widening it is a reviewed change | **Widen deliberately, enumerated, never wildcarded** (R7), and re-pin the test to the new exact array. |
| Capabilities | `capabilities/default.json` — one capability for all six windows; custom commands are not capability-gated | Any window can invoke any `editor_*` command today | **Two layers** (R8): app-manifest scoping of `editor_*` commands to an `editor` capability, plus a native caller-label check in every command. |
| Capture engine | `screen/src/session/{mod,audio,mux,pacing,windows_session}.rs`: 3 threads, one `CaptureClock`; `audio.rs` mixes N inputs to ONE stereo track via `vault_buddy_capture::mixer` | The staged `.mp4` has exactly one mixed audio stream — per-input stems cannot be recovered from it | Stems and synchronized webcam are **capture-session extensions** (R10, R11), not editor features. |
| Cross-domain guard | `capture_guard.rs` `CaptureGuard` (one claim, keyed release), `shutdown_gate.rs` (three predicates, read by quit, Alt+F4, the updater) | Exactly one `release(CaptureKind::Screen)` site, pinned structurally | Webcam takes do NOT claim the guard (R10); render jobs join the shutdown gate as a fourth predicate replacing export's (R12). |
| Recovery | `screen_recovery/{mod,decide}.rs` sweeps staging on startup; classifies anything unknown as Foreign | New staging file names would be Foreign (never touched, never cleaned) unless classified | Stem/webcam parts get classified in the same task that creates them (Tasks 51, 53). |
| Stores / roots | `src/roots/index.ts` `rootFor(label)`; each window its own Pinia; `EditorRoot` installs NO store today | AGENTS.md "Frontend state" documents the three store-less roots | The editor root **installs four stores** (`editorProject`, `editorWorkspace`, `editorJobs`, `editorOnboarding`). AGENTS.md's paragraph is rewritten in Task 60. |
| UI primitives | `src/components/ui/*` + `@theme` tokens in `src/style.css` (`fg`, `accent`, `focus`, `danger`…) | New UI must consume them (GAP-66) | Editor tokens are added to the same `@theme` layer (Task 16), mapped from `contracts/design-tokens.json`. |
| Quality gates | LOC caps 800 Rust / 500 frontend (`scripts/loc-baseline.json`, shrink-only); fallow ratchet (`scripts/quality-baseline.json`: deadCode 0, complexFunctions 13, criticalComplexity 3, MI 91.6); llvm-cov ≥ 94 on core/capture/transcribe/screen; frontend coverage floors in `vite.config.ts`; Playwright e2e against built `dist/` | No new file may enter the allowlist; counters never loosen | Every module in this design is sized under its cap from the start; splits are pre-planned (§4). |
| Dependencies | core: serde, serde_json, serde_yaml_ng, chrono, getrandom…; `sha2 0.10` already in the lockfile via transcribe; `tauri-plugin-dialog` already a dependency (`dialog:allow-open`) | — | **One new crate:** `zip` (R9). `sha2` is added to core (already locked). `dialog:allow-save` is added to the editor capability. |

**Verified absences (the bundle's assumptions that are still true):** no multi-track model, no project persistence, no render of layers/effects/captions, no webcam, no onboarding, no Tauri `Channel` usage anywhere in the shell, no app-manifest command scoping, no crypto digest in core.

**Stale bundle claims, corrected here:** "no final editor root/window API" (the `editor` window, `EditorRoot` and `open_capture_editor` exist); "no asset/media scope" (the staging scope exists and is test-pinned); "do not add FFmpeg" (a user-installed ffmpeg route exists, deliberately, since commit `1b458dd`); "the sidecar uses direct `std::fs::write`" (it has used `write_atomic_replacing` since the phase-4 fix wave).

---

## 2. Rulings

Each ruling states the decision, why, and what it costs. Numbers are cited by the plan.

### R1 — Rendering extends the existing user-installed-ffmpeg route. No Media Foundation compositor, no bundled encoder.

The bundle says "do not add FFmpeg as an incidental dependency". It is not incidental here: the repo already chose a **user-installed, detected, never-bundled** ffmpeg for export, for licensing (a ~100 MB copyleft binary in an MIT light installer) and above all **testability** — the export's correctness is a pure function from a plan to an argv vector, and CI runs a real round trip. A Media Foundation compositor with overlays, masks, text, crossfades and a mixer would execute in no automated test on any platform (GAP-117's situation, multiplied).

So: `core::editor::render_plan` (pure) freezes an acknowledged revision + range into a `RenderPlan`; `screen::render::{video_graph, audio_graph, ass, args}` (pure) turn it into ONE `filter_complex` invocation; `screen::render::run` reuses `export.rs`'s spawn/`-progress`/cancel/kill/delete-truncated machinery. Teaching cues, captions and title-card text are rendered through a generated **ASS subtitle document** (`ass` filter, libass) because ASS natively carries timed, positioned text, vector drawings (arrows, outlines, step badges, opaque covers), per-event fades and styles — all of which are pure string generation, golden-testable on Linux. Zoom is a time-expression `crop`+`scale` on the composed canvas; frame shapes are a `geq` alpha mask; transitions are `xfade`/`acrossfade` (`c1=qsin:c2=qsin` for equal power); speed is `setpts` + an `atempo` chain (pitch preserved) or `asetrate`+`aresample` (not preserved).

The identity case keeps the lossless fast path: `RenderPlan::is_identity()` (one full-range clip of one source, canvas = source geometry, no effects/fades/captions/overlays, unity gain) remuxes with `export::remux_args`. This is the successor of `Timeline::is_untouched` and is keyed on the same principle — never on "the timeline field is absent".

Costs, stated plainly: ffmpeg becomes required for **render, import probing, waveforms and thumbnails**; editing a staged capture and saving the project do not need it (the staged capture's duration/geometry come from the sidecar). A capability probe (`ffmpeg -filters`, `-encoders`) refuses a render that needs a missing filter (`ass`, `xfade`, `acrossfade`, `atempo`, `geq`) with `encoderUnavailable` naming it, before any work starts. Font availability for libass on Windows ffmpeg builds is a residual hardware gate (§7).

Departure recorded: `NATIVE-MEDIA.md` § Scope. Affected F-IDs: F-04, F-05, F-16–F-19, F-21, F-23, F-27–F-33, F-35, F-37–F-39, F-41, F-42. Replacement evidence: `screen/tests/render_roundtrip.rs` decodes real output in CI (frame colour samples at named timestamps, audio RMS per window, stream duration), plus golden argv/ASS tests.

### R2 — The multi-track model is a NEW pure aggregate in `core::editor`; `core::timeline` is preserved, not extended.

`vault_buddy_core::editor` owns the project graph, semantic validation, time mapping, commands, history, migrations, checks, captions parsing, the render plan and the companion note. It has no Tauri types and is tested on Linux. Rust is authoritative for committed edits (`ARCHITECTURE-AND-STACK.md` § Command consistency). `core::timeline` + `screen::select` stay as they are; `editor::migrate` consumes them, and the identity fast path is cross-checked against `Timeline::is_untouched` in a test.

### R3 — The persisted document keeps the interchange spelling verbatim; the IPC envelope is camelCase.

`vault-buddy-workspace/1` wrapping `vault-buddy-video-project/3` (snake_case fields, including the reference's `fontSize` exception on effects) is serialized byte-compatibly, so a browser-reference project opens natively and vice versa. Every entity carries `#[serde(flatten)] extra: Map` — unknown fields **round-trip**, never silently drop (`DATA-MODEL.md` § Validation order). Unknown enum *variants* reject. IPC envelopes (`EditorSnapshot`, `ExecuteRequest`, receipts, job progress) are `rename_all = "camelCase"` like every other DTO in the repo; the `project` graph travels inside them in its document spelling. One literal-JSON test per envelope pins the wire (the GAP-135 pattern: a literal, never a re-serialization of the struct against itself).

`destination.vault` holds the **Obsidian vault ID** natively (the registry hex key — the repo addresses vaults by ID, never name). A reference document whose `destination.vault` is a display name is mapped on import: exactly one vault with that name → its ID; zero or several → destination left unset and surfaced as a check (never guessed).

### R4 — Evolve `EditorRoot.vue`; replace its internals; retire the phase-4 algebra only when nothing needs it.

The window, the root file, the stash/drain and `editor:open` survive. The body becomes the new shell. The phase-4 TS algebra (`timelineGeometry.ts`, `useEditorTimeline`, `useEditorSelection`) and `TimelineStrip`/`ExportBar` are deleted in Task 59, after Tasks 1–58 cover every capability they carry. **GAP-136's parity rule is not dropped, it moves:** the TypeScript side no longer implements edit *operations* (Rust is authoritative), but it does implement *read-side time mapping* for the preview, the drag preview and the ruler. That mapping is held to Rust by a new shared fixture, `tests/fixtures/editor-time-cases.json`, read by `core/src/editor/time.rs` (`include_str!`) and `tests/editorTimeFixtures.test.ts`, size-guarded in both languages exactly as `timeline-cases.json` is. When the TS twin of the sequential algebra is deleted, the TS half of `timeline-cases.json` is deleted with it and the Rust half stays (it guards migration and `is_untouched`).

### R5 — Project store lives outside every vault; "Save project" is a durable store commit; portable files are exports.

```
%LOCALAPPDATA%\com.vaultbuddy.desktop\
  screen-captures\                       existing staging (unchanged owner)
  editor-projects\<projectId>\
    project.json                         committed workspace envelope (last good), write_atomic_replacing
    sources.json                         native source registry: assetId -> locator + identity
    products.json                        immutable product ledger (committed at render completion)
    recovery.json                        uncommitted-edit journal (latest acknowledged revision)
    media\<assetId>.<ext>                imported originals (copied in, never modified)
    takes\<takeId>.webm | .<takeId>.webm.part   webcam takes
    products\<productId>.mp4             rendered products (immutable)
    cache\<assetId>.peaks.json, <assetId>-<ms>.jpg   derived, deletable
    jobs\<jobId>\out.mp4.part, cues.ass  job-owned temporaries
  editor-prefs\guide-progress.json       onboarding progress (app preference, not project)
```

- `projectId` = 16 base36 chars from the OS CSPRNG (the `tasks::new_task_id` generator, widened), matching `^[a-zA-Z0-9_-]{1,100}$`.
- **Save project** (`editor_save_project`) freezes revision `r`, writes `project.json` via `capture_note::write_atomic_replacing` (temp → fsync → replacing rename), then returns `SaveReceipt {sessionId, savedRevision:r, projectFileId}`. `projectFileId` is the `projectId` (opaque to the UI). If the session has advanced to `r+1`, it stays dirty. A failure leaves the previous `project.json` intact — this is the "last good project" A20 requires.
- **Recovery journal**: after every acknowledged command the shell rewrites `recovery.json` (same atomic writer, debounced to at most one write per 500 ms, flushed on close). On startup `editor_list_projects` reports `hasRecovery`; Resume opens with the journal, Discard deletes only `recovery.json`.
- **Portable package** (`.vbproject.zip`, `vault-buddy-project-package/1`) and **lightweight** (`.vbproject.json`) are *exports* to a user-chosen path through the native save dialog, written as an owned same-directory temp then `rename_noreplace` (a first write, never a replace). Opening either **imports into the store as a new project** (fresh `projectId` if the id already exists), validated completely before anything is installed.
- **Products ledger** is committed independently of the edit (`products.json`, atomic replacing) the moment a render publishes into `products\`. Departure from the bundle's "save the project afterward to retain lineage": a rendered file whose record depends on a later manual save is an orphan-in-waiting; committing the record with the product is strictly safer. On save/export, `record.products` is assembled from the ledger.

### R6 — A staged capture is ADOPTED by reference and PINNED, not moved.

`editor_open_staged(stagedBase)` validates the base (`is_safe_base`), reads the sidecar natively, resolves `vault_id` from it (F-01/A01), and:
1. If the sidecar's `extra.editorProjectId` names an existing store project → reopen it (idempotent; A duplicated `editor:open` or a second click never creates a second project).
2. Otherwise mint a project, register the staged `.mp4` in `sources.json` as `{"store":"staging","base":"<base>"}`, migrate the sidecar (+ its legacy `timeline`) into a v3 project, commit `project.json`, and only then write `editorProjectId` into the sidecar (`write_sidecar`, atomic). A crash between the two leaves an unpinned staged capture and an orphan project the recovery sweep adopts back (it finds a project whose staging source's sidecar lacks the pin and re-pins it).

Staging becomes project-aware in exactly three places: `staged_commands::discard_conflict` refuses a pinned capture ("used by tutorial project …; discard the project first"), `staging_commands::clear_staged_captures` skips pinned captures and reports them, and `StagedCaptureList` labels the row. The staging recovery sweep is untouched (it only ever acts on `.part` files). Discarding the *project* (`editor_close_session` with `discardProject`) removes the project directory (owned files only, no-follow) and clears the pin — the staged capture returns to being an ordinary staged capture, which the user may then Discard through the existing confirm-gated path. **Nothing a user recorded is ever deleted as a side effect of discarding an edit.**

Why not move: moving the `.mp4` out of staging would require a second recovery protocol for half-moved captures, break `StagedCaptureList`'s visibility, and gain nothing — both directories are app-owned and on the same volume.

### R7 — The asset-protocol scope is widened to an enumerated list; the pin test is re-pointed, not deleted.

New exact scope:
```json
["$APPLOCALDATA/screen-captures/*",
 "$APPLOCALDATA/editor-projects/*/media/*",
 "$APPLOCALDATA/editor-projects/*/takes/*",
 "$APPLOCALDATA/editor-projects/*/products/*",
 "$APPLOCALDATA/editor-projects/*/cache/*"]
```
CSP becomes `img-src 'self' data: asset: http://asset.localhost; media-src 'self' asset: http://asset.localhost blob: mediastream:`. Nothing under `jobs\`, no `project.json`, no vault path, no `$APPLOCALDATA/*`. The frontend never builds a path: `editor_media_url(sessionId, assetId | productId)` returns the absolute path for a *registered* entity only, and the UI passes it to `convertFileSrc`. `convertFileSrc` is URL conversion, not authorization (bundle § Access control) — the scope is. Imported originals are **copied into `media\`**; a scope entry for an arbitrary user folder is never added.

### R8 — Command authorization is two layers: app-manifest scoping plus a native caller check.

1. `src-tauri/build.rs` declares every `editor_*` command in `tauri_build::Attributes::new().app_manifest(AppManifest::new().commands(&[…]))`; a new `capabilities/editor.json` (windows: `["editor"]`) grants exactly those `allow-editor-*` permissions plus `dialog:allow-open`/`dialog:allow-save`. `default.json` does not list them.
2. Every `editor_*` command takes `window: tauri::WebviewWindow` and returns `unauthorizedSource` unless `window.label() == "editor"` (`editor::authz::require_editor_window`), and validates `sessionId` ownership against the session registry. A structural test scans the editor command files and fails if a `#[tauri::command]` lacks the call.

Layer 1 is Tauri-version-dependent and verified by building (`npx tauri build --no-bundle`); layer 2 is ours and unit-tested. Neither replaces the other.

### R9 — One new Rust dependency: `zip` (store + deflate), for the portable package.

The bundle's portable format is a ZIP. `zip` (MIT) with `default-features = false, features = ["deflate"]` goes in `core` (the package reader/validator is pure). Writing uses `Stored` for media (already compressed; avoids ratio attacks on our own output) and `Deflated` for JSON. Reading enforces, *before* extraction: entry count ≤ 1,000, declared total ≤ 220 MiB, per-entry ratio ≤ 100:1, no absolute/drive/UNC/`..` paths, no duplicate case-folded names, no symlink entries, manifest present and first. `cargo deny check` gates its licence and advisories. This is a dependency decision the brief lists as an approval gate: it is recorded here as the architect's ruling and flagged in the plan's Task 38 for the user's sign-off before merge.

Native package limits equal the reference limits (JSON 8 MiB, media 200 MiB, package 220 MiB). Export of a larger project refuses the portable format with a message offering the lightweight file; raising the limit is a product decision (residual R-P1, §7), not an implementation one.

### R10 — Webcam takes (F-20) are recorded in the webview; native synchronized capture (F-22) is a screen-session extension.

- **Takes (F-20, F-21):** `getUserMedia` + `MediaRecorder` in the editor webview (WebView2 renders its own permission prompt; nothing is requested until the user presses *Enable camera*). Chunks (≤ 1 MiB, 1 s timeslice) go to `editor_webcam_append` as a **raw binary invoke body** (`tauri::ipc::Request`, headers `x-editor-session`, `x-editor-take`, `x-editor-seq`), appended to `takes\.<takeId>.webm.part`; `editor_webcam_finish` validates the sequence, remuxes `-c copy` into `takes\<takeId>.webm` (MediaRecorder WebM lacks a duration/cue index), probes it, and registers the asset through the session's serial queue. No frames or samples cross JSON or Pinia. A take does **not** claim `CaptureGuard` (it holds no native device). *(Amended by Task 60, §9 (a):)* `editor_webcam_begin` also refuses NATIVELY while `CaptureGuard::active()` is `Some` (`deviceUnavailable`, "Stop the screen recording first."; Task 49) — stronger than the UI-only rule this ruling first stated. The two directions are intentionally not the same scope: nothing refuses a capture that starts while a take is recording, which is GAP-N1 = docs/Gaps.md GAP-193 (§7).
- **Synchronized screen + webcam (F-22):** a fourth producer in `screen::session` (`session/webcam.rs`, `cfg(windows)`, Media Foundation `IMFSourceReader` on a video-capture device) stamping from the SAME `CaptureClock`, writing `.<base>.webcam.mp4.part` through a second `sink` instance (video-only). Pure parts (device descriptor, file naming, sidecar `webcam` block, offset metadata) are Linux-tested; the producer executes in no automated test (GAP-117's class), so its acceptance is the Windows checklist (drift over 10 min, unplug mid-capture). Migration places the webcam file on its own video track at its MEASURED offset (the sidecar's `offsetMs`, the capture-clock time of the webcam's first frame — not 0; §4 and §9 (b)) — "synchronized" is claimed only for this path, never for a later take (`SCREENS-AND-INTERACTIONS.md` 05).

No native WebView2 `PermissionRequested` handler is installed: doing so needs a direct `webview2-com` dependency version-locked to wry's (the `windows` 0.61/0.62 trap AGENTS.md documents). The consequence — camera permission is WebView2's default prompt, not restricted to the `editor` window by us — is recorded as GAP-N2.

### R11 — Separate audio stems are delivered as an opt-in capture extension, honestly scoped.

`screen/src/session/audio.rs` mixes N inputs to one stereo track; stems cannot be recovered after the fact. Scope decision: a per-vault `screenAudioStems: bool` (default false; Screen settings tab) makes `session/audio.rs` additionally write each input's mono stream at 48 kHz into `.<base>.stem-<n>.m4a.part` via an audio-only `sink` instance, stamped from the same clock; the mixed track in the main `.mp4` is unchanged (so every existing consumer, remux and recovery rule is unaffected). The sidecar gains `stems: [{index, input, file}]`; migration creates one audio track per stem with the main clip's embedded audio muted. Like F-22, the writer is `cfg(windows)` and hardware-gated. Without the setting, the editor still offers F-24 (detach the mixed track) and says in the Audio inspector that separate stems require the setting for NEW recordings.

### R12 — Render jobs replace export in the shutdown gate; quit cancels a render the way it cancels an export.

`shutdown_gate::shutdown_is_blocked` gains `render_jobs::blocks_shutdown()` (true while any job is in `rendering` or `publishing` — *widened by Task 46 to every not-yet-ended RENDER job, §9 (f)*) and loses `export_shutdown` in Task 59. *(Amended by Task 60, §9 (f):)* publish has its OWN term, `publish::blocks_shutdown()`, and its own bounded cancel; the gate is recording + screen capture + render + publish. The quit workers call `render_jobs::cancel_all_bounded(5 s)` first, exactly as they call `export_shutdown::cancel_if_exporting` today. `tray::hide_buddy` does NOT gate on renders (the buddy is the recording indicator). Unsaved edits never block quit: the recovery journal is the durability mechanism, and the next launch offers Resume.

### R13 — Publication is the TENTH sanctioned vault write, on the ninth's rails.

`editor_publish_product` copies an immutable product into the vault with a companion note. It reuses `export_worker::vault_dir::{prepare_export_dir, rollback_export_dir}` (moved to `src-tauri/src/editor/vault_dir.rs` by Task 59, when the rest of `export_worker/` was retired), `core::screen_capture_paths` pairwise reservation and `rename_noreplace` with ` (N)` retry, **video first, note second**, a note failure degrading to a warning. It differs from the ninth in two ways, both deliberate: it **copies** (`move_or_copy_noreplace` from a same-directory temp, then `rename_noreplace`) because the product must remain playable in the project, and it deletes nothing afterwards. Its journal (`jobs\<jobId>\publish.json`: reserved names, step reached) lets startup recovery report a partial publication instead of guessing. The companion note (`core::editor::note`) embeds the reserved final file name (A21) and lists chapters at output time. AGENTS.md's vault-domain list grows to ten in Task 48.

### R14 — Commands are a serial per-session queue; history is snapshot-based and bounded.

`core::editor::session::EditorSession { project, revision, persisted_revision, history: History, recent_commands: VecDeque<(commandId, revision)> }`. *(Amended by Task 60, §9 (c):)* `InternalCommand` (`AddAssets`, `ImportCaptions`, `RestoreSnapshot`, `RelinkAssets`) deliberately has NO `commandId` field — it is never deserialized from IPC; an internal request that needs replay/undo bookkeeping (`editor_restore_product`) threads its `commandId` beside it (`EditorSession::execute_internal_as`). `execute(req)`: wrong session → `sessionGone`; `expected_revision != revision` → `revisionConflict` (no state change); a replayed `commandId` returns the stored result without re-applying; otherwise validate-then-apply on a clone, `validate::project` the candidate, and only then install it (all-or-nothing, A04). Undo/redo move through `History` (max 100 snapshots, oldest evicted) and **advance** the revision. History never references products or deletes sources.

### R15 — Time is integer milliseconds in the graph; rational media clocks only inside the render plan.

`core::editor::time`: output duration `round((out_ms - in_ms) / speed)` with round-half-away-from-zero on `f64` of bounded integers (≤ 7.2e6 / 0.25, exact in f64); `source_at(clip, t) = in_ms + round((t - start_ms) * speed)` clamped to `[in_ms, out_ms)`; half-open everywhere. `frame_timestamp(n, p, q)` is the starter's checked `u128` formula. Cue output spans are the intersection of the cue's source span with the clip's source range, mapped. These functions are the ones the TS twin mirrors under the shared fixture (R4).

### R16 — Workspace and guide progress are preferences, persisted apart from the project and from Undo.

Workspace (`selection_clip_ids, selected, playhead_ms, library_tab, property_tab, timeline_zoom, timeline_height, timeline_scroll_left, timeline_scroll_top, snap, delete_mode, monitor_muted, playback_rate, library_hidden, properties_hidden, properties_open, focus_preview, caption_settings_open`) is sanitized on read (every field optional, clamped, unknown dropped — it grants nothing) and stored in the project envelope's `workspace` on save plus a per-project `workspace.json` debounced by `editor_save_workspace`. Guide progress lives in `editor-prefs\guide-progress.json` with `contentRevision`, validated step IDs, never media or paths.

### R17 — Terminology.

`CONTEXT.md` already defines **Project** as Task metadata. The editor's document is therefore a **Tutorial Project** in docs, UI copy and code comments (`project` stays the field/module name inside `core::editor`, whose scope disambiguates it). New terms, added to `CONTEXT.md` in Task 60: **Tutorial Project**, **Clip**, **Track**, **Teaching Cue**, **Take**, **Rendered Product**, **Render** (produce a product outside every vault), **Publish** (the tenth vault write). **Export** keeps its current definition until Task 59 retires the command, after which the entry is amended to "the phase-5 quick save, superseded by Render + Publish".

### R18 — Onboarding content ships as data; targets are typed keys, not CSS selectors.

`contracts/onboarding.steps.json` and `onboarding.chapters.json` are copied verbatim into `src/editor/guide/` (22 steps, 7 chapters). Each step's reference selector maps to a typed `GuideTargetKey` (`projectbar`, `library.import`, `transport`, `timeline.toolbar`, `clip.selected`, `timeline.split`, `timeline.undo`, `inspector`, `timeline.more`, `track.menu`, `library.webcam`, `inspector.layout`, `inspector.fades`, `mixer`, `preview.toolstrip`, `library.captions`, `library.chapters`, `header.checks`, `header.save`, `header.render`, `library.products`, `header.help`) resolved through a registry of component refs. A test fails if any step lacks a key or any key lacks a registered target in the mounted shell.

### R19 — Accessibility and privacy are executed checks, with Windows AT as a residual gate.

Keyboard journey and forced-colors/reduced-motion are covered by Vitest + Playwright (Chromium forced-colors emulation). Narrator/NVDA on WebView2 cannot be executed here — residual gate R-A1. Editor logs pass through a `redact` helper (no file names, paths, caption text); a test greps the editor module sources for `log::` calls interpolating a `Path`/`name` without it.

### R20 — Nothing is faked.

No automatic transcription, no tracked redaction, no loudness normalization, no simulated device, no progress bar that completes without a terminal record, no control that silently succeeds. Every native capability that cannot be exercised on this host is a named residual gate with a checklist row (§7).

---

## 3. Architecture

### 3.1 Rust module map (all sized under 800 nonblank lines; tests inline)

`src-tauri/core/src/editor/` (pure, Linux-tested):

| File | Owns |
|---|---|
| `mod.rs` | re-exports; schema identifiers `PROJECT_SCHEMA = "vault-buddy-video-project/3"`, `WORKSPACE_SCHEMA = "vault-buddy-workspace/1"`, `PACKAGE_SCHEMA = "vault-buddy-project-package/1"`; `limits` consts |
| `ids.rs` | `is_valid_id`, `new_entity_id(prefix)`, `new_project_id()` |
| `error.rs` | `EditorErrorCode`, `EditorError { code, message, retryable, operation_id, retained_asset_ids }` |
| `model.rs` | `Project, Canvas, Asset, Track, Clip, Adjustments, Card, Destination` |
| `model_cues.rs` | `Effect, EffectKind, CaptionSettings, CaptionCue, Marker, Transition, TransitionKind, Product, RenderRange, Record, WorkspaceEnvelope` |
| `workspace.rs` | `Workspace` sanitizer |
| `validate.rs` / `validate_media.rs` | semantic validation steps 3–8 (split by entity family) |
| `time.rs` | R15 mapping + `frame_timestamp` + shared-fixture tests |
| `migrate.rs` | sidecar (+ legacy timeline, + stems, + webcam) → v3; reference-name destination mapping |
| `fingerprint.rs` | canonical JSON + SHA-256 `edit_fingerprint` |
| `session.rs` / `history.rs` | R14 |
| `commands/mod.rs` | `EditorCommand` enum + dispatch |
| `commands/{clips,groups,tracks,mix,fades,layout,cues,captions,cards}.rs` | one family each |
| `captions_io.rs` | SRT / WebVTT parse + export |
| `checks.rs` | before-you-share findings |
| `render_plan.rs` / `render_plan_audio.rs` | pure RenderPlan |
| `note.rs` | tutorial companion note |
| `package.rs` | package manifest + archive validation (`zip`) |
| `probe.rs` | image header sniff (PNG/JPEG/WebP dims), import allowlist |
| `peaks.rs` | s16le → peak buckets |

`src-tauri/screen/src/render/` (pure except `run`): `mod.rs`, `video_graph.rs`, `video_layers.rs`, `audio_graph.rs`, `ass.rs`, `args.rs`, `run.rs` (reuses `export.rs` runner internals, extracted to `screen/src/ffmpeg_run.rs` in Task 42). `screen/tests/render_roundtrip.rs`.

`src-tauri/src/editor/` (shell, directory module): `mod.rs` (state + registration list), `authz.rs`, `project_store.rs`, `store_io.rs`, `session_commands.rs`, `save_commands.rs`, `package_commands.rs`, `media_commands.rs`, `media_jobs.rs`, `render_jobs.rs`, `render_commands.rs`, `publish.rs`, `webcam_commands.rs`, `prefs_commands.rs`, `recovery.rs`, `diagnostics.rs`, `authz_guard.rs` (test-only structural scan).

### 3.2 Frontend map (all under 500 nonblank lines)

- `src/editorTypes.ts` (DTOs, split from `types.ts` like `screenTypes.ts`), `src/editor/decode.ts`, `src/editor/port.ts`, `src/editor/listenerScope.ts`, `src/editor/timeMap.ts`, `src/editor/actions.ts`, `src/editor/shortcuts.ts`, `src/editor/cueGeometry.ts`, `src/editor/previewController.ts`, `src/editor/guide/{steps.json,chapters.json,content.ts,targets.ts}`.
- Stores: `src/stores/{editorProject,editorWorkspace,editorJobs,editorOnboarding}.ts`.
- Components: `src/components/editor/shell/{EditorShell,EditorHeader,PreviewToolbar,DialogHost}.vue`, `…/timeline/{TimelineView,TrackHeader,TrackLane,ClipItem,TimelineRuler,TimelineToolbar}.vue`, `…/preview/{PreviewSurface,CueOverlay,TransportBar}.vue`, `…/inspector/{InspectorPanel,ClipSection,LayoutSection,FadesSection,AudioSection,SpeedSection,ColorSection,EffectSection}.vue`, `…/library/{MediaLibrary,TitlesLibrary,CaptionsLibrary,ChaptersLibrary,ProductLibrary}.vue`, `…/dialogs/{SaveProjectDialog,RenderDialog,ChecksDialog,WebcamDialog,ReconnectDialog,CloseGuardDialog,RecoveryDialog}.vue`, `…/menus/ContextMenu.vue`, `…/guide/{GuideInvitation,GuideCoach,LearningCenter}.vue`.

### 3.3 IPC contract (exact names; all `editor_*` are editor-window-only, R8 — `list_tutorial_projects` and `open_project_editor` are the documented exception: panel-callable, granted through `capabilities/default.json`, never `editor.json`, because neither name starts with `editor_` and neither lives under `src-tauri/src/editor/`)

| Command | Sync/async | Args | Returns |
|---|---|---|---|
| `open_capture_editor` *(existing)* | sync | `base` | `()` |
| `take_editor_request` *(existing, widened in Task 37)* | sync | — | `{ kind: "staged" \| "project", value: string } \| null` |
| `list_tutorial_projects` *(panel-callable, NOT editor-scoped — F4, Task 37 Part B)* | async | — | `ProjectSummaryDto[]` — the same listing `editor_list_projects` reads, without opening or touching a session |
| `open_project_editor` *(panel-callable, NOT editor-scoped — F4, Task 37 Part B)* | sync | `projectFileId` | `()` — stash + `editor:open`, exactly like `open_capture_editor` |
| `editor_open_staged` | async | `stagedBase: string` | `EditorOpenResult` |
| `editor_open_project` | async | `projectFileId: string, useRecovery: boolean` | `EditorOpenResult` |
| `editor_list_projects` | async | — | `ProjectSummaryDto[]` |
| `editor_get_snapshot` | async | `sessionId, knownRevision: number \| null` | `EditorProjection` |
| `editor_execute` | async | `request: ExecuteRequest` | `EditorProjection` |
| `editor_save_project` | async | `sessionId, expectedRevision` | `SaveReceipt` |
| `editor_export_package` | async | `sessionId, expectedRevision, format: "portable" \| "lightweight"` | `PackageReceipt \| null` (null = dialog cancelled) |
| `editor_import_package` | async | — | `EditorOpenResult \| null` |
| `editor_import_media` | async | `sessionId, onProgress: Channel<JobProgressDto>` | `{ jobId }` |
| `editor_relink_media` | async | `sessionId, assetIds: string[], confirmReplace: boolean` | `RelinkReport \| null` |
| `editor_media_url` | async | `sessionId, ref: { assetId } \| { productId } \| { reviewJobId }` (the last, Task 47) | `string` (absolute path for `convertFileSrc`) |
| `editor_media_peaks` | async | `sessionId, assetId, buckets: number` | `{ peaks: number[] }` |
| `editor_media_thumbnail` | async | `sessionId, assetId, atMs` | `string` |
| `editor_get_checks` | async | `sessionId` | `CheckFinding[]` |
| `editor_import_captions` | async | `sessionId, clipId, replace` | `CaptionImportResult { projection, imported, skipped } \| null` |
| `editor_start_render` | async | `request: RenderRequest, onProgress: Channel<JobProgressDto>` | `{ jobId, revision }` |
| `editor_cancel_job` | sync | `sessionId, jobId` | `()` |
| `editor_get_jobs` | async | `sessionId` | `JobRecordDto[]` |
| `editor_get_products` | async | `sessionId` | `ProductDto[]` |
| `editor_restore_product` | async | `sessionId, expectedRevision, productId, commandId` | `EditorProjection` |
| `editor_publish_product` | async | `sessionId, productId, destination: PublishDestination` | `PublishReceipt` |
| `editor_export_subtitles` | async | `sessionId, format: "srt" \| "vtt"` | `string \| null` |
| `editor_get_workspace` / `editor_save_workspace` | async | `sessionId` / `sessionId, workspace` | `Workspace` / `()` |
| `editor_get_guide_progress` / `editor_save_guide_progress` | async | — / `progress: GuideProgress` | `GuideProgress` / `()` |
| `editor_export_guide_progress` / `editor_import_guide_progress` | async | — | `string \| null` / `GuideProgress \| null` (native dialogs) |
| `list_capture_webcams` *(screen domain, not editor-scoped)* | async | — | `{ id, label }[]` (degrades to `[]`) |
| `editor_webcam_begin` | async | `sessionId, mimeType` | `{ takeId }` |
| `editor_webcam_append` | async | raw body; headers `x-editor-session`, `x-editor-take`, `x-editor-seq` | `()` |
| `editor_webcam_finish` | async | `sessionId, takeId, lastSeq` | `TakeDto` |
| `editor_webcam_discard` | async | `sessionId, takeId` | `()` |
| `editor_close_session` | async | `sessionId, disposition: "keep" \| "discardRecovery" \| "discardProject"` | `()` |
| `editor_hide_window` | sync | — | `()` |
| `editor_export_diagnostics` | async | — | `string \| null` |

Errors: every `editor_*` returns `Result<T, EditorError>`; `EditorErrorCode` ∈ `invalidRequest, invalidProject, revisionConflict, sessionGone, unauthorizedSource, sourceMissing, unsupportedMedia, deviceUnavailable, permissionDenied, diskFull, writeDenied, destinationUnavailable, encoderUnavailable, cancelled, internal` — exactly the bundle's set.

Job progress (Channel, never `app.emit`): `JobProgressDto { sessionId, jobId, kind: "import" | "render" | "peaks" | "publish", sequence, phase: "queued" | "preparing" | "rendering" | "publishing" | "complete" | "cancelled" | "failed", fraction, terminal: { productId?, assetIds?, error? } | null }`. Import uses phases `queued/preparing/complete/...` with per-file results in `terminal.assetIds` and errors in a `perFile: [{name, error}]` array (names are display names only, never paths).

Events: `editor:open` (existing, `emit_to("editor")`) and one new, `editor:closeRequested` (`emit_to("editor")`, payload `{}`) emitted by `window_close.rs` instead of hiding directly; the frontend answers with the close-guard dialog and then `editor_hide_window`.

### 3.4 Data flows

- **Capture → editor (F-01):** `screen:stopped` → `ScreenCaptureBar` **Edit** / `StagedCaptureList` **Edit** → `open_capture_editor(base)` → `editor:open` → `EditorRoot` drains `take_editor_request` → `editor_open_staged(base)` → native sidecar read (vault from sidecar) → pin → `EditorOpenResult`. The capture store is never asked for a vault.
- **Edit:** UI transient preview → pointer-up → `editor_execute({sessionId, expectedRevision, commandId, command})` → serial queue → validate/apply/validate → projection → store installs only if `sessionId` and generation match and `revision` increased.
- **Save:** `editor_save_project` → atomic commit → `SaveReceipt` → store marks `persistedRevision` only when the receipt's session and revision match (A18).
- **Render:** `editor_start_render` freezes revision → `RenderPlan` → capability probe → `jobs\<jobId>\out.mp4.part` → probe duration/streams → `products\<productId>.mp4` → `products.json` commit → Channel `complete{productId}`.
- **Publish:** `editor_publish_product` → free-space → reserve pair → copy video → note → journal complete → `PublishReceipt`.

---

## 4. Data model mapping and migration

- Interchange entities map 1:1 onto `core::editor::model*`. Optional interchange fields are `Option<T>` with `skip_serializing_if = "Option::is_none"` so a round trip is byte-stable modulo key order; the fixture test compares parsed `serde_json::Value`s.
- **Native source registry** (`sources.json`, never in the interchange document — the schema forbids `path/url/src`): `{ "<assetId>": { "store": "staging" | "media" | "takes" | "builtin", "base"?: string, "file"?: string, "sha256"?: hex, "size": u64, "durationMs": u64, "width"?: u32, "height"?: u32, "hasAudio": bool, "hasVideo": bool, "mediaKind": "video" | "audio" | "image" } }`. Hashes are computed on a named `editor-hash` thread, off the UI path.
- **Staged-capture migration** (`migrate::from_staged`): asset `src` (kind video, duration = sidecar `duration_ms`, width/height from sidecar); tracks `v1` (video, "Screen") and `a1` (audio, "Audio", empty unless stems); each legacy `timeline` segment `[start,end)` becomes a clip on `v1`, laid end to end from 0 (sequential semantics preserved: project duration == `Timeline::output_duration_ms`); absent/malformed legacy timeline → one whole clip (the `timeline_from_sidecar` degrade, reused); an explicitly empty legacy timeline → zero clips (the user deleted everything; never resurrected). Stems → one audio track per stem, main clip `muted: true`. Webcam → track `v2`, one clip per placed legacy segment at that segment's output start, clipped to what the webcam covers from its sidecar `offsetMs` — e.g. a webcam whose first frame reached the capture clock at `offsetMs: 420` starts 420 ms into the recording, not at 0 (§9 (b)) — at `x=0.775,y=0.06,w=0.19,h=0.3378` on 16:9 (`h` keeps the circle square on each canvas, `tests/fixtures/editor-presenter-placement.json`), `frame_shape:"circle",fit:"cover"` (the reference presenter placement).
- Canvas for a migrated capture: the nearest of the four supported canvases by aspect (16:9 → 1280×720, 9:16 → 720×1280, 1:1 → 720×720, 4:3 → 960×720), fps 30, with a `canvasReview` check when the source aspect is not exact.

---

## 5. F-ID ownership map (all 50)

Owner = the module that is authoritative; UI = where the user reaches it; Task = the plan task(s) that deliver it.

| F-ID | Capability | Authoritative module (Rust) | UI owner | Tasks |
|---|---|---|---|---|
| F-01 | Native capture handoff | `src-tauri/src/editor/session_commands.rs::editor_open_staged` + `staging::read_sidecar` + `editor::migrate` | `EditorRoot.vue`, `ScreenCaptureBar`, `StagedCaptureList` | 4, 9, 10, 15 |
| F-02 | Local media import | `editor/media_commands.rs` + `media_jobs.rs`; `core::editor::probe` | `MediaLibrary.vue` | 24, 25 |
| F-03 | Reconnect originals | `editor/media_commands.rs::editor_relink_media`; identity in `sources.json` | `ReconnectDialog.vue` | 40 |
| F-04 | Multi-track video | `core::editor::commands::clips`; `render::video_layers` | `TimelineView`, `PreviewSurface` | 7, 20, 26, 42 |
| F-05 | Multi-track audio | `core::editor::commands::mix`; `render::audio_graph` | `AudioSection`, mixer | 27, 44 |
| F-06 | Track management | `core::editor::commands::tracks` | `TrackHeader.vue` | 23 |
| F-07 | Split | `commands::clips::split` | `TimelineToolbar`, context menu | 7, 21 |
| F-08 | Trim | `commands::clips::trim` | `ClipItem` handles, `ClipSection` | 7, 21 |
| F-09 | Delete and ripple | `commands::clips::delete` (track-local ripple) | `TimelineToolbar` | 7, 21 |
| F-10 | Move and reorder | `commands::clips::{move_clips, reorder}` | `TimelineView`, `ClipSection` | 7, 21, 26 |
| F-11 | Multi-selection and grouping | `commands::groups` | `editorWorkspace` selection, `TimelineView` | 8, 21 |
| F-12 | Copy/cut/paste/duplicate | `commands::groups::{paste, duplicate}` + `clipboard_fragment` query | action registry | 8, 17 |
| F-13 | Undo and redo | `core::editor::history` | header/toolbar, Ctrl+Z/Ctrl+Shift+Z/Ctrl+Y | 6, 21 |
| F-14 | Timeline navigation | TS `timeMap` (read-side, fixture-held) | `TimelineView`, `TransportBar` | 3, 20, 22 |
| F-15 | Contextual menus | action registry (`src/editor/actions.ts`) | `ContextMenu.vue`, Shift+F10 | 17 |
| F-16 | Clip speed | `commands::layout::set_speed`; `render::audio_graph` atempo | `SpeedSection` | 31, 42, 44 |
| F-17 | Video fades | `commands::fades` | `FadesSection`, gold handles | 29, 42 |
| F-18 | Audio fades | `commands::fades`; `render::audio_graph` afade | `FadesSection` | 29, 44 |
| F-19 | Clip transitions | `commands::fades::{add_transition, remove_transition}` | context menu, `FadesSection` | 30, 42, 44 |
| F-20 | Webcam recording | `editor/webcam_commands.rs` | `WebcamDialog.vue` | 49, 50 |
| F-21 | Presenter PiP | `commands::layout` | `PreviewSurface` handles, `LayoutSection` | 31, 50 |
| F-22 | Synchronized screen + webcam | `screen::session::webcam` + `staging` webcam block + `migrate` | `ScreenSourcePicker` webcam toggle | 51, 52 |
| F-23 | Source layout/transforms | `commands::layout`; `render::video_layers` | `LayoutSection` | 31, 42 |
| F-24 | Audio detachment | `commands::mix::detach_audio` (linked asset DAG) | `AudioSection`, context menu | 27 |
| F-25 | Mixer and monitoring | `commands::mix` (render-affecting) vs `Workspace.monitor_muted` (not) | mixer, `TransportBar` | 22, 27 |
| F-26 | Waveforms | `core::editor::peaks` + `media_jobs` (cancelable, bounded) | `ClipItem` | 28 |
| F-27 | Text callouts | `commands::cues`; `render::ass` | teaching toolbar, `EffectSection` | 34, 35, 43 |
| F-28 | Arrows | `commands::cues`; `render::ass` drawing | `CueOverlay` endpoint handles | 34, 35, 43 |
| F-29 | Highlights | `commands::cues`; `render::ass` | `CueOverlay` | 34, 35, 43 |
| F-30 | Spotlight | `commands::cues`; `render::ass` (four-rect dim) | `CueOverlay` | 34, 35, 43 |
| F-31 | Zoom and focus | `commands::cues`; `render::video_graph` crop/scale expr | `CueOverlay` focal handle | 34, 35, 42 |
| F-32 | Numbered steps | `commands::cues`; `render::ass` | `EffectSection` | 34, 35, 43 |
| F-33 | Privacy covers | `commands::cues` + `checks` warning; `render::ass` opaque | `EffectSection`, Checks | 34, 35, 43, 54 |
| F-34 | Caption authoring/import | `core::editor::captions_io` + `commands::captions` | `CaptionsLibrary.vue` | 36 |
| F-35 | Caption outputs | `render::ass` burn-in + `editor_export_subtitles` | `CaptionsLibrary`, `RenderDialog` | 36, 43, 48 |
| F-36 | Chapter markers | `commands::cues` (markers) + `render_plan` chapter map + `note` | `ChaptersLibrary.vue` | 36, 48 |
| F-37 | Title/background cards | `commands::cards` (incl. intro-before-all) ; `render::ass` + `color` source | `TitlesLibrary.vue` | 33, 42 |
| F-38 | Canvas formats | `commands::layout::set_canvas` + `checks` review | `PreviewToolbar` ratio | 32 |
| F-39 | Color treatment | `commands::layout::set_adjustments`; `render::video_layers` eq/hue/colorchannelmixer | `ColorSection` | 32, 42 |
| F-40 | Save editable project | `editor/save_commands.rs`, `package_commands.rs`, `core::editor::package` | `SaveProjectDialog.vue` | 12, 38, 39 |
| F-41 | Rendered products | `editor/render_jobs.rs`, `products.json` ledger | `ProductLibrary.vue` | 46, 47 |
| F-42 | Output review | `render_jobs` (range render) + `editor_media_url({productId})` | `RenderDialog`, product player | 46, 47 |
| F-43 | Companion note | `core::editor::note` + `editor/publish.rs` | Publish dialog | 48 |
| F-44 | Project/session recovery | `editor/recovery.rs` + `recovery.json` + close guard | `RecoveryDialog`, `CloseGuardDialog` | 9, 37 |
| F-45 | Before-you-share checks | `core::editor::checks` | `ChecksDialog.vue` | 54 |
| F-46 | Guided onboarding | `src/editor/guide/*` (frontend-owned) + `editor_*_guide_progress` | `GuideInvitation`, `GuideCoach` | 55, 56 |
| F-47 | Learning center | frontend | `LearningCenter.vue` | 57 |
| F-48 | Focused responsive shell | frontend | `EditorShell`, drawers, themes | 16, 18 |
| F-49 | Keyboard/accessible controls | action registry + shared UI | all | 17, 56, 58 |
| F-50 | Local privacy/diagnostics | `editor/diagnostics.rs` + `redact` | Help menu | 58 |

No F-ID is dropped. F-22 and the stem half of F-05's capture story are delivered as code whose acceptance is hardware-only (§7); that is stated, not hidden.

---

## 6. Acceptance scenarios → evidence

| Scenario | Automated evidence (task) | Residual |
|---|---|---|
| A01 vault from sidecar | shell test: sidecar vault A, no store state → `EditorOpenResult.project.destination.vault == A` (10) | — |
| A02 stillSaving | `screenCapture` store test: stillSaving never enables Edit (15) | — |
| A03 cut/save/reopen | core session test + shell tempdir test; source SHA-256 unchanged (7, 12) | — |
| A04 atomic group move | `commands::groups` test with a locked track (8) | — |
| A05 intro insertion | `commands::cards` test (33) | — |
| A06 monitoring ≠ export mute | render_plan test: `monitor_muted` ignored (41) | — |
| A07 fades/crossfades | golden argv + round-trip RMS ramps (29, 30, 44, 45) | — |
| A08/A09 webcam independent/retained | webcam command tests with synthetic WebM bytes (49) | R-H1 physical camera |
| A10 resize survives serialization | round-trip test (31) | — |
| A11 source-linked cues | split/trim/move/speed cue tests (7, 34) | — |
| A12 caption timing | captions_io + commands tests (36) | — |
| A13 range timestamps | render_plan range test + note chapter shift (41, 48) | — |
| A14 context target precision | ContextMenu test: clicked time, not playhead (17) | — |
| A15 save without render | save test (12) | — |
| A16 product lineage | restore test: product file bytes unchanged (47) | — |
| A17 historical dependency | package collector includes snapshot-only assets (39) | — |
| A18 matching receipt | store test: stale receipt ignored (14) | — |
| A19 safe reconnect | ambiguity refusal test (40) | — |
| A20 save fault recovery | injected-failure writer test (12, 37) | R-H4 real disk-full |
| A21 partial publication | publish journal tests (48) | — |
| A22 malicious project | package validator threat tests (38) | — |
| A23–A26 guide | guide tests (55, 56) | — |
| A27 startup error clarity | recovery dialog test with malformed project (37) | — |
| A28 no repeated toolbar | shell test: one preview toolbar at 960×640 (Playwright, 16) | — |
| A29 native temporal output | render round trip (45) | R-H2 Windows ffmpeg + MF source |
| A30 privacy boundary | diagnostics test (58) | — |

---

## 7. Residual gates (cannot be verified on this host or in CI) and new gaps

Each becomes a row in the new checklist `docs/superpowers/specs/2026-09-21-tutorial-editor-windows-verification.md` (created in Task 15, appended to by each hardware-touching task, completed in Task 60). An empty Result means unrun, not passed.

- **R-H1** Physical camera + mic: permission prompt, denial, unplug mid-take, device switch, camera light off after close (F-20).
- **R-H2** Windows ffmpeg build against a real staged fragmented MP4: layered render, `ass` fonts on Windows (libass font provider), `xfade`/`acrossfade`, hardware H.264 encoder selection (F-41, F-42, A29).
- **R-H3** F-22 synchronized webcam producer: 10-minute drift measurement, source closure, webcam unplug (MF source reader).
- **R-H4** F-05 stems writer: per-input stems align with the mixed track within one audio frame.
- **R-H5** Disk-full during save, render and publish on a real volume (A20, A21).
- **R-A1** Narrator/NVDA on WebView2: main journey, menus, dialogs, coach (F-49).
- **R-A2** Windows high-contrast + 150 %/200 % scaling at 960×640 (F-48, F-49).
- **R-P1** Product decision: native package size limit beyond 200 MiB media (R9).
- **R-P2** Product decision: the `zip` crate dependency (R9).
- **R-M1** Memory: ten-minute 1080p30 tutorial, 3 video + 2 audio layers, peak/steady RSS of the app and the ffmpeg child, measured on a named machine (`PRODUCT-SPEC.md` NFR).

New docs/Gaps.md entries created by the tasks (numbers assigned when written, after GAP-169): **GAP-N1** webcam takes are not excluded by `CaptureGuard` (UI-only exclusion); **GAP-N2** no native WebView2 permission handler restricting camera to the editor window; **GAP-N3** the webview preview is an approximation of the ffmpeg render (GAP-145's successor), bridged only by range Review renders; **GAP-N4** publication copies products, doubling disk use until the project is discarded; **GAP-N5** app-manifest command scoping is verified by build, not by a runtime test.

*(Task 60)* The placeholders map to real entries: **GAP-N1 → GAP-193** (amended: the begin-side refusal is native, §9 (a)); **GAP-N2 → GAP-194**; **GAP-N3 → GAP-173** (GAP-145 is marked superseded by it); **GAP-N4 → GAP-212** for the disk-doubling this record names, while the plan's own GAP-N4 wording (F36, "publish has no resume-from-journal") became **GAP-192**; **GAP-N5 → GAP-170**. The same mapping is the table `tests/editorEvidence.test.ts` checks in `docs/superpowers/specs/2026-09-21-tutorial-editor-acceptance-evidence.md`.

---

## 8. Invariants this design adds (AGENTS.md, Task 60)

1. `core::editor` is the only authority for committed edits; the frontend never applies an edit locally except as a transient preview discarded on pointer-up/Escape.
2. Every `editor_*` command checks the caller label and session ownership (structurally pinned).
3. The asset scope is the enumerated list in R7 (test-pinned exactly).
4. Project files are written only through `write_atomic_replacing` (store) or temp + `rename_noreplace` (exports); never `std::fs::write`, never `std::fs::rename` for a first write.
5. Products are immutable: nothing opens a product file for writing after `products.json` records it.
6. A staged capture pinned by a project is never discarded or cleared; discarding a project never deletes a staged capture.
7. Job progress travels only on per-job `Channel`s; no `app.emit` for editor jobs.
8. The render fast path keys on `RenderPlan::is_identity`, never on field absence.
9. `editor-time-cases.json` holds TS time mapping to Rust (size-guarded, both languages).
10. Guide actions never execute editor commands (a test replays all 22 steps and asserts zero `editor_execute` calls).

---

## 9. Amendments after implementation (Task 60, pre-flight F35 and F19)

The plan kept its own behaviour wherever it drifted from this record (controller ruling on finding 35); this section amends the record to match what shipped, one paragraph per drift, each naming the task that introduced it. Where the text above was edited in place it is marked *(Amended by Task 60)*.

**(a) R10 — the webcam take's `CaptureGuard` refusal is native (Task 49).** R10 described the exclusion between a webcam take and a running capture as UI state only. Task 49 backs it with a native check: `editor_webcam_begin` refuses (`deviceUnavailable`, "Stop the screen recording first." / "…audio recording…") while `CaptureGuard::active()` is `Some` — a stronger guarantee than R10 stated. The take still does NOT claim the guard, so the reverse direction (a capture started while a take records) is unrefused; the two scopes are intentionally not identical, and that residual is GAP-N1 = docs/Gaps.md GAP-193.

**(b) §4 — a synchronized webcam's offset is real, not 0 (Tasks 51, 52).** §4's example placed the webcam clip "at 0". The sidecar's `webcam.offsetMs` is the capture-clock time of the webcam file's first frame (the file itself is rebased to that frame, Task 52, GAP-199), so it is normally non-zero and may be negative; migration places the presenter from that offset, one clip per placed legacy segment. `0` in the original example was illustrative, never a constraint — §4 now shows a non-zero example.

**(c) R14 — `InternalCommand` has no `commandId` (Task 46).** Every `EditorCommand` travels inside an `ExecuteRequest` carrying a `commandId`; the plan's early per-command wording implied the internal commands were uniform with that. They are not: `InternalCommand` (`AddAssets`, `ImportCaptions`, `RestoreSnapshot`, `RelinkAssets`) is never deserialized from IPC and deliberately has no `commandId` field. `editor_restore_product` — the one internal command a user triggers as an undoable edit with replay protection — takes `commandId` as its own argument and threads it beside the payload (`EditorSession::execute_internal_as`).

**(d) §3.3 — `JobProgressDto.kind` includes `"publish"` (Task 48, F19).** A publish registers in the same job registry as renders, before a byte is written, so `editor_get_jobs`, the close guard and the shutdown gate see it; the kind list is `import | render | peaks | publish`. A publish job has no `Channel` (its receipt is the command's reply).

**(e) The wire spelling of the inner kind fields (Task 6).** The commands that carry a second "kind" — `addTrack`, `addTransition`, `addEffect` — spell it `trackKind`, `transitionKind` and `effectKind` on the wire, because the command enum is internally tagged on `kind` and a payload field of the same name cannot round-trip. The contract text that read `addTrack{kind, …}` is superseded by `addTrack{trackKind, name, index}`, `addTransition{fromClipId, toClipId, durationMs, transitionKind}`, `addEffect{clipId, effectKind, startMs, endMs, props}`; `props` is decoded against the sibling `effectKind`.

**(f) R12 — the gate's render term is wider and the publish term is its own (Tasks 46, 48).** R12 said the render term is true "while any job is in `rendering` or `publishing`". Task 46 widened the phase set to every not-yet-ended render job (`queued`, `preparing`, `rendering`, `publishing`) — matching "cancel it the way it cancels an export", which blocked from its reservation on — and narrowed the job set to kind `render` only, so a publish is not counted as a render (re-counting every kind would reopen GAP-190's loop). Task 48 added publish as its own fifth term (`publish::blocks_shutdown`), with its own bounded cancel (`publish::cancel_all_bounded`, 5 s) and abandon latch, run after the render cancel by both quit workers, and refused by the updater's prepare step. After Task 59 retired the export, the gate is: audio recording + screen capture + render + publish.

**(g) The companion note's `created-by` value (Task 48).** The tutorial note (`core::editor::note`) writes `created-by: vault-buddy`, while every other Vault Buddy note (recording, transcript, imported document) writes `created-by: Vault Buddy`. Task 60 changes no behaviour, so the difference is recorded, not reconciled: docs/Gaps.md GAP-213 (a provenance query on the other notes' value misses tutorial notes).

**(h) Smaller §3.3 drifts (Tasks 40, 47, 59).** `editor_relink_media` takes `confirmReplace` (Task 40); `editor_media_url`'s `ref` also accepts `{ reviewJobId }` for the one Review render in the project's `cache\` (Task 47), and `RenderRequest` carries an optional `review` flag; R13's `vault_dir` helpers moved to `src-tauri/src/editor/vault_dir.rs` when Task 59 retired `export_worker/` and the ninth vault write (nine of the ten numbered writes are live).
