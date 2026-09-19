# Implementation-session brief

Use this file as the initial brief for a developer or coding-agent session. It is intentionally self-contained and points to the current authoritative documents, not conversation history.

## Assignment

Implement Vault Buddy's Tutorial Editor in the existing **Rust + Tauri + TypeScript + Vue + Pinia** application. Start by inspecting the target branch, current capture engine, root routing, semantic design tokens, persisted types, tests, file-ownership helpers and security permissions. Read the product specification, feature catalog, architecture, data/IPC/persistence/media contracts and execution plan in this bundle. Open the standalone reference and exercise the full onboarding journey.

Produce an implementation map before changing shared data structures. The inspected baseline has native screen capture and `screen:stopped`, but no final editor root/window API; reconcile current code rather than trusting a stale pull-request summary. The stopped DTO lacks a vault ID, so native staging sidecar metadata supplies the destination. Existing audio mixdown is not separable stems. Existing sequential timeline algebra is not the full multi-track model. Existing direct sidecar writes are not crash-safe project saves.

## Non-negotiable product behavior

Keep one preview toolbar row and the clean contextual layout. Keep every F-ID in scope. Keep Save project separate from Render video, with originals and immutable product snapshots retained. Keep multiple video/audio tracks, webcam overlay editing, fades/transitions, teaching tools, captions, chapters and context menus. Keep onboarding optional and resumable with all 22 lessons. No permission request, recording, mutation, file write or rendering starts from passive guide navigation.

Use Vue components and scoped composables; do not port the browser's global functions, repeated DOM rendering or wrappers wholesale. Keep resources out of deep reactive state. Rust owns validated committed operations, persistence and production output. Use runtime validation and revision/session guards, not just `invoke<T>` assertions. Do not send raw frames through global JSON events. Do not add FFmpeg/another framework as an incidental dependency. Do not weaken existing file safety or quality gates.

## Work sequence

Execute P00 first and record concrete code paths, type conflicts and reuse decisions. Then complete the smallest native vertical slice: stopped capture → editor → edit → save project → reopen. Expand by dependency packages, keeping fixtures and behavior checks aligned. Before claiming a package done, run its real tests and attach results. Use mocks for deterministic frontend tests but distinguish them from physical-device/native-storage/render verification.

## Reporting format

State implemented behavior, changed files, model/IPC decisions, executed checks with actual outputs, blocked checks and exact remaining release gates. Cite the corresponding feature IDs and document sections. Do not claim success from a file existing, a progress bar reaching 100%, or a browser-only test. Decode actual rendered media; test injected disk failures and stale async results. Do not replace unfinished work with silently disabled controls. Preserve source media, staged work and the last good project on every rejected operation.

## Approval gates

Ask for a product decision only when changing agreed scope, data compatibility, platform support, media licensing/dependencies, privacy/access boundaries or retention policy. Routine code structure and test improvements should proceed within existing standards. A complete technical implementation still needs the native release checklist and product acceptance; this reference is not a pre-approved production binary.
