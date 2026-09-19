# Product specification

## Product outcome

Vault Buddy Tutorial Editor helps a person who can demonstrate a task on screen turn that demonstration into reusable knowledge. They record or import footage, remove unnecessary moments, make important details visible, add narration/presenter context and save an editable project. They then publish a video and an optional Obsidian companion note. They can return to the project after publication without reconstructing the edit.

The intended user is a subject-matter expert, delivery lead, support colleague, trainer or personal knowledge worker—not a professional colorist. The product prioritizes a fast first useful tutorial, understandable edit semantics and reliable resumption over broad film-production features.

## Core jobs and success conditions

| Job | Completion condition |
|---|---|
| Explain a workflow | A viewer can see the important interaction, read necessary text and hear speech without competing audio. |
| Remove a mistake | The unwanted span disappears, later clips follow the chosen gap behavior, and the action can be undone. |
| Emphasize a detail | Text, arrow, highlight, spotlight, numbered step or zoom is visible only during its intended source span. |
| Add a presenter | A camera take becomes an independently timed, movable/resizable layer; it is not baked into the screen source. |
| Work later | Saving the project does not require rendering. Reopening restores the composition and workspace or explicitly lists missing sources. |
| Publish and revise | Rendering creates a separate output with an immutable edit snapshot. Subsequent editing does not replace that output. |
| Learn without risk | The walkthrough can be read without applying edits or granting device access; dismissal retains the current lesson. |

These are acceptance outcomes. No user study or timed usability benchmark is implied by this handover.

## Primary journey

Capture finishes in app-owned staging outside the vault. The editor opens the corresponding project with the intended destination. The user reviews, cuts and arranges footage, adds other audio/video layers, adds teaching cues, checks the tutorial, saves the project, renders an output, reviews the encoded file, and explicitly publishes the selected output/note into the vault. They may save at any earlier point and render later.

Importing an existing video or opening a project enters the same editing workspace. Starting a new project requires confirmation when the current project contains unsaved work. Closing the editor does not implicitly discard staged media.

## Product objects

**Source media** is immutable recorded/imported content. **Clip** is a reference to a source range at a timeline position. **Track** is an ordered composition layer with editing and audio controls. **Teaching cue** is linked to a clip and source-time span. **Workspace** is view state such as playhead, selection and open panels. **Project** combines the edit and source references with the workspace and render history. **Rendered product** is a flattened, independently playable output with provenance. **Companion note** is optional Markdown that embeds the selected product and documents chapters/context. **Guide progress** is app/user learning state and is never part of the rendered video.

## Scope

The complete scope is indexed by feature IDs in [FEATURE-CATALOG.md](FEATURE-CATALOG.md). It includes multi-track editing, fades/transitions, webcam takes and picture-in-picture, direct-manipulation/context actions, teaching annotations, captions, chapter markers, title cards, image assets, basic color/speed/transforms, audio mixing, portable projects, rendered history, recoverable missing media and onboarding.

Native simultaneous screen/webcam capture is an integration requirement. The browser reference records a webcam take for an existing edit; it does not demonstrate hardware-synchronized combined capture. Independent mic and desktop-audio stems require a capture extension because the checked screen-capture foundation mixes selected inputs into stereo.

Automatic transcription, AI generation, background removal, motion tracking, cloud collaboration, stock-content subscriptions, professional color management and secure automatic redaction are not represented as implemented. They are not required to match this handover. A static privacy cover must never be marketed as tracked or guaranteed redaction.

## Interface principles

One permanent home per command family; contextual shortcuts may expose the same commands. The app header owns files and publication. The preview header owns teaching and view controls. The timeline owns selection edits. Properties owns detailed parameters. Hidden/overflow tools remain discoverable and keyboard reachable. Preserve the project before changing implementation architecture.

A disabled command explains the prerequisite. Risky actions name their consequences. A download request is not described as a confirmed native save. A progress bar reaches completion only after the corresponding operation actually completes. A render is not the same thing as an editable project.

## Non-functional acceptance targets

These are proposed product acceptance targets, not measured native claims. Confirm the minimum Windows machine during implementation planning.

| Concern | Target and verification |
|---|---|
| Interaction | Pointer/keyboard feedback must remain responsive during media preparation and background rendering; measure input-to-visible-feedback p95 on the agreed machine. Target ≤100 ms for ordinary edit commands. |
| Render correctness | Decode and inspect start/middle/end frames, timestamps, speech continuity, transitions and audio, not only container metadata. Frame/sample policies are defined in NATIVE-MEDIA.md. |
| Persistence | No loss of the last committed project during failure injection at each save/publish phase; no clobber of unrelated user files. |
| Memory | Bounded queues/caches independent of output duration. Demonstrate a ten-minute 1080p/30 tutorial with three video/two audio layers; report peak and steady memory. Set the limit from a measured baseline. |
| Accessibility | Keyboard completion of the main journey; usable focus, menus, dialogs and field labels at Windows scaling/zoom; Narrator/NVDA and high-contrast manual acceptance. |
| Privacy | No media uploads or content telemetry by default. Logs/diagnostics omit frames, audio, captions, filenames and absolute paths unless explicitly opted into a support export. |
| Recovery | Missing media, permission denial, device removal, disk full and interrupted render must leave a usable project with a concrete next action. |

## Acceptance authority

The behavior rules, catalog and data invariants define the product. Screenshots define layout intent, not numeric media semantics. The browser provides an executable interaction reference; the Rust engine is the native authority. Any necessary departure must be recorded with a reason, affected feature IDs and replacement acceptance evidence—not silently reduced during implementation.
