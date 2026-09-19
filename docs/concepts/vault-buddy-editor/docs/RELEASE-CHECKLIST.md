# Native release acceptance checklist

**Status:** implementation gates, not a statement that the native product has passed. Browser reference verification is documented separately. Every item must have an owner, environment, result, evidence link and disposition. Blocked/untested items are not passes.

## Product and UX

- [ ] Every feature F-01…F-50 is implemented or explicitly removed by an approved scope decision, never silently lost during porting.
- [ ] One preview-header row, contextual command ownership, compact 960×640 behavior, light/dark themes and reversible panel layouts match the reference.
- [ ] Primary beginner journey completes: capture/import → cut/arrange → teach/presenter → captions → save/reopen → render/watch → refine/second product.
- [ ] All 22 lessons / 7 chapters, help search, dismiss/resume/collapse/restart and native progress retention work without unintended edits or permission requests.
- [ ] Keyboard/numeric alternatives, menu/dialog focus, reduced motion, high contrast and a real assistive-technology pass have evidence.

## Repository and architecture

- [ ] Reconciled schema/state/root/store/module ownership; no duplicate capture engine, unnecessary router/framework or copied imperative UI wrapper stack.
- [ ] Application lockfiles and approved Rust/Node/TS toolchains used; no unreviewed dependency upgrade.
- [ ] Existing frontend build/typecheck/lint/unit/coverage/LOC/quality gates pass with editor tests.
- [ ] Existing applicable Rust fmt/clippy/test/coverage/license/advisory gates pass; Windows release build/installer runs.
- [ ] DTO fixtures, runtime validators, generation/revision guards, event cleanup and command-permission tests pass.

## Media correctness and devices

- [ ] Actual Windows screen/window capture; webcam/mic/system audio permissions, denial, unplug and device-change behavior verified.
- [ ] Simultaneous sources use shared monotonic/pause clock; independent audio stems are recorded when presented as editable stems.
- [ ] All transformations/cues/captions/fades/transitions match preview and decoded output at boundaries, speed changes and aspect ratios.
- [ ] Output frame cadence, duration, color/alpha, audio channels/energy, clipping policy and A/V offset meet agreed tolerances on named hardware.
- [ ] Whole/range rendering, actual product playback, repeated render products and immutable snapshots verified.
- [ ] Encoder-unavailable/slow hardware/cancel/hidden or closed editor scenarios preserve the project and report truthful status.

## Persistence, security and recovery

- [ ] Save independent of rendering; receipt follows durable commit; stale save receipt cannot clear newer edits.
- [ ] Portable project reopens with current and historical sources; lightweight project reports missing media and refuses ambiguous relinks.
- [ ] Disk-full/permission-denied/locked-file/name-collision/kill-during-save cases preserve previous good project and originals.
- [ ] Output + note + product-record partial publication is recoverable/idempotent with no unrelated overwrite.
- [ ] Session/vault resolution uses native staging metadata; imported JSON paths cannot authorize arbitrary reads/writes.
- [ ] App-command ACL, narrow capability/media scopes, native caller/session checks and archive path/size/cycle validation pass adversarial tests.
- [ ] Owned-resource cleanup respects active jobs/Undo/product references; no blanket vault or media-extension deletion.
- [ ] Privacy covers never imply source redaction; project sharing warns about uncensored originals; diagnostics do not disclose media by default.

## Performance and operation

- [ ] Named hardware/project fixtures and product limits approved; latency/cadence/memory targets are measured under supported conditions.
- [ ] Large timeline/caption lists remain responsive with virtualization/caches; idle preview performs no repeated DOM rebuilds.
- [ ] Media/observer/listener/URL resources release after repeated session and dialog cycles; no growth from disposed jobs.
- [ ] User documentation, native diagnostics, supported formats, recovery instructions and release notes match actual shipping capabilities.
- [ ] Product owner, delivery/engineering owner and QA sign off on evidence and residual risks.
