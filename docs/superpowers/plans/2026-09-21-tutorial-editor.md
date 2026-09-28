# Tutorial Editor Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Each `## Task N:` is sized for ONE subagent session. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn a staged screen capture into an editable, multi-track **Tutorial Project** — cut, arrange, layer, fade, annotate, caption, chapter, add a presenter take — save it without rendering, render immutable **Rendered Products** through the user-installed ffmpeg, review the actual file, and **Publish** a product plus companion note into a vault. Every one of the concept bundle's 50 F-IDs is delivered or carried as an explicit, named residual gate.

**Spec:**
- Integration decision record (P00): `docs/superpowers/specs/2026-09-21-tutorial-editor-integration-design.md` — rulings R1–R20 are cited by number below and are binding.
- Concept bundle: `docs/concepts/vault-buddy-editor/` (START-HERE.md, docs/PRODUCT-SPEC.md, docs/FEATURE-CATALOG.md, contracts/feature-catalog.json, docs/DATA-MODEL.md, docs/IPC-CONTRACTS.md, docs/NATIVE-MEDIA.md, docs/PERSISTENCE-AND-SECURITY.md, docs/SCREENS-AND-INTERACTIONS.md, docs/ONBOARDING.md, docs/ACCEPTANCE-SCENARIOS.md, contracts/workspace.schema.json, contracts/reference-workspace.example.json, implementation-starter/). Where the bundle and the ADR disagree, **the ADR wins** (it records why).

**Architecture:** A pure `vault_buddy_core::editor` aggregate (model, validation, time mapping, commands, history, migration, checks, captions, render plan, note, package) is the single authority for committed edits (R2, R14). The Tauri shell's `src-tauri/src/editor/` directory module owns sessions, the on-disk project store outside every vault (R5), caller authorization (R8), media/render/publish jobs on per-job `Channel`s, and the tenth sanctioned vault write (R13). Rendering is one `filter_complex` ffmpeg pass built by pure builders in `screen::render` and round-trip-tested in CI (R1). The existing `editor` window and `EditorRoot.vue` are evolved (R4); four Pinia stores hold projections, workspace, jobs and guide state.

**Tech Stack:** Rust (`vault_buddy_core`, `vault_buddy_screen`, the Tauri 2 shell), user-installed ffmpeg/ffprobe (with libass), Vue 3 + Pinia + Tailwind 4, Vitest + happy-dom, Playwright (built `dist/`). One new crate: `zip` (R9). `sha2` added to core (already in the lockfile).

## Pre-flight rulings applied

The controller ruled on all 38 findings in `.superpowers/sdd/2026-09-21-tutorial-editor/preflight-scan.md`; every ruling was accepted as written and is folded into the task text below. Finding → task(s) touched:
1→9 · 2→9 · 3→15 · 4→37 · 5→4,41 · 6→4 · 7→9,10 · 8→3 · 9→6 · 10→3,8,13 · 11→29 · 12→30 · 13→8 · 14→21 · 15→27 · 16→18 · 17→18 · 18→47 · 19→48,60 · 20→48 · 21→45 · 22→24 · 23→29,44 · 24→51 · 25→51 · 26→9,51,53 · 27→54 · 28→58 · 29→31,59 · 30→Global Constraints,28,52,54,55,57 · 31→6,13,15,52,54 · 32→40 · 33→60 · 34→37 · 35→50,60 · 36→46,48 · 37→37 (split into Part A / Part B) · 38→58,59.

## Global Constraints

Every task's requirements implicitly include this section. A task is not done until every item holds.

- **Branch & PRs.** Work on the branch the orchestrator names (default: `claude/tutorial-editor`, cut from `claude/vault-buddy-improvement-polish-95f5bc` = PR #79's head). Never push to `main`; never open a PR unless the orchestrator says so.
- **Read before touching.** AGENTS.md (window system, vault domain, screen-capture section, testing conventions), the ADR, and the task's cited bundle sections. `docs/Gaps.md` before "discovering" a known problem.
- **TDD, always.** Write the named failing tests first, run them, see them fail for the stated reason, then implement. Regression tests name the failure mode in a comment. Every task carries a **mutation check**: break the thing the test names (one change at a time), run, confirm red with the expected message, restore byte-identically (back up with `cp`, compare with `git diff --stat`; **never** `git checkout --` to restore). A mutation that stays green is a finding — report it.
- **The fixture flaw.** A fixture that trips more than one guard, or asserts a value captured before the action, proves nothing. Fixtures are asymmetric (non-square canvases, non-unit speeds, distinct x/y) so a swapped axis or a missing scale fails.
- **LOC caps:** 800 nonblank lines per Rust file (tests inline), 500 per frontend `.ts`/`.vue`. **No new allowlist entry, ever.** Plan the split before the file grows (the ADR §3 file lists already are). The baselines are shrink-only; never run `npm run check:loc -- --update` or `npm run check:quality -- --update` (they rewrite descriptions). If a metric improves, hand-edit the single number.
- **Quality ratchet:** `deadCodeIssues 0`, `complexFunctions 13`, `criticalComplexity 3`, `averageMaintainability ≥ 91.6`, `cloneGroups 0`, `circularDependencies 0` (`scripts/quality-baseline.json`). A complex function is extracted, never allowlisted. Rust coverage floor `--fail-under-lines 94` (core/capture/transcribe/screen); frontend floors in `vite.config.ts`.
- **Where logic goes.** Anything without Tauri types goes in `core` (or `screen` for ffmpeg argv/ASS/media). The shell wires. Frontend components are presentational where possible; `invoke` lives only in `src/editor/port.ts`.
- **Rust is authoritative for committed edits** (R14). The frontend never mutates the project projection locally except as a transient drag/trim preview that pointer-up turns into ONE `editor_execute` and Escape discards.
- **Vault writes.** The only new vault write is Task 48's publication (the tenth, R13), on the ninth's rails: pairwise reservation, `rename_noreplace` + ` (N)`, video first, note second, never `std::fs::rename` for a first write, never `std::fs::write` into a vault. No other task writes into a vault.
- **Project-store writes** use `vault_buddy_core::capture_note::write_atomic_replacing` (store files) or an owned same-directory temp + `capture_paths::rename_noreplace` (exports). Deletion is owned-file-only, no-follow (`symlink_metadata` first), never `remove_dir_all` on anything not proven to be a project directory we created.
- **Asset scope is a security boundary** (R7). It changes in exactly one task (Task 22) to exactly the ADR's array, and `tray.rs`'s pin test is re-pointed to that exact array in the same commit.
- **Every `editor_*` command** takes `window: tauri::WebviewWindow`, calls `editor::authz::require_editor_window(&window)?` first, and validates session ownership (R8). The structural scan from Task 10 enforces it; new commands are added to its list.
- **Sync vs async** (AGENTS.md): window-touching commands are sync (main thread) and never block; everything that touches disk, a child process or a lock that can be held long is `async` + `spawn_blocking`. Every spawned thread is named (`std::thread::Builder`). No swallowed errors (`log::warn!`/`log::error!`, `src/logging.ts`).
- **`cfg(windows)` bodies:** crate-qualify or import every sibling-module path inside a `cfg(windows)` region (`cfg_windows_guard.rs`). On this Windows host, also run `cargo clippy -p vault-buddy --all-targets -- -D warnings` and `cargo test -p vault-buddy --lib` (CI's `windows-app` runs neither — GAP-168).
- **ffmpeg** is user-installed and detected, never bundled; add no ffmpeg crate. Round-trip tests SKIP VISIBLY without ffmpeg (`eprintln!("SKIP: …")`); CI's `rust-core` installs it. A skip is not a pass — report it.
- **IPC wire discipline:** camelCase envelopes; the project graph in document spelling (R3). Every new DTO gets a literal-JSON pin test in Rust and a decoder test in TS (never a struct re-serialized against itself). `src/editorTypes.ts` is the single TS declaration site.
- **Count, never increment:** after adding commands, re-measure with `awk '/generate_handler!\[/,/\]\)/' src-tauri/src/lib.rs | grep -cE '^\s+[a-z_]+::[a-z_]+,$'` and write the measured number into AGENTS.md's IPC section in the same task.
- **"Registration files"** (defined once here; later tasks cite it by name instead of re-listing it): the four places a new `editor_*` command must land — `src-tauri/src/lib.rs`'s `generate_handler!` list, `src-tauri/build.rs`'s `EDITOR_COMMANDS` const (Task 11), `src-tauri/capabilities/editor.json`'s permission list (Task 11), and AGENTS.md's IPC table (a new row plus the re-measured count from the bullet above). Tasks 28, 52, 54, 55 and 57 each add an explicit "update the registration files" step for this reason.
- **Gates (run all; redirect output to a file and read the file, or append `; echo "exit=$?"` — never pipe a gate through `tail`/`grep` that hides its exit code):**
  ```bash
  npm run lint && npm run check:loc && npm run check:quality && npm run build && npm run test:e2e && npm run test:coverage
  cd src-tauri && cargo fmt --check
  cd src-tauri && cargo clippy --workspace --all-targets -- -D warnings
  cd src-tauri && cargo test -p vault_buddy_core && cargo test -p vault_buddy_screen && cargo test -p vault-buddy --lib
  cd src-tauri && cargo machete . && cargo deny check
  ```
  (`check:quality` must run with no `coverage/` dir present, so `test:coverage` stays last.) A task that touches only one surface still runs every gate before committing — each task leaves the tree green.
- **Commits:** Conventional Commits (`feat(editor)`, `feat(core)`, `feat(screen)`, `fix(…)`, `test(…)`, `docs(editor)`), imperative subject, body explaining the why and the failure mode prevented. Write the message to a file and `git commit -F <file>`; no backticks in messages. End with exactly:
  ```
  Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
  ```
- **Docs move with code.** A task that adds a command/event/window rule/vault write updates AGENTS.md in the same commit; a task that finds a gap adds a `docs/Gaps.md` entry (next free number after GAP-169, assigned at write time); a task whose behaviour is only verifiable on hardware appends a row to `docs/superpowers/specs/2026-09-21-tutorial-editor-windows-verification.md` (created in Task 15).
- **Nothing is faked** (R20): no simulated device, no automatic transcription, no progress that completes without a terminal record, no control that silently succeeds. A disabled control carries a reason string.

## Contract reference (exact values — do not invent alternatives)

**Schema identifiers** (`core::editor`): `PROJECT_SCHEMA = "vault-buddy-video-project/3"`, `WORKSPACE_SCHEMA = "vault-buddy-workspace/1"`, `PACKAGE_SCHEMA = "vault-buddy-project-package/1"`.

**Limits** (`core::editor::limits`): `MAX_ASSETS 200`, `MAX_TRACKS 32`, `MAX_CLIPS 600`, `MAX_EFFECTS 1200`, `MAX_MARKERS 300`, `MAX_TRANSITIONS 300`, `MAX_CAPTIONS 2000`, `MAX_PRODUCTS 40`, `MAX_DURATION_MS 7_200_000`, `MAX_PROJECT_JSON_BYTES 8 * 1024 * 1024`, `MAX_PACKAGE_MEDIA_BYTES 200 * 1024 * 1024`, `MAX_PACKAGE_BYTES 220 * 1024 * 1024`, `MAX_PACKAGE_ENTRIES 1000`, `MAX_ENTRY_RATIO 100`, `MAX_HISTORY 100`, `MAX_RECENT_COMMANDS 256`, `MAX_CAPTION_FILE_BYTES 2 * 1024 * 1024`, `MAX_TITLE_CHARS 160`, `MAX_NAME_CHARS 300`, `MAX_EFFECT_TEXT_CHARS 1000`, `MAX_CAPTION_TEXT_CHARS 500`. Canvases: `(1280,720)`, `(720,1280)`, `(720,720)`, `(960,720)`, fps 30. Speed `0.25..=4.0`. ID regex `^[a-zA-Z0-9_-]{1,100}$`.

**Error codes** (`EditorErrorCode`, serialized camelCase): `invalidRequest, invalidProject, revisionConflict, sessionGone, unauthorizedSource, sourceMissing, unsupportedMedia, deviceUnavailable, permissionDenied, diskFull, writeDenied, destinationUnavailable, encoderUnavailable, cancelled, internal`. `EditorError { code, message, retryable, operationId, retainedAssetIds? }`.

**Envelopes** (camelCase):
- `EditorSnapshot { sessionId, projectId, revision, persistedRevision: number|null, title, durationMs, canUndo, canRedo, undoLabel: string|null, redoLabel: string|null }`
- `EditorProjection { snapshot, project }` · `EditorOpenResult { snapshot, project, workspace, missing: MissingMedia[], sourceBase: string|null, recovered: boolean }` · `MissingMedia { assetId, name, expectedSize, expectedDurationMs }`
- `ExecuteRequest { sessionId, expectedRevision, commandId, command }` · `SaveReceipt { sessionId, savedRevision, projectFileId }` · `PackageReceipt { sessionId, savedRevision, fileName, format }`
- `ProjectSummaryDto { projectFileId, title, updatedAt, persistedRevision, hasRecovery, sourceBase: string|null }`
- `JobProgressDto { sessionId, jobId, kind: "import"|"render"|"peaks"|"publish", sequence, phase: "queued"|"preparing"|"rendering"|"publishing"|"complete"|"cancelled"|"failed", fraction, terminal: JobTerminal|null }` · `JobTerminal { productId?: string, assetIds?: string[], perFile?: {name, error}[], error?: EditorError }` · `JobRecordDto { jobId, kind, phase, fraction, terminal }`
- `RenderRequest { sessionId, expectedRevision, name, range: {startMs, endMs}|null, quality: "low"|"balanced"|"high" }`
- `ProductDto { id, projectId, name, filename, mime, revision, durationMs, createdAt, editFingerprint, renderRange: {startMs,endMs}|null, available }`
- `PublishDestination { vaultId, folder, dated, createNote }` · `PublishReceipt { videoPath, notePath: string|null, vaultId, vaultName, warning: string|null }`
- `TakeDto { takeId, assetId, durationMs, width, height, hasAudio }`
- `CheckFinding { id, severity: "blocking"|"warning"|"info", code, message, target: {kind: "project"|"asset"|"track"|"clip"|"effect"|"caption"|"marker", id: string|null}, action: "reconnect"|"select"|"openCaptions"|"openLayout"|"openAudio"|"openWebcam"|"reviewCanvas"|"setDestination"|null }`
- `GuideProgress { contentRevision, currentStepId: string|null, reviewed: string[], explored: string[], invitationDismissed, active, collapsed, completed, preferences: { dimming: boolean, motion: "system"|"reduced"|"full" } }`

**Commands** (`EditorCommand`, `#[serde(tag = "kind", rename_all = "camelCase")]`, payload fields camelCase, all times integer ms; output-time unless named `source…`):
`rename{title}` · `undo` · `redo` · `insertClip{assetId, trackId, startMs, inMs, outMs}` · `updateClip{clipId, name}` · `splitClip{clipId, atMs}` · `trimClip{clipId, startMs, inMs, outMs}` · `deleteClips{clipIds, closeGap}` · `moveClips{clipIds, deltaMs, trackId: string|null}` · `reorderClip{clipId, direction: "earlier"|"later"}` · `groupClips{clipIds}` · `ungroupClips{groupId}` · `duplicateClips{clipIds, offsetMs}` · `pasteFragment{fragment, trackId, atMs}` · `cutClips{clipIds, closeGap}` · `addTrack{kind, name, index}` · `renameTrack{trackId, name}` · `moveTrack{trackId, toIndex}` · `setTrackFlags{trackId, visible?, locked?, muted?, solo?, volume?}` · `deleteTrack{trackId}` · `setClipMix{clipIds, volume?, muted?}` · `setMasterGain{gain}` · `detachAudio{clipId, audioTrackId: string|null}` · `setFades{clipId, fadeInMs?, fadeOutMs?, fadeCurve?}` · `addTransition{fromClipId, toClipId, durationMs, kind}` · `setTransitionDuration{transitionId, durationMs}` · `removeTransition{transitionId}` · `setSpeed{clipId, speed, preservePitch}` · `setLayout{clipIds, x?, y?, w?, h?, opacity?, fit?, frameShape?, rotation?, mirror?, flipY?, cropZoom?, cropX?, cropY?}` · `setAdjustments{clipIds, adjustments: Adjustments|null}` · `setCanvas{width, height}` · `addCard{preset, trackId: string|null, startMs, durationMs, title, subtitle}` · `updateCard{clipId, title?, subtitle?, background?, foreground?, accent?}` · `insertIntro{durationMs, title, subtitle}` · `addEffect{clipId, kind, startMs, endMs, props}` · `updateEffect{effectId, startMs?, endMs?, props?}` · `removeEffect{effectId}` · `setCaptionSettings{enabled?, burnIn?, fontSize?, position?, background?}` · `addCaption{clipId, startMs, endMs, text}` · `updateCaption{captionId, startMs?, endMs?, text?}` · `splitCaption{captionId, atMs}` · `removeCaptions{captionIds}` · `addMarker{clipId, sourceMs, title}` · `updateMarker{markerId, sourceMs?, title?}` · `removeMarker{markerId}` · `setDestination{vaultId, folder, dated}`.
Native-only (`InternalCommand`, never deserialized from IPC): `AddAssets{assets}`, `ImportCaptions{clipId, cues, replace}`, `RestoreSnapshot{productId, project}`, `RelinkAssets{assetIds}`. Effect/caption `startMs/endMs` and marker `sourceMs` are SOURCE time.

**IPC commands:** exactly the ADR §3.3 table (which includes the later additions `open_project_editor`, `editor_import_captions`, `editor_export_guide_progress`, `editor_import_guide_progress` and `list_capture_webcams`, and `take_editor_request`'s widened `{kind: "staged" | "project", value}` return from Task 37). **Events:** `editor:open` (existing) and `editor:closeRequested` (`emit_to("editor")`, payload `{}`, Task 37). Jobs never use events.

**On-disk layout** (R5): `%LOCALAPPDATA%\com.vaultbuddy.desktop\editor-projects\<projectId>\{project.json, sources.json, products.json, recovery.json, workspace.json, media\, takes\, products\, cache\, jobs\<jobId>\}` and `…\editor-prefs\guide-progress.json`. Sidecar pin key: `editorProjectId`.

## Package-to-task map

| Package | Tasks |
|---|---|
| P01 contracts | 1–6 |
| P04 domain core | 7–8 |
| P02 open staged work | 9–15 |
| P03 shell | 16–20 |
| P04 end to end | 21–22 (with 7, 8, 12) |
| P05 layers & audio | 23–28 |
| P06 fades/transforms/cards | 29–33 |
| P08 teaching/captions/chapters | 34–36 |
| P09 persistence | 37–40 |
| P10 render | 41–48 |
| P07 webcam/sync/stems | 49–53 |
| P12 checks/quality | 54, 58 |
| P11 learning | 55–57 |
| Retirement + P13 | 59–60 |

## Task 1: Editor model and interchange round trip

**Package:** P01 · **F-IDs:** foundation for all (F-40 format) · **Depends:** none · **Rulings:** R2, R3

**Files:**
- Create: `src-tauri/core/src/editor/mod.rs`, `ids.rs`, `error.rs`, `model.rs`, `model_cues.rs`
- Create: `tests/fixtures/editor/reference-workspace.example.json` (verbatim copy of `docs/concepts/vault-buddy-editor/contracts/reference-workspace.example.json`)
- Modify: `src-tauri/core/src/lib.rs` (`pub mod editor;`)

**Behavior:**
- `mod.rs`: schema constants, `pub mod limits` (Contract reference values), re-exports.
- `ids.rs`: `is_valid_id(&str) -> bool` (regex semantics without a regex crate: 1..=100 of `[A-Za-z0-9_-]`); `new_entity_id(prefix: &str) -> String` = `format!("{prefix}-{}", 10 base36 CSPRNG chars)` via `getrandom`; `new_project_id() -> String` = 16 base36 chars.
- `error.rs`: `EditorErrorCode` (15 variants, `#[serde(rename_all = "camelCase")]`), `EditorError { code, message, retryable, operation_id, retained_asset_ids: Option<Vec<String>> }` (`rename_all = "camelCase"`, `skip_serializing_if` on the option), constructors `EditorError::new(code, message)` (retryable per code: `revisionConflict`, `diskFull`, `destinationUnavailable`, `deviceUnavailable` true; others false; `operation_id` = `new_entity_id("op")`).
- `model.rs`: `Project { schema, id, title, canvas: Canvas, master_gain: f64, assets: Vec<Asset>, tracks: Vec<Track>, clips: Vec<Clip>, effects: Vec<Effect>, markers: Vec<Marker>, transitions: Vec<Transition>, captions: Option<CaptionSettings>, destination: Destination, #[serde(flatten)] extra: Map }`; `Canvas {width, height, fps, extra}`; `Asset {id, kind: AssetKind(video|audio), name, duration_ms: u64, width/height/size: Option, builtin: Option<Builtin>, media_type: Option<MediaType(image)>, linked_asset: Option<String>, original_name: Option<String>, extra}`; `Track {id, kind: TrackKind, name, visible, locked, muted, solo, volume: f64, extra}`; `Clip {` all schema fields, optional ones `Option`, `adjustments: Option<Adjustments>`, `card: Option<Card>`, `extra }`; `FadeCurve (linear|smooth|equal-power)`, `FrameShape`, `Fit`, `Rotation` (serialize as integer 0/90/180/270 via `#[serde(try_from = "u16", into = "u16")]`), `Destination {vault, folder, dated, extra}`.
- `model_cues.rs`: `Effect {id, clip_id, kind: EffectKind(text|arrow|highlight|spotlight|zoom|step|mask), start_ms, end_ms, x, y, color, text?, w?, h?, x2?, y2?, factor?, #[serde(rename = "fontSize")] font_size?, stroke?, dim?, easing?, number?, background?, extra}`; `CaptionSettings {enabled, burn_in, font_size, position(top|bottom), background, cues: Vec<CaptionCue>, extra}`; `CaptionCue`, `Marker`, `Transition {id, from, to, duration_ms, kind(dissolve|equal-power), extra}`, `Product {…schema fields…, snapshot: Option<Box<Project>>, render_range: Option<RenderRange>, extra}`, `Record`, `WorkspaceEnvelope {schema, project, workspace: serde_json::Value, record, saved_at, extra}`.
- Every optional field: `#[serde(default, skip_serializing_if = "Option::is_none")]`. Unknown enum variants fail deserialization (no `#[serde(other)]`).

**Tests first** (in `model.rs` / `model_cues.rs` `#[cfg(test)]`):
- `reference_example_round_trips_as_json_value` — `include_str!("../../../../tests/fixtures/editor/reference-workspace.example.json")` → `WorkspaceEnvelope` → `to_value` equals the original parsed `Value`.
- `unknown_fields_survive_a_round_trip` — add `"future_field": {"a":1}` to one clip, one effect and the project; all three present after round trip.
- `unknown_enum_variant_is_rejected` — effect `"kind":"blur"` → `Err`; track `"kind":"midi"` → `Err`.
- `rotation_accepts_only_quarter_turns` — 90 ok, 45 `Err`.
- `font_size_keeps_its_camel_case_spelling` — serialized effect contains `"fontSize"` and not `"font_size"`.
- `error_serializes_camel_case_literal` (`error.rs`) — literal JSON `{"code":"revisionConflict","message":"m","retryable":true,"operationId":"op-x"}` compared with a fixed `operation_id`.
- `ids`: `valid_ids_match_the_schema_pattern` (`"c1"`, `"a_b-C"` ok; `""`, 101 chars, `"a b"`, `"é"` rejected); `new_ids_are_valid_and_distinct` (1,000 draws, all valid, all distinct).

**Mutation check:** remove `extra` from `Clip` → `unknown_fields_survive_a_round_trip` red naming the clip; add `#[serde(other)]` to `EffectKind` → `unknown_enum_variant_is_rejected` red.

**Verify:** `cd src-tauri/core && cargo test editor:: && cargo clippy --all-targets -- -D warnings`, then all gates.

**Acceptance:** the reference example round-trips as a `Value`; no file over 800 lines; `cargo machete` clean.

**Commit:** `feat(core): add the tutorial editor's interchange model`

---

## Task 2: Semantic validation and the workspace sanitizer

**Package:** P01 · **F-IDs:** F-40, F-44 (validation before install) · **Depends:** 1 · **Rulings:** R3; bundle DATA-MODEL § Validation order

**Files:** Create `src-tauri/core/src/editor/validate.rs`, `validate_media.rs`, `workspace.rs`; modify `editor/mod.rs`.

**Behavior:**
- `pub fn validate_project(p: &Project) -> Result<(), EditorError>` (code `invalidProject`, message names the entity id and rule). Steps in order: schema string; collection sizes vs limits; every id valid and unique per collection; title ≤ 160 chars, names ≤ 300; canvas ∈ the four pairs, fps 30; `master_gain ∈ [0,1]`; asset `duration_ms ≤ MAX_DURATION_MS`; clip `asset_id`/`track_id` resolve; track kind compatible (audio-only asset → audio track; video/image → video track); `in_ms < out_ms ≤ asset.duration_ms` (images: `out_ms ≤ MAX_DURATION_MS`); `start_ms + output_duration ≤ MAX_DURATION_MS`; x,y ∈ [0,1], w,h ∈ [0.1,1]; speed ∈ [0.25,4]; crop_zoom ∈ [1,3]; fades each ≤ half clip output duration; effect `clip_id` resolves, `start_ms < end_ms`, kind-specific required props (arrow: x2,y2; zoom: factor; step: number; text/step: text); caption cues resolve and `start_ms < end_ms`; markers resolve and `source_ms` inside the clip's asset duration; transitions: `from != to`, both exist, same track, same kind, `from` ends where `to` starts (after overlap rule, Task 30 finalises), positive `duration_ms` ≤ min(half of each clip); at most one transition per clip side; `linked_asset` resolves and the link graph is acyclic (DFS with colour marks).
- `validate_media.rs` holds the asset/transition/link checks so `validate.rs` stays small.
- `pub fn validate_envelope(e: &WorkspaceEnvelope) -> Result<(), EditorError>`: workspace schema string, project, record (`revision ≥ 1`, products ≤ 40, product ids unique, each product `project_id == project.id`, each `snapshot` validated with `validate_project`).
- `workspace.rs`: `pub struct Workspace` (the 18 fields in ADR R16, all `Option`), `pub fn sanitize(v: &serde_json::Value) -> Workspace` — unknown keys dropped, wrong types dropped, numbers clamped (`timeline_zoom 0.1..=20`, `timeline_height 160..=900`, `playback_rate 0.25..=2`), `delete_mode ∈ {gap, close}` else `None`, selection ids filtered through `is_valid_id`, max 600.

**Tests first:**
- `validate.rs`: `reference_example_is_valid`; `duplicate_clip_ids_are_rejected_naming_the_id`; `dangling_asset_reference_is_rejected`; `audio_asset_on_video_track_is_rejected`; `empty_source_range_is_rejected` (`in_ms == out_ms`); `out_ms_beyond_asset_duration_is_rejected`; `fade_longer_than_half_the_clip_is_rejected` (clip 1000 ms, fade_in 501 → Err, 500 → Ok); `cyclic_linked_assets_are_rejected` (a→b→a); `self_transition_is_rejected`; `cross_track_transition_is_rejected`; `oversized_collection_is_rejected` (601 clips); `snapshot_is_validated_too` (a product whose snapshot has a dangling ref → Err).
- `workspace.rs`: `sanitize_drops_unknown_and_mistyped_fields`; `sanitize_clamps_zoom_and_height` (zoom 99 → 20, height 5 → 160); `sanitize_filters_invalid_selection_ids`.

**Mutation check:** skip the cycle check → `cyclic_linked_assets_are_rejected` red; change fade limit to `<` full duration → the 501 case red.

**Verify:** `cargo test -p vault_buddy_core editor::`; all gates.

**Acceptance:** every DATA-MODEL validation step 3–7 has a named test; failure messages name the offending id.

**Commit:** `feat(core): validate tutorial projects before they are installed`

---

## Task 3: Time mapping and the shared TS/Rust time fixture

**Package:** P01 · **F-IDs:** F-14, F-16 (mapping), F-07 (half-open) · **Depends:** 1 · **Rulings:** R4, R15

**Files:**
- Create: `src-tauri/core/src/editor/time.rs`, `tests/fixtures/editor-time-cases.json`, `src/editor/timeMap.ts`, `src/editorTypes.ts` (F10: the single TS declaration site — created here with only the entity types `time.rs`/`timeMap.ts` need; Task 8 and Task 13 extend it), `tests/editorTimeFixtures.test.ts`

**Behavior (identical in both languages):**
- `clip_output_duration(in, out, speed) = round_half_away((out - in) / speed)`.
- `clip_output_end(clip) = start + duration`; `clip_is_active(clip, t)` = `start ≤ t < end` (half-open).
- `source_at(clip, t) -> Option<u64>` = `in + round_half_away((t - start) * speed)`, clamped to `[in, out-1]`, `None` outside the active interval.
- `output_at(clip, source) -> Option<u64>` = `start + round_half_away((source - in) / speed)` for `source ∈ [in, out)`.
- `cue_output_span(clip, cue_start_src, cue_end_src) -> Option<(u64,u64)>` = intersection of `[cue_start, cue_end)` with `[in, out)`, mapped through `output_at` (end mapped as `start + round((min(end,out) - in)/speed)`), `None` when empty.
- `frame_timestamp(frame, num, den) -> Result<u64, &str>` (Rust only; the starter's `u128` formula).
- `project_duration(project) = max clip_output_end` over visible AND hidden tracks (hidden tracks still define length; 0 when no clips).

**Fixture:** `{"version":1,"cases":[{"name","clip":{"start_ms","in_ms","out_ms","speed"},"durationMs","sourceAt":[[t, s|null]...],"outputAt":[[s, t|null]...],"cueSpans":[[a,b,[x,y]|null]...]}]}` with **exactly 10 cases**: unit speed; speed 2; speed 0.5; speed 0.25 with odd range (in 3, out 1000); speed 4; speed 2 with a `.5` rounding boundary (in 0, out 5, → duration 2.5 → 3 — F8: the case must actually land on a `.5` so a banker's-rounding mutation goes red; speed 1.5/in 0/out 3 gives exactly 2.0 and does not exercise the rounding rule); a combined "start 0 vs start 1500" case (F8: counted as ONE case, not two); a cue fully inside; a cue straddling `out_ms` (clipped); a cue ending exactly at `in_ms` (empty → null). Each case lists `sourceAt` at `start`, `end-1`, `end`, and at `start-1` only when `start > 0` (F8: `start-1` is undefined for a clip starting at 0 and must be omitted there, not probed).

**Tests first:**
- Rust `time.rs`: `shared_fixture_table_has_ten_cases` (`assert_eq!(cases.len(), 10)`), `shared_fixture_table_source_and_output_mapping_agree`, `shared_fixture_table_cue_spans_agree`; `frame_timestamps_do_not_accumulate_rounding` (`(1,30,1)=333_333`, `(30,30,1)=10_000_000`, `(30_000,30_000,1001)=10_010_000_000`); `invalid_or_overflowing_rates_are_rejected`; `end_is_exclusive` (t = end → None).
- TS `tests/editorTimeFixtures.test.ts`: `it("the fixture table has 10 cases")`; `it.each(cases)("maps %s identically to Rust")` covering sourceAt/outputAt/cueSpans; `it("rounds half away from zero, not banker's")` (1.5→2, 2.5→3).

**Mutation check:** change TS `Math.round` to banker's rounding helper → the `.5` case red in TS only; make `clip_is_active` inclusive of `end` → `end_is_exclusive` and the `end` fixture rows red in BOTH languages.

**Verify:** `cargo test -p vault_buddy_core editor::time`; `npx vitest run tests/editorTimeFixtures.test.ts`; all gates.

**Acceptance:** both languages read the same file (moving it breaks the Rust build via `include_str!`); both assert the count 10.

**Commit:** `feat(editor): shared half-open time mapping held by one fixture table`

---

## Task 4: Migrating a staged capture into a tutorial project

**Package:** P01 · **F-IDs:** F-01 · **Depends:** 1, 2, 3 · **Rulings:** R2, R6, ADR §4

**Files:** Create `src-tauri/core/src/editor/migrate.rs`; modify `editor/mod.rs`. (Core cannot depend on `screen`; take plain inputs.)

**Behavior:**
- `pub struct StagedInput<'a> { base, vault_id, source_title, duration_ms, width, height, has_audio: bool, legacy_timeline: Option<&'a serde_json::Value>, stems: &'a [StemInput], webcam: Option<WebcamInput> }` (`StemInput { index: u32, input: String }`, `WebcamInput { duration_ms, width, height }` — the last two stay empty until Tasks 51/53).
- `pub fn from_staged(input: &StagedInput, project_id: &str) -> MigrationResult { project: Project, dropped_segments: u32 }` (F6: wraps `Project` rather than returning it bare, so a dropped backwards segment is visible to the caller and the test — see below): asset `src` (video, `name = source_title`, duration, dims); tracks `v1` "Screen" (video) and `a1` "Audio" (audio); legacy timeline parsed with the SAME degrade as `export_commands::timeline_from_sidecar` (move that parsing into `core::timeline::Timeline::from_sidecar_value(&Value, duration_ms) -> Timeline` in this task and make `export_commands` call it — one reader): absent/malformed → whole; explicit `[]` → zero clips (F6: a segment whose migrated `in_ms ≥ out_ms` — the `saturating_sub` backwards-segment case — is DROPPED, never installed as a zero/negative-length clip, and counted in `dropped_segments`). Each surviving segment → clip `c<n>` on `v1`, laid end to end from 0. Canvas = nearest supported by aspect (ADR §4). `destination = {vault: vault_id, folder: "", dated: false}` (folder resolved from vault config at publish time; empty = the vault's `screen_capture_root()`). `title = source_title` truncated to 160.
- `pub fn nearest_canvas(w, h) -> (u32, u32)` and `pub fn canvas_is_exact(w, h) -> bool` (F5: `canvas_is_exact` is true when the canvas and the source share ASPECT RATIO — not raw dimensions — so an untouched 1080p capture migrated onto its nearest `1280×720` canvas still reads as exact and Task 41's `is_identity` can remux it at source resolution instead of always re-encoding; equal dims would make the identity path unreachable for every real capture).
- `pub fn map_reference_destination(name_or_id: &str, vaults: &[(id, name)]) -> Option<String>` (R3: id match first, then unique name match, else `None`).

**Tests first:**
- `untouched_capture_becomes_one_whole_clip` (duration 12_345 → one clip `in 0, out 12_345, start 0`).
- `legacy_segments_become_consecutive_clips` — for EVERY case in `tests/fixtures/timeline-cases.json` (read via `include_str!`), the migrated project's duration equals `Timeline::output_duration_ms()` and, at each probe the fixture lists, `time::source_at` over the active clip equals `Timeline::to_source_ms`.
- `backwards_segment_is_dropped_and_counted` (F6: the fixture's "a backwards segment" case, 4000→3000 — `migrated_project_validates` would otherwise see `in_ms ≥ out_ms`; assert the clip is absent AND `dropped_segments == 1`).
- `explicitly_empty_timeline_migrates_to_no_clips` (the user deleted everything — never resurrected).
- `malformed_timeline_degrades_to_the_whole_capture` (`"timeline": "junk"`).
- `migrated_project_validates` (`validate_project` Ok for every fixture case).
- `vault_id_comes_from_the_input_not_a_default` (A01 core half).
- `nearest_canvas_picks_by_aspect` (1920×1080 → 1280×720; 1080×1920 → 720×1280; 1000×1000 → 720×720; 1024×768 → 960×720; 2560×1080 → 1280×720 and `canvas_is_exact == false`).
- `export_commands`: existing `timeline_from_sidecar` tests stay green unchanged (they now exercise the moved reader).

**Mutation check:** migrate an explicit `[]` as whole → `explicitly_empty_timeline_migrates_to_no_clips` red; lay segments by their SOURCE start instead of end-to-end → `legacy_segments_become_consecutive_clips` red on the reorder case; keep the backwards segment instead of dropping it → `backwards_segment_is_dropped_and_counted` red.

**Verify:** `cargo test -p vault_buddy_core editor::migrate`, `cargo test -p vault-buddy --lib export_commands`; all gates.

**Acceptance:** exactly one reader of the sidecar `timeline` field exists (`grep -rn "fn timeline_from_sidecar\|from_sidecar_value" src-tauri` shows the core fn and a thin shell call).

**Commit:** `feat(core): migrate staged captures into tutorial projects`

---

## Task 5: Canonical fingerprint and product lineage

**Package:** P01 · **F-IDs:** F-41 (lineage), F-13 (products untouched by undo) · **Depends:** 1, 2 · **Rulings:** R5

**Files:** Create `src-tauri/core/src/editor/fingerprint.rs`; modify `src-tauri/core/Cargo.toml` (`sha2 = "0.10"` — same version already locked by `transcribe`), `editor/mod.rs`.

**Behavior:**
- `pub fn canonical_json(p: &Project) -> String` — serialize to `Value`, recursively sort object keys, emit compact JSON; floats as serde_json prints them.
- `pub fn edit_fingerprint(p: &Project) -> String` = `"sha256:" + hex(sha256(canonical_json(p)))` — 71 chars (schema `maxLength 80`).
- `pub fn new_product(project: &Project, revision: u64, product_id: &str, name: &str, filename: &str, duration_ms: u64, range: Option<RenderRange>, created_at: &str) -> Product` — embeds `snapshot: Some(Box::new(project.clone()))`, mime `"video/mp4"`.
- `pub fn assets_referenced(project: &Project, products: &[Product]) -> BTreeSet<String>` — union over the current graph AND every product snapshot (A17), following `linked_asset`.

**Tests first:** `fingerprint_ignores_key_order` (two `Value`s with shuffled keys → same hash); `fingerprint_changes_when_a_clip_moves` (start_ms +1 → different); `fingerprint_has_the_schema_length` (≤ 80, prefix `sha256:`); `product_snapshot_is_independent_of_later_edits` (mutate the project after `new_product`, snapshot unchanged); `assets_referenced_includes_snapshot_only_assets` (asset removed from the current graph but present in a product snapshot → included); `assets_referenced_follows_linked_assets`.

**Mutation check:** drop the key sort → `fingerprint_ignores_key_order` red.

**Verify:** `cargo test -p vault_buddy_core editor::fingerprint`; `cargo deny check`; all gates.

**Commit:** `feat(core): fingerprint tutorial projects and track product lineage`

---

## Task 6: Session, history and the command envelope

**Package:** P01/P04 · **F-IDs:** F-13 · **Depends:** 1, 2, 5 · **Rulings:** R14

**Files:** Create `src-tauri/core/src/editor/session.rs`, `history.rs`, `commands/mod.rs`, `commands/meta.rs`, `commands/payloads.rs` (F31: the ~50 payload structs live here, split out of `commands/mod.rs` from the start so the dispatch/enum file does not grow toward the 800-line cap as later tasks add arms); modify `editor/mod.rs`.

**Behavior:**
- `commands/payloads.rs`: every `EditorCommand`/`InternalCommand` payload struct, including the two the wire needs before their owning tasks land (F9): `ClipboardFragment { clips, effects, captions, markers, origin_ms }` (Task 8 defines its Rust behaviour; the struct shape is fixed here) and the `EffectProps` enum (the seven-kind union Task 34 implements — `Text`, `Arrow`, `Highlight`, `Spotlight`, `Zoom`, `Step`, `Mask`, each with the field set Task 34's Behavior section lists). Both are `#[serde(deny_unknown_fields)]`-free at this stage (fields are added as later tasks need them) but the enum tags and struct names are FIXED here and never renamed.
- `commands/mod.rs`: `EditorCommand` enum with ALL variants from the Contract reference, payload types from `commands/payloads.rs` (variants not yet implemented return `EditorError{code: invalidRequest, message: "<kind> is not available yet"}` from `apply` — each later task replaces its arm and removes its row from the `unimplemented_kinds_are_invalid_request_not_panic` table test, so the table shrinks monotonically task by task rather than being re-verified as a whole at the end; say so explicitly in the task report of every task that removes a row). `InternalCommand` enum (not `Deserialize`). `pub fn apply(project: &Project, cmd: &EditorCommand) -> Result<(Project, String /* undo label */), EditorError>` dispatching to family modules; `pub fn apply_internal(...)`.
- `commands/meta.rs`: `rename` (trim, 1..=160 chars), `setDestination` (vault id non-empty, folder passes `capture_paths::safe_recording_root`'s lexical rule — reuse its validator on a dummy root), `AddAssets` (ids unique, count limit).
- `history.rs`: `History { undo: VecDeque<(Project, String)>, redo: Vec<(Project, String)> }`, `push(prev, label)` (evict oldest beyond `MAX_HISTORY`, clear redo), `undo(current) -> Option<(Project, String)>`, `redo(current)`.
- `session.rs`: `EditorSession { session_id, project, revision: u64, persisted_revision: Option<u64>, history, recent: VecDeque<(String /*commandId*/, u64 /*revision*/)> }`; `pub fn execute(&mut self, req: &ExecuteRequest) -> Result<EditorSnapshot, EditorError>`: wrong `session_id` → `sessionGone`; `command_id` invalid → `invalidRequest`; replayed `command_id` → current snapshot, no change; `expected_revision != revision` → `revisionConflict`; `undo`/`redo` via history; otherwise `apply` → `validate_project(candidate)` → install, `revision += 1`, push history. Every success advances the revision (undo too). `snapshot()` builds `EditorSnapshot` (duration via `time::project_duration`, labels from history). `mark_saved(r)` sets `persisted_revision = Some(r)` only if `r <= revision`. `execute_internal` for `InternalCommand`s (same queue rules, no `commandId`).
- `ExecuteRequest`, `EditorSnapshot` structs with camelCase + `deny_unknown_fields`, as in the Contract reference.

**Tests first:** `rename_advances_the_revision_and_is_undoable`; `stale_expected_revision_is_a_conflict_and_changes_nothing` (project bytes equal before/after); `replayed_command_id_is_not_applied_twice` (two identical renames with the same id → revision +1 once); `wrong_session_is_session_gone`; `undo_advances_the_revision` (rename r1→r2, undo → r3, title restored); `redo_after_new_edit_is_cleared`; `history_is_bounded` (101 renames → 100 undos possible); `invalid_candidate_is_rejected_atomically` (a command producing an invalid project leaves project + revision unchanged — use `rename` to a 161-char title); `unimplemented_kinds_are_invalid_request_not_panic` (each variant's arm, one table test); `execute_request_wire_literal` (literal JSON `{"sessionId":"s","expectedRevision":1,"commandId":"c1","command":{"kind":"splitClip","clipId":"c","atMs":1200}}` deserializes; an extra key is rejected); `snapshot_wire_literal`.

**Mutation check:** skip the revision comparison → the conflict test red; install before validating → the atomic test red.

**Verify:** `cargo test -p vault_buddy_core editor::`; all gates.

**Commit:** `feat(core): serial editor sessions with bounded undo`

---

## Task 7: Clip commands — insert, split, trim, delete/ripple, move, reorder

**Package:** P04 · **F-IDs:** F-07, F-08, F-09, F-10 · **Depends:** 3, 6 · **Bundle:** DATA-MODEL § Timing rules; A03, A11

**Files:** Create `src-tauri/core/src/editor/commands/clips.rs` (+ `commands/cue_follow.rs` for cue reassignment if `clips.rs` nears 700 lines); modify `commands/mod.rs`.

**Behavior:**
- Shared guard `ensure_unlocked(project, track_id)` → `invalidRequest` "Track <name> is locked" (used by EVERY clip-mutating command in all later tasks).
- `insertClip`: mints `clip-…` id, defaults (fades 0, curve `linear`, opacity 1, volume 1, muted false, `x 0, y 0, w 1, h 1`), refuses an overlap with another clip on the same track.
- `splitClip{clipId, atMs}`: `atMs` strictly inside the clip's output interval (not at start/end) else `invalidRequest` "Split point is at a clip boundary"; source split point `s = source_at(clip, atMs)`; left `[in, s)`, right `[s, out)`, right `start = atMs`; fades: left keeps fade_in, right keeps fade_out; transitions re-pointed (`from` → right half, `to` → left half as appropriate). Effects/captions: a cue entirely in one half moves to that half's clip id; a cue straddling `s` is split into two cues (`[a, s)` on left, `[s, b)` on right, the right one gets a fresh id). Markers go to the half whose half-open source range contains `source_ms` (exactly one).
- `trimClip{clipId, startMs, inMs, outMs}`: `in < out`, within asset, no overlap; cue times untouched (source-linked).
- `deleteClips{clipIds, closeGap}`: removes clips and their effects/captions/markers/transitions; `closeGap` shifts LATER clips on the SAME track only (track-local ripple) by the removed span; refused if a later clip on that track is grouped with a clip on another track ("Ripple would break group <id>") or a transition spans the gap.
- `moveClips{clipIds, deltaMs, trackId}`: one shared delta; resulting start clamped at 0 by clamping the DELTA for the whole group (never per clip); overlap on any destination → reject all; `trackId` only when `clipIds.len() == 1` (else `invalidRequest`); kind-compatible.
- `reorderClip{clipId, direction}`: swaps with the adjacent clip on the same track (by start), preserving the pair's combined span; no-op neighbour → `invalidRequest` "Already first/last".
- `updateClip{clipId, name}`.

**Tests first** (fixtures use speed 2 on at least one clip and asymmetric cues):
- `split_conserves_total_duration`; `split_at_boundary_is_refused`; `split_moves_whole_cues_and_splits_straddling_cues`; `split_assigns_a_marker_on_the_cut_to_exactly_one_half` (marker at `s` → right half only); `split_at_non_unit_speed_maps_the_source_point` (speed 2, atMs start+500 → source split in+1000); `trim_keeps_source_cue_times`; `trim_to_empty_is_refused`; `delete_leave_gap_moves_nothing`; `delete_close_gap_ripples_only_its_track` (clips on `a1` unchanged); `ripple_through_a_group_is_refused_atomically`; `move_group_clamps_the_shared_delta` (clips at 100 and 500, delta −300 → 0 and 400); `move_into_overlap_rejects_the_whole_group`; `locked_track_refuses_split_trim_delete_move` (table test, four commands); `reorder_swaps_adjacent_clips_keeping_span`; `split_save_reopen_round_trip_keeps_cue_intervals` (serialize → deserialize → equal).

**Mutation check:** clamp per clip instead of the shared delta → the clamp test red; ripple all tracks → `delete_close_gap_ripples_only_its_track` red; inclusive marker test at `s` → marker test red.

**Verify:** `cargo test -p vault_buddy_core editor::commands`; all gates.

**Commit:** `feat(core): split, trim, delete, move and reorder clips with source-linked cues`

---

## Task 8: Groups, duplicate and paste

**Package:** P04 · **F-IDs:** F-11, F-12 · **Depends:** 7 · **Bundle:** A04

**Files:** Create `src-tauri/core/src/editor/commands/groups.rs`; create `src/editor/fragment.ts` + `tests/editorFragment.test.ts` (read-side fragment builder); modify `src-tauri/core/src/editor/commands/clips.rs` (F13: `moveClips`'s group expansion, below), `src/editorTypes.ts` (F10: extend the entity types Task 3 created with the group/clipboard shapes this task's Rust and TS need).

**Behavior:**
- `groupClips{clipIds}` (≥ 2 clips, sets a fresh `group_id`), `ungroupClips{groupId}`.
- `moveClips` (Task 7, in `commands/clips.rs`) expands a selection to whole groups before computing the shared delta (grouped partners move too); any locked member → reject all — this expansion is implemented and tested in `clips.rs` in THIS task, not Task 7 (F13: Task 7 ships `moveClips` before groups exist).
- `ClipboardFragment { clips: Vec<Clip>, effects, captions: Vec<CaptionCue>, markers, origin_ms: u64 }` (serde camelCase envelope, entities in document spelling).
- `duplicateClips{clipIds, offsetMs}` and `pasteFragment{fragment, trackId, atMs}`: every clip/effect/cue/marker gets a fresh id; clip→cue references re-pointed to the new clip ids; group ids re-minted consistently; relative offsets preserved; target must be unlocked and overlap-free; fragment assets must already exist in the project (else `sourceMissing`). `cutClips` = fragment-free delete (the UI copies first) with label "Cut".
- `fragment.ts`: `buildFragment(project, clipIds): ClipboardFragment` — copies the clips and their attached cues/markers, `originMs` = min start.

**Tests first:** Rust — `group_move_preserves_relative_offsets`; `group_move_with_a_locked_member_changes_nothing` (A04); `duplicate_mints_fresh_ids_everywhere` (no id collides with any existing id across all collections); `paste_repoints_cues_to_new_clips`; `paste_of_unknown_asset_is_source_missing`; `undo_of_paste_restores_the_whole_operation`. TS — `buildFragment copies attached cues and markers only`; `buildFragment originMs is the earliest start`.

**Mutation check:** reuse the source clip's id for one pasted clip → the fresh-id test red.

**Verify:** `cargo test -p vault_buddy_core editor::commands::groups`; `npx vitest run tests/editorFragment.test.ts`; all gates.

**Commit:** `feat(core): group, duplicate and paste clips atomically`

---

## Task 9: The project store on disk and the staging pin

**Package:** P02/P09 · **F-IDs:** F-01, F-40, F-44 · **Depends:** 4, 6 · **Rulings:** R5, R6

**Files:**
- Create: `src-tauri/src/editor/mod.rs` (module decl only + `EditorState` placeholder, `#[allow(dead_code)]` per F1), `src-tauri/src/editor/project_store.rs`, `src-tauri/src/editor/store_io.rs`
- Modify: `src-tauri/src/lib.rs` (`mod editor;`), `src-tauri/src/staged_commands.rs` (`discard_conflict` gains the pin), `src-tauri/src/staging_commands.rs` (clear skips pinned), `src-tauri/src/staged_commands.rs`'s `StagedCaptureSummaryDto` (+`projectId: Option<String>`), `src-tauri/src/export_worker/mod.rs` (F2: `remove_staged_capture` skips a pinned base), `src/screenTypes.ts`, `src/components/StagedCaptureList.vue` (F7 partial: never render Edit for a recovered row), `tests/stagedCaptureList.test.ts`

**Behavior:**
- `project_store.rs` (pure paths + ownership, tempdir-testable): `STORE_DIR = "editor-projects"`, `project_dir(root, id)` (refuses an invalid id), `SourceLocator` enum `{ Staging{base}, Media{file}, Takes{file}, Builtin }` + `SourceRecord { locator, sha256: Option<String>, size: u64, duration_ms, width, height, has_audio, has_video, media_kind }`, `resolve_source(root_local_app_data, project_id, &SourceRecord) -> Option<PathBuf>` (staging via `staging::staging_dir` + `mp4_file_name`; media/takes joined and parent-checked like `write_sidecar`).
- `store_io.rs`: `create_project(root, project: &Project, sources: &BTreeMap<String, SourceRecord>) -> io::Result<()>` (create dir exclusively — `create_dir` not `_all` for the leaf; write `sources.json` then `project.json` via `write_atomic_replacing`), `load_project(root, id) -> Result<(WorkspaceEnvelope, sources), EditorError>` (bounded read ≤ `MAX_PROJECT_JSON_BYTES`, `validate_envelope`, malformed → `invalidProject` and the file is NEVER rewritten), `commit_project(root, id, &WorkspaceEnvelope)`, `list_projects(root) -> Vec<ProjectSummaryDto>` (degrading), `remove_project(root, id)` (only if `project.json` parses and its `project.id == id`; walks with `symlink_metadata`, refuses symlinks, removes files then empty dirs deepest first).
- Pin: `pin_staged(staging_dir, base, project_id)` / `unpin_staged(staging_dir, base, project_id)` read-modify-write the sidecar's `extra["editorProjectId"]` via `staging::write_sidecar`; `pinned_project(&StagedSidecar) -> Option<String>`.
- `discard_conflict(exporting, base)` becomes `discard_conflict(exporting, base, pinned: Option<&str>)`: pinned → `"This capture is used by a tutorial project. Discard the project first."`. `clear_staged_captures` skips pinned bases and reports them in `ClearStagedResultDto.skippedPinned: u32`.
- F2 (a pinned capture must survive the legacy phase-4 Save while both paths coexist through Task 59): `export_worker`'s `remove_staged_capture` — which deletes the staged capture LAST, after the video and note commit — skips a base that is pinned (`pinned_project` returns `Some`) and logs why; the legacy export still succeeds and the pin (and the tutorial project it points at) survive it.
- F1: `src-tauri/src/editor/mod.rs`'s `mod editor;` declaration carries a temporary `#[allow(dead_code)]` with a comment naming Task 10 as the removal point — at this commit the new modules are exercised only by their own `#[cfg(test)]` code, and workspace `clippy -D warnings` would otherwise fail on a lib crate with no external caller yet.
- F7 (partial; Task 10 does the refusal): `StagedCaptureList.vue`'s pinned-row rendering never offers **Edit** for a recovered sidecar (`sidecar.recovered == true` — the existing `recovered`/`duration_ms == 0` case from the phase-5 recovery sweep) even if such a row somehow carries a pin; Task 10 is the enforcement point that stops one from ever being created.
- `StagedCaptureList.vue`: a pinned row shows "In a tutorial project" and **Edit** (opens the editor as before, except for a recovered row per the bullet above), no Discard button.

**Tests first:**
- Rust: `create_then_load_round_trips`; `load_refuses_an_oversized_file`; `malformed_project_is_reported_and_left_byte_identical` (A27); `remove_project_refuses_a_directory_whose_project_id_differs`; `remove_project_never_follows_a_symlink` (Unix-only symlink; on Windows skip visibly without the dev-mode privilege, the `981bf67` pattern); `resolve_source_refuses_escaping_file_names` (`"../x"`, `"C:x"`); `pin_round_trips_through_the_sidecar_extra`; `pinned_capture_cannot_be_discarded`; `clear_skips_pinned_captures_and_counts_them`; `legacy_export_of_a_pinned_capture_keeps_the_project_and_still_saves` (F2: the video/note commit succeeds, the staged capture and its pin remain).
- TS: `a pinned staged capture offers Edit but not Discard`; `a pinned recovered row offers neither Edit nor Discard` (F7 partial); existing `stagedCaptureList` tests stay green.

**Mutation check:** drop the pin arm in `discard_conflict` → the discard test red; `remove_dir_all` in `remove_project` → symlink test red (Unix); remove the pinned skip from `remove_staged_capture` → `legacy_export_of_a_pinned_capture_keeps_the_project_and_still_saves` red.

**Verify:** `cargo test -p vault-buddy --lib editor:: staged_commands staging_commands`; `npx vitest run tests/stagedCaptureList.test.ts`; all gates (on this host include shell clippy).

**Acceptance:** no project write uses `std::fs::write`; no staging file is ever deleted by store code.

**Commit:** `feat(editor): an owned project store outside every vault, and pin-aware staging`

---

## Task 10: Editor session commands and caller authorization

**Package:** P02 · **F-IDs:** F-01, F-13 · **Depends:** 9 · **Rulings:** R6, R8, R14 · **Bundle:** A01

**Files:**
- Create: `src-tauri/src/editor/authz.rs`, `src-tauri/src/editor/session_commands.rs`, `src-tauri/src/editor/authz_guard.rs` (`#[cfg(test)]` structural scan)
- Modify: `src-tauri/src/editor/mod.rs` (`EditorState { sessions: Mutex<HashMap<String, EditorSession>>, by_project: Mutex<HashMap<String, String>> }`, managed in `lib.rs` setup), `src-tauri/src/lib.rs` (`generate_handler!` + `.manage`), `AGENTS.md` (IPC table rows + measured count)

**Behavior:**
- `authz::require_editor_window(window: &WebviewWindow) -> Result<(), EditorError>`: label `"editor"` else `unauthorizedSource`. `require_session(state, session_id) -> Result<MutexGuard…>` → `sessionGone`.
- `editor_open_staged(window, app, staged_base)`: `is_safe_base` → read sidecar from `staged_commands::staging_dir_for` (missing → `sourceMissing`); a `recovered: true` sidecar or one whose `duration_ms == 0` is refused BEFORE any create/pin step, `invalidRequest` "This recording's original data is gone — its length is unknown, so it cannot be edited. You can still discard it." (F7: a recovered capture migrated as-is would become an empty, permanently-pinned project no one could discard); pinned + project exists → load it (idempotent; A duplicated open returns the SAME `projectId`, and an existing live session for that project is reused); else `migrate::from_staged` (`has_audio` from `sidecar.inputs` non-empty), `create_project`, THEN `pin_staged`. Registers a session (`session_id = new_entity_id("ses")`). Returns `EditorOpenResult` (`workspace` = sanitized `{}`, `missing` = sources whose `resolve_source` file is absent, `sourceBase = Some(base)`).
- `editor_get_snapshot(window, session_id, known_revision)` → `EditorProjection`.
- `editor_execute(window, request)` → `EditorProjection` (runs on `spawn_blocking`; the session mutex is held only for the in-memory apply).
- `editor_close_session(window, session_id, disposition)`: `keep` drops the session; `discardProject` removes the project dir and `unpin_staged` for a staging source; `discardRecovery` handled in Task 37 (returns `invalidRequest` until then).
- `editor_hide_window(window)` (sync): hides the editor.
- `authz_guard.rs`: scans EVERY `.rs` file under `src-tauri/src/editor/` (whatever its name); every `#[tauri::command]` fn body must contain `require_editor_window(&window)` before any other statement that isn't a `let` binding of args, and the fn must take `window: WebviewWindow` — the failure names the fn.

**Tests first:**
- `open_staged_resolves_the_vault_from_the_sidecar` (A01: sidecar vault `"vaultA"`, returned `project.destination.vault == "vaultA"`) — test the pure `open_staged_in(root, staging_dir, base)` helper the command wraps.
- `open_staged_twice_returns_the_same_project` (idempotent, one directory under `editor-projects`).
- `open_staged_crash_between_create_and_pin_is_adopted_on_reopen` (create project, skip pin, reopen → still one project because the helper scans for a project whose `sources.json` has `Staging{base}` before minting).
- `open_staged_refuses_an_unsafe_base` (`"../x"` → `invalidRequest`).
- `open_staged_refuses_a_recovered_or_zero_duration_sidecar` (F7: `recovered: true` and, separately, `duration_ms: 0` → `invalidRequest`, no project directory created, nothing pinned).
- `every_editor_command_checks_the_caller_window` (authz_guard scan; mutate one command during the mutation check).
- `require_editor_window_refuses_other_labels` (unit on the label predicate `is_editor_label`).

**Mutation check:** remove `require_editor_window` from `editor_execute` → the scan test red naming `editor_execute`; mint a new project on every open → idempotency test red; drop the recovered/zero-duration refusal → `open_staged_refuses_a_recovered_or_zero_duration_sidecar` red.

**Verify:** `cargo test -p vault-buddy --lib editor::`; `cargo clippy -p vault-buddy --all-targets -- -D warnings`; re-measure the command count; all gates.

**Commit:** `feat(editor): open staged captures into authorized editor sessions`

---

## Task 11: App-command permission scoping

**Package:** P02 · **F-IDs:** F-50 (access boundary) · **Depends:** 10 · **Rulings:** R8

**Files:** Modify `src-tauri/build.rs`, `src-tauri/capabilities/default.json` (description text only); create `src-tauri/capabilities/editor.json`; create `src-tauri/src/editor/capability_guard.rs` (`#[cfg(test)]`); modify `docs/Gaps.md` (GAP-N5: runtime enforcement verified by build only).

**Behavior:**
- `build.rs`: `tauri_build::try_build(tauri_build::Attributes::new().app_manifest(tauri_build::AppManifest::new().commands(EDITOR_COMMANDS)))` where `EDITOR_COMMANDS` is a `const &[&str]` listing every `editor_*` command registered so far (later tasks append).
- `capabilities/editor.json`: `{"identifier":"editor","windows":["editor"],"permissions":["allow-editor-open-staged", …one per command…, "dialog:allow-open", "dialog:allow-save"]}`.
- Non-editor commands stay un-scoped (not in the manifest), so every existing window keeps working unchanged.
- `capability_guard.rs` test parses `build.rs` (the const list), `editor.json` and `lib.rs`'s `generate_handler!` block: every registered `editor_*` command is in the manifest list AND granted in `editor.json`, and `default.json` grants none of them.

**Tests first:** `every_editor_command_is_scoped_and_granted_only_to_the_editor_window`; `default_capability_grants_no_editor_command`.

**Mutation check:** drop one command from `editor.json` → the test names it.

**Verify:** `npx tauri build --no-bundle` on this host MUST succeed (this is the real check of layer 1 — record the output in the task report); `cargo test -p vault-buddy --lib editor::capability_guard`; all gates. If the Tauri release's manifest API differs, stop and report rather than improvising (the native layer from Task 10 stays in force regardless).

**Commit:** `feat(editor): scope editor commands to the editor window's capability`

---

## Task 12: Save project, list projects and reopen

**Package:** P04/P09 · **F-IDs:** F-40, F-44 · **Depends:** 10 · **Bundle:** A03, A15, A18, A20

**Files:** Create `src-tauri/src/editor/save_commands.rs`; modify `editor/mod.rs`, `lib.rs`, `build.rs` + `capabilities/editor.json` (new commands), `AGENTS.md`.

**Behavior:**
- `editor_save_project(window, session_id, expected_revision)`: session revision must equal `expected_revision` (else `revisionConflict`); builds `WorkspaceEnvelope { schema, project, workspace (last saved workspace or `{}`), record {id: project.id, revision, created_at (preserved), updated_at: now, products: [] (Task 46 fills)}, saved_at }`; `commit_project` on `spawn_blocking`; maps `ErrorKind::StorageFull`/raw OS 112 → `diskFull`, `PermissionDenied` → `writeDenied`; on success `mark_saved(r)` and returns `SaveReceipt`. Failure leaves the previous `project.json` byte-identical.
- `editor_list_projects(window)` → `list_projects` (sorted by `updatedAt` desc).
- `editor_open_project(window, project_file_id, use_recovery)` → load + session; `use_recovery` handled in Task 37 (`false` only for now; `true` → `invalidRequest`).
- A `ProjectWriter` trait seam (`fn write(&self, path, content) -> io::Result<()>`) in `store_io` so tests can inject failure.

**Tests first:** `save_commits_the_acknowledged_revision`; `save_with_a_stale_revision_is_a_conflict_and_writes_nothing`; `injected_write_failure_keeps_the_last_good_file` (A20: pre-existing `project.json` bytes unchanged, session `persisted_revision` unchanged); `disk_full_maps_to_disk_full` (inject `io::Error::from_raw_os_error(112)` on Windows / `StorageFull` elsewhere); `reopen_after_save_restores_clips_and_cues` (A03: split + trim + save + close + open → equal project; staged source file SHA-256 unchanged); `edit_after_save_stays_dirty` (save r2, edit → snapshot `persistedRevision 2, revision 3`); `save_receipt_wire_literal`.

**Mutation check:** call `mark_saved` before the write → the injected-failure test red.

**Verify:** `cargo test -p vault-buddy --lib editor::save_commands`; `npx tauri build --no-bundle`; all gates.

**Commit:** `feat(editor): save tutorial projects durably without rendering`

---

## Task 13: Frontend DTOs, runtime decoders and the editor port

**Package:** P02 · **F-IDs:** foundation (F-01, F-40) · **Depends:** 10, 12 · **Rulings:** R3

**Files:** Modify `src/editorTypes.ts` (F10: created in Task 3 with the entity types `timeMap.ts` needs and extended in Task 8 with the group/clipboard shapes — this task adds every remaining Contract-reference envelope and the full `EditorCommand` union); create `src/editor/editorCommandTypes.ts` (F31: the ~50-variant `EditorCommand` discriminated union and its payload types split out of `editorTypes.ts` here from the start, re-exported from `editorTypes.ts`, so the envelope/entity file does not carry the union's growth toward the 500-line cap), `src/editor/decode.ts`, `src/editor/port.ts`, `src/editor/listenerScope.ts`, `tests/editorDecode.test.ts`, `tests/editorPort.test.ts`.

**Behavior:**
- `editorTypes.ts` (extended, not created — see Files above): every Contract-reference envelope + the document entities (snake_case fields), re-exporting `editorCommandTypes.ts`'s `EditorCommand` discriminated union (all variants) so `editorTypes.ts` stays the one import site callers use.
- `decode.ts`: `ProtocolError`; `decodeSnapshot`, `decodeProjection`, `decodeOpenResult`, `decodeSaveReceipt`, `decodeEditorError` (`isEditorError(e)` for rejected invokes), `decodeProject` (structural checks: arrays, id pattern, integer ms ≤ 7_200_000, enum members), `decodeJobProgress` (phase set, `0 ≤ fraction ≤ 1`, integer `sequence`), `decodeProjectSummaries`. The starter's decoders are the model; unknown enum members throw.
- `port.ts`: `interface EditorPort { openStaged(base); openProject(id, useRecovery); listProjects(); getSnapshot(sessionId, knownRevision); execute(req); save(sessionId, expectedRevision); closeSession(sessionId, disposition); hideWindow() }` and `createTauriEditorPort(): EditorPort` — the ONLY file calling `invoke` for `editor_*` (a lint-style Vitest scan asserts no other `src/` file contains `invoke("editor_`). Each method decodes its response; rejections are converted with `decodeEditorError` into a thrown `EditorPortError { error: EditorError }`.
- `listenerScope.ts`: the starter's unmount-safe listener scope (an async registration completing after dispose is immediately un-listened).

**Tests first:** `decodeSnapshot rejects persistedRevision greater than revision`; `decodeSnapshot rejects unsafe integers` (`2**53`); `decodeProject rejects an unknown effect kind`; `decodeJobProgress rejects fraction 1.01 and phase "done"`; `decodeEditorError accepts the Rust literal` (paste the Task 1 literal); `port.execute sends the exact invoke payload` (mockIPC captures `editor_execute` with `{ request: {...} }`); `port converts a rejected invoke into EditorPortError`; `only port.ts invokes editor commands` (source scan over `src/**`); `listenerScope unlistens a registration that resolves after dispose`.

**Mutation check:** drop the `persistedRevision > revision` check → its test red.

**Verify:** `npx vitest run tests/editorDecode.test.ts tests/editorPort.test.ts`; all gates.

**Commit:** `feat(editor): typed editor port with runtime-decoded responses`

---

## Task 14: The editorProject store with generation and receipt guards

**Package:** P03 · **F-IDs:** F-13, F-40, F-44 · **Depends:** 13 · **Bundle:** A18; ARCHITECTURE § Pinia boundaries

**Files:** Create `src/stores/editorProject.ts`, `tests/editorProjectStore.test.ts`.

**Behavior:** options store (repo style). State: `sessionId`, `generation` (bumped on every open/close), `snapshot`, `project` (`shallowRef`-style replacement, never deep-mutated), `missing`, `sourceBase`, `pending: Set<commandId>`, `lastError`, `conflictIntent: EditorCommand|null`. Getters: `dirty` (`persistedRevision !== revision`), `canUndo`, `canRedo`, `clipById`, `trackById`, `durationMs`. Actions: `openStaged(base)`, `openProject(id, useRecovery)`, `execute(command)` (mints `commandId` with `crypto.randomUUID()`-free base36 helper; captures `generation`; on resolve installs only if generation unchanged AND `result.snapshot.sessionId === sessionId` AND `revision > current`; on `revisionConflict` refetches via `getSnapshot` and stores the command in `conflictIntent` for an explicit Retry — never re-sends automatically), `save()` (installs `persistedRevision` only from a receipt whose `sessionId` matches and `savedRevision ≤ revision`), `close(disposition)`. The port is injected (`setPort`) so tests use a fake.

**Tests first:** `a result from a previous session is ignored` (open A, execute slow, open B, resolve A's → store still B); `a non-increasing revision is ignored`; `revisionConflict refetches and keeps the intent for retry`; `a stale save receipt does not clear dirty` (A18: receipt for r2 while at r3 → `dirty` stays true); `a receipt from another session is ignored`; `dirty is derived, not stored` (no `dirty` in state); `rejection keeps the current project and surfaces lastError`.

**Mutation check:** remove the generation comparison → the previous-session test red.

**Verify:** `npx vitest run tests/editorProjectStore.test.ts`; all gates.

**Commit:** `feat(editor): editorProject store that only installs acknowledged revisions`

---

## Task 15: EditorRoot opens staged work in the new session

**Package:** P02 · **F-IDs:** F-01 · **Depends:** 14 · **Bundle:** A01, A02

**Files:**
- Modify: `src/roots/EditorRoot.vue` (drain `take_editor_request` → `editorProject.openStaged(base)`; mount a temporary `EditorShellPlaceholder` showing title/duration/dirty + the legacy phase-4 editor behind a feature switch until Task 21 — keep the phase-4 path working), `src/main.ts` only if the editor root needs Pinia (it already mounts Pinia for every window — verify), `src-tauri/tauri.conf.json` (editor `width 1280, height 820, minWidth 960, minHeight 640`), `tests/editorRoot.test.ts`, `tests/e2e/editorLayout.spec.ts` (viewport set: 960×640, 1280×820, 1600×1000, 1920×1080), `src/stores/screenCapture.ts` (no change expected — add test only)
- Create: `src/components/editor/LegacyCaptureEditor.vue` (F31: the phase-4 timeline/preview/export UI extracted into its own component behind the feature switch, so `EditorRoot.vue` does not carry both the legacy surface and the new session-opening logic toward its 500-line cap; F3: this component keeps calling `load_staged_capture` for its own detail — see Behavior below), `docs/superpowers/specs/2026-09-21-tutorial-editor-windows-verification.md` (header modeled on the screen-capture checklist, including the re-measure one-liner; rows T1–T3: open from capture bar; open from staged list; second open of the same capture focuses the same project), `tests/screenCaptureEditHandoff.test.ts`

**Behavior:** `editor:open` and mount both drain the stash; a second `editor:open` for the SAME base while that session is open does not create a second session (store-level: `openStaged` short-circuits when `sourceBase === base` and a session is live). The phase-4 `load_staged_capture` command is NOT removed in this task (F3): `EditorRoot.vue` no longer calls it directly, but `LegacyCaptureEditor.vue` — the feature-switch fallback rendered while a projection-backed path doesn't yet exist — keeps calling it for its `assetPath` and other detail, because a projection carries no resolvable asset path until Task 22's `editor_media_url` lands. The new `editorProject.openStaged(base)` path runs alongside it unconditionally (both open a session so the store/pin/recovery invariants are exercised from this task on); only the LEGACY PREVIEW keeps reading `load_staged_capture` until Task 59 deletes `LegacyCaptureEditor.vue` and removes the command.

**Tests first:** `EditorRoot opens the stashed base through editor_open_staged`; `a second editor:open for the same base reuses the session`; `EditorRoot never reads a vault id from the screenCapture store` (mock store with vault B; opened project reports sidecar vault A); `stillSaving does not expose Edit` (`screenCapture` store: after `stop()` returns `{stillSaving:true}` and no `screen:stopped`, `lastStaged === null` — A02); Playwright: `the editor fits at 960x640 without horizontal scroll` (measure `main`'s `scrollWidth <= clientWidth`, per the e2e rules in AGENTS.md).

**Mutation check:** drop the same-base short-circuit → reuse test red.

**Verify:** `npx vitest run tests/editorRoot.test.ts tests/screenCaptureEditHandoff.test.ts`; `npm run build && npm run test:e2e`; all gates.

**Acceptance:** the capture → editor path works end to end in `npm run test-build` (manual smoke on this host, recorded in the checklist row T1 result column only if actually run).

**Commit:** `feat(editor): open staged captures into tutorial project sessions`

---

## Task 16: Editor tokens and the responsive shell layout

**Package:** P03 · **F-IDs:** F-48 · **Depends:** 15 · **Bundle:** SCREENS 02, 12; DESIGN-SYSTEM.md; `contracts/design-tokens.json` · **Bundle:** A28

**Files:** Modify `src/style.css` (`@theme` additions); create `src/components/editor/shell/EditorShell.vue`, `EditorHeader.vue`, `tests/editorShell.test.ts`, `tests/e2e/editorShell.spec.ts`; modify `src/roots/EditorRoot.vue` (mount `EditorShell` for the new path).

**Behavior:**
- Tokens (semantic names, mapped from design-tokens.json; dark default + `[data-theme="light"]` overrides): `--color-stage`, `--color-panel`, `--color-raised`, `--color-line`, `--color-video`, `--color-video-bg`, `--color-audio`, `--color-audio-bg`, `--color-gold`, `--color-gold-bg`; sizes `--editor-sidebar 244px`, `--editor-inspector 276px`, `--editor-timeline 400px`, `--editor-label 196px`. Reuse existing `fg`/`accent`/`focus`/`danger` for everything that already has a token.
- `EditorShell`: CSS grid — header row; library | preview | inspector; timeline row. Slots for each region (filled by later tasks with placeholders now). Below 1180 px width the library and inspector become drawers toggled from the header (`aria-expanded`), and a drawer never hides the header (the route back).
- `EditorHeader`: title (inline rename → `rename` command), status text derived from `editorProject` (`Saved`, `Unsaved changes`, `Saving…`, `Save failed`), Help, Checks, **Save project**, **Render video** — Save and Render are separate buttons, Render disabled with a reason until Task 47.
- `prefers-color-scheme` + a theme toggle stored in workspace (Task 18 persists it; local state for now).

**Tests first:** `header shows Save project and Render video as separate buttons`; `status reads Unsaved changes when dirty and Saved after a matching receipt`; `below 1180px the library is a drawer and the header stays visible`; Playwright `one preview toolbar row at 960x640` (count `[data-testid=preview-toolbar]` = 1 — the element is a placeholder until Task 17), `no horizontal page scroll at 960x640`.

**Mutation check:** derive status from a timer (`setTimeout` → "Saved") → the receipt test red.

**Verify:** `npx vitest run tests/editorShell.test.ts`; `npm run build && npm run test:e2e`; all gates.

**Commit:** `feat(editor): responsive tutorial editor shell and header`

---

## Task 17: Action registry, shortcuts, the single preview toolbar and the context menu

**Package:** P03 · **F-IDs:** F-15, F-49, F-12 (clipboard actions), F-48 · **Depends:** 16 · **Bundle:** SCREENS 02–03; A14

**Files:** Create `src/editor/actions.ts`, `src/editor/shortcuts.ts`, `src/components/editor/shell/PreviewToolbar.vue`, `src/components/editor/menus/ContextMenu.vue`, `tests/editorActions.test.ts`, `tests/editorContextMenu.test.ts`.

**Behavior:**
- `actions.ts`: `ActionId` union (`split, delete, deleteClose, undo, redo, copy, cut, paste, duplicate, group, ungroup, earlier, later, addText, addArrow, addHighlight, addSpotlight, addZoom, addStep, addMask, addCaption, addMarker, addTrackVideo, addTrackAudio, fadeIn, fadeOut, transition, detachAudio, save, render, checks, help, importMedia, webcam, toggleLibrary, toggleInspector, focusPreview, ratio`) and `resolveActions(ctx: ActionContext) → Record<ActionId, { enabled: boolean; reason: string | null; label: string; shortcut: string | null }>` — pure; `ctx` = projection + selection + pointer target (`{kind: "clip"|"effect"|"track"|"asset"|"gap"|"layer", id, timeMs}`) + clipboard presence + locked flags. Reasons are human text ("Select a clip first", "Track Screen is locked", "The playhead is at a clip boundary"). `commandFor(actionId, ctx) → EditorCommand | null` builds the command using the **pointer target's `timeMs`** when present, the playhead otherwise.
- `shortcuts.ts`: map (`S` split, `Delete`/`Backspace` delete (leave gap), `Shift+Delete` close gap, `Ctrl+Z`, `Ctrl+Shift+Z`, `Ctrl+Y`, `Ctrl+C/X/V/D`, `Ctrl+G`, `Ctrl+Shift+G`, `Ctrl+S` save, `Ctrl+E` render, `F1`/`?` help, `F6` guide focus, `Shift+F10` context menu); `shouldHandle(event)` false when the target is an input/textarea/contenteditable, or a menu/dialog/guide owns keys.
- `PreviewToolbar.vue`: ONE row (`data-testid="preview-toolbar"`, `role="toolbar"`, roving tabindex with ←/→/Home/End); teaching tools, ratio, Review, panel toggles; tools that don't fit move into a labelled **More** menu fed by the same registry (a `ResizeObserver` measures; happy-dom tests drive `overflowCount` via prop).
- `ContextMenu.vue`: `role="menu"`, items from `resolveActions` for the target, disabled items keep `aria-disabled` + the reason as `title`/description; ↑/↓/Home/End, Enter, Escape closes and returns focus to the invoker; Shift+F10 or the Menu key on a focused clip opens it at the clip.

**Tests first:** `split uses the right-clicked time, not the playhead` (A14: playhead 1000, target timeMs 4000 → `splitClip.atMs === 4000`); `disabled actions carry a reason`; `locked track disables every clip mutation with the lock reason`; `shortcuts are ignored inside text inputs`; `context menu arrow navigation wraps and Escape returns focus`; `overflowed tools appear in More, not a second toolbar` (count of `role=toolbar` stays 1).

**Mutation check:** use the playhead in `commandFor` for split → A14 test red.

**Verify:** `npx vitest run tests/editorActions.test.ts tests/editorContextMenu.test.ts`; all gates.

**Commit:** `feat(editor): one action registry behind toolbar, shortcuts and context menus`

---

## Task 18: The editorWorkspace store and workspace persistence

**Package:** P03 · **F-IDs:** F-48, F-25 (monitoring prefs), F-14 · **Depends:** 16 · **Rulings:** R16

**Files:** Create `src-tauri/src/editor/prefs_commands.rs` (workspace half), `src/stores/editorWorkspace.ts`, `tests/editorWorkspaceStore.test.ts`; modify `src/editor/port.ts`, `src-tauri/src/editor/mod.rs`, `lib.rs`, `build.rs`, `capabilities/editor.json`, `src-tauri/src/editor/workspace.rs` (F16: the sanitizer gains `theme`), `src-tauri/src/editor/save_commands.rs` (F17: `editor_save_project`, from Task 12, embeds the sanitized `workspace.json`), `AGENTS.md`.

**Behavior:**
- Rust: `editor_get_workspace(window, session_id) -> serde_json::Value` (reads `workspace.json`, sanitized; missing → `{}`), `editor_save_workspace(window, session_id, workspace: Value)` (sanitize, ≤ 64 KiB, `write_atomic_replacing`). Workspace writes never touch the revision or history.
- F16: `core::editor::workspace::Workspace` (Task 2) gains an 19th field, `theme: Option<Theme>` (`Theme(dark|light)`), and `sanitize` keeps it instead of dropping it as an unknown key — without this the toggle in `EditorHeader` (Task 16) never survives a reopen.
- F17: `editor_save_project` (Task 12's `save_commands.rs`) currently builds `WorkspaceEnvelope.workspace` from "the last saved workspace or `{}`" without reading the live `workspace.json` this task introduces. This task changes that call site so `editor_save_project` reads and embeds the SANITIZED `workspace.json` (via this task's own read helper, not a raw file read) in the envelope it writes — R16 requires the saved project to carry the workspace, and nothing wired that until now.
- Store: state mirrors the 18 R16 fields plus `theme`; `select(ids)`, `setPlayhead(ms)` (clamped to duration), `setZoom`, `fit()`, `toggleSnap`, `setDeleteMode`, `toggleMonitorMute`, panel toggles; a debounced (750 ms) `persist()`; `hydrate(sessionId)`. Selection ids that no longer exist in the projection are pruned on every projection install (watch on `editorProject.snapshot.revision`, not on the whole graph).

**Tests first:** Rust `workspace_save_sanitizes_and_never_changes_the_revision`; `oversized_workspace_is_refused`; `sanitize_keeps_theme_and_rejects_other_values` (F16: `"dark"`/`"light"` survive, `"purple"` drops); `save_project_embeds_the_current_workspace` (F17: `editor_save_workspace` a non-default workspace, then `editor_save_project`, then reload — the envelope's `workspace` field equals the sanitized value, not `{}`). TS `persist is debounced to one save per burst`; `selection is pruned when a clip disappears`; `monitor mute is workspace state only` (no `editor_execute` call when toggled); `playhead clamps to project duration`.

**Mutation check:** remove the pruning watch → prune test red; keep embedding `{}` in `editor_save_project` → `save_project_embeds_the_current_workspace` red.

**Verify:** targeted tests; all gates.

**Commit:** `feat(editor): persist editor workspace state separately from the project`

---

## Task 19: Inspector shell and dialog host

**Package:** P03 · **F-IDs:** F-48, F-49 · **Depends:** 17, 18 · **Bundle:** SCREENS 04

**Files:** Create `src/components/editor/inspector/InspectorPanel.vue`, `src/components/editor/shell/DialogHost.vue`, `src/editor/dialogs.ts` (a tiny typed dialog stack), `tests/editorInspector.test.ts`, `tests/editorDialogHost.test.ts`.

**Behavior:**
- `InspectorPanel`: six category tabs **Clip, Layout, Fades, Audio, Speed, Color** (`role="tablist"`), bound to `editorWorkspace.property_tab`; with no selection it renders teaching copy ("Select a clip to adjust it…"), not disabled controls; with a multi-selection it states "N clips selected" and scopes controls. Sections are slots filled by later tasks. A shared `useInspectorDraft(field, commit)` composable: a local draft buffer, validation message shown inline, one command on commit (Enter/blur), Escape reverts — never binds an input straight to the projection.
- `DialogHost`: renders the top of the stack, focus-trapped (Tab cycles inside), Escape closes when the dialog allows, restores focus to the opener; emits `suspend`/`resume` for the guide (Task 56).

**Tests first:** `no selection shows guidance, not disabled controls`; `multi-selection states its scope`; `draft commits once on Enter and reverts on Escape`; `invalid draft stays visible with its correction` (speed "9" → "Speed must be between 0.25× and 4×", no command sent); `dialog restores focus to the opener`; `dialog host announces suspend and resume`.

**Verify:** targeted tests; all gates.

**Commit:** `feat(editor): inspector categories and a focus-safe dialog host`

---

## Task 20: Timeline view — tracks, clips, ruler, zoom, fit, snap

**Package:** P03/P04 · **F-IDs:** F-04 (display), F-14, F-26 (lane slot) · **Depends:** 18, 3 · **Bundle:** SCREENS 03

**Files:** Create `src/components/editor/timeline/TimelineView.vue`, `TrackLane.vue`, `ClipItem.vue`, `TimelineRuler.vue`, `TimelineToolbar.vue`, `src/editor/timelineLayout.ts`, `tests/timelineLayout.test.ts`, `tests/editorTimelineView.test.ts`.

**Behavior:**
- `timelineLayout.ts` (pure): `pxPerMs(zoom)`, `msToX`, `xToMs`, `fitZoom(durationMs, viewportPx)`, `snapTargets(project, playhead)` (clip edges, markers, playhead), `snap(ms, targets, thresholdPx, zoom)`, `visibleClips(clips, scrollLeft, width, zoom)` (virtualization: only clips intersecting the viewport ± one screen are rendered), ruler tick spacing.
- Track lanes ordered like `project.tracks` (index 0 = top = frontmost video); clips keyed by id; `ClipItem` shows name, media-kind colour token, selection ring, fade wedges (gold) and trim handles (visual only until Task 21); `aria-label` "Clip <name>, <start>–<end>".
- Ruler click/drag seeks (workspace playhead). `TimelineToolbar`: split/delete/undo/redo/snap/zoom/fit buttons from the action registry; timeline height drag handle (workspace `timeline_height`).

**Tests first:** `msToX and xToMs are inverse within a pixel`; `fit shows the entire edit` (fitZoom × duration ≤ viewport); `snap picks the nearest target within the threshold only`; `virtualization renders only visible clips` (600 clips, viewport 1000 px → rendered count < 60); `tracks render in project order`; `ruler click moves the playhead, not the selection`.

**Mutation check:** off-by-one viewport padding removed (`visibleClips` without the ± screen) → a clip half-visible at the edge disappears → test red.

**Verify:** targeted tests; all gates.

**Commit:** `feat(editor): virtualized multi-track timeline view`

---

## Task 21: Timeline editing interactions and numeric alternatives

**Package:** P04 · **F-IDs:** F-07, F-08, F-09, F-10, F-11, F-13 · **Depends:** 7, 8, 17, 19, 20 · **Bundle:** A03; SCREENS 03

**Files:** Create `src/composables/useTimelineDrag.ts`, `src/components/editor/inspector/ClipSection.vue`, `tests/useTimelineDrag.test.ts`, `tests/editorClipSection.test.ts`; modify `ClipItem.vue`, `TimelineView.vue`, `EditorShell.vue`, `src-tauri/core/src/editor/mod.rs` (F14: `pub mod limits` gains `MIN_CLIP_MS: u64 = 100`), `src-tauri/core/src/editor/commands/clips.rs` (F14: `trimClip`/`splitClip` enforce it).

**Behavior:**
- Click selects (Ctrl/Shift extends), marquee not required. Drag body → transient preview offset (local only), pointer-up → ONE `moveClips` (with `trackId` when dropped on another compatible lane and a single clip), Escape during drag → preview discarded, no command. Trim handles → preview → ONE `trimClip`. Minimum clip length 100 ms enforced in the preview AND by Rust (F14: `limits::MIN_CLIP_MS = 100`; `trimClip` refuses an `out_ms - in_ms` output duration below it, `splitClip` refuses a split that would leave either half below it — both `invalidRequest` naming the minimum; the Rust minimum was previously enforced nowhere, so a numeric inspector entry bypassing the UI preview clamp could create a sub-100 ms clip).
- Keyboard: focused clip ←/→ nudges by one frame (33 ms) or 1 s with Shift, submitting one `moveClips` per key press and KEEPING focus on the clip after the projection re-renders (stable keys + `nextTick` focus restore).
- Split/delete/undo/redo/copy/cut/paste/duplicate/group/ungroup via the registry; clipboard is window-local state holding a `ClipboardFragment` (Task 8's `buildFragment`); Cut = copy + `cutClips`.
- Delete mode toggle (leave gap / close gap) visible in the toolbar; the close-gap button says "on this track".
- `ClipSection`: name, start, in, out, duration (derived) as numeric fields with `useInspectorDraft`; Earlier/Later buttons.

**Tests first:** `a drag submits exactly one moveClips on pointer-up`; `Escape during a drag sends nothing and restores the clip`; `leftward drags produce negative deltas` (the EditorRoot drag-seam lesson: test BOTH directions); `trim preview respects the 100 ms minimum`; `keyboard nudge keeps focus on the clip after re-render`; `numeric start entry sends trimClip with unchanged in/out`; `cut copies then deletes as one undo step`. Rust (`clips.rs`, F14): `trim_below_the_minimum_is_refused_naming_100ms`; `split_that_would_leave_a_sub_minimum_half_is_refused`.

**Mutation check:** compute delta as `abs()` → leftward test red; drop the Rust minimum check → `trim_below_the_minimum_is_refused_naming_100ms` red.

**Verify:** targeted tests; `cargo test -p vault_buddy_core editor::commands::clips`; `npm run build && npm run test:e2e`; all gates.

**Acceptance:** capture → cut → save → reopen works through the UI in the Vitest integration harness (mock port backed by an in-memory fake that applies nothing locally — the fake returns canned projections).

**Commit:** `feat(editor): drag, trim, split and delete through acknowledged commands`

---

## Task 22: Preview surface, preview controller, transport — and the asset scope

**Package:** P04 · **F-IDs:** F-14, F-25 (monitoring), F-04 (preview) · **Depends:** 20, 12 · **Rulings:** R7 · **Bundle:** NATIVE-MEDIA § Preview

**Files:**
- Create: `src-tauri/src/editor/media_commands.rs` (only `editor_media_url` in this task), `src/editor/previewController.ts`, `src/components/editor/preview/PreviewSurface.vue`, `TransportBar.vue`, `tests/previewController.test.ts`, `tests/editorTransport.test.ts`
- Modify: `src-tauri/tauri.conf.json` (assetProtocol scope → the ADR R7 array; CSP → ADR R7), `src-tauri/src/tray.rs` (rename the pin test to `the_asset_protocol_scope_is_pinned_to_staging_and_the_editor_project_media_dirs` and assert the exact new array + the new CSP clauses), registration files, `AGENTS.md` (asset-scope paragraph), `docs/Gaps.md` (GAP-N3: preview approximates the render)

**Behavior:**
- `editor_media_url(window, session_id, ref)` → absolute path for a registered asset (via `resolve_source`) or product; unknown id → `unauthorizedSource`; missing file → `sourceMissing`. The frontend never constructs a path.
- `previewController.ts` (non-reactive class, never stored in Pinia): owns one `HTMLVideoElement`/`HTMLImageElement` per active visual clip (pooled, max 8), `AudioContext` + per-clip `GainNode`s for monitoring, rAF clock; `seek(ms)` cancels the previous seek (token); `play()`/`pause()`; `layout(project, t)` computes for each active clip the CSS box from normalized `x,y,w,h` letterboxed into the stage (`contain` of canvas into stage), `opacity`, z-order = reverse track index, `muted` from track/clip mute ∨ workspace `monitor_muted`. Emits low-rate (10 Hz) `timeupdate` to the workspace playhead.
- `PreviewSurface.vue`: stage with canvas aspect, hosts the controller's elements in a container ref; pointer → canvas mapping `clientToCanvas(evt)` undoes letterboxing (pure, exported, tested). `TransportBar`: play/pause (Space), current/total time, monitoring volume + mute (workspace only), playback rate.

**Tests first:** Rust `media_url_refuses_unregistered_assets`; `the_asset_protocol_scope_is_pinned_…` (exact array). TS `clientToCanvas undoes letterboxing on a portrait canvas in a landscape stage` (720×1280 in 1000×500 → a click at the stage centre maps to (360, 640)); `layout stacks upper tracks above lower tracks`; `hidden tracks are not laid out`; `monitor mute silences preview gain without an editor command`; `seek cancels an older pending seek`.

**Mutation check:** widen the scope to `$APPLOCALDATA/*` → the pin test red (then restore).

**Verify:** targeted tests; `npx tauri build --no-bundle`; all gates.

**Commit:** `feat(editor): layered preview surface with a pinned media scope`

---

## Task 23: Track management

**Package:** P05 · **F-IDs:** F-06 · **Depends:** 7, 20

**Files:** Create `src-tauri/core/src/editor/commands/tracks.rs`, `src/components/editor/timeline/TrackHeader.vue`, `tests/editorTrackHeader.test.ts`; modify `commands/mod.rs`, `TrackLane.vue`.

**Behavior:** `addTrack{kind, name, index}` (≤ 32, id `trk-…`), `renameTrack` (1..=200 chars), `moveTrack{trackId, toIndex}` (index 0 = frontmost), `setTrackFlags` (`volume ∈ [0,2]`; `locked` may always be toggled; any other flag change on a locked track is refused), `deleteTrack` (refused when locked; removes its clips and their cues/transitions in one undo step, label "Delete track <name>"). `solo` semantics documented in code: when any audio track is soloed, only soloed tracks are audible (used by the mixer and the render plan). `TrackHeader`: name (inline rename), visibility eye, lock, mute, solo, volume slider (audio), a track menu (context-menu actions), each control with `aria-pressed`.

**Tests first:** Rust `locked_track_refuses_rename_move_delete_and_flag_changes_but_allows_unlock`; `delete_track_removes_its_clips_and_cues_in_one_undo`; `move_track_reorders_compositing_order`; `track_limit_is_enforced` (33rd → Err). TS `lock toggles through setTrackFlags`; `a locked track's clips are not draggable` (registry reason shown).

**Mutation check:** allow `renameTrack` on a locked track → the lock test red.

**Verify:** targeted tests; all gates.

**Commit:** `feat(editor): create, order, lock, hide, mute, solo and delete tracks`

---

## Task 24: Media probing and import planning (pure)

**Package:** P05 · **F-IDs:** F-02 · **Depends:** 1 · **Rulings:** R1

**Files:** Create `src-tauri/core/src/editor/probe.rs`; modify `src-tauri/src/ffmpeg.rs` (`SourceFacts` gains `has_video`, `has_audio`, `audio_rate: Option<u32>`; `parse_probe_output` reads stream types — keep existing tests green and add new ones).

**Behavior:**
- `ImportKind { Video, Audio, Image }`; `classify_extension(name) -> Option<ImportKind>` — allowlist `mp4 m4v mov webm mkv` (video), `mp3 wav m4a aac ogg opus flac` (audio), `png jpg jpeg webp` (image); anything else `None` (`unsupportedMedia`).
- `sniff_image(bytes: &[u8]) -> Option<(format, width, height)>` for PNG (IHDR), JPEG (SOF0/1/2 scan, bounded to the first 256 KiB), WebP (VP8/VP8L/VP8X); dimensions ≤ 16384.
- `MAX_IMPORT_FILE_BYTES = 4 GiB`; `import_plan(files: &[(display_name, size, ImportKind)]) -> Vec<Result<PlannedImport, String>>` — per-file outcomes, never all-or-nothing.
- `pub struct ProbeFacts { duration_ms, width: Option<u32>, height: Option<u32>, has_video: bool, has_audio: bool }` (F22: a plain core-owned type — core cannot name the shell's `ffmpeg.rs::SourceFacts`, which is `pub(crate)` to the shell crate and carries shell-specific fields this function doesn't need). `asset_from_probe(asset_id, display_name, kind, facts: ProbeFacts, image_dims: Option<(u32,u32)>) -> Asset` (images: `kind video`, `media_type image`, `duration_ms 5000` default still length, bounded). The shell (`ffmpeg.rs`) converts its own `SourceFacts` into a `ProbeFacts` at the one call site that needs an `Asset`.

**Tests first:** `classify_extension_is_case_insensitive_and_allowlisted` (`.MP4` ok, `.exe` None, `.mp4.exe` None); `png_dimensions_are_read_from_ihdr` (hand-built 33-byte header, 640×360 — asymmetric); `jpeg_sof_scan_finds_dimensions`; `webp_vp8x_dimensions`; `truncated_images_are_rejected_not_panicking` (every prefix length 0..len of the PNG fixture); `oversized_dimensions_are_rejected`; `import_plan_reports_per_file`; ffmpeg.rs `probe_output_reports_audio_only_files`; ffmpeg.rs `source_facts_converts_to_probe_facts` (F22: the shell-side conversion carries every field `asset_from_probe` reads).

**Mutation check:** swap width/height in the PNG reader → the asymmetric test red.

**Verify:** `cargo test -p vault_buddy_core editor::probe`; `cargo test -p vault-buddy --lib ffmpeg`; all gates.

**Commit:** `feat(core): classify and sniff importable media without trusting names`

---

## Task 25: Media import command, job channel and the media library

**Package:** P05 · **F-IDs:** F-02 · **Depends:** 22, 24 · **Bundle:** A-scenarios for F-02; IPC § Progress

**Files:** Create `src-tauri/src/editor/media_jobs.rs`, `src/stores/editorJobs.ts`, `src/components/editor/library/MediaLibrary.vue`, `tests/editorJobsStore.test.ts`, `tests/editorMediaLibrary.test.ts`; modify `media_commands.rs`, registration files, `port.ts`, `AGENTS.md`.

**Behavior:**
- `editor_import_media(window, app, session_id, on_progress: tauri::ipc::Channel<JobProgressDto>)` → `{jobId}` immediately; a named `editor-import` thread: native multi-file open dialog (`tauri_plugin_dialog`, filters from the allowlist) — the dialog, not a frontend string, grants access; per file: classify → size check → copy into `media\<assetId>.<ext>` via an owned `.part` + `rename_noreplace` → probe (ffprobe for audio/video — missing ffmpeg is a per-file `encoderUnavailable` error naming ffmpeg; images sniffed natively) → SHA-256 on the same thread → `sources.json` update → collect assets; one `InternalCommand::AddAssets` at the end through the session (so a batch is one undo step). Progress messages have strictly increasing `sequence`; the terminal message carries `assetIds` and `perFile` errors (display names only). Cancellation (`editor_cancel_job`, sync, sets an `AtomicBool`) stops FUTURE files; already-imported files stay. A file that fails leaves no `.part` behind.
- `editor_cancel_job(window, session_id, job_id)` and `editor_get_jobs(window, session_id)` land here (render reuses them in Task 46): a `JobRegistry { jobs: HashMap<jobId, JobRecord> }` in `EditorState`.
- `editorJobs` store: per job, ignores wrong-session/job messages and non-increasing sequences; after terminal ignores everything; `reconcile()` calls `editor_get_jobs` (authoritative).
- `MediaLibrary`: search, asset cards (kind, duration, availability), Import button, "+" inserts at the playhead onto the first compatible unlocked track (`insertClip`), drag onto a lane (handled via Task 26).

**Tests first:** Rust (tempdir, fake probe seam): `a failing file does not discard the successful ones`; `cancel stops future files and keeps finished ones`; `a batch is one undo step`; `no part file survives a failed copy`; `progress sequence strictly increases and ends with exactly one terminal`. TS: `store ignores a non-increasing sequence`; `store ignores messages after terminal`; `store ignores another session's job`; `reconcile replaces stale event state`; `library + inserts at the playhead on the first unlocked compatible track`.

**Mutation check:** emit two terminals → the store test for "after terminal" must hold AND the Rust exactly-one test red.

**Verify:** targeted tests; all gates. Checklist row T4: import a mixed batch with one corrupt file on Windows.

**Commit:** `feat(editor): import media with per-file results on a job channel`

---

## Task 26: Multi-track placement and still images

**Package:** P05 · **F-IDs:** F-04, F-10 · **Depends:** 21, 25

**Files:** Modify `commands/clips.rs` (placement rules), `TimelineView.vue`, `useTimelineDrag.ts`, `MediaLibrary.vue`; create `tests/editorPlacement.test.ts`.

**Behavior:** dropping an asset on a lane → `insertClip` at the snapped drop time (audio assets only on audio tracks; video/image only on video tracks — the lane shows a refusal cursor + reason); dropping onto empty space below the last lane creates a track of the right kind first (one `addTrack` + one `insertClip` — two undo steps, labelled); images get `out_ms = 5000` by default and can be trimmed longer up to `MAX_DURATION_MS` (images have no source bound beyond that); a clip moved between lanes keeps its source-linked cues. Rust: `insertClip` of an image with `out_ms > duration_ms` is allowed only for `media_type image`.

**Tests first:** Rust `image_clips_may_extend_past_their_nominal_duration`; `video_clips_may_not`; TS `audio asset cannot be dropped on a video lane (reason shown)`; `drop below the last lane creates a track then inserts`; `cross-lane move keeps cue ids`.

**Verify:** targeted tests; all gates.

**Commit:** `feat(editor): place media on any compatible track, including stills`

---

## Task 27: Mixer — gains, mute/solo, monitoring and audio detachment

**Package:** P05 · **F-IDs:** F-05, F-25, F-24 · **Depends:** 23, 25 · **Bundle:** A06

**Files:** Create `src-tauri/core/src/editor/commands/mix.rs`, `src/components/editor/inspector/AudioSection.vue`, `src/components/editor/shell/MixerPopover.vue`, `tests/editorAudioSection.test.ts`; modify `commands/mod.rs`, `validate_media.rs`, `src-tauri/core/src/editor/session.rs` (F15: `EditorSession::execute`/`execute_internal` call the new `apply(project, cmd, &CommandContext)` signature and must build the context), `src-tauri/src/editor/session_commands.rs` (F15: `editor_execute`'s caller fills `CommandContext.assets_with_audio` from `sources.json` before calling into the session — the ONLY place shell state crosses into a core call, since core itself stays Tauri-free).

**Behavior:**
- `setClipMix{clipIds, volume?, muted?}` (`volume ∈ [0,2]`), `setMasterGain{gain ∈ [0,1]}`.
- `detachAudio{clipId, audioTrackId}`: the clip's asset must be a video with an audio stream. That fact lives in `sources.json`, not the interchange graph, so `commands::apply` gains a context argument — `apply(project, cmd, &CommandContext { assets_with_audio: &BTreeSet<String> })` — which the shell fills from `sources.json` (core stays Tauri-free; update every existing call site and test helper in this task, INCLUDING `session.rs::execute`/`execute_internal`, whose signatures gain the same context parameter, and `session_commands.rs`, whose `editor_execute` command builds it from the session's `sources.json` before calling `execute` — F15: without this, `execute`'s existing callers from Tasks 6–26 would not compile once `apply`'s signature changes). Creates asset `<id>-audio` (kind audio, `linked_asset = source id`, same duration, `name = "<name> · audio"`), a clip on `audioTrackId` (or a new audio track) with identical `start/in/out/speed`, and sets the original clip `muted = true`. One undo step. Cycle check from Task 2 covers the link.
- `AudioSection`: clip volume (dB readout, linear stored), mute, **Detach audio**; `MixerPopover`: per-track volume/mute/solo + master; a monitoring-only mute clearly labelled "Mute preview (does not affect the video)". Sample-peak meters are labelled "peak", never "loudness".

**Tests first:** Rust `detach_creates_a_linked_asset_and_mutes_the_original`; `detach_of_a_silent_source_is_refused`; `detached_audio_follows_the_same_source_range`; `master_gain_out_of_range_is_refused`; `session_execute_builds_the_context_from_sources_json` (F15: a `detachAudio` sent through `editor_execute` succeeds only when the session's `sources.json` marks the asset as having audio — proves the shell wiring, not just the pure `apply`). TS `monitoring mute never sends a command` (A06 UI half); `volume draft commits once`.

**Verify:** targeted tests; all gates.

**Commit:** `feat(editor): mixer controls and linked audio detachment`

---

## Task 28: Waveforms and thumbnails

**Package:** P05 · **F-IDs:** F-26 · **Depends:** 25

**Files:** Create `src-tauri/core/src/editor/peaks.rs`; modify `media_jobs.rs` (peaks/thumbnail jobs), `media_commands.rs` (`editor_media_peaks`, `editor_media_thumbnail`), `ClipItem.vue`, registration files (F30: `lib.rs` `generate_handler!`, `build.rs` `EDITOR_COMMANDS`, `capabilities/editor.json`, AGENTS.md's IPC table row + re-measured count — see Global Constraints); create `tests/editorWaveform.test.ts`.

**Behavior:**
- `peaks.rs`: `fold_s16le(chunk: &[u8], state: &mut PeakState)` streaming fold into `buckets` max-abs values (0..=1), bounded memory (state is `Vec<f32>` of `buckets ≤ 4000`); odd trailing byte carried over.
- Shell: `ffmpeg -v error -i <src> -ac 1 -ar 8000 -f s16le -` streamed through `external_tool` (named thread `editor-peaks`, cancelable, `CREATE_NO_WINDOW`), cached as `cache\<assetId>.peaks.<buckets>.json`; thumbnails `-ss <t> -frames:v 1 -vf scale=160:-2` to `cache\<assetId>-<ms>.jpg` (≤ 1 per 250 ms of zoomed timeline, LRU ≤ 200 files per project). Both refuse politely without ffmpeg (waveform lane shows "Install ffmpeg to see waveforms").
- `ClipItem` draws peaks with an SVG polyline only for visible clips.

**Tests first:** Rust `peaks_fold_is_chunk_boundary_independent` (same bytes split at every offset 0..17 give identical peaks); `peaks_are_normalised_max_abs`; `odd_trailing_byte_is_carried`; `buckets_are_bounded`. TS `waveform renders only for visible clips`; `missing ffmpeg shows the install hint`.

**Mutation check:** drop the carry-over byte → the boundary test red.

**Verify:** targeted tests; all gates.

**Commit:** `feat(editor): bounded, cancelable waveforms and thumbnails`

---

## Task 29: Fades

**Package:** P06 · **F-IDs:** F-17, F-18 · **Depends:** 21, 27 · **Bundle:** A07; DATA-MODEL § Fade rules

**Files:** Create `src-tauri/core/src/editor/commands/fades.rs`, `src/components/editor/inspector/FadesSection.vue`, `src/editor/fadeCurves.ts`, `tests/editorFades.test.ts`; modify `ClipItem.vue` (gold fade handles), `commands/mod.rs`, `src-tauri/core/src/editor/commands/clips.rs` (F11: `trimClip`'s fade-clamp arm, below — listed here because it lands in THIS task, not Task 7).

**Behavior:** `setFades{clipId, fadeInMs?, fadeOutMs?, fadeCurve?}` — each ≤ half the clip's OUTPUT duration, else `invalidRequest` naming the maximum. F11: Task 7's `trimClip` (as shipped) does NOT clamp existing fades — this task adds that clamp to `clips.rs`'s `trimClip` (shrink a clip whose fades now exceed the new half-duration, clamp each to the new half, label the undo step "Trim (fades adjusted)") and to Task 31's `setSpeed` in the same way; the "(tested there)" cross-reference in earlier drafts was wrong — the test lives in THIS task, beside the code that implements it. `fadeCurves.ts`: `gainAt(curve, u)` for `linear` (u), `smooth` (smoothstep 3u²−2u³), `equal-power` (sin(u·π/2)); F23: the render (Task 44) maps these to ffmpeg `afade` curve names `linear→tri`, `smooth→hsin`, `equal-power→qsin` — NOT `smooth→esin`, which is a different, non-smoothstep-shaped curve; the preview (CSS/canvas gain) and the render (ffmpeg's `hsin`) are a close but not bit-identical approximation of the same smoothstep shape, recorded under GAP-N3 (Task 22 writes that gap entry) rather than claimed as exact — fixture values in `tests/fixtures/editor-fade-cases.json` read by Rust `fades.rs` tests and `tests/editorFades.test.ts` (**6 cases**, size-guarded both sides). Handle drag on the clip corner = preview then one `setFades`.

**Tests first:** Rust `fade_longer_than_half_is_refused_naming_the_limit`; `shared_fade_curve_table_has_six_cases_and_agrees`; `trim_that_shrinks_a_clip_clamps_its_fades_and_labels_the_step` (F11: a 1000 ms clip with 400/400 ms fades trimmed to 600 ms output → both fades clamp to 300 ms, undo label names the adjustment). TS `handle drag and numeric entry send the same command`; `equal-power midpoint is 0.7071, not 0.5`.

**Mutation check:** use linear for equal-power in TS → midpoint test + fixture row red; skip the clamp on trim → `trim_that_shrinks_a_clip_clamps_its_fades_and_labels_the_step` red.

**Verify:** targeted tests; all gates.

**Commit:** `feat(editor): video and audio edge fades with shared curve fixtures`

---

## Task 30: Transitions

**Package:** P06 · **F-IDs:** F-19 · **Depends:** 29 · **Bundle:** A07; DATA-MODEL (one paired transition per clip side)

**Files:** Modify `commands/fades.rs` (or create `commands/transitions.rs` if `fades.rs` > 600 lines), `validate_media.rs`, `FadesSection.vue`, context menu actions, `src-tauri/core/src/editor/commands/clips.rs` (F12: `deleteClips`'s close-gap ripple and the overlap checks change — below; not listed in earlier drafts of this task); create `tests/editorTransitions.test.ts`.

**Behavior:** `addTransition{fromClipId, toClipId, durationMs, kind}`: same track, adjacent (`from` ends exactly where `to` starts), same media kind (video clip → `dissolve`, audio → `equal-power`; mismatch refused), no existing transition on `from`'s out side or `to`'s in side, `durationMs ≤ min(half of each)`. Applying it shifts `to` and every LATER clip on THAT track earlier by `durationMs` (the overlap), never other tracks; refused if that shift would cross a locked clip or break a cross-track group. `removeTransition` restores the spacing (shifts later clips back by the duration). `setTransitionDuration` adjusts both. Deleting either clip removes the transition (Task 7 already) and restores spacing. F12: this task also changes `clips.rs` behaviour that Task 7 shipped without transitions in mind — `deleteClips{closeGap}`'s ripple must now treat the overlap a transition creates as a legal, non-error adjacency (Task 7's "a transition spans the gap" refusal only covered a transition being SPLIT by the ripple; a ripple that merely passes beside an existing transition without splitting it is allowed), and the overlap check `insertClip`/`moveClips` use must allow the specific transition-created overlap between two transitioned clips while still rejecting every other overlap.

**Tests first:** `transition_shortens_only_its_track`; `remove_restores_the_original_spacing` (clip starts equal the pre-add starts); `second_transition_on_the_same_side_is_refused`; `cross_kind_transition_is_refused`; `transition_through_a_group_is_refused_atomically`; `move_of_a_clip_beside_a_transition_is_unaffected_by_the_overlap_allowance` (F12: moving a THIRD clip into the transition's overlap window is still refused — the allowance is narrowly scoped to the transitioned pair); `close_gap_delete_beside_a_transition_ripples_correctly` (F12: deleting a clip on the same track as a transitioned pair, without touching either transitioned clip, still closes the gap normally). TS `transition action is disabled with a reason when clips are not adjacent`.

**Mutation check:** shift all tracks → the only-its-track test red.

**Verify:** targeted tests; all gates.

**Commit:** `feat(editor): paired dissolves and equal-power crossfades`

---

## Task 31: Speed, layout and transforms (picture-in-picture)

**Package:** P06 · **F-IDs:** F-16, F-21, F-23 · **Depends:** 22, 29 · **Bundle:** A10

**Files:** Create `src-tauri/core/src/editor/commands/layout.rs`, `src/components/editor/inspector/LayoutSection.vue`, `SpeedSection.vue`, `src/components/editor/preview/LayoutHandles.vue`, `src/editor/layoutGeometry.ts`, `tests/editorPipLayout.test.ts` (F29: named `editorPipLayout`, not `editorLayout` — `tests/editorLayout.test.ts` already exists, the phase-4 layout contract test that Task 15/16 touch; a same-named new file would collide with it); modify `commands/mod.rs`.

**Behavior:**
- `setSpeed{clipId, speed, preservePitch}`: `speed ∈ [0.25, 4]`; output duration recomputed via `time`; refused if the longer clip would overlap the next clip on its track (message offers "close gap" is NOT automatic); fades clamped; source-linked cues unchanged (their output spans follow via mapping).
- `setLayout{clipIds, …}`: ranges per schema; `rotation ∈ {0,90,180,270}`; `frame_shape`, `fit`, `mirror`, `flip_y`, `crop_zoom ∈ [1,3]`, `crop_x/crop_y ∈ [0,1]`; multi-clip applies the same values to all (atomic).
- `layoutGeometry.ts`: `cornerPreset(corner, canvas, size = 0.19)` → normalized box with a 0.025 margin and aspect-correct height (`h = w × canvasW / canvasH × sourceH / sourceW` for `cover` circles so circles stay circular — F-38), `resizeFromHandle(box, handle, dx, dy, keepAspect)`, `clampBox`.
- `LayoutHandles.vue`: selected visual clip's box in preview with 8 handles + move; preview-only during drag; ONE `setLayout` on release; never rendered into output (it is a sibling overlay, not part of the stage).
- `LayoutSection`: numeric x/y/w/h (percent), corner presets (TL/TR/BL/BR), shape (rectangle/rounded/circle), fit, crop zoom/anchor, rotation, mirror, flip, opacity. `SpeedSection`: speed presets (0.5/1/1.5/2) + numeric + preserve pitch.

**Tests first:** Rust `speed_two_halves_output_duration_and_keeps_cue_source_times`; `speed_change_that_overlaps_the_next_clip_is_refused`; `layout_round_trips_through_serialization` (A10: set → save → load → equal values, asymmetric 0.61/0.23/0.27/0.19); `multi_clip_layout_is_atomic`. TS `corner preset keeps a circle circular on a portrait canvas`; `resize handle drag sends one setLayout`; `handles are not inside the stage element` (DOM assertion).

**Mutation check:** omit the canvas aspect in `cornerPreset` → circle test red.

**Verify:** targeted tests; all gates.

**Commit:** `feat(editor): speed, crop, rotation and picture-in-picture layout`

---

## Task 32: Canvas formats and colour treatment

**Package:** P06 · **F-IDs:** F-38, F-39 · **Depends:** 31

**Files:** Modify `commands/layout.rs` (`setCanvas`, `setAdjustments`); create `src/components/editor/inspector/ColorSection.vue`, `src/editor/colorPresets.ts`, `tests/editorColor.test.ts`; modify `PreviewToolbar.vue` (ratio control), `previewController.ts` (CSS filter for adjustments).

**Behavior:** `setCanvas{width,height}` accepts only the four pairs and stores nothing else: the "review crop, text and caption placement" prompt is a COMPUTED check (Task 54 fires it whenever a text/caption/effect box leaves the safe area of the current canvas, or a source's aspect differs from the canvas), so there is no stored flag to go stale. The UI shows a one-time toast after `setCanvas` pointing at Checks. `setAdjustments{clipIds, adjustments|null}` validates ranges (brightness/contrast 0.25–2, saturation 0–2, sepia/grayscale 0–1); presets in `colorPresets.ts`: `none, vivid (sat 1.3, con 1.1), warm (sepia 0.2, sat 1.1), cool (sat 0.9, bri 1.02 + hue handled as none — cool is saturation-only in v1, documented), mono (grayscale 1), sepia (sepia 0.8)`. Preview maps to CSS `filter:` (brightness/contrast/saturate/sepia/grayscale); teaching cues are unaffected (they are drawn above the source). The colour controls apply to source footage only; card clips refuse adjustments ("Colour applies to footage, not title cards").

**Tests first:** Rust `only_the_four_canvases_are_accepted`; `adjustments_ranges_are_enforced`; `cards_refuse_adjustments`. TS `preset maps to the documented CSS filter string`; `ratio control sends setCanvas`.

**Verify:** targeted tests; all gates.

**Commit:** `feat(editor): canvas formats and basic colour treatment`

---

## Task 33: Title cards and intro-before-all

**Package:** P06 · **F-IDs:** F-37 · **Depends:** 23, 31 · **Bundle:** A05

**Files:** Create `src-tauri/core/src/editor/commands/cards.rs`, `src/components/editor/library/TitlesLibrary.vue`, `tests/editorTitles.test.ts`; modify `commands/mod.rs`, `previewController.ts` (card rendering as a styled DOM layer).

**Behavior:** `addCard{preset(intro|chapter|outro|blank), trackId|null, startMs, durationMs, title, subtitle}` creates (once per project) a builtin asset `card` (`builtin: "card"`, kind video, `duration_ms = MAX_DURATION_MS`) and a clip with `card {preset, title, subtitle, background "#18191e", foreground "#f0eef6", accent "#b6a2f5"}` (preset-specific defaults), on the given or a new top video track. `updateCard` edits text/colours (`#rrggbb` validated). `insertIntro{durationMs, title, subtitle}` shifts EVERY clip on every populated track right by `durationMs` and inserts an intro card at 0 on the top video track — refused atomically if any populated track is locked ("Unlock <track> to insert an intro before everything"). Cards are editable after insertion (never flattened).

**Tests first:** `intro_shifts_every_track_together` (A05: all starts +durationMs, cue source times unchanged); `intro_is_refused_when_a_populated_track_is_locked_and_nothing_moves`; `empty_locked_track_does_not_block_intro`; `card_colours_are_validated`; TS `titles library inserts a chapter card at the playhead`.

**Verify:** targeted tests; all gates.

**Commit:** `feat(editor): editable title cards and intro insertion across tracks`

---

## Task 34: Teaching cue commands

**Package:** P08 · **F-IDs:** F-27–F-33 · **Depends:** 7, 31 · **Bundle:** A11

**Files:** Create `src-tauri/core/src/editor/commands/cues.rs`; modify `commands/mod.rs`, `validate.rs` (kind-specific props).

**Behavior:** `addEffect{clipId, kind, startMs, endMs, props}` — `startMs/endMs` are SOURCE times inside the clip's source range (if given as output times by the UI, the UI converts with `timeMap` first — the command is unambiguous); `props` is a typed union per kind: text `{x,y,w,h,text,fontSize,color,background}`, arrow `{x,y,x2,y2,color,stroke}`, highlight `{x,y,w,h,color,stroke}`, spotlight `{x,y,w,h,dim}`, zoom `{x,y,factor,easing}`, step `{x,y,number,text,color}`, mask `{x,y,w,h,color}` (mask colour opaque). Defaults per kind when omitted. `updateEffect`, `removeEffect`. Effects inherit Task 7's split/trim/move/speed behaviour (already source-linked). Default durations: 3000 ms or to the clip's source end.

**Tests first:** `each_kind_requires_its_props` (table of 7); `cue_times_are_source_times_and_survive_a_move` (A11); `cue_output_span_follows_speed` (speed 2 → output span halves, via `time::cue_output_span`); `cue_outside_its_clip_source_range_is_refused`; `split_through_a_cue_yields_two_cues` (re-assert with an arrow to cover props copying).

**Mutation check:** store output times instead of source → the move test red.

**Verify:** `cargo test -p vault_buddy_core editor::commands::cues`; all gates.

**Commit:** `feat(core): seven kinds of source-linked teaching cues`

---

## Task 35: Teaching cue overlay and effect inspector

**Package:** P08 · **F-IDs:** F-27–F-33 · **Depends:** 22, 34

**Files:** Create `src/editor/cueGeometry.ts`, `src/components/editor/preview/CueOverlay.vue`, `src/components/editor/inspector/EffectSection.vue`, `tests/cueGeometry.test.ts`, `tests/editorCueOverlay.test.ts`; modify `PreviewToolbar.vue` (teaching tool buttons), `PreviewSurface.vue`.

**Behavior:** `cueGeometry.ts` — `activeCues(project, t)` via `timeMap`; `arrowPath(x,y,x2,y2,stroke, canvas)` (shaft + head polygon in canvas px — the SAME numbers Task 43's ASS generator uses; shared fixture `tests/fixtures/editor-arrow-cases.json`, **4 cases**, read by `ass.rs` tests too); `spotlightRects(box)` → four dim rectangles; `zoomTransform(effect, t)` → scale/translate for the stage with ease in/out over `easing` ms and clamped so no empty margin shows. `CueOverlay` renders an SVG over the stage (above clips, below handles): text boxes, arrows (endpoint handles when selected), highlights, spotlight, step badges, masks, zoom focal marker; selection/handles are a separate layer excluded from output. Clicking a teaching tool adds a cue at the playhead on the topmost visible clip under the stage centre (or the selected clip). `EffectSection` edits every prop (text, size, colour, stroke, dim, factor, easing, number, start/end shown in output time, converted to source on commit); privacy mask shows the permanent warning "Covers pixels only while visible. It does not track motion and the original recording is unchanged."

**Tests first:** `arrow path matches the shared fixture` (4 rows, size-guarded); `zoom never exposes empty margins` (focal point at the corner, factor 2 → translate clamped); `only cues intersecting the playhead render`; `endpoint drag sends one updateEffect`; `mask inspector always shows the limitation warning`; `adding a cue converts the playhead to source time` (speed 2 clip).

**Mutation check:** drop the zoom clamp → margin test red.

**Verify:** targeted tests; all gates.

**Commit:** `feat(editor): preview overlay and inspector for teaching cues`

---

## Task 36: Captions and chapters

**Package:** P08 · **F-IDs:** F-34, F-35 (settings), F-36 · **Depends:** 34, 35 · **Bundle:** A12

**Files:** Create `src-tauri/core/src/editor/captions_io.rs`, `src-tauri/core/src/editor/commands/captions.rs`, `src/components/editor/library/CaptionsLibrary.vue`, `ChaptersLibrary.vue`, `tests/editorCaptions.test.ts`; modify `commands/cues.rs` (markers), `media_commands.rs` (`editor_import_captions`), registration files, `port.ts`, `AGENTS.md`.

**Behavior:**
- `captions_io.rs`: `parse_srt(text) -> Result<Vec<ParsedCue>, CaptionParseError{line, message}>` and `parse_vtt` (plain WebVTT: header, optional cue ids, `-->` timings, no styling blocks — `STYLE`/`REGION` blocks skipped, tags stripped to text), both bounded (≤ 2 MiB, ≤ 2000 cues, text ≤ 500 chars, times ≤ 7.2e6), `\r\n` tolerant; `export_srt(cues_in_output_time)`/`export_vtt` (used by Task 48).
- Commands: `setCaptionSettings` (font 18–56, position top|bottom), `addCaption`, `updateCaption`, `splitCaption{captionId, atMs /*source*/}`, `removeCaptions`, `InternalCommand::ImportCaptions{clipId, cues (timeline-relative input converted to the clip's source time), replace}`.
- `editor_import_captions(window, session_id, clip_id, replace)`: native open dialog (`srt`, `vtt`, `txt`), read bounded, parse, convert: imported times are OUTPUT times relative to the clip start → source via `output_at` inverse; cues outside the clip are reported, not silently dropped: the command returns `CaptionImportResult { projection, imported, skipped } | null` (null = dialog cancelled).
- Markers: `addMarker{clipId, sourceMs, title}`, `updateMarker`, `removeMarker`.
- `CaptionsLibrary`: cue list (virtualized, editable text/timing in output time), Import, Add at playhead, Split at playhead, burn-in toggle + preview placement, density notice (> 20 chars/s) and overlap notice (two cues overlapping in output time) each with a "Select cue" action. "Automatic transcription is not available" stated where users look for it. `ChaptersLibrary`: list sorted by output time (derived), add at playhead (on the topmost clip), rename, delete, jump.

**Tests first:** Rust `srt_parses_crlf_and_multiline_cues`; `srt_error_names_the_line`; `vtt_skips_style_blocks_and_strips_tags`; `bounds_are_enforced` (2001 cues → Err); `export_srt_formats_hours_and_commas` (3_723_004 ms → `01:02:03,004`); `split_caption_keeps_both_halves_on_the_clip`; `import_converts_output_to_source_time_at_speed_two`; `marker_on_a_cut_belongs_to_one_half` (already in Task 7 — reference it, add a chapter-derived output test `chapters_resolve_to_output_time_after_trim`). TS `density notice appears above 20 chars per second`; `overlap notice selects the cue`; `library never claims automatic transcription`.

**Mutation check:** treat imported times as source times → the speed-two import test red.

**Verify:** targeted tests; all gates.

**Commit:** `feat(editor): caption authoring and import, and chapter markers`

---

## Task 37: Recovery journal, close guard and startup recovery

**Package:** P09 · **F-IDs:** F-44 · **Depends:** 12, 16 · **Rulings:** R5, R12 · **Bundle:** A20, A27; PERSISTENCE § Recovery

F37: this task is over-sized for one subagent session (it was accumulating journal/close-guard machinery AND a whole panel-facing IPC surface) and is split into two self-contained parts, each with its own files/tests/mutation-check/verify/acceptance/commit and each leaving the tree green on its own commit. They are dispatched as two sessions: **Part A lands first** (it owns the recovery/pin state Part B's panel listing reads); **Part B depends on Part A's commit** and adds nothing Part A needs.

### Part A — journal, close guard, startup recovery and re-pin

**Files:** Create `src-tauri/src/editor/recovery.rs`, `src/components/editor/dialogs/CloseGuardDialog.vue`, `RecoveryDialog.vue`, `tests/editorCloseGuard.test.ts`, `tests/editorRecoveryDialog.test.ts`; modify `session_commands.rs` (journal after each acknowledged command; `discardRecovery`), `save_commands.rs` (`use_recovery`), `src-tauri/src/window_close.rs` + `window_close_guard.rs` (editor X → `emit_to("editor", "editor:closeRequested", json!({}))` instead of `hide()`; if the emit fails, fall back to hide and `log::warn!` — never strand the window), `src-tauri/src/lib.rs` (F34: wire the new startup re-pin sweep into `setup()`, named thread `editor-recovery-sweep`, after `run_screen_recovery` — same ordering rationale as that sweep: staging state must be settled before the editor's project store is reconciled against it), `src/roots/EditorRoot.vue` (close-guard/recovery-dialog rendering only — NOT the stash-draining behaviour, which is Part B), `AGENTS.md` (window-system editor bullet + events table; the project-store row in "Where state lives on disk" gains one sentence on the re-pin sweep).

**Behavior:**
- After every acknowledged `editor_execute`, a per-session debouncer (named thread `editor-journal`, 500 ms) writes `recovery.json` = `{schema: "vault-buddy-recovery/1", sessionRevision, savedRevision, project}` via `write_atomic_replacing`; a successful save whose revision equals the current revision deletes `recovery.json`. `editor_close_session(keep)` flushes the pending journal synchronously first.
- `editor_open_project(id, use_recovery: true)` loads `recovery.json` (validated like a project — malformed → `invalidProject`, file kept byte-identical) as the working copy, `persistedRevision` = the store's committed revision → the session starts dirty. `discardRecovery` deletes only `recovery.json` (owned file check).
- Close guard: on `editor:closeRequested` the root shows `CloseGuardDialog` when dirty (**Save project**, **Keep for later** (journal kept, session kept, window hides), **Discard changes** (discardRecovery then hide), Cancel) or when a render/publish job is live ("A render is running — keep it running in the background, or cancel it"); clean → hide directly via `editor_hide_window`. Never kills a native render on unmount.
- F34 (ADR R6, previously unimplemented anywhere): a startup re-pin sweep, `recovery::run_startup_repin(root, staging_dir) -> RepinReport { repinned: Vec<(projectId, base)>, orphaned: Vec<projectId> }`, scans every `editor-projects/<id>/sources.json` for a `SourceLocator::Staging{base}` whose staged sidecar's `editorProjectId` is absent or points elsewhere — a project ADR R6 calls "orphaned", reachable when a crash lands between `create_project` and `pin_staged` in a way Task 10's open-time adoption never observes (the user never reopens that exact base) or a sidecar gets rewritten by the coexisting legacy export path. It RE-PINS (writes `editorProjectId` back via `staging::write_sidecar`, the same owned-file discipline as Task 9's `pin_staged`) when the staged base still exists on disk, and only REPORTS (never deletes, never invents a pin) when the base is gone — matching this codebase's "report, never silently delete" posture. Wired into `lib.rs`'s `setup()`.

**Tests first:** Rust `journal_is_written_after_an_acknowledged_command`; `save_at_current_revision_removes_the_journal`; `save_of_an_older_revision_keeps_the_journal`; `malformed_journal_is_reported_and_kept`; `window_close_emits_close_requested_for_the_editor` (structural + unit on the routing fn); `startup_sweep_repins_an_unpinned_project_whose_staged_base_still_exists` (F34); `startup_sweep_reports_without_repinning_when_the_staged_base_is_gone` (F34). TS `dirty close offers Save, Keep and Discard`; `clean close hides immediately`; `a live render changes the close copy and never cancels by itself`.

**Mutation check:** delete the journal on ANY save → the older-revision test red; repin unconditionally without checking the staged base exists → `startup_sweep_reports_without_repinning_when_the_staged_base_is_gone` red.

**Verify:** targeted tests; `cargo clippy -p vault-buddy --all-targets -- -D warnings`; all gates. Checklist rows T5 (kill the app mid-edit, relaunch, Resume restores the last acknowledged edit), T6 (close with unsaved changes → Keep → reopen).

**Acceptance:** every acknowledged edit survives a kill-and-relaunch as a recovery-flagged project; no project is ever silently repinned onto a staged base that does not exist.

**Commit:** `feat(editor): recovery journal, close guard and startup re-pin`

### Part B — open_project_editor, list_tutorial_projects and the projects list

**Depends:** Part A (same commit range; reads the pin/recovery state Part A produces).

**Files:** Modify `src-tauri/src/editor_commands.rs` (F4: NEW `list_tutorial_projects(app) -> Vec<ProjectSummaryDto>` — a panel-callable, READ-ONLY listing that delegates to the SAME `editor::project_store::list_projects` helper Task 12's `editor_list_projects` uses, without opening or touching any session; it takes no `window: WebviewWindow` and does not call `require_editor_window` — it lives here, beside `open_capture_editor`, deliberately OUTSIDE `src-tauri/src/editor/` so neither Task 10's `authz_guard` structural scan nor Task 11's editor-only capability manifest ever sees or constrains it, and the panel calls it under its existing default capability, never the editor's; NEW `open_project_editor(app, project_file_id: String)` — sync, the `open_capture_editor` shape (stash → `editor:open` → `show()` → roll back the stash on a failed emit); `EditorRequest`'s inner `Option<String>` becomes `Option<EditorRequestKind>` where `EditorRequestKind { Staged(String), Project(String) }`; `take_editor_request` returns the widened `{kind: "staged"|"project", value}` shape (camelCase over IPC) instead of a bare string), `src-tauri/src/lib.rs` (`generate_handler!` gains `list_tutorial_projects` and `open_project_editor` — PANEL-capability commands, not editor-window commands, so they are granted through the panel's existing default capability and are NOT added to Task 11's editor-only manifest), `src/components/StagedCaptureList.vue` (a "Tutorial projects" block listing every `list_tutorial_projects()` result with title, updated date, a "Has unsaved changes" badge from `hasRecovery`, and a **Resume** action → `open_project_editor(projectFileId)` — F4: NO Discard button in the panel; discarding a project is a session-scoped, revision-aware operation (`editor_close_session(discardProject)`) that only the editor's own close/recovery UI, built in Part A, performs), `src/roots/EditorRoot.vue` (drain the widened stash: `{kind:"staged"}` → `editorProject.openStaged(value)`, `{kind:"project"}` → `editorProject.openProject(value, false)`), `tests/editorTutorialProjectsList.test.ts`; modify `AGENTS.md` (IPC table rows for `list_tutorial_projects`/`open_project_editor`, re-measured count) and `docs/superpowers/specs/2026-09-21-tutorial-editor-integration-design.md` §3.3 (F4: add both commands to the IPC table as panel-callable, non-editor-scoped rows — the ADR's table previously implied every editor-adjacent command is editor-window-only, which `editor_list_projects`/`editor_close_session` still are; these two are the documented exception).

**Behavior:**
- F4: `editor_list_projects` (Task 12) and `editor_close_session` (Task 10) are refused for the panel window by BOTH of R8's authorization layers — the native `require_editor_window` check and Task 11's capability manifest, which grants every `editor_*` command to the `editor` window alone — so a panel caller can never reach either. `list_tutorial_projects` is a genuinely separate, panel-callable, read-only command that shares only the underlying listing helper, never a session. Resume opens a session (`open_project_editor`, which does no more than `open_capture_editor` already does); Discard needs an open, revision-aware session and stays in the editor's own UI (Part A's `CloseGuardDialog`/`RecoveryDialog` surface, plus the existing discard flow inside an open project).
- The Record Screen picker's "Tutorial projects" block shows each project from `list_tutorial_projects`; Resume calls the new sync `open_project_editor(project_file_id)`.

**Tests first:** Rust `editor_request_stash_carries_staged_or_project`; `list_tutorial_projects_never_opens_or_touches_a_session` (calls the command, asserts no `EditorState` session map entry is created); `list_tutorial_projects_is_not_editor_window_scoped` (F4: a structural assertion that Task 10's `authz_guard` file scan under `src-tauri/src/editor/` does not include, and was never meant to include, `editor_commands.rs`); `open_project_editor_stashes_and_emits_like_open_capture_editor`. TS `Tutorial projects block lists projects with a Resume button and no Discard button` (F4); `recovery badge shows for hasRecovery projects`.

**Mutation check:** route `list_tutorial_projects` through `require_editor_window` → `list_tutorial_projects_is_not_editor_window_scoped` red, and a real panel call would then be wrongly refused — exactly the bug F4 exists to prevent.

**Verify:** targeted tests; `cargo clippy -p vault-buddy --all-targets -- -D warnings`; re-measure the IPC count; all gates.

**Acceptance:** a panel window can list and resume tutorial projects without ever calling an `editor_*` command; the editor window remains the only caller of `editor_list_projects`/`editor_close_session`.

**Commit:** `feat(editor): panel-callable tutorial project listing and resume`

---

## Task 38: Package format validation (pure)

**Package:** P09 · **F-IDs:** F-40 · **Depends:** 5 · **Rulings:** R9 · **Bundle:** A17, A22

**Files:** Modify `src-tauri/core/Cargo.toml` (`zip = { version = "2", default-features = false, features = ["deflate"] }` — pin the current 2.x the resolver picks; check `cargo deny check` licences), `src-tauri/deny.toml` only if the licence list needs an allowed entry (justify in the commit); create `src-tauri/core/src/editor/package.rs`.

**Behavior:**
- Manifest `package.json` (first entry): `{schema: PACKAGE_SCHEMA, projectId, savedAt, workspace: "workspace.json", media: [{assetId, path: "media/<assetId>.<ext>", size, sha256}], products: [{productId, path: "products/<productId>.mp4", size, sha256}]}`.
- `validate_entry_name(name) -> Result<(), String>`: relative, `/`-separated, no `..`, no leading `/` or `\`, no drive (`C:`), no UNC, no NUL/control chars, ≤ 255 bytes, only `package.json`, `workspace.json`, `media/…`, `products/…`.
- `inspect_archive<R: Read + Seek>(r) -> Result<PackageIndex, EditorError>`: enforce `MAX_PACKAGE_ENTRIES`, sum of declared uncompressed ≤ `MAX_PACKAGE_BYTES`, per-entry ratio ≤ `MAX_ENTRY_RATIO`, no duplicate case-folded names, no symlink/unix-mode-link entries, `package.json` first; parse+validate manifest; every manifest path present; workspace ≤ `MAX_PROJECT_JSON_BYTES` → `validate_envelope`; every asset referenced by `fingerprint::assets_referenced` (current + snapshots) is either in `media` or is `builtin`, or reported missing (lightweight semantics).
- `extract_entry_bounded(archive, name, dest_writer, max)` — counts bytes while copying, fails past `max` regardless of the declared size (a lying header cannot overrun).
- `write_package<W: Write + Seek>(w, manifest, workspace_json, media: impl Iterator<(name, Read)>)`: `package.json` first, JSON `Deflated`, media `Stored`, streaming.

**Tests first** (archives built in-memory with `zip::ZipWriter`): `round_trip_package_validates`; `traversal_names_are_rejected` (`../x`, `media/../../x`, `/abs`, `C:x`, `\\\\server\\s`); `duplicate_case_folded_names_are_rejected` (`media/A.mp4` + `media/a.mp4`); `too_many_entries_are_rejected`; `ratio_bomb_is_rejected` (1 MiB of zeros deflated); `lying_size_header_cannot_overrun_extraction`; `manifest_must_come_first`; `snapshot_only_assets_are_required` (A17); `invalid_workspace_rejects_the_whole_package` (A22).

**Mutation check:** accept `..` inside a later path segment → traversal test red.

**Verify:** `cargo test -p vault_buddy_core editor::package`; `cargo deny check`; `cargo machete .`; all gates.

**Acceptance / approval gate:** the task report quotes ADR R9 and asks the orchestrator to obtain the user's sign-off on the `zip` dependency before the branch merges (residual R-P2). Do not wait on it to finish the task.

**Commit:** `feat(core): validate portable tutorial project packages before extraction`

---

## Task 39: Portable and lightweight export/import, and the Save project dialog

**Package:** P09 · **F-IDs:** F-40 · **Depends:** 37, 38 · **Bundle:** SCREENS 08; A17, A22

**Files:** Create `src-tauri/src/editor/package_commands.rs`, `src/components/editor/dialogs/SaveProjectDialog.vue`, `tests/editorSaveDialog.test.ts`; modify registration files, `port.ts`, `EditorHeader.vue` (Save project menu: Save / Save a portable copy / Save a lightweight copy / Open a project file), `AGENTS.md`.

**Behavior:**
- `editor_export_package(window, session_id, expected_revision, format)`: freeze revision; native save dialog (`*.vbproject.zip` or `*.vbproject.json`) — cancel → `Ok(None)`; collect sources (`assets_referenced` incl. snapshots, products from `products.json` when available); portable: total media > `MAX_PACKAGE_MEDIA_BYTES` → `invalidRequest` "This project is too large for a portable file (limit 200 MiB). Save a lightweight copy instead."; write to `<chosen>.part-<rand>` in the SAME directory, fsync, `rename_noreplace` onto the chosen name — if the user chose an existing file (the dialog's overwrite confirmation), the target must be an existing `.vbproject.*` whose manifest `projectId` equals ours, then `write_atomic_replacing`-style replace; otherwise refuse `writeDenied` "Choose a new name — that file is not this project". Streaming, named thread `editor-package`. Returns `PackageReceipt`.
- `editor_import_package(window)`: native open dialog; `inspect_archive` (portable) or bounded JSON read + `validate_envelope` (lightweight); install into the store under the manifest's `projectId`, or a fresh id when that id exists; media extracted with `extract_entry_bounded` into `media\` and hashed/verified against the manifest; lightweight → sources left as `Media{file}` placeholders reported in `missing`. Nothing is installed if any step fails (build in `editor-projects\.<newId>.importing\`, rename into place last; a startup sweep in `recovery.rs` deletes stale `.importing` dirs older than 1 h).
- `SaveProjectDialog`: explains portable vs lightweight, included originals, retained products, reconnection; radio targets normal size; scrollable body with sticky heading/actions; states pending/success/failure/cancel; copy says "Saved to <file name>" only after a receipt.

**Tests first:** Rust (tempdir; the dialog is behind a `PathChooser` seam): `portable_export_then_import_reopens_identically`; `lightweight_import_lists_missing_media`; `export_refuses_to_replace_an_unrelated_file`; `oversized_portable_export_suggests_lightweight`; `failed_import_installs_nothing` (corrupt entry mid-archive → no new project dir, no `.importing` left); `existing_project_id_imports_as_a_copy`. TS `dialog shows success only after a receipt`; `cancelled dialog shows no success`; `portable warns that originals are included`.

**Mutation check:** install before verifying hashes → the corrupt-entry test red.

**Verify:** targeted tests; all gates. Checklist row T7: export portable on Windows, move to another folder, import, play.

**Commit:** `feat(editor): portable and lightweight project files`

---

## Task 40: Missing media and reconnect

**Package:** P09 · **F-IDs:** F-03 · **Depends:** 39 · **Bundle:** A19; SCREENS 07

**Files:** Create `src-tauri/core/src/editor/relink.rs` (F32: pure matching belongs in `core`, not the shell — "logic without Tauri types goes in core" is a repo-wide rule and `match_candidates` takes no Tauri type; the command itself, which DOES need `WebviewWindow`/dialog/IO, still lives in `media_commands.rs`), `src/components/editor/dialogs/ReconnectDialog.vue`, `tests/editorReconnect.test.ts`; modify `src-tauri/core/src/editor/mod.rs` (`pub mod relink;`), `media_commands.rs` (register `editor_relink_media`, calling `vault_buddy_core::editor::relink::match_candidates`), registration files, `port.ts`, `AGENTS.md`.

**Behavior:**
- Pure `match_candidates(expected: &[(assetId, SourceRecord)], candidates: &[CandidateFacts]) -> RelinkReport { matched: [(assetId, candidateIdx)], ambiguous: [(assetId, [candidateIdx])], unmatched: [assetId], mismatched: [(assetId, candidateIdx, reason)] }`: exact sha256 match wins; without a hash, size AND duration (±50 ms) AND kind must match; two or more equally good candidates → ambiguous (never auto-selected); a candidate matching no identity → mismatched with the reason ("different duration: 12.4 s vs 31.0 s").
- `editor_relink_media(window, session_id, asset_ids)`: native multi-open dialog; probe + hash candidates on `editor-relink`; copy matched files into `media\` and update `sources.json`; applies `InternalCommand::RelinkAssets` (bumps the revision; the graph's ids and cue links unchanged); ambiguous/mismatched left untouched and returned for the user to resolve one by one (a second call with a single asset id and a single chosen file; a mismatched candidate needs `confirmReplace: true` and then gets a NEW asset identity recorded as a replacement).
- `ReconnectDialog`: expected metadata per missing asset, batch results, per-asset resolution of ambiguity.

**Tests first:** `hash_match_beats_name`; `same_size_same_duration_twice_is_ambiguous_and_unselected` (A19); `different_duration_is_mismatched_with_a_reason`; `relink_preserves_clip_and_cue_ids`; `mismatched_replace_requires_confirmation`; TS `ambiguous assets require an explicit choice`.

**Verify:** targeted tests; all gates.

**Commit:** `feat(editor): reconnect missing originals without guessing`

---

## Task 41: The render plan (pure)

**Package:** P10 · **F-IDs:** F-04, F-05, F-41, F-42 (range) · **Depends:** 3, 29–36 · **Rulings:** R1, R15 · **Bundle:** NATIVE-MEDIA § Render plan; A06, A13

**Files:** Create `src-tauri/core/src/editor/render_plan.rs`, `render_plan_audio.rs`; modify `editor/mod.rs`.

**Behavior:** `pub fn plan(project: &Project, sources: &BTreeMap<String, PlanSource>, range: Option<(u64,u64)>) -> Result<RenderPlan, EditorError>` where `PlanSource { input_index, has_video, has_audio, width, height, is_image }`.
- `RenderPlan { canvas, duration_ms, inputs: Vec<PlanInput>, video_layers: Vec<VideoLayer> /* bottom→top */, audio: Vec<AudioContribution>, cues: Vec<PlannedCue>, captions: Option<PlannedCaptions>, chapters: Vec<(u64, String)>, cards: Vec<PlannedCard>, master_gain }`.
- Visible video tracks only (hidden excluded); ordering: track index 0 = TOP; `VideoLayer { input, output_start, output_end, source_in, source_out, speed, box (px, even-rounded), fit, frame_shape, rotation, mirror, flip_y, crop, opacity, fade_in, fade_out, adjustments, transition_in: Option<(kind, dur)> }`.
- Audio: from video clips with audio (unless clip muted / track muted / not soloed while any solo) and audio clips; gain = clip × track × (master applied once at the end); `AudioContribution { input, output_start, source_in, source_out, speed, preserve_pitch, gain, fade_in, fade_out, curve, crossfade_in }`. `Workspace.monitor_muted` is not an input — the function signature makes that impossible (A06).
- Cues/captions: mapped to OUTPUT time via `time::cue_output_span`; captions only if `enabled && burn_in`.
- Range: every timestamp shifted by `-start` and clipped to `[0, end-start)`; items fully outside dropped; chapters clipped and re-based (A13).
- Missing source → `sourceMissing` listing asset ids in `retained_asset_ids` (never a silent black layer).
- `pub fn is_identity(&self) -> bool` (R1): exactly one video layer, one input, full source range from 0, `speed 1`, `canvas_is_exact` true for the source dims (F5: aspect match, NOT `canvas == source dims` — a migrated 1920×1080 capture on its nearest `1280×720` canvas must still be identity-eligible, or the fast path is unreachable for every real capture; the remux itself runs at SOURCE resolution, not the canvas's, when this path is taken), box = full canvas, no fades/transitions/cues/captions/cards/adjustments/crop/rotation/mirror/flip/shape, opacity 1, one audio contribution at unity gain with no fades, master 1.

**Tests first:** `upper_tracks_are_composited_last`; `hidden_tracks_are_excluded`; `solo_silences_non_soloed_tracks`; `monitor_mute_cannot_affect_the_plan` (compile-level: `plan` has no workspace parameter — plus a behavioural test that the same project gives the same plan); `range_rebases_every_timestamp_and_clips_chapters` (A13: range 10_000–20_000, chapter at 12_000 → 2_000; chapter at 25_000 dropped); `missing_source_is_reported_not_blacked_out`; `identity_detection_matches_is_untouched` (F5: for every `timeline-cases.json` case migrated via Task 4, built with `has_audio: true` and dimensions that are exact-aspect for one of the four canvases — a `has_audio: false` or off-aspect fixture would make the cross-check fail regardless of the plan/timeline agreement, which is not what this test is proving: `plan(..).is_identity() == Timeline::is_untouched(...)`); `trimmed_front_is_not_identity` (the GAP-136 front-trim trap); `speed_two_halves_the_layer_and_its_cues`.

**Mutation check:** order layers top-first → compositing test red; forget the range shift for captions → A13 test red.

**Verify:** `cargo test -p vault_buddy_core editor::render_plan`; all gates.

**Commit:** `feat(core): a pure render plan from an acknowledged revision`

---

## Task 42: Extract the ffmpeg runner and build the video graph

**Package:** P10 · **F-IDs:** F-04, F-16, F-17, F-19, F-23, F-31, F-37, F-39 · **Depends:** 41 · **Rulings:** R1

**Files:** Create `src-tauri/screen/src/ffmpeg_run.rs` (extracted from `export.rs`: spawn, `-progress` pipe, bounded stderr drain, timed cancel poll, kill+reap, truncated-output removal), `src-tauri/screen/src/render/mod.rs`, `video_graph.rs`, `video_layers.rs`, `expr.rs`; modify `src-tauri/screen/src/export.rs` (call `ffmpeg_run`; update the structural test that splits on `"\nfn run("` to point at the new file — keep its assertions), `screen/src/lib.rs`.

**Behavior:**
- Extraction first, behaviour-neutral: all existing `export` tests and `export_roundtrip.rs` stay green unchanged.
- `expr.rs`: `escape_filter_value(s)` (ffmpeg filtergraph escaping of `\ ' : , ; [ ]`), `seconds(ms)` (reuse `ms_to_ffmpeg_seconds`).
- `video_layers.rs`: per `VideoLayer` a chain: `[i:v]trim=start=S:end=E,setpts=(PTS-STARTPTS)/speed` (images: `-loop 1 -t D` input + `setpts`), `transpose`/`hflip`/`vflip` per rotation/mirror/flip, `crop` from crop zoom/anchor, `scale` to box with `force_original_aspect_ratio=increase|decrease` + `crop`/`pad` per `cover`/`contain`, frame mask (`format=rgba,geq=...` circle / rounded via `min(hypot)` expression; rectangle none), `eq=brightness=(b-1):contrast=c:saturation=s`, grayscale → `hue=s=0` blended by amount via `colorchannelmixer`, sepia via `colorchannelmixer` matrix scaled by amount, opacity + fades via `format=rgba,colorchannelmixer=aa=<o>,fade=t=in:st=0:d=F:alpha=1,fade=t=out:st=…:alpha=1`, then `setpts=PTS+output_start/TB`.
- `video_graph.rs`: `build_video_graph(plan) -> (Vec<String> /* extra input args */, String /* filter_complex video part */, String /* final label */)`: base `color=c=black:s=WxH:r=30:d=D`; `overlay=x=X:y=Y:eof_action=pass:enable='between(t,a,b)'` layer by layer bottom→top; dissolve transitions via `xfade=transition=fade:duration=d:offset=o` between the two same-track layers before overlaying; cards via `color=c=<bg>` sources (text comes from the ASS pass, Task 43); zoom cues via a final `crop=w='…':h='…':x='…':y='…',scale=W:H` with piecewise `if(between(t,…))` expressions and eased ramps; the ASS overlay hook is a named label `[vcomp]` that Task 43 extends with `ass=`.
- `pub fn render_args(plan, inputs: &[PathBuf], dest, ass_path: Option<&Path>, settings: &EncodeSettings) -> Vec<String>`; identity plan → `ffmpeg_args::remux_args` (fast path, R1).

**Tests first** (golden strings, asymmetric fixtures — a 1280×720 canvas with a 0.19×0.338 layer at (0.775, 0.06)): `extraction_keeps_export_behaviour` (existing suite); `layers_overlay_bottom_to_top`; `cover_scales_up_and_crops_contain_pads`; `rotation_ninety_uses_transpose_one`; `mirror_and_flip_map_to_hflip_vflip`; `speed_two_divides_pts`; `fade_is_alpha_not_black`; `dissolve_uses_xfade_with_the_overlap_offset`; `zoom_expression_is_piecewise_and_eased`; `escape_filter_value_escapes_colons_and_quotes`; `identity_plan_remuxes`; `box_is_even_rounded` (odd widths rounded down to even).

**Mutation check:** overlay top→bottom → ordering test red; drop `alpha=1` → fade test red.

**Verify:** `cargo test -p vault_buddy_screen`; `cargo clippy -p vault_buddy_screen --all-targets -- -D warnings`; all gates.

**Commit:** `feat(screen): build the render's video filter graph from the plan`

---

## Task 43: The ASS generator for cues, captions and cards

**Package:** P10 · **F-IDs:** F-27–F-33, F-35 (burn-in), F-37 (card text) · **Depends:** 41, 42, 35

**Files:** Create `src-tauri/screen/src/render/ass.rs`; modify `video_graph.rs` (`[vcomp]ass=<escaped path>:fontsdir=…` hook when cues/captions/cards exist), `render/mod.rs`.

**Behavior:** `build_ass(plan) -> String` — `[Script Info]` with `PlayResX/PlayResY` = canvas (so coordinates are canvas px), `WrapStyle: 2`, `ScaledBorderAndShadow: yes`; styles `Callout`, `Caption`, `Step`, `CardTitle`, `CardSubtitle` (font `Segoe UI` with fallback `Arial` documented; font size from the cue); one `Dialogue` per cue with `\pos`, `\an7`, colours in ASS `&HAABBGGRR` (converted from `#rrggbb`, tested), `\fad(in,out)` 150 ms for cues; text escaped (`{`, `}`, `\`, newlines → `\N`); drawings with `\p1`: arrow (shaft rect + head triangle from the SAME numbers as `cueGeometry.arrowPath` — reads `tests/fixtures/editor-arrow-cases.json`, 4 rows, size-guarded), highlight (outline via two nested rects drawn as a ring), spotlight (four rects, `\1a` from `dim`), step (circle via bezier approximation + number text), mask (opaque rect, no fade — a privacy cover must not fade in over visible content: `\fad(0,0)`); captions positioned top/bottom with an optional box (`BorderStyle=3`). Times formatted `H:MM:SS.cc` rounding DOWN the start and UP the end to the centisecond so a cue never shows before its span, and half-open ends never bleed into the next cue (tested at a boundary).

**Tests first:** `colour_converts_to_ass_bgr_order` (`#112233` → `&H00332211`); `times_floor_start_and_ceil_end` (`1234 ms → 0:00:01.23`, end `1234 → 0:00:01.24`); `text_is_escaped` (`{\b1}` literal survives as text); `arrow_matches_the_shared_fixture`; `mask_has_no_fade`; `captions_at_bottom_use_the_bottom_alignment`; `play_res_equals_the_canvas` (portrait 720×1280); `range_plan_emits_rebased_times`.

**Mutation check:** RGB order → colour test red; round the end down → the boundary test red.

**Verify:** `cargo test -p vault_buddy_screen render::ass`; all gates.

**Commit:** `feat(screen): render teaching cues, captions and card text as ASS`

---

## Task 44: The audio graph builder

**Package:** P10 · **F-IDs:** F-05, F-16, F-18, F-19 · **Depends:** 41, 42, 29

**Files:** Create `src-tauri/screen/src/render/audio_graph.rs`; modify `render/mod.rs`, `video_graph.rs` → `render_args` combines both.

**Behavior:** per `AudioContribution`: `[i:a]atrim=start:end,asetpts=PTS-STARTPTS`, speed with `preserve_pitch` → `atempo` chain (each factor within 0.5–2: 4 → `atempo=2,atempo=2`; 0.25 → `atempo=0.5,atempo=0.5`), without → `asetrate=48000*s,aresample=48000`; `volume=<gain>`; `afade=t=in|out:st=:d=:curve=` with the fixed mapping `linear→tri`, `smooth→hsin`, `equal-power→qsin` (F23: `hsin`, not `esin` — ffmpeg's `esin` is a different, non-smoothstep-shaped curve; `hsin` is the closer approximation of TS's `smooth` (smoothstep, `3u²−2u³`), and the residual gap between the two is recorded under GAP-N3, not claimed as exact; table-tested against Task 29's curve names, which list the SAME three ffmpeg names `tri`/`hsin`/`qsin`); `adelay=<output_start>|<output_start>`; crossfades via `acrossfade=d=:c1=qsin:c2=qsin` between the paired clips before delaying; all into `amix=inputs=N:normalize=0:dropout_transition=0`, then `volume=<master>`, then `alimiter=limit=0.891` (−1 dBFS sample-peak headroom — documented as a peak limiter, not loudness normalisation), `aresample=48000`, stereo. No contributions → `anullsrc=r=48000:cl=stereo` trimmed to the duration (the output always has audio, matching the capture format).

**Tests first:** `atempo_chain_splits_four_into_two_twos`; `quarter_speed_splits_into_two_halves`; `pitch_off_uses_asetrate`; `fade_curves_map_to_ffmpeg_names` (3 rows); `equal_power_crossfade_uses_qsin_both_sides`; `amix_does_not_normalise`; `delay_applies_to_both_channels`; `silent_project_gets_anullsrc`.

**Mutation check:** `normalize=1` → test red.

**Verify:** `cargo test -p vault_buddy_screen render::audio_graph`; all gates.

**Commit:** `feat(screen): mix render audio with fades, speed and crossfades`

---

## Task 45: The render round trip in CI, and the capability probe

**Package:** P10 · **F-IDs:** F-41, F-42, F-04, F-05, F-17–F-19, F-27–F-33 · **Depends:** 42, 43, 44 · **Bundle:** A07, A29; NATIVE-MEDIA § fixtures

**Files:** Create `src-tauri/screen/src/render/run.rs`, `src-tauri/screen/tests/render_roundtrip.rs`; modify `src-tauri/screen/src/render/mod.rs` (F21: `pub struct FfmpegCapabilities { filters: BTreeSet<String>, encoders: BTreeSet<String> }` and `pub fn parse_filters_output(text: &str) -> FfmpegCapabilities` live HERE, in `screen`, not the shell — `screen::render::run::render_refusal` consumes the type and `screen` cannot depend on the shell crate), `src-tauri/src/ffmpeg.rs` (F21: `probe_capabilities(tools)` stays in the shell but is now a thin wrapper — it spawns `ffmpeg -hide_banner -filters`/`-encoders` (the process/IO the shell owns) and calls `screen::render::parse_filters_output` to build the `screen`-owned `FfmpegCapabilities`, rather than defining the type itself).

**Behavior:**
- `run.rs`: `render(req: RenderRequestNative<'_>, cancel, on_progress) -> Result<RenderOutcome, ScreenError>` = write `cues.ass` into the job dir (if needed) → `render_args` → `ffmpeg_run::run` → verify output (ffprobe duration within ±1 frame + 40 ms of `plan.duration_ms`, ≥ 1 video and 1 audio stream) → `RenderOutcome { duration_ms, remuxed }`. `required_filters(plan) -> BTreeSet<&'static str>` (`overlay, xfade, acrossfade, atempo, amix, alimiter, geq, ass, fade, afade`), and `render_refusal(plan, caps: &FfmpegCapabilities) -> Option<String>` naming the missing filter/encoder (`encoderUnavailable`).
- `render_roundtrip.rs` (skips VISIBLY without ffmpeg; CI installs it): synthesize inputs with lavfi — A: `testsrc2` 1280×720 4 s + `sine=f=440`; B: solid `color=red` 640×360 4 s; C: `sine=f=1000` audio-only 4 s — then plans built from real `Project`s via `core::editor::render_plan`:
  1. `upper_layer_covers_lower_layer` — B as a 0.25×0.25 box at (0.7, 0.05) over A: sample pixel at (1100, 90) at t=1 s is red (R>200, G<60, B<60); pixel at (100, 600) is not red.
  2. `hidden_track_is_absent` — hide B's track: the same pixel is not red.
  3. `two_audible_sources_both_present` — A + C: band energy at 440 Hz and at 1000 Hz both above threshold (FFT-free check: `ffmpeg -af bandpass=f=…:w=50,astats` RMS).
  4. `audio_fade_ramps` — 1 s fade-in on A: RMS of 0–0.2 s < 30 % of RMS of 1.5–2 s.
  5. `dissolve_blends_midpoint` — A→B dissolve 1 s: midpoint frame pixel is neither pure A nor pure B.
  6. `split_and_reorder_timing` — input D is `color=blue` 2 s concatenated with `color=green` 2 s (lavfi `concat`); a project with D split at 2 s and the halves swapped: output pixel at t=0.5 s is green and at t=2.5 s is blue.
  7. `text_cue_is_burned_in` — a white text box cue on black: region pixel mean increases during the cue span only (skips visibly when `ass` filter is missing).
  8. `range_render_starts_at_zero` — range 1000–3000: output duration 2000 ± 40 ms and first frame equals source t=1 s.
  9. `speed_two_halves_duration`.
  10. `identity_render_is_a_remux` — `RenderOutcome.remuxed == true` and duration equals source.

**Tests first:** the ten above, written first against the unimplemented `run.rs` (compile-failing), then implemented. Plus unit: `required_filters_lists_ass_only_when_cues_exist`; `refusal_names_the_missing_filter`; `parse_filters_output_reads_names`.

**Mutation check:** swap overlay order in `video_graph` → test 1 red on real decoded pixels (record the output in the task report).

**Verify:** `cargo test -p vault_buddy_screen --test render_roundtrip -- --nocapture` (confirm NO skip lines on this host if ffmpeg is installed; otherwise state it and rely on CI); all gates. Checklist rows T8–T10 (R-H2: Windows ffmpeg render of a real staged fMP4 with cues; libass fonts; hardware encoder).

**Commit:** `test(screen): decode real renders to prove layering, mixing, fades and cues`

---

## Task 46: Render jobs, the products ledger and the shutdown gate

**Package:** P10 · **F-IDs:** F-41, F-42 · **Depends:** 45, 25, 5 · **Rulings:** R5, R12 · **Bundle:** A16

**Files:** Create `src-tauri/src/editor/render_jobs.rs`, `src-tauri/src/editor/render_commands.rs`; modify `src-tauri/src/shutdown_gate.rs` (fourth predicate), the quit workers (call `render_jobs::cancel_all_bounded(Duration::from_secs(5))` BEFORE the capture finalizes, next to `export_shutdown::cancel_if_exporting`), `save_commands.rs` (`record.products` from the ledger), registration files, `AGENTS.md` (shutdown-gate paragraph, IPC), `src-tauri/src/editor/mod.rs`, `docs/Gaps.md` (F36: a new GAP-N-series entry — "the 40-product cap has no removal UI; the 41st render refuses instead of prompting to delete an older product" — the Behavior bullet below said this was "recorded in Gaps" without ever adding the entry), `port.ts`.

**Behavior:**
- `editor_start_render(window, app, request, on_progress)`: validate session + `expectedRevision == revision` (a pending edit cannot be rendered — `revisionConflict`); resolve ffmpeg (`encoderUnavailable` naming the fix); build `PlanSource`s from `sources.json`; `render_plan::plan`; capability refusal; free-space check (`disk::free_bytes`, `export_size_estimate_bytes` on the canvas, `export_space_shortfall` — `None` = proceed); register a job (`JobRecord` phases `queued → preparing → rendering → publishing → complete`); named thread `editor-render`; output into `jobs\<jobId>\out.mp4.part`; on success move (`rename_noreplace`, same volume) to `products\<productId>.mp4`, then `products.json` commit (`fingerprint::new_product` with the frozen snapshot, `render_range`), then terminal `complete{productId}`. Cancel → kill, delete the part, terminal `cancelled`, project untouched. Failure → terminal `failed{error}`, part deleted. One render per session at a time (`invalidRequest` "A render is already running").
- `editor_get_products(window, session_id)` → `ProductDto[]` with `available` = file exists. `editor_restore_product(window, session_id, expected_revision, product_id, command_id)` → `InternalCommand::RestoreSnapshot` (new revision, product untouched, undo returns to the pre-restore graph).
- Products are never opened for writing after the ledger records them; `products.json` capped at 40 (the 41st render refuses with "Remove an older product first" — removal is a later product decision; recorded in Gaps).
- `blocks_shutdown()` true while any job is `rendering`/`publishing`.

**Tests first:** Rust (fake runner seam): `render_of_a_stale_revision_is_refused`; `completed_render_records_an_immutable_product` (A16: bytes + ledger entry; a later edit + restore leaves the product file hash unchanged); `cancel_deletes_the_part_and_keeps_the_project`; `failed_render_leaves_no_product_and_no_part`; `only_one_render_per_session`; `ledger_survives_without_saving_the_project` (render, drop the session unsaved, reload → product listed); `shutdown_is_blocked_while_rendering` (+ the existing three-predicate composition test extended to four); `quit_cancels_a_render_before_finalizing_captures` (structural: order of calls in both quit workers).

**Mutation check:** record the ledger before the move → the failed-render test red (ledger entry with no file).

**Verify:** targeted tests; `cargo clippy -p vault-buddy --all-targets -- -D warnings`; all gates.

**Commit:** `feat(editor): render jobs that produce immutable products`

---

## Task 47: Render dialog, product library, review and restore

**Package:** P10 · **F-IDs:** F-41, F-42 · **Depends:** 46, 19 · **Bundle:** SCREENS 09; A16

**Files:** Create `src/components/editor/dialogs/RenderDialog.vue`, `src/components/editor/library/ProductLibrary.vue`, `src/components/editor/preview/ProductPlayer.vue`, `tests/editorRenderDialog.test.ts`, `tests/editorProductLibrary.test.ts`; modify `EditorHeader.vue` (Render enabled), `PreviewToolbar.vue` (Review = short range render of the current selection or ±5 s around the playhead), `editorJobs.ts`, `port.ts`, `src-tauri/src/editor/render_jobs.rs` and/or `render_commands.rs` (F18: Review's output path and cleanup — below).

**Behavior:** `RenderDialog`: product name (default `<title> v<n>`), quality (low/balanced/high), whole or range (numeric start/end in output time, defaulting to the in/out range if set), the open checks summary (Task 54 fills; placeholder list until then), the statement "Your originals and this project are not changed"; progress with phases and Cancel; completion shows **Watch rendered file**, **Publish to vault…** (Task 48), **Render another**. The bar reaches 100 % only on a `complete` terminal, never on `fraction: 1`. `ProductLibrary`: product cards (name, revision, range, created, available/missing), **Watch** plays the actual file through `editor_media_url({productId})` in `ProductPlayer` (a plain `<video>` — never the editable preview), **Restore this edit** → confirm → `editor_restore_product`. F18: **Review renders write `jobs\<jobId>\cache\review-<jobId>.mp4`** (inside the pinned `$APPLOCALDATA/editor-projects/*` asset scope, so `editor_media_url` can still serve it, but NOT `products\<productId>.mp4` and NOT a `products.json` ledger entry) — a Review is a disposable preview render, not a Rendered Product, and at the 40-product cap with no removal UI (Task 46, GAP-N-series) a Review that consumed a ledger slot would make rendering silently impossible after enough reviews. The review file is deletable on job cleanup like any other job-scoped cache artifact and never appears in `ProductLibrary`.

**Tests first:** `fraction 1 without a terminal does not show complete`; `cancel sends editor_cancel_job and shows cancelled, not failed`; `watch uses the product url, not the preview`; `restore asks for confirmation and sends restore_product`; `a missing product file shows unavailable but keeps its lineage`; `review renders never add a ProductLibrary entry` (F18: a Review job's terminal payload carries no `productId`, and forty consecutive Reviews still leave `editor_get_products` empty).

**Verify:** targeted tests; `npm run build && npm run test:e2e`; all gates.

**Commit:** `feat(editor): render dialog, product library and real-file review`

---

## Task 48: Publish to vault, the companion note and subtitle export

**Package:** P10 · **F-IDs:** F-43, F-35, F-36 · **Depends:** 46, 47, 36 · **Rulings:** R13 · **Bundle:** A13, A21

**Files:** Create `src-tauri/core/src/editor/note.rs`, `src-tauri/src/editor/publish.rs`, `src/components/editor/dialogs/PublishDialog.vue`, `tests/editorPublishDialog.test.ts`; modify `media_commands.rs` (`editor_export_subtitles`), registration files, `port.ts`, `recovery.rs` (publish-journal report), `src-tauri/src/editor/render_jobs.rs` (F19: publish registers in the same `JobRegistry` render jobs use — see Behavior), `src-tauri/src/export_worker/vault_dir.rs` (F20: widen `prepare_export_dir`/`rollback_export_dir` from `pub(super)` to `pub(crate)`), `src-tauri/core/src/screen_note.rs` (F20: widen `embed` from private to `pub(crate)`), `AGENTS.md` (vault domain: **ten** sanctioned writes; the tenth described like the ninth; shutdown-gate paragraph gains publish), `docs/Gaps.md` (F36: GAP-N4, below), `CONTEXT.md` is NOT touched here (Task 60).

**Behavior:**
- `note.rs`: `render_tutorial_note(meta: &TutorialNoteMeta, mp4_file_name: &str) -> String` — frontmatter (`type: Tutorial`, `created-by: vault-buddy`, `recorded`, `duration`, `product`, `revision`, `range`?, plus the vault's `screen_extra_frontmatter` through `core::template::render_extra_frontmatter` with reserved keys `type, created-by, recorded, duration, product, revision, range, chapters`), the embed through `core::screen_note::embed` (wikilink-metachar safe), a `## Chapters` list at OUTPUT time (range-shifted, `MM:SS title`, titles escaped as Markdown text), the body template when set. All strings YAML-quoted via the existing helpers.
- F20: `export_worker`'s `vault_dir::{prepare_export_dir, rollback_export_dir}` are `pub(super)` and `core::screen_note::embed` is private — neither is callable from `src-tauri/src/editor/publish.rs` or `note.rs`, which live in different modules. This task widens both to `pub(crate)` (crate-scoped, not `pub`, since nothing outside these two crates should call them) — Task 59 still does the eventual file MOVE into `src-tauri/src/editor/vault_dir.rs`; this task only changes visibility so publish compiles before that move happens.
- `editor_publish_product(window, app, session_id, product_id, destination)`: product exists and `available`; vault by id (containment via `safe_recording_root` + `assert_path_inside_vault`), folder default `screen_capture_root()`, dated per `destination.dated`; `vault_dir::prepare_export_dir`; free-space check; journal `jobs\<pubId>\publish.json` (`{step: "reserved"|"video"|"note"|"complete", video, note}`) updated after each step; `core::screen_capture_paths::reserve_final_screen` pairwise; copy product → same-dir temp → `rename_noreplace` with ` (N)` retry; note written with the FINAL video name (A21); note failure → `warning`, video kept; any failure before the video lands → `rollback_export_dir`. Returns `PublishReceipt`. The product and the project are never modified.
- F19: publish (the tenth sanctioned vault write) registers itself in Task 46's `JobRegistry` under `kind: "publish"` (already the Contract reference's `JobProgressDto.kind` union — no new kind is invented) BEFORE it starts writing, and Task 46's `blocks_shutdown()` already checks any job whose phase is in `{rendering, publishing}` — publishing a product is therefore covered by the SAME shutdown-gate predicate a render is, closing the gap where quitting mid-publish orphaned the tenth write the way quitting mid-render used to orphan the ninth. (Task 60 amends the ADR §3.3 kind list to name `"publish"` explicitly, since the ADR text predates this wiring.)
- F36: `docs/Gaps.md` gains GAP-N4 — "publish has no resume-from-journal: an interrupted publish is reported (`recovery.rs`) but the user must republish from scratch rather than continue from the last completed step" — the honest statement of what the journal buys (a truthful report) versus what it does not (automatic resumption).
- `recovery.rs`: a stale publish journal not at `complete` is REPORTED on the next open ("A publish was interrupted: the video was saved as … but its note was not") — never auto-deleted, never auto-retried.
- `editor_export_subtitles(window, session_id, format)`: native save dialog; cues in OUTPUT time (`captions_io::export_srt/vtt` over the whole timeline); owned temp + `rename_noreplace`; returns the chosen file name.
- `PublishDialog`: vault picker (defaults to `project.destination.vault` — the capture's vault, F-01), folder, dated toggle, create note; result shows the landed names and an Open action (`open_screen_capture`).

**Tests first:** Rust `note_embeds_the_final_reserved_name` (collision → ` (1)` in both file and embed — A21); `note_lists_chapters_at_range_relative_time`; `note_escapes_markdown_and_yaml` (title `a: "b" [[c]] #d`); `publish_never_overwrites` (pre-existing same-name `.mp4` and `.md` → ` (1)`); `note_failure_keeps_the_video_and_warns`; `failure_before_the_video_rolls_back_created_dirs`; `interrupted_publish_is_reported_not_deleted`; `publish_leaves_the_product_byte_identical`; `subtitle_export_uses_output_time`; `shutdown_is_blocked_while_publishing` (F19: extends Task 46's four-predicate composition test to cover a `publishing`-phase job, not only `rendering`); `publish_registers_as_a_job_before_writing_the_video` (F19: `editor_get_jobs` reports `kind:"publish"` with a live phase during the copy, before `complete`). TS `publish defaults to the capture's vault, not the panel's selection`.

**Mutation check:** write the note with the reserved-before-retry name → A21 test red.

**Verify:** targeted tests; `cargo clippy -p vault-buddy --all-targets -- -D warnings`; all gates. Checklist rows T11 (publish into a vault with an existing same-name file), T12 (disk full during publish — R-H5).

**Commit:** `feat(editor): publish rendered products and companion notes into a vault`

---

## Task 49: Webcam take pipeline (native side)

**Package:** P07 · **F-IDs:** F-20 · **Depends:** 25 · **Rulings:** R10 · **Bundle:** A08, A09; PERSISTENCE § Privacy

**Files:** Create `src-tauri/src/editor/webcam_commands.rs`, `src-tauri/core/src/editor/take.rs` (pure take state machine + sequence validation); modify registration files, `port.ts`, `AGENTS.md`, `docs/Gaps.md` (GAP-N1 takes do not claim `CaptureGuard`; GAP-N2 no native permission handler).

**Behavior:**
- `take.rs`: `TakeState { Recording{next_seq, bytes}, Finished, Discarded }`; `accept_chunk(seq, len) -> Result<(), TakeError>` (seq must equal `next_seq`; each chunk ≤ 1 MiB; total ≤ 4 GiB); `finish(last_seq)` requires `last_seq + 1 == next_seq`.
- `editor_webcam_begin(window, session_id, mime_type)`: `mime_type` ∈ {`video/webm;codecs=vp8,opus`, `video/webm;codecs=vp9,opus`, `video/webm`}; refused while `CaptureGuard::active()` is `Some` ("Stop the screen recording first" — R10's UI rule backed natively); creates `takes\.<takeId>.webm.part` exclusively; returns `{takeId}`.
- `editor_webcam_append(window, request: tauri::ipc::Request<'_>)`: body must be `InvokeBody::Raw`; headers `x-editor-session`, `x-editor-take`, `x-editor-seq`; validates via `take.rs`; appends (buffered writer, flush per chunk); any error marks the take failed (the part is kept for `finish`/`discard` to resolve).
- `editor_webcam_finish(window, session_id, take_id, last_seq)`: validates the sequence; ffmpeg `-c copy` remux `.part` → `takes\<takeId>.webm` (MediaRecorder WebM lacks a seekable index); probe (duration, dims, audio); SHA-256; `sources.json` `Takes{file}`; `InternalCommand::AddAssets` (asset `take-…`, name "Webcam take N"); returns `TakeDto`. Without ffmpeg: keeps the raw `.webm` (still playable in the preview) and returns `encoderUnavailable` with `retainedAssetIds: [assetId]` after registering it — the take is never lost.
- `editor_webcam_discard(window, session_id, take_id)`: removes the owned `.part`/`.webm` only if no clip references the asset (else `invalidRequest`); unsaved takes on session close are reported by the close guard (Task 37 copy extended: "You have an unsaved webcam take").

**Tests first:** Rust `sequence_gaps_are_refused`; `oversized_chunk_is_refused`; `finish_requires_the_last_sequence`; `append_rejects_json_bodies`; `begin_is_refused_during_a_screen_capture`; `finish_without_ffmpeg_keeps_and_registers_the_raw_take` (A09); `discard_refuses_a_take_in_use`; `take_becomes_an_independent_asset_not_part_of_the_screen_clip` (A08 core half: new asset, no change to `src` clips).

**Mutation check:** accept any `seq ≥ next_seq` → gap test red.

**Verify:** targeted tests; all gates.

**Commit:** `feat(editor): stream webcam takes into the project store`

---

## Task 50: Webcam dialog and presenter placement

**Package:** P07 · **F-IDs:** F-20, F-21 · **Depends:** 49, 31 · **Bundle:** SCREENS 05; A08, A09

**Files:** Create `src/components/editor/dialogs/WebcamDialog.vue`, `src/editor/webcamRecorder.ts`, `tests/editorWebcamDialog.test.ts`, `tests/webcamRecorder.test.ts`; modify `MediaLibrary.vue` (Webcam entry), `port.ts` (raw-body `invoke` with headers), `docs/superpowers/specs/2026-09-21-tutorial-editor-windows-verification.md` (rows T13–T17: R-H1).

**Behavior:**
- `webcamRecorder.ts` (non-reactive class): states `idle → requesting → ready → countdown → recording → review → committing`; `enable(deviceId?, withMic)` calls `navigator.mediaDevices.getUserMedia` ONLY here; `listDevices()` after permission; `start()` after a 3-2-1 countdown (cancellable), `MediaRecorder` with the first supported mime from Task 49's list, `timeslice 1000`, each `dataavailable` → `port.webcamAppend(sessionId, takeId, seq, bytes)` sequentially (a queue, never parallel); `stop()` → `finish`; `retake()` → `discard` then back to ready; `dispose()` stops every track (`track.stop()`) and is called on dialog close, on `pagehide`, and on errors. Errors map to copy: `NotAllowedError` → permissionDenied text, `NotFoundError`/`NotReadableError` → deviceUnavailable text; project work is untouched either way.
- `WebcamDialog`: idle explanation + **Enable camera** (nothing is requested before this), device select, optional microphone checkbox, countdown, record/stop, review player (plays the finished take through `editor_media_url`), **Retake**, **Add to timeline** (inserts the take asset on a new top video track at the playhead with the presenter preset: TR corner, `w 0.19`, circle, cover, at the ADR's own placement constant `(x 0.775, y 0.06)` — F35: use the SAME named constant Task 31's `cornerPreset` and the ADR §4 example both cite, not the `(0.785, 0.025)` margin `cornerPreset`'s generic default would compute for a 0.19-wide TR box; define it once (e.g. `layoutGeometry.ts`'s `PRESENTER_CORNER`) and have this dialog pass it explicitly rather than relying on `cornerPreset`'s default margin — `insertClip` + `setLayout`, one labelled step each), Cancel; closing with an un-added take asks Keep in library / Discard.
- Presenter editing is Task 31's `LayoutHandles`/`LayoutSection` — the take is an ordinary independent clip (never baked into the screen source).

**Tests first** (mock `navigator.mediaDevices` and `MediaRecorder`): `opening the dialog requests no device`; `enable requests exactly once`; `close stops every track`; `denial leaves the project untouched and shows the permission copy`; `chunks are sent strictly in order even if one append is slow`; `add to timeline inserts a separate clip on a new top track with the presenter layout at the ADR placement constant` (F35: asserts `x === 0.775, y === 0.06`, not `cornerPreset`'s default margin); `an unsaved take asks before closing`.

**Mutation check:** send appends concurrently → the ordering test red.

**Verify:** targeted tests; `npm run build && npm run test:e2e`; all gates.

**Commit:** `feat(editor): record, review and place webcam takes`

---

## Task 51: Synchronized webcam capture — pure parts, sidecar and staging ownership

**Package:** P07 · **F-IDs:** F-22 · **Depends:** 4 · **Rulings:** R10 · **Bundle:** NATIVE-MEDIA § Acquisition

**Files:** Modify `src-tauri/screen/src/staging.rs` (`webcam_part_file_name(base) = ".<base>.webcam.mp4.part"`, `webcam_file_name(base) = "<base>.webcam.mp4"`, sidecar `webcam: Option<WebcamSidecar { file, width, height, device_label, offset_ms: i64 }>` with `#[serde(default)]`), `src-tauri/screen/src/staging_files.rs` (F24: `capture_file_names(base, stems: &[String]) -> Vec<String>` gains a `stems` parameter — the file NAMES sourced from the sidecar's stem list, since the count is not derivable from `base` alone; this task always calls it with an empty slice (no stem sidecar field exists until Task 53, which threads the real list through the same signature); the webcam name IS derivable from `base` alone (one owned file, not a variable count) and needs no parameter — update every caller: discard, clear, usage), `src-tauri/screen/src/staging_title.rs` (F25: extend the `.export`-suffix disambiguation — a window title that would otherwise produce a base ending in `.webcam` or `.stem-<n>` collides with these new owned-file suffixes exactly as a `.export`-ending title collides with the export temp suffix; apply the SAME disambiguation rule to both), `src-tauri/src/screen_recovery/decide.rs` (classify `.<base>.webcam.mp4.part` as an OWNED part of `<base>` by exact name, and `.<base>.stem-<n>.m4a.part` as owned by PATTERN — F24: a regex-shaped match on `stem-\d+\.m4a\.part$`, since an orphan sweep has no sidecar to read an exact stem list from — promoted with the capture when footage exists, never Foreign), `src-tauri/screen/src/source.rs` (pure: `WebcamDeviceId` encode/parse `webcam:<symbolic-link-hash>` canonical round-trip, like `region`), `src-tauri/src/editor/project_store.rs` (F26: `SourceLocator` gains a `StagingFile { base: String, file: String }` variant — `SourceLocator::Staging{base}` alone can only resolve `<base>.mp4`; `StagingFile` addresses a companion staging file such as `<base>.webcam.mp4` and, for Task 53, a stem file; `resolve_source`'s `StagingFile` arm REFUSES any `file` that is not literally a member of `staging_files::capture_file_names(base, &sidecar_stems)`'s output — the same validated-membership discipline `resolve_source`'s other arms already apply, so a `StagingFile` can never be used to address an arbitrary path under staging), `core/src/editor/migrate.rs` (F26: `WebcamInput` gains `file: String` and `offset_ms: i64` — the Task 4 struct shipped with only `duration_ms, width, height` and explicitly deferred the rest to this task; place the webcam file on track `v2` with the presenter layout at offset `offset_ms`, addressed via a `StagingFile{base, file}` source record), `src-tauri/src/editor/session_commands.rs` (F26: `editor_open_staged`'s construction of `StagedInput` reads the sidecar's `webcam: Option<WebcamSidecar>` and fills `WebcamInput{file, offset_ms, ..}`, and registers the corresponding `StagingFile` source record in `sources.json` — Task 10 registered only the main video source; a project with a webcam take needs a second, resolvable source record or `editor_media_url`/preview can never reach the webcam file); create no new crate.

**Behavior:** all pure and Linux-tested: naming, sidecar forward/backward compatibility (an old sidecar without `webcam` parses; a new one written by this build round-trips through an older-shaped struct via `extra`), ownership (discard/clear delete the webcam/stem files too — through the SAME `discard_staged_files` two-pass symlink refusal), recovery classification, migration placement, source-record resolution. "Synchronized" is set as `asset.extra["capture_sync"] = "shared-clock"` only for this path.

**Tests first:** `webcam_names_round_trip_with_base_from_part`; `old_sidecars_without_webcam_still_parse`; `capture_file_names_includes_webcam_and_accepts_a_stem_list` (F24: called with `stems: &[]` here — behaviourally empty until Task 53 — and asserted to include the webcam name unconditionally); `staging_title_disambiguates_webcam_and_stem_suffixed_titles` (F25: a window title ending `.webcam` or `.stem-3`, run through `reserve_base`, does not collide with an actual webcam/stem file); `discard_removes_the_webcam_file_and_refuses_a_symlink_wearing_its_name`; `recovery_treats_a_webcam_part_as_owned_not_foreign`; `recovery_treats_any_stem_indexed_part_as_owned_by_pattern` (F24: `.foo.stem-7.m4a.part` classified owned with no sidecar present); `webcam_device_id_requires_a_canonical_round_trip` (`"webcam: x"`, `"webcam:"` rejected); `staging_file_locator_refuses_a_name_capture_file_names_does_not_own` (F26: `StagingFile{base, file: "../x"}` and `StagingFile{base, file: "unrelated.mp4"}` both fail to resolve); `open_staged_registers_a_resolvable_webcam_source` (F26: a sidecar with `webcam: Some(..)` produces a `sources.json` entry whose `StagingFile` locator resolves to the real `<base>.webcam.mp4` path); `migration_places_the_webcam_on_its_own_track_at_its_offset` (offset 120 ms → clip start 120); `only_synchronized_webcam_assets_are_marked_shared_clock`.

**Mutation check:** leave the webcam file out of `capture_file_names` → the discard test red; skip the pattern classification for stem parts → `recovery_treats_any_stem_indexed_part_as_owned_by_pattern` red; let `StagingFile` resolve an unvalidated name → `staging_file_locator_refuses_a_name_capture_file_names_does_not_own` red.

**Verify:** `cargo test -p vault_buddy_screen`; `cargo test -p vault-buddy --lib screen_recovery staged_commands staging_commands editor::`; `cargo test -p vault_buddy_core editor::migrate`; all gates.

**Commit:** `feat(screen): own and migrate a synchronized webcam track in staging`

---

## Task 52: Synchronized webcam producer and wiring (Windows)

**Package:** P07 · **F-IDs:** F-22 · **Depends:** 51 · **Rulings:** R10 · **Bundle:** A-residual R-H3

**Files:** Create `src-tauri/screen/src/session/webcam.rs` (`cfg(windows)` producer; `Unsupported` arm elsewhere), `src-tauri/src/screen_webcam_commands.rs` (F31: `list_capture_webcams` lives in its own new file, not appended to `screen_commands.rs` (741 nb) or `screen_capture_worker.rs` — both are already close to the 800-line cap and this command's own async degrade-to-`[]` logic does not belong inside either's existing responsibilities); modify `src-tauri/screen/src/session/mod.rs` (optional fourth producer stamping from the shared `CaptureClock`, its own bounded channel, its own `sink` instance built with a video-only configuration via `sink_format`), `src-tauri/screen/src/source.rs` (`list_webcams()` via `MFEnumDeviceSources` with `MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID`, `cfg(windows)`), `src-tauri/screen/Cargo.toml` (only the `windows` features needed, e.g. `Win32_Media_MediaFoundation` is present — add `Win32_System_Com_StructuredStorage` only if required, comment why), `src-tauri/src/screen_commands.rs` + `screen_capture_worker.rs` (`start_screen_capture` gains optional `webcamId: string | null` — additive, `None` = today's behaviour), `screen_dto.rs`, `src/components/ScreenSourcePicker.vue` (+ a webcam select listing `list_capture_webcams`), `src/stores/screenCapture.ts`, `tests/screenSourcePicker.test.ts`, registration files (F30: `list_capture_webcams` is a NON-editor-scoped command — it registers in `lib.rs`'s `generate_handler!` and AGENTS.md's IPC table like any ordinary screen-capture command, not through Task 11's editor-only capability manifest), the checklist (rows T18–T22).

**Behavior:** `IMFSourceReader` on the chosen device, NV12 or YUY2 → converted to NV12 (reuse `convert` where applicable), timestamps mapped into `CaptureClock` time (the reader's sample time minus the first sample time plus the clock's elapsed at first-sample arrival; pause drains-and-discards like the other producers), written to `.<base>.webcam.mp4.part`; a vanished webcam → `screen:warning` and the webcam file finalizes cleanly while the screen capture continues (spec §14 posture); a failed webcam finalize retains the part (recovery promotes it). The `CaptureGuard` claim is unchanged (one screen capture claim covers its producers). `list_capture_webcams` (async, degrades to `[]`). Every new thread named (`screen-webcam`).

**Tests first:** pure — `webcam_timestamps_map_into_the_capture_clock` (first sample at reader 5_000_000 ×100 ns arriving at clock 2_000 ms → output 2_000 ms; later sample +333_333 → 2_033 ms); `paused_webcam_samples_are_discarded`; `start_without_a_webcam_is_byte_identical_to_today` (structural: the default path does not construct the producer); TS `picker offers no webcam option when none are listed`; `start sends webcamId only when chosen`. Windows-only code executes in no automated test — the task report must say so; `cargo clippy -p vault_buddy_screen --all-targets --target x86_64-pc-windows-msvc -- -D warnings` (works) and on this host the native `cargo clippy -p vault-buddy --all-targets -- -D warnings` + `npx tauri build --no-bundle` are the compile proof.

**Mutation check:** subtract the clock offset twice → the timestamp test red.

**Verify:** as above + all gates. Checklist rows: T18 record screen+webcam 10 min, measure A/V drift of a clap visible on both; T19 unplug webcam mid-capture; T20 pause/resume keeps both aligned; T21 webcam busy in another app; T22 no webcam selected = unchanged capture.

**Commit:** `feat(screen): capture a webcam on the screen session's shared clock`

---

## Task 53: Separate audio stems (capture extension)

**Package:** P07 · **F-IDs:** F-05 (stems), F-24 context · **Depends:** 51 · **Rulings:** R11

**Files:** `src-tauri/core/src/vault_config.rs` is an allowlisted hotspot (1282 nonblank lines; entries may shrink, never grow), so this task FIRST extracts the seven screen fields' parse/serialize/accessors into a new `src-tauri/core/src/vault_config_screen.rs` (moving them out shrinks the entry — hand-edit its `loc` DOWN in `scripts/loc-baseline.json` and append the reason), then adds `screen_audio_stems: bool` (config key `screenAudioStems`, default false) in the new module. Also modify: `src-tauri/screen/src/session/audio.rs` (tee each input's resampled mono 48 kHz slice to a per-stem channel when enabled), a new `src-tauri/screen/src/session/stems.rs` (audio-only sink instances writing `.<base>.stem-<n>.m4a.part`, published with the capture), `screen/src/staging.rs` (sidecar `stems: Vec<StemSidecar { index, input, file }>`, `#[serde(default)]`), `src-tauri/src/screen_config_commands.rs` + `src/components/ScreenCaptureConfigTab.vue` (toggle "Keep each audio input as a separate track (new recordings only)"), `core/src/editor/migrate.rs` (F26: `StemInput` gains `file: String` alongside its existing `index`/`input` fields — Task 4 shipped it without a resolvable file name; one audio track per stem, main clip muted, each stem clip addressed via a `StagingFile{base, file}` source record), `src-tauri/src/editor/project_store.rs` (F24/F26: `capture_file_names`'s callers here now pass the sidecar's REAL stem file list instead of Task 51's empty placeholder; `resolve_source`'s `StagingFile` validation therefore starts accepting real stem names), `src-tauri/src/editor/session_commands.rs` (F26: `editor_open_staged`'s `StagedInput` construction reads `sidecar.stems` and fills `StemInput{file, ..}` for each, registering one `StagingFile` source record per stem — the "T51 likewise for stems" half of F26), the checklist (rows T23–T24: R-H4).

**Behavior:** default off → the capture path is byte-identical to today (structural test). On: each input also becomes a stem file sharing the mixed track's sample clock (same resampled samples before mixing — so stems and the mix cannot drift by construction). Stem write failure → `screen:warning`, the mixed capture still succeeds. The editor's Audio inspector explains stems when absent.

**Tests first:** `vault_config_round_trips_every_screen_field_after_the_split` (existing tests moved, all green; the allowlist number decreased); `stems_default_off`; `stem_names_are_owned_parts`; `stems_share_the_mixed_sample_stream` (pure: the tee receives the exact post-resample slices the mixer receives — test on the pure mixing helper); `migration_creates_one_audio_track_per_stem_and_mutes_the_embedded_mix`; `open_staged_registers_a_resolvable_stem_source_per_stem` (F26: mirrors Task 51's webcam source-registration test, for `sources.json`'s stem `StagingFile` entries); `capture_file_names_enumerates_the_sidecars_actual_stems` (F24: with a real 3-stem sidecar, the returned names match exactly — closing the placeholder Task 51 left). TS `the stems toggle states it applies to new recordings`.

**Mutation check:** tee before resampling → the shared-stream test red; register the stem source without validating it against `capture_file_names` → `open_staged_registers_a_resolvable_stem_source_per_stem` red.

**Verify:** `cargo test -p vault_buddy_core`, `cargo test -p vault_buddy_screen`, shell tests; `npm run check:loc` (the allowlist entry must SHRINK); all gates.

**Commit:** `feat(screen): optional per-input audio stems alongside the mixed track`

---

## Task 54: Before-you-share checks

**Package:** P12 · **F-IDs:** F-45, F-33, F-38 · **Depends:** 36, 41 · **Bundle:** SCREENS 07

**Files:** Create `src-tauri/core/src/editor/checks.rs`, `src-tauri/src/editor/checks_commands.rs` (F31: `editor_get_checks` lives in its own new file rather than being appended to `media_commands.rs`, which is already accumulating seven-plus commands across Tasks 22/25/28/36/40 toward the 800-line cap), `src/components/editor/dialogs/ChecksDialog.vue`, `tests/editorChecks.test.ts`; modify registration files (F30: `lib.rs` `generate_handler!`, `build.rs` `EDITOR_COMMANDS`, `capabilities/editor.json`, AGENTS.md's IPC table row + re-measured count), `port.ts`, `EditorHeader.vue` (Checks badge count), `RenderDialog.vue` (summary + blocking list).

**Behavior:** `run_checks(project, missing: &BTreeSet<assetId>, pending_takes: usize) -> Vec<CheckFinding>` (stable ids `chk-<code>-<target>`): `missingMedia` (blocking, action `reconnect`), `emptyProject` (blocking), `noDestination` (warning, `setDestination`), `gap` (info, gaps > 1 s on the top populated video track, `select`), `allMuted` (warning, `openAudio`), `clipping` (warning when summed unity gains > 1 on overlapping audio — "may clip", never a loudness claim), `excludedCaptions` (warning when cues exist and captions are disabled, `openCaptions`), `captionOverlap`/`captionDensity` (warning, `openCaptions`), `pendingTake` (warning, `openWebcam`), `transparentClip` (warning, opacity < 0.05 or all-hidden visual track with clips, `openLayout`), `textCollision` (info, two text/step cues overlapping in time and space), `privacyCover` (warning on every mask: "Stationary cover — check the rendered video; originals are uncensored", `select`), `canvasReview` (warning when a source aspect ≠ canvas or a text/caption box leaves the 5 % safe area, `reviewCanvas`). No score. `ChecksDialog` lists findings grouped by severity; each row's action reveals the object (selects it, opens the inspector tab/library panel, scrolls the timeline). Render is blocked only by `blocking` findings.

**Tests first:** one Rust test per code (F27: **14** — `missingMedia, emptyProject, noDestination, gap, allMuted, clipping, excludedCaptions, captionOverlap, captionDensity, pendingTake, transparentClip, textCollision, privacyCover, canvasReview`) with a minimal project that triggers exactly that finding and a control that does not; `no_quality_score_is_ever_emitted` (serialize → no `score` key); TS `each finding's action reveals its target`; `warnings do not block render, blocking findings do`.

**Mutation check:** make `privacyCover` info-level → its test red.

**Verify:** targeted tests; all gates.

**Commit:** `feat(editor): actionable checks before sharing`

---

## Task 55: Guide content, target registry and progress persistence

**Package:** P11 · **F-IDs:** F-46, F-47 (storage) · **Depends:** 17, 19, 22, 25, 27, 29, 34, 36, 47, 50, 54 · **Rulings:** R18 · **Bundle:** ONBOARDING; A23, A24

**Files:** Create `src/editor/guide/steps.json`, `chapters.json` (verbatim copies from `docs/concepts/vault-buddy-editor/contracts/`), `src/editor/guide/content.ts`, `targets.ts`, `src/stores/editorOnboarding.ts`, `tests/editorGuideContent.test.ts`, `tests/editorOnboardingStore.test.ts`; modify `prefs_commands.rs` (`editor_get_guide_progress`, `editor_save_guide_progress`), registration files (F30: `lib.rs` `generate_handler!`, `build.rs` `EDITOR_COMMANDS`, `capabilities/editor.json`, AGENTS.md's IPC table row + re-measured count), `port.ts`; add `data-guide-target` bindings through a `useGuideTarget(key)` composable in the components that own each target (header, library, transport, timeline toolbar, clip item, split/undo buttons, inspector, More, track menu, webcam entry, layout/fades sections, mixer, preview toolstrip, captions/chapters/products panels, checks/save/render/help buttons).

**Behavior:** `content.ts` maps each of the 22 step ids to a `GuideTargetKey` (ADR R18 list, in step order) and exports `CONTENT_REVISION = 1`. `targets.ts`: a registry `register(key, elRef)`/`resolve(key)`; when the owning control is in the More overflow, `resolve` returns the More item and `revealed: "overflow"`. Rust prefs: `guide-progress.json` in `editor-prefs\` (validated: known step ids only; unknown/retired ids map to the chapter's first step via an explicit `RETIRED_STEP_MAP` (empty now); ≤ 16 KiB; never contains paths). Store: `start()`, `next()`, `back()`, `pause()`, `collapse()`, `dismissInvitation()`, `markExplored(id)` (never auto-advances), `restart()` (guide state only), `suspend()`/`resume()` (transient — not persisted), `persist()` debounced; storage failure → `sessionOnly: true` flag shown in the UI.

**Tests first:** `there are 22 steps in 7 chapters` (ids and order match the concept file); `every step maps to a target key`; `every target key is registered by the mounted shell` (mount `EditorShell` with a fixture projection; all 22 resolve); `progress resumes at the exact step id`; `unknown ids fall back to the chapter's first step`; `storage failure reports session only and does not throw`; Rust `guide_progress_rejects_unknown_ids_and_paths`.

**Mutation check:** remove one `useGuideTarget` binding → the registry test names the key.

**Verify:** targeted tests; all gates.

**Commit:** `feat(editor): onboarding content, typed guide targets and saved progress`

---

## Task 56: Guide invitation, coach and suspension

**Package:** P11 · **F-IDs:** F-46, F-49 · **Depends:** 55 · **Bundle:** SCREENS 01, 10; A23–A26

**Files:** Create `src/components/editor/guide/GuideInvitation.vue`, `GuideCoach.vue`, `src/editor/guide/position.ts`, `tests/editorGuideCoach.test.ts`, `tests/e2e/editorGuide.spec.ts`; modify `EditorShell.vue`, `DialogHost.vue` (suspend/resume wiring), `shortcuts.ts` (F1/?/F6).

**Behavior:** first-use nonmodal invitation (**Show me around** / **Not now**) that neither steals focus nor blocks editing; the coach card positions beside the target without covering it (`position.ts`, pure: candidate sides by available space; docked mode below 1100 px keeps a visible target region), highlights the REAL control (outline ring, optional dimming off by default when reduced motion), Back/Next/Pause/Collapse/Close, chapter progress; F6 moves focus between the card and the target; Escape in an open menu closes the menu first; a dialog opening suspends the coach and closing it resumes the same step; the coach and highlight live outside the preview stage, so nothing guide-related can reach a render.

**Tests first:** `walking all 22 steps sends zero editor_execute and zero device or file requests` (A23: spy on the port and `navigator.mediaDevices`); `closing and reopening resumes the exact step` (A24); `opening a dialog suspends and closing resumes the same step` (A25); `guide elements are not descendants of the preview stage` (A26); `F6 toggles focus between card and target`; `position.ts never overlaps the target rect` (table of 6 target rects × 3 viewports); Playwright `coach resolves every target at 960x640`.

**Mutation check:** auto-advance on `markExplored` → a test asserting the step is unchanged goes red.

**Verify:** targeted tests; `npm run build && npm run test:e2e`; all gates.

**Commit:** `feat(editor): optional, resumable guided walkthrough`

---

## Task 57: Learning center

**Package:** P11 · **F-IDs:** F-47 · **Depends:** 56

**Files:** Create `src/components/editor/guide/LearningCenter.vue`, `src/editor/guide/answers.ts` (quick answers derived from step bodies + a shortcut table from `shortcuts.ts`), `tests/editorLearningCenter.test.ts`; modify `EditorHeader.vue` (Help menu: Learning center, Resume walkthrough, Keyboard shortcuts, Export diagnostics (Task 58)), `prefs_commands.rs` (`editor_export_guide_progress`, `editor_import_guide_progress` — see Behavior below), registration files (F30: `lib.rs` `generate_handler!`, `build.rs` `EDITOR_COMMANDS`, `capabilities/editor.json`, AGENTS.md's IPC table row + re-measured count), `port.ts`.

**Behavior:** searchable quick answers (case/diacritic-insensitive substring over title + body), chapter navigation, per-lesson jump (starts the coach at that step), progress + preferences (dimming, motion), **Start over** behind a confirm that resets guide state only, **Save progress file** / **Restore progress file** (native save/open dialog through two small commands in `prefs_commands.rs`: `editor_export_guide_progress`, `editor_import_guide_progress` — validated, contains no media, restoring never activates the camera or edits content). Answers describe where controls are now, never UI history.

**Tests first:** `search finds a lesson by a word in its body`; `start over asks first and leaves the project untouched`; `restoring progress does not start the guide or touch devices`; `shortcut table matches shortcuts.ts` (single source); Rust `guide_progress_file_round_trips_and_rejects_foreign_json`.

**Verify:** targeted tests; all gates.

**Commit:** `feat(editor): searchable learning center with portable progress`

---

## Task 58: Accessibility, keyboard journey, diagnostics and privacy

**Package:** P12 · **F-IDs:** F-49, F-50, F-48 · **Depends:** 47, 54, 56 · **Bundle:** A28, A30; PRODUCT-SPEC NFR table

**Files:** Create `src-tauri/src/editor/diagnostics.rs`, `tests/e2e/editorKeyboard.spec.ts`, `tests/editorA11y.test.ts`, `src-tauri/src/editor/redact.rs`; modify components found lacking by the tests (labels, roles, focus rings), `src/style.css` (`@media (forced-colors: active)` rules for selection, playhead, handles, focus; `prefers-reduced-motion` disables coach/zoom animations), registration files, `port.ts`, `AGENTS.md`.

**Behavior:**
- Keyboard journey (Playwright against built `dist/`, with the runtime stubbed the way `tests/e2e/tauriStub.ts` already stubs it for the phase-4 editor layout spec — F28: NOT a `window.__VB_EDITOR_PORT__` test-only build flag, which would ship a production global purely to be overridden in a browser test and directly contradicts the pattern this file's own header describes ("This is a stub of the RUNTIME, not of the app"). Instead, `page.addInitScript` installs `window.__TAURI_INTERNALS__.invoke`/`metadata.currentWindow.label` BEFORE the module graph evaluates, exactly like `installTauriStub`, with `invoke` replies for `editor_open_staged`, `editor_execute`, `editor_get_snapshot`, `editor_save_project` and the other commands this journey exercises. No production code changes to support it): open → Tab to timeline → select a clip → `S` split → `Delete` → add a text cue from the toolbar with Enter → type text → `Ctrl+S` → the header reads Saved. No pointer used.
- Forced-colors: Chromium emulation `forcedColors: 'active'` — selected clip, playhead and focus ring remain visible (computed outline style not `none`).
- `redact.rs`: `redact_path(&Path) -> String` → `"<path:#hash8>"`; every `log::` call in `src-tauri/src/editor/**` that formats a path, file name, caption or title uses it — a structural test scans for `{}`/`{:?}` arguments named `path|file|name|title|text|caption` without `redact`, AND (F38) for any `.display()` call or `{:?}` formatting of a `Path`/`PathBuf`-typed expression regardless of the argument's NAME — the name-only heuristic misses `dir.display()` (`dir` matches none of `path|file|name|title|text|caption`), which is exactly the call `export_worker/vault_dir.rs` makes today and Task 59 is about to move under `src-tauri/src/editor/`.
- `editor_export_diagnostics(window)`: native save dialog → JSON `{appVersion, os, ffmpeg: {found, version, filters: [...]}, sessions: n, projects: n, products: n, jobs: [{kind, phase, errorCode}], webview2Version}` — counts, capabilities and codes only; no names, paths, captions, frames or audio.

**Tests first:** `keyboard-only journey completes` (Playwright); `forced colors keep selection visible` (Playwright); `every icon-only button has an accessible name` (Vitest over all editor components mounted in the shell); `menus and dialogs return focus`; `production dist ships no editor test hook` (F28: grep the built `dist/` for `__VB_EDITOR_PORT__` — must not appear); Rust `diagnostics_contain_no_paths_or_names` (seed a project titled "Secret plan" with media "C:\\Users\\x\\a.mp4" → output contains neither); `editor_logs_redact_paths` (structural); `editor_logs_redact_display_calls_regardless_of_argument_name` (F38: a `log::warn!("{}", dir.display())`-shaped line in a fixture module is flagged even though `dir` matches none of the name heuristic's words).

**Mutation check:** log a raw path in one editor module → the structural test names the line; add a `.display()` call on a variable named something other than `path|file|name|title|text|caption` → `editor_logs_redact_display_calls_regardless_of_argument_name` red.

**Verify:** targeted tests; `npm run build && npm run test:e2e`; all gates. Checklist rows T25–T27 (R-A1 Narrator + NVDA journey; R-A2 high contrast at 150 %/200 %).

**Commit:** `feat(editor): keyboard, forced-colors and content-free diagnostics`

---

## Task 59: Retire the phase-4 editor path and the phase-5 export command

**Package:** P12 · **F-IDs:** — (consolidation; every capability now lives in 1–58) · **Depends:** 21, 46, 47, 48 · **Rulings:** R4, R12, R17

**Files:** Delete `src/composables/useEditorTimeline.ts`, `useEditorSelection.ts`, `useEditorExport.ts`, `src/components/editor/TimelineStrip.vue`, `ExportBar.vue`, `CapturePreview.vue` (if `ProductPlayer` supersedes it — otherwise keep and document), `src/components/editor/LegacyCaptureEditor.vue` (Task 15's feature-switch fallback — its whole reason to exist ends here, now that `load_staged_capture` itself is retired), `src/utils/timelineGeometry.ts`, their tests (`tests/useEditorTimeline.test.ts`, `editorSelection.test.ts`, `editorEditing.test.ts`, `exportBar.test.ts`, `timelineStrip.test.ts`, `timelineGeometry.test.ts`, the TS half `tests/timelineFixtures.test.ts`, `tests/editorLayout.test.ts` — F29: the phase-4 layout contract test, which asserted the retired components' layout and has no successor once they are gone); remove commands `load_staged_capture`, `save_capture_timeline`, `export_and_save_capture`, `cancel_export` and the four `screen:export*` events; delete `src-tauri/src/export_commands.rs`'s lifecycle (keep `timeline_from_sidecar`'s thin wrapper only if still called — otherwise delete; `emit_discarded` moves to `staged_commands.rs` with its structural test), `export_shutdown.rs` (its role is Task 46's render cancel — update `shutdown_gate.rs` and both quit workers and their structural tests); keep `export_worker/vault_dir.rs` (used by publish) — move it to `src-tauri/src/editor/vault_dir.rs` and delete the rest of `export_worker/`; keep `screen::export` only for `remux_args`/`ffmpeg_run` users. Modify `src-tauri/src/staged_commands.rs` (F29: `discard_conflict` DROPS its `exporting` parameter — the legacy `ExportState` it checked no longer exists once `export_and_save_capture` is gone; the remaining guard is `pinned` alone, since the render/publish job system's own guards live elsewhere and never routed through this function); update `tests/fixtures/timeline-cases.json` consumers: Rust keeps it (migration + identity), the TS size guard is deleted with its test — note this in the file's header comment; modify `tests/e2e/editorLayout.spec.ts` (F29: Task 15 retargeted it at the new window's viewport set but it may still assert against retired phase-4 components — update or fold its assertions into `tests/e2e/editorShell.spec.ts` from Task 16, whichever leaves no dangling reference), `tests/e2e/tauriStub.ts` (F29: its `invoke` stub replies for `load_staged_capture`/`save_capture_timeline` are dead code once those commands are gone — remove them; keep the stub module itself, since Task 58's keyboard-journey spec reuses its `installTauriStub` pattern for the NEW `editor_*` commands), `tests/editorRoot.test.ts` (F29: delete the phase-4/legacy-path test cases Task 15 added alongside the new-session tests — the legacy branch they exercised no longer exists), `AGENTS.md`, `CONTEXT.md` (Export → superseded), `docs/Gaps.md` (retire GAP-136's TS half, GAP-145 → superseded by GAP-N3), `src/components/ScreenCaptureBar.vue` / `StagedCaptureList.vue` (the old "Save" wording now reads "Edit to render and publish"), `src-tauri/core/src/timeline.rs` (header comment: now read by migration and `is_identity` cross-checks only), `src-tauri/src/editor/vault_dir.rs` (F38: the moved file's `log::` calls that format a path via `.display()` or `{:?}` on a `Path`/`PathBuf` — e.g. `dir.display()` — are redacted through Task 58's `redact::redact_path`, which the widened scan from that task would otherwise flag at its NEW location).

**Behavior:** no user-visible capability is lost: every staged capture opens in the tutorial editor, and "save unchanged" = Render (identity → lossless remux) + Publish. A staged capture is no longer deleted by saving (R6); users discard it explicitly. The stop toast copy ("Edit it from the capture bar to save it into a vault") becomes "Open it in the editor to render and publish it" — update `stopped_toast_copy` and its test (still no "saved").

**Tests first:** `no source file references a retired command` (scan `src/` and `src-tauri/src` for the four command names and four event names → none); `identity render of an untouched capture is lossless` (re-run Task 45 test 10 as the successor of the phase-5 fast-path test); `shutdown gate composes captures and renders` (updated structural test); `the stop toast never claims a save` (existing belt, updated copy); `discard_conflict_no_longer_takes_an_exporting_flag` (F29: signature test — the legacy export guard is gone, pinned is the only remaining refusal); `moved_vault_dir_logs_still_redact_their_paths` (F38: the file's `.display()` calls pass the widened Task 58 scan at its new `src-tauri/src/editor/` location).

**Mutation check:** leave `cancel_export` registered → the scan test red; leave a raw `.display()` call unredacted in the moved `vault_dir.rs` → `moved_vault_dir_logs_still_redact_their_paths` red.

**Verify:** all gates; re-measure the IPC count; `npx tauri build --no-bundle`; LOC and quality baselines must not loosen (deleting code may IMPROVE `complexFunctions`/MI — hand-edit the number down/up if it moves the right way).

**Commit:** `refactor(editor): retire the phase-4 editor and the phase-5 export path`

---

## Task 60: Final verification, documentation and release evidence (P13)

**Package:** P13 · **F-IDs:** all 50 (evidence) · **Depends:** 1–59

**Files:** Modify `AGENTS.md` (Tutorial Editor domain section: architecture, the ten invariants from ADR §8, the project store in "Where state lives on disk", the editor window bullet rewritten (1280×820, stores installed, close guard), Frontend-state paragraph (EditorRoot installs four stores), repository map, IPC table with the measured count, events table, vault domain = ten writes, shutdown gate = captures + renders), `CONTEXT.md` (Tutorial Project, Clip, Track, Teaching Cue, Take, Rendered Product, Render, Publish; Export amended), `docs/Gaps.md` (GAP-N1…N5 plus any found during tasks, each with file refs and failure scenario), `docs/superpowers/specs/2026-09-21-tutorial-editor-integration-design.md` (F35, F19: the ADR itself — see Behavior below), `docs/superpowers/specs/2026-09-21-tutorial-editor-windows-verification.md` (re-measured row count in its header, every residual R-H*/R-A*/R-M1 row present, results column empty unless actually run), a NEW evidence file (the concept bundle under `docs/concepts/` is a frozen input and is not edited): `docs/superpowers/specs/2026-09-21-tutorial-editor-acceptance-evidence.md` (one row per F-ID: automated evidence = test names + files; native result = "not yet evaluated" or the checklist row), `README.md` (user-facing: the tutorial editor, ffmpeg requirement for rendering/import/waveforms), `docs/DEVELOPMENT.md` (editor store layout, render round-trip tests need ffmpeg with libass).

**Behavior / checks:**
- Run every gate on this host and paste the real tail of each output into the task report: `npm run lint`, `check:loc`, `check:quality`, `build`, `test:e2e`, `test:coverage`; `cargo fmt --check`; `cargo clippy --workspace --all-targets -- -D warnings`; `cargo test -p vault_buddy_core -p vault_buddy_screen -p vault_buddy_capture -p vault_buddy_transcribe -p vault_buddy_mcp`; `cargo test -p vault-buddy --lib`; `cargo test -p vault_buddy_screen --test render_roundtrip -- --nocapture` (state whether any SKIP lines printed); `cargo llvm-cov … --fail-under-lines 94` (if llvm-cov is installed; otherwise say so — CI runs it); `cargo machete .`; `cargo deny check`; `npx tauri build --no-bundle`.
- Walk the concept bundle's final representative journey (IMPLEMENTATION-PLAN § Final acceptance tasks) in `npm run test-build` ONLY if a human is driving; otherwise record it as an unrun checklist row — never claim it.
- Confirm by grep: 50 F-IDs present in the evidence file; every `## Task` commit exists on THIS branch (F33: `git log <base>..HEAD --oneline | grep -c "(editor)\|(core)\|(screen)"` ≥ 59, where `<base>` is the branch point named in Global Constraints — NOT `git log --oneline` over the whole repository history, which counts commits from before this plan existed and asserts nothing about this branch's own completeness); no file added to `scripts/loc-baseline.json`; `scripts/quality-baseline.json` counters not increased.
- F35 (minor ADR/plan drifts, kept as the plan's behaviour — amend the ADR text to match rather than change any code): amend `docs/superpowers/specs/2026-09-21-tutorial-editor-integration-design.md` with one paragraph per drift, each naming the task that introduced it — (a) Task 49's native `CaptureGuard` refusal for `editor_webcam_begin` is a stronger guarantee than ADR ruling R10 describes (R10 states a UI-only convention; the plan backs it with a native check too) — amend R10's text to say so, and add `docs/Gaps.md` GAP-N1 noting the two are intentionally not identical scopes; (b) Task 51's `WebcamSidecar.offset_ms` carries a real, non-zero offset where the ADR's §4 example shows `0` — amend §4's example to show a non-zero offset, since `0` was illustrative, not a constraint; (c) Task 46's `InternalCommand::RestoreSnapshot` carries no `commandId` field (it never replays over IPC, unlike every `EditorCommand`) while `editor_restore_product`'s own signature threads a `command_id` argument separately for its undo-history entry — amend the ADR's command-envelope section to state that `InternalCommand` deliberately has no `commandId` field, distinguishing it from the plan's earlier per-command wording that implied uniformity. (d) F19: also amend ADR §3.3's `JobProgressDto.kind` list to include `"publish"` explicitly (Task 48 wired it in; the ADR text predates that and still reads as if only `import|render|peaks` exist).
- Residual gates listed explicitly as open: R-H1–R-H5, R-A1, R-A2, R-M1, R-P1, R-P2.

**Tests first:** `docs/superpowers/specs/…-acceptance-evidence.md` is checked by a tiny Vitest (`tests/editorEvidence.test.ts`) asserting all 50 F-IDs appear and each names at least one existing test file path (the test resolves the paths) — a doc that drifts from the tests fails CI.

**Verify:** all of the above.

**Commit:** `docs(editor): record the tutorial editor's architecture, gaps and release evidence`
