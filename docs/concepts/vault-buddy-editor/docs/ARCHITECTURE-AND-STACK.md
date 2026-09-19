# Architecture and technology-stack alignment

## Implementation decision

Implement this editor **inside the existing Rust / Tauri / TypeScript / Vue / Pinia application**. The self-contained HTML is the acceptance and interaction reference, not the production application architecture. Do not embed the imperative browser application as an iframe, introduce another frontend framework, add a browser Node runtime, or bypass the existing native capture domain.

This review inspected repository source at `3f330a970ec26ca8311af2215d8df0dc8f17a737`, on September 19, 2026. It did not build or run that native application. The pull-request description and implementation are not equally current; file contents are the evidence. Reconcile against the target branch before changing code. [Repository evidence](REFERENCES.md#repository-evidence).

## Verified stack and integration points

| Surface | Observed source | Required treatment |
|---|---|---|
| Vue | `vue: ^3.5` | Vue single-file components, preferably `<script setup lang="ts">`; reuse application conventions. |
| Pinia | `pinia: ^3` and an options-style capture store | Separate serializable domain projections from view state and media resources. Keep the existing capture store. |
| TypeScript | `typescript: ^6.0.3`, `vue-tsc: ^3.3.6` | Strict DTOs and discriminated commands; runtime validation at file and IPC boundaries. |
| Tauri | `@tauri-apps/api: ^2`, native Tauri configuration | Invoke typed native commands through a replaceable adapter. Scope access to the editor. |
| Build/style | Vite `^8.1.3`, Tailwind `^4` | Integrate into the existing build and semantic tokens. No new CSS framework required. |
| Frontend verification | Vitest `^4.1.10`, Vue Test Utils, Happy DOM, ESLint | Add unit/component coverage; do not substitute browser screenshots for these gates. |
| Roots/window routing | `rootFor(label)` dispatches main/panel/bubble | Add `editor → EditorRoot`; a Vue Router dependency is unnecessary for a single new window. |
| Capture state | `src/stores/screenCapture.ts` | Preserve authoritative status resync, sequence guards, paused-time accounting and `stillSaving` behavior. |
| Native capture | `vault_buddy_screen`, `windows-capture 2.0.1`, `windows 0.62` | Extend existing native acquisition, staging and Media Foundation infrastructure. |
| Timeline algebra | `core::timeline` sequential half-open segments | Reuse tested segment logic where applicable; introduce a validated multi-track composition aggregate rather than claiming a sequential list already implements layering. |

The versions above are **declared dependency ranges**, not a lockfile-resolution report. Use the repository lockfiles and current approved toolchain. This handover does not require dependency upgrades.

## Logical boundaries

```text
EditorRoot.vue
  ├─ EditorHeader / PreviewToolbar / MediaLibrary / Timeline / Inspector
  ├─ ContextMenu / DialogHost / LearningCenter / GuideOverlay
  └─ composables + Pinia projections
       ├─ EditorPort (mockable TypeScript interface)
       ├─ PreviewController (non-reactive media resources)
       └─ GuideTargetRegistry (component refs, not brittle document search)
                │ scoped Tauri commands / small progress messages
Rust shell      │ window identity, session authorization, dialogs, jobs
                ▼
core::editor    │ validated composition, commands, history, render plan
screen          │ capture/decode/compose/encode workers, shared media clock
capture         │ existing device selection, mixing/resampling infrastructure
project service │ staged work, durable commits, sources and product lineage
vault service   │ configured destinations, no-clobber publication, companion note
```

This is a logical decomposition, not a requirement to create a crate for every box. Extend existing modules first and use their documented ownership rules. Pure edit/time/geometry code must remain testable without a Windows screen or Tauri window.

## Vue component map

| Component | Ownership | Important contract |
|---|---|---|
| `EditorRoot` | Session open/close, injected services, error boundary | Resolve staged session once; release listeners when detached, not source files. |
| `EditorHeader` | Project title/status, Help, Checks, Save project, Render video | Save and render remain distinct. Status is derived from receipts, not timers. |
| `PreviewToolbar` | One row of teaching tools, ratio, review and panel visibility | Overflow uses one action registry, not a second toolbar. |
| `MediaLibrary` / `MediaAssetCard` | Search/import, source availability, contextual insertion | Never expose arbitrary filesystem paths as import authorization. |
| `PreviewSurface` | Canvas/native surface attachment and pointer transforms | Stable controller; no frame data in Pinia. User handles are not encoded. |
| `TransportBar` | Play/pause, seek, volume monitoring, current time | Monitoring mute differs from exported audio settings. |
| `TimelineView` / `TrackLane` / `ClipItem` | Viewport, selection, movement/trim/fade interaction | One drag gesture becomes one committed operation, not hundreds of undo entries. |
| `Inspector` and six panels | Clip, Layout, Fades, Audio, Speed, Color | Dirty form buffers and validation precede one command; do not bind uncontrolled inputs straight to native domain state. |
| `CaptionPanel` / `ChapterPanel` | Timed instructional content | Source-linked cues survive moves and trims. |
| `ContextMenu` | Target-aware action list and keyboard behavior | The right-click target and clicked time are explicit; no accidental playhead substitution. |
| `SaveProjectDialog` / `RenderDialog` | Preflight, bounded operations, cancellation | Pending/cancelled/failed/complete are distinct states. |
| `LearningCenter` / `GuideOverlay` | Progress and contextual teaching | Guide preparation may reveal UI, never silently edit media. |

Use the actual reference screenshots and `contracts/design-tokens.json` for appearance. Translate layout into Vue templates rather than copying the reference's DOM relocation/wrapper techniques. Stable keys preserve focus. Use text binding for user titles, filenames and caption content; no untrusted `v-html`.

## Pinia state boundaries

| Store/service | Persisted? | Contents |
|---|---|---|
| Existing `screenCapture` | Native session truth, not editor file | Capture status, error/warning, staged result. |
| `editorProject` | Composition committed by Rust | Current revision, validated graph/projection, saved revision, undo/redo availability, product summaries. |
| `editorWorkspace` | Debounced local preference/session write | Selection IDs, playhead, panel sizes, tabs, timeline zoom/scroll, snap/ripple, monitoring preferences. |
| `editorJobs` | Queryable native job ledger | Session/job identity, ordered progress, cancellation request, terminal result. |
| `editorOnboarding` | Separate application preference document | Content revision, current stable step, explored/completed IDs, dismissed/active/collapsed, motion/dimming preferences. |
| `MediaRegistry` / `PreviewController` | Never serialized | Native handles, media elements, object URLs, audio nodes, canvas, observers, frame queues. |

Use getters/computed values for derived duration/dirty state/selected entity rather than duplicated facts. Pinia's typed options stores fit the repository's current style; setup stores can be justified for focused compositions. Use `storeToRefs` where reactive state is destructured. Large immutable projections should use shallow reactive boundaries; replace changed projections rather than deeply proxying every frame/resource. Virtualize long timeline/library lists. These are design decisions informed by the official [Pinia](REFERENCES.md#official-guidance) and [Vue](REFERENCES.md#official-guidance) documentation, not a mandate to persist all stores automatically.

Do not serialize all Pinia state to localStorage. Do not watch the whole edit graph on every playhead tick. UI progress and media sample time have different update rates and ownership.

## Command consistency and Undo

Rust is authoritative for committed edits and durable revision numbers. A command has `sessionId`, `expectedRevision`, a unique `commandId` and a discriminated payload. A serial command queue validates against the current graph; one atomic operation either succeeds completely or returns an actionable error. The result returns the acknowledged revision/projection and undo/redo state.

During a drag, the UI uses a transient local preview. Pointer-up submits one command; Escape discards it. Pending edits cannot begin a render of a revision that does not yet exist. On conflict, re-fetch the acknowledged graph and preserve the user's uncommitted intent for explicit retry. Never apply stale results after a project switch. Undo creates a new current revision; it does not delete historical products or their source dependencies.

The starter demonstrates acknowledged updates, generation guards, mockable ports, runtime DTO decoding and save-receipt checks. It is not a complete domain engine or finished Vue frontend. Read its actual verification scope before copying it.

## Exact screen-capture handoff

The existing `screen:stopped` event provides `base`, `path`, `durationMs`, `sourceTitle`, `width` and `height`. It **does not contain `vaultId`**. The capture store resets its active vault when stopping. Therefore `EditorRoot` must not infer the destination from the reset store or whichever vault happens to be selected later.

Invoke a proposed native `editor_open_staged({stagedBase})`. Rust validates the base against its owned staging registry, reads the matching sidecar and resolves `vaultId` there. It registers source assets and creates/resumes an editor session. Do not trust a frontend-supplied path merely because an earlier event contained a path string. Handle duplicate delivery idempotently: reopen the existing session rather than adding another copy.

`stop_screen_capture` returning `stillSaving` is not a failed recording and is not a completed staged capture. Wait for stopped/failed or authoritative status reconciliation. Preserve the capture store's sequence checks against started/stopped races. The current sidecar's `timeline: Option<Value>` is not permission to install an unvalidated multi-track document.

## Tauri window and security integration

The inspected configuration has main/panel/bubble only and no asset/media scope. Add an independently resizable `editor` window (recommended initial 1280×820; support 960×640 compact behavior) and explicit `EditorRoot` selection. It must not inherit the companion panel's focus-out hiding or always-on-top behavior. Follow native main-thread window rules and position hidden windows before showing them. User close asks Save / Keep staged / Discard when necessary; an active native render is resolved deliberately rather than killed by a UI unmount.

Use bundled Vue assets under the production CSP. The standalone HTML intentionally contains inline script; do not weaken the application's `script-src` to embed it. Preview media should be exposed by narrowly scoped asset protocol or a session-authorized media service, never a wildcard home/vault read scope. Scope staging/project media paths and required `media-src`/`img-src`; validate exact URLs under the target WebView2 build. `convertFileSrc` creates a URL; it does not authorize access.

A dedicated capability file alone is **not sufficient for custom app commands**. Tauri documents that commands registered with `invoke_handler` are available to all app windows by default unless restricted through the application manifest/permissions configuration. Configure `AppManifest::commands`, generated app permissions and editor capability grants as appropriate to the installed Tauri release; additionally validate native caller window, session ownership and authorized file IDs. Review overlapping capabilities because their grants combine. See [security specification](PERSISTENCE-AND-SECURITY.md).

## Stack acceptance

The application build must pass its existing build/lint/coverage/size/quality gates. Add actual Vue component/Pinia tests to the repository rather than relying only on the browser global API. Native Windows tests must exercise real permissions, frame cadence, audio, shutdown, disk faults and synchronization. Linux pure-module tests remain useful but cannot certify Windows capture/render behavior. [Release gates](RELEASE-CHECKLIST.md) distinguish every unverified requirement.
