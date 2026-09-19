# Implementation plan and delivery packages

## Execution rules

The target is the full specified editor, not a second reduced editor. Implement incrementally against the feature catalog and preserve the reviewed interaction contract. **P00 is mandatory:** consolidate with the application's existing data model and capture implementation before adding another schema, store or engine.

Use the existing repository quality gates and lockfiles. Assign one owner for shared contracts; parallel work is safe only after contracts and fixtures are merged. The browser's global wrappers/DOM rewrites are reference glue, not production Vue patterns. Each package provides an executable vertical behavior, unit/component/native tests, updated documentation and an evidence-backed review. No package is complete merely because controls are visible.

## Package dependency map

```text
P00 → P01 → P02 → P03 → P04 → P05 → P06
          └─────────────┘         ├──→ P07
                   P01+P04 → P09 └──→ P08
                       P05+P06+P08+P09 → P10
          UI + media + persistence + render → P11 → P12 → P13
```

The table below, rather than the simplified diagram, is authoritative for dependencies. Sequential steps do not imply a time estimate or fixed staffing commitment.

## P00 — Reconcile the existing application

**Dependencies:** None. **Accountable delivery roles:** Solution architect + Rust/TypeScript developers.

Inventory actual capture/root/window/config types, tested timeline primitives, asset permissions and quality gates. Map every F-ID to an owning module. Compare the checked source revision with the target branch.

**Acceptance:** A recorded integration ADR, ownership map and approved contracts. No duplicate capture engine or assumed editor IPC.

## P01 — Introduce validated edit and workspace contracts

**Dependencies:** P00. **Accountable delivery roles:** Rust domain developer + TypeScript developer.

Define multi-track aggregate, half-open timing, source identity, linked cues, revision/product lineage and explicit migrations. Generate or parity-test Rust/TS DTOs. Add structural and semantic fixtures.

**Acceptance:** Valid example round-trips; invalid IDs/cycles/ranges/unknown edit semantics reject before state replacement; sequential timeline behavior remains covered.

## P02 — Open staged work in a real editor window

**Dependencies:** P01. **Accountable delivery roles:** Tauri/Rust developer + Vue developer.

Add editor root/window, command permissions, session ownership, staged-base resolution and exact sidecar vault handoff. Wire capture stopped/resync without double-opening. Preserve companion-window behavior.

**Acceptance:** A real native capture opens its correct editable session; late/stale capture messages cannot resurrect state or select the wrong vault. No arbitrary source reads.

## P03 — Implement shell, stores and action registry

**Dependencies:** P02. **Accountable delivery roles:** Vue/Pinia developer + UX designer.

Build header, single preview toolbar, library, timeline shell, inspector, dialogs, theme/compact layouts. Separate project/workspace/jobs/onboarding stores; use generation/revision guards and stable keyed focus.

**Acceptance:** One persistent home per primary action, functional keyboard/menu/focus behavior, six property categories and compact screenshots matching the reference.

## P04 — Edit and arrange a single source end to end

**Dependencies:** P01,P03. **Accountable delivery roles:** Rust domain developer + Vue developer.

Implement insert/select/scrub/split/trim/delete/undo/redo via native acknowledged commands, transient drag previews and numeric alternatives. Keep file bytes immutable.

**Acceptance:** Capture → cut → save editable project → reopen succeeds with the same source mapping and no data loss.

## P05 — Manage independent layers and audio

**Dependencies:** P04. **Accountable delivery roles:** Media developer + Vue developer.

Add multi-track composition, movement/grouping/copy/paste, track controls, stills, audio detach/mixer, shared offset/locks and explicit track-local ripple. Add waveform/thumbnail caches.

**Acceptance:** Layer stacking and audio mix match fixtures; group/intro operations preserve sync or reject atomically; monitor mute does not alter output.

## P06 — Add fades, transformations and generated cards

**Dependencies:** P05. **Accountable delivery roles:** Media developer + TypeScript developer.

Implement edge envelopes and paired transitions, speed and preserve-pitch policy, crop/frame/rotation/flip, color treatments and editable title cards. Validate geometry on canvas changes.

**Acceptance:** Golden frames/audio samples confirm the documented operation order and timing; transitions cannot dangle or create unexplained overlaps.

## P07 — Capture and place webcam material

**Dependencies:** P02,P05. **Accountable delivery roles:** Native capture developer + Vue developer.

Implement explicit permission/device flow, countdown, record/review/retake/add, resource cleanup and unsaved-take handling. Implement synchronized multi-input capture only through the shared native session clock/guard.

**Acceptance:** Physical camera/mic denial and unplug cases pass. Presenter stays an editable independent source with resize/corner/frame/crop/fade controls.

## P08 — Implement instructional overlays, captions and chapters

**Dependencies:** P05,P06. **Accountable delivery roles:** Vue developer + media developer + requirements engineer.

Implement all seven teaching cue kinds, source-linked timing, manual subtitle import/editing, burn-in, density/overlap warnings, chapters and companion-note preview. No pretend automatic transcription.

**Acceptance:** Move/trim/split/speed/reopen preserve cues and chapters; arrows/zoom render correctly; privacy warning remains explicit.

## P09 — Persist and recover projects durably

**Dependencies:** P01,P04. **Accountable delivery roles:** Rust storage developer + test engineer.

Build project transaction service, portable/lightweight interchange, historical dependency retention, missing-media reconnect, safe names, journals and conflict-safe saves. Guide/workspace persistence is separate.

**Acceptance:** Kill/disk-full/collision/stale receipt/ambiguous reconnect tests preserve last good project and source media. Save never requires render.

## P10 — Render immutable video products

**Dependencies:** P05,P06,P08,P09. **Accountable delivery roles:** Native media developer + test engineer.

Implement bounded deterministic decode/compose/mix/encode jobs, range output, progress/query/cancel, no-clobber video/note publication and immutable product snapshot history.

**Acceptance:** Actual decoded Windows output meets frame/duration/audio/sync acceptance on named hardware; output review plays the product file; later edits cannot mutate it.

## P11 — Deliver the full learning experience

**Dependencies:** P03,P04,P05,P06,P07,P08,P09,P10. **Accountable delivery roles:** Vue developer + UX designer.

Implement all 22 supplied lessons/7 chapters with stable target refs, invitation, pause/resume/collapse, F1/F6, context prep, modal suspension, progress/preferences and quick answers.

**Acceptance:** Passive walkthrough changes no composition; every target resolves at compact sizes; native reload resumes exact step; permissions/downloads never start automatically.

## P12 — Integrate checks, failure recovery and quality controls

**Dependencies:** P07,P08,P09,P10,P11. **Accountable delivery roles:** Whole team + accessibility/test specialist.

Centralize actionable checks, pending-job/save/take state, import report, keyboard ownership, forced colors/reduced motion, virtualized timeline, resource disposal, safe diagnostics and crash handling.

**Acceptance:** Every warning has a relevant recovery route. No startup/close path strands work. Accessibility/performance/privacy matrix is executed, not inferred from screenshots.

## P13 — Accept and package the native release

**Dependencies:** P12. **Accountable delivery roles:** Delivery manager + product owner + test engineer.

Run full catalog acceptance, actual repository build/type/lint/coverage/quality gates, native device/render/durability tests and beginner task walkthroughs. Freeze supported formats/hardware/limits and documentation.

**Acceptance:** Signed release checklist with known residuals and explicit acceptance; no skipped tests counted as passes, no native capability described as done without evidence.

## Three-agent implementation pattern

After P00/P01, the domain agent owns pure Rust model/commands/fixtures; the frontend agent owns Vue components/Pinia/UI tests; the native-integration agent owns scoped IPC, media/storage workers and Windows tests. They agree request/result contracts and sample files before coding. The integrator reviews whole user flows, catches signature/schema drift and runs the complete test matrix. Avoid concurrent edits to generated DTOs, root routing or shared registries without a clear owner.

## Definition of Ready for a PBI

The use case names the user outcome, source/selection prerequisites, normal flow, error/cancel flow, accepted graph changes, permission scope, affected F-IDs, target screen and testable Given/When/Then examples. Timing, crop, ripple and file-ownership ambiguities are resolved before implementation. A design screenshot alone is not a complete requirement.

## Definition of Done

The behavior works through actual UI and domain commands; all state transitions and failure/cancel paths are covered; originals/products remain safe; keyboard/numeric alternatives and contextual Help work; type/lint/unit/component/native checks pass where applicable; true output is decoded for media cases; docs/contracts are current; residual platform checks are explicit. No unimplemented control silently succeeds, no simulated service is represented as connected, and no historical document is required to understand the delivered feature.

## Final acceptance tasks for representative users

A beginner imports/captures a short screen recording, removes a mistake, rearranges an explanation, adds an arrow/zoom/text cue and presenter overlay, fades audio/video, corrects captions and adds chapters, saves without rendering, closes/reopens, renders and watches the actual product, then makes another edit and a second product. The guide can be used, dismissed and resumed throughout. Observe completion/blockers and confusion rather than relying only on test-suite counts.
