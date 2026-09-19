# Native-stack integration starter

This is a **small, typed integration reference**, not a second editor and not a completed native port. The full interaction target is `../vault-buddy-editor.html` and the implementation contract is `../docs/ARCHITECTURE-AND-STACK.md`.

The proposed `editor_*` commands must be implemented and registered in Rust before the adapter can run inside Vault Buddy. No command is presented as already existing. The existing screen-capture store and commands are reused, not replaced.

Use the application's installed, lockfile-controlled dependencies when integrating. In an isolated checkout, run `npm install`, then `npm run typecheck` and `npm test`, and commit the resulting reviewed lockfile. Do not upgrade the application as a side effect of copying these examples.

**Verification:** the dependency-free DTO/timing/listener subset compiles under the available TypeScript 5.8.3 and passes 12 Node tests. The repository targets TypeScript 6. Full `vue-tsc`/Vue/Pinia/Vitest execution was **blocked** here because registry dependency installation could not resolve the network host. No lockfile-resolved test run is claimed, and no fake dependency declaration stubs were used. The Rust examples were not compiled: Cargo is unavailable in this environment. Run both sets in the application's actual toolchain before integration.

## What this demonstrates

Vue `<script setup lang="ts">`, typed Pinia option stores consistent with the application, a mockable Tauri port, runtime decoding of untrusted IPC responses, revision-aware native acknowledgments, protection from stale asynchronous results, serializable view/guide state, isolated preview-controller handles, event-listener disposal and integer temporal conversion.

`contracts.ts` is a deliberately small native **session-summary protocol**, not the entire render graph. Use the full graph specification under `../contracts/` for media/composition. Add real validated commands incrementally. Do not replace the application's existing stores or its Rust segment algebra with these teaching examples.

`PreviewSurface.vue` is a component boundary example. It does not implement media decoding or the final editor layout. Bind a stable controller instance for a component's lifetime; recreate the component for a new controller/session.

`createGuideStore` demonstrates progress state. Connect the complete 22-step content, overlay positioning, context resolution, keyboard and suspension behavior from the reference. Its native DTO intentionally differs from the browser recovery file: write an explicit adapter, not a schema-string rename.

## Persistence and resource ownership

No files, URLs, streams, media elements or canvas frames enter Pinia serialization. No giant `$subscribe` writes the whole store to localStorage. Rust owns project commits and durability receipts. The store clears its dirty state only after a matching save receipt. Detaching a view is not discarding a project or cancelling a render. Native session teardown, job query/recovery and cancellation are separate explicit commands.

## Files and execution

`tests/run-pure.sh` runs the independently checked TypeScript helpers; `tests/pure-results.json` records the actual result. `tests/stores.test.ts` contains 22 proposed Vitest cases for the Vue/Pinia integration and must run in the repository toolchain. `rust/editor_contracts.rs` contains matching summary DTO and timebase examples; it does not register fake working commands or implement the media engine.
