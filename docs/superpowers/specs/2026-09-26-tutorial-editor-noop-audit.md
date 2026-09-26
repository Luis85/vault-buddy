# Tutorial editor — no-op control audit

Date: 2026-09-26 · Branch `claude/vault-buddy-improvement-polish-95f5bc` (HEAD `b6e9556c`) · bundle `dist/` built 21:46, after HEAD.

## Method

**Static.** I read every `@click`/`@change`/`@select`/`@keydown` handler under `src/components/editor/**` and `src/roots/EditorRoot.vue` (the grep inventory: 190 handler sites). I read the whole action registry: `src/editor/actionMeta.ts`, `actions.ts`, `clipboard.ts` (`activateEditorAction`) and `shortcuts.ts`. I traced each one to an IPC command, a store mutation, a dialog, or nothing. I also traced what happens when Rust refuses an `editor_execute` (`src/stores/editorProject.ts` `handleExecuteError` sets `lastError`), and I grepped every reader of `lastError`, `conflictIntent` and `retryConflict`.

**Dynamic.** `.superpowers/research/visual-parity/noop.spec.ts` with `noop.config.ts` (throwaway, git-ignored). It drives the production `dist/` through `tests/e2e/tauriStub.ts`. The project is populated (4 tracks, 6 clips, 1 text cue, 1 marker, 1 transition, 2 captions, 1 product and 1 Checks finding) at 1600×1000, plus a 960×640 compact pass. It runs 36 scenarios: base, clip selected, each inspector tab for three clips, each library tab, Edit actions, the right-click menu on two clips, the track ⋮ menus, the Save ▾ and Help menus, the mixer, the Checks/Render/Learning/Save-copy/Webcam/Publish/Review/Discard dialogs, compact, and compact More.

In each scenario, the spec clicks every visible `button`, `menuitem`, `tab`, `option`, `select` and `checkbox`. For a `select`, it picks a different option instead of clicking. Before each click it reloads and re-applies the scenario. It then records four signals:

- the new `window.__calls` entries;
- `MutationObserver` records, net of a 250 ms idle baseline;
- whether focus moved to anything other than the clicked control;
- the change in the number of open `dialog`/`menu` elements.

A second test presses every bound shortcut, plus a set of unbound keys, with a clip selected. Its result ignores `editor_save_workspace` because that call is debounced from the selection itself. Raw output is in `noop-results.json`, `shortcut-results.json` and `more-results.json`.

### Stub artifacts (not no-ops)

- **`editor_execute` is answered with `undefined`.** The port's decoder throws `ProtocolError`, and the store puts that in `lastError`. So every edit button records **one IPC call and 0 DOM mutations**. I count the IPC call as "did something". The **0 mutations is the real UI behavior for any refusal**, and it is the evidence for finding 1.
- **Checkboxes** (`caption-import-replace`, `webcam-mic`, `publish-create-note`, `speed-section-pitch`) change the `checked` property, which is not a DOM attribute, so `MutationObserver` cannot see it. Statically they are all `v-model`/`@change` wired, and they work.
- **Radio-like controls that are already active**, re-clicked, show nothing: the active library or inspector tab, `Leave gap` while it is already the mode, `render-dialog-quality-balanced`, `render-dialog-scope-whole` and `save-project-format-portable`. That is expected radio behaviour, not a defect.
- **Controls that are `aria-disabled`**, clicked with `force`, show nothing: Split at a clip boundary; Paste with an empty clipboard; Group or Ungroup with one clip selected; Earlier on the first clip; Transition when one already exists; Move up on the top track. This is the intended disabled behaviour. Each one carries a reason, except the one in finding 6.

## Control table

Verdicts: **OK** means it has an observable effect. **NO-OP** means it is enabled but nothing happens. **SILENT-FAIL** means it acts, but a refusal or failure is invisible. **DEAD** means it is registered or emitted but has no consumer. **DISABLED** means disabled as designed.

| Control | Where | How reached | Static verdict | Dynamic verdict | Evidence |
|---|---|---|---|---|---|
| Project title (rename) | shell/EditorHeader.vue:176 | header | OK → `rename` via execute | OK (input mounted, focus moved) | refusal → SILENT-FAIL (finding 1) |
| Help ▾ → Learning center / Resume / Keyboard shortcuts / Export diagnostics | shell/GuideHelpButton.vue:103,122 | header | OK: dialog, `onboarding.start`, dialog, `editor_export_diagnostics` + toast | OK: all four | – |
| Checks | shell/ChecksButton.vue:51 | header | OK → ChecksDialog | OK: `editor_get_checks` + dialog | – |
| Theme toggle ☀/🌙 | shell/EditorHeader.vue:202 | header | OK | OK (label and icon change) | – |
| Save project | shell/EditorHeader.vue:211 | header | OK → `editor_save_project` | OK: status text flips | a failure shows only "Save failed" with no reason (finding 7) |
| Save ▾ → Save / portable copy / lightweight copy / Open a project file / Discard project | menus/SaveProjectMenu.vue:67,88 | header | OK: save, SaveProjectDialog ×2, `editor_import_package`, DiscardProjectDialog | OK: all five | – |
| Render video | shell/RenderVideoButton.vue:82 | header | OK → RenderDialog | OK | – |
| Library 📁 / Inspector ⚙️ (compact) | shell/EditorHeader.vue:154,235 | header, below 1180 px | OK: opens the drawer | OK: aria-hidden and style change | – |
| Preview tools Text / Arrow / Highlight / Spotlight / Zoom / Step / Privacy cover | shell/PreviewToolbar.vue:405, `addCue` :295 | preview toolbar | OK → `addEffect` (disabled with a reason when no clip is under the playhead) | OK: `editor_execute` | refusal → SILENT-FAIL (finding 1) |
| Aspect-ratio select | shell/RatioSelect.vue:64 → `onRatioChange` | preview toolbar | OK → `setCanvas` + toast | OK: `editor_execute` | refusal → SILENT-FAIL |
| Review | PreviewToolbar `runAction("render")` | preview toolbar, Ctrl+E | OK → ReviewDialog + `editor_start_render` | OK | – |
| **Focus preview** | shell/PreviewToolbar.vue:254 → shell/EditorShell.vue:186 | preview toolbar, visible at 1600 | **NO-OP at or above 1180 px** (only closes drawers, which do not exist there; the component documents this) | **NO-OP**: 0 calls, 0 mutations; the preview box is unchanged (1032×628 before and after) | finding 2 |
| **Library / Inspector toggle (toolbar)** | shell/PreviewToolbar.vue:244 → EditorShell `libraryOpen`/`inspectorOpen` | preview toolbar; inside **More** at 1600 | **NO-OP at or above 1180 px**: it flips a hidden ref, and `showLibrary = !isCompact \|\| libraryOpen` | **NO-OP**: both panels visible before and after, preview box unchanged; only the More menu closes | finding 2 |
| More (preview toolbar) | shell/PreviewToolbar.vue:431 | preview toolbar | OK → overflow menu | OK | – |
| Play / pause | preview/TransportBar.vue:91 | transport | OK | OK: media loads, 58 mutations | – |
| Sound (monitor mute) and volume | preview/TransportBar.vue:108,119 | transport | OK (workspace) | OK | – |
| Playback rate select | preview/TransportBar.vue:130 | transport | OK → `workspace.setPlaybackRate` | OK | – |
| Audio mixer toggle, track M/S, master, monitor mute, Close | shell/MixerPopover.vue:113; MixerTrackRow.vue:69 | transport | OK: `setTrackFlags` / `setMasterGain` | OK | refusal → SILENT-FAIL |
| Preview stage click | preview/PreviewSurface.vue:251 | click the video | **DEAD emit**: `canvas-pointerdown` has no listener (`<PreviewSurface />` at roots/EditorRoot.vue:397); the only effect is deselecting a cue | not a button, so not swept | finding 5 |
| Library tabs Media / Titles / Captions / Chapters / Products | library/LibraryPanel.vue:109 | library | OK | OK | – |
| Import… | library/MediaLibrary.vue:172 | Media | OK → `editor_import_media` | OK | – |
| Webcam… | library/MediaLibrary.vue:183 | Media | OK → WebcamDialog | OK | – |
| Reconnect… | library/MediaLibrary.vue:200 | Media, only when something is missing | OK → ReconnectDialog | not reachable (nothing missing in the fixture) | – |
| "+" add to timeline | library/LibraryAssetCard.vue:75 → MediaLibrary.vue:116 | Media card | OK → `insertClip` at the playhead on the **first** unlocked track of that kind | OK: `editor_execute` | **SILENT-FAIL whenever the playhead overlaps a clip on that track**; Rust refuses "clip would overlap" (core clips.rs:227) (finding 1a) |
| Titles Intro / Chapter / Outro / Blank | library/TitlesLibrary.vue:46,97 | Titles | OK → `addCard` at the playhead on the first video track | OK: the call was `{addCard, trackId:"v3", startMs:0, durationMs:3000}`, which overlaps c5 (1500–33500), 0 mutations | **SILENT-FAIL**: Rust refuses "card would overlap" (core cards.rs:263) (finding 1a) |
| Captions: Import SRT/WebVTT, Add at playhead, Split at playhead, Replace, Export .srt/.vtt, Enabled / Burn-in / Background / Position / Font size, per-cue time, text, delete, notice-select | library/Captions*.vue | Captions | OK; Split is disabled with a reason when no caption is at the playhead | OK: `editor_import_captions` / `editor_execute` / `editor_export_subtitles` | import refusal (for example "nothing inside the clip") → SILENT-FAIL: `importCaptions` returns `null`, and the status line is written only on success (CaptionsLibrary.vue:96). Add and delete refusals are silent too |
| Chapters: Add at playhead, jump, rename, delete | library/ChaptersLibrary.vue:32,56,87,98,105 | Chapters | OK | OK | add/delete refusal → SILENT-FAIL; rename reverts silently |
| Products: Watch, Publish to vault…, Restore, confirm/cancel | library/ProductCard.vue:80–129 | Products | OK; a refusal is shown in `product-library-error` | OK | – |
| Inspector tabs Clip / Layout / Fades / Audio / Speed / Color | inspector/InspectorPanel.vue:152 | inspector | OK | OK | – |
| Clip: name / start / in / out fields, Move earlier / later | inspector/ClipSection.vue:199,228 | inspector | OK | OK: `editor_execute` | a refused field silently reverts (composables/useInspectorDraft.ts:195) (finding 1b) |
| Audio: Mute clip audio, Detach audio, volume | inspector/AudioSection.vue:81; AudioDetachControl.vue:50; AudioVolumeField.vue | inspector | OK | OK | a detach refused for "source has no sound" (Rust-only check) → SILENT-FAIL; the file's own doc (AudioDetachControl.vue:8) says it "comes back refused, through `lastError`", which nothing renders |
| Fades: in/out fields, curve, Add transition, transition duration, Remove | inspector/FadesSection.vue:113–150; ClipTransitions.vue:74; TransitionRow.vue:69,87 | inspector | OK | OK | refusal → silent revert or SILENT-FAIL |
| Layout: corners, shape, fit, rotation, mirror, flip, and the numeric fields; canvas handles | inspector/LayoutSection.vue:178–258; preview/LayoutHandles.vue | inspector, preview | OK | OK | refusal → SILENT-FAIL |
| Speed: presets, value, keep pitch | inspector/SpeedSection.vue:99,116 | inspector | OK; re-clicking the current preset is a deliberate no-op (`aria-pressed`) | OK | – |
| Color: presets and sliders | inspector/ColorSection.vue:142 | inspector | OK | OK | – |
| Cue inspector: fields, colour, background, Remove, Back | inspector/EffectSection.vue:208–232; InspectorPanel.vue:122 | select a cue | OK | not swept (the cue was not selected in any scenario) | – |
| Timeline toolbar Split / Undo / Redo / Delete | timeline/TimelineToolbar.vue:128,140 | timeline | OK via the registry; disabled with a tooltip reason | OK, or DISABLED | refusal → SILENT-FAIL |
| Edit actions | timeline/TimelineToolbar.vue:147 | timeline | OK → the same ContextMenu, for the selection | OK | – |
| Leave gap / Close gap on this track / Snap / − / + / Fit | timeline/TimelineToolbar.vue:161–220 | timeline | OK (workspace) | OK | – |
| Timeline resize handle | timeline/TimelineView.vue:347 | drag | OK | not swept (drag) | – |
| Clip context menu: Split, Copy, Cut, Paste, Duplicate, Group, Ungroup, Move earlier, Move later, Add transition, Delete (close gap), Delete | timeline/TimelineView.vue CLIP_CONTEXT_ITEMS; menus/ContextMenu.vue:196 | right-click, Shift+F10, Menu key, Edit actions | OK; Copy is local only (clipboard.ts:102) | OK: every enabled item sends `editor_execute`, except Copy (menu closes, Paste becomes enabled) | Copy gives no confirmation (finding 8); refusals are SILENT-FAIL |
| Track header name / eye / lock / M / S / volume | timeline/TrackHeader.vue:244–317 | timeline | OK → `renameTrack` / `setTrackFlags` | OK: `editor_execute` | refusal → SILENT-FAIL |
| Track ⋮ → Move up / Move down / Delete track | timeline/TrackHeader.vue:334–376 | timeline | OK → `moveTrack` / `deleteTrack` | OK | **Move up on the top track and Move down on the bottom track are `aria-disabled` with no reason** (`menuItemTitle` is `undefined` unless the track is locked, TrackHeader.vue:194) (finding 6) |
| Registry actions `addTrackVideo` / `addTrackAudio` | editor/actions.ts:281; actionMeta.ts:240 | **no surface anywhere** | **DEAD / placeholder**: permanently disabled with "…arrives in a later update."; no toolbar, menu or shortcut renders them | – | finding 3 |
| Registry actions `fadeIn` / `fadeOut` / `detachAudio` / `addCaption` / `addMarker` / `importMedia` / `webcam` / `checks` / `save` / `ratio` / `toggle*` / `focusPreview` | editor/actions.ts RESOLVERS | read by FadesSection / AudioDetachControl / MediaLibrary / ChecksButton | these have consumers (resolver used for enablement; the effect is sent directly) | – | `addCaption` and `addMarker` are never read by any component; CaptionsLibrary and ChaptersLibrary call `captionRules` directly. They are harmless duplicates |
| `retryConflict` / `conflictIntent` | stores/editorProject.ts:133,414 | **no UI** | **DEAD**: a `revisionConflict` parks the user's edit in `conflictIntent`, and no component reads it or offers Retry | not reachable with the stub | finding 4 |
| Dialog buttons: Checks (Close, Back, Show it, Continue to render), Render (quality, scope, Open Checks, Start, Close), Review (Close, Cancel), Learning center (tabs, chapters, lessons, Start over, Resume, Close, Save/Restore progress file, dimming, motion), Save-copy (format, Save, Cancel), Webcam (Enable, Record, Stop, Retake, Add, camera/mic, Close), Publish (vault, folder, dated, note, Publish, Open, Close), Discard (Cancel, Discard) | components/editor/dialogs/*, guide/* | dialogs | OK | OK, apart from the stub artifacts above | – |
| Guide invitation, coach (Back, Next, Pause, Collapse, Show me), Resume guide | guide/GuideInvitation.vue:42–66; GuideCoachCard.vue:56–146; GuideCoach.vue:292 | guide | OK → `editorOnboarding` | not swept (the invitation was dismissed) | – |

### Keyboard (clip c2 selected, playhead at 0)

Every shortcut the learning center lists does something when its action is enabled:

- Delete, Backspace, Shift+Delete, Ctrl+Z, Ctrl+Shift+Z, Ctrl+Y, Ctrl+X, Ctrl+V and Ctrl+D send `editor_execute`.
- Ctrl+C fills the clipboard.
- Ctrl+S sends `editor_save_project`.
- Ctrl+E opens Review.
- F1 and `?` start the guide.
- Shift+F10 and the Menu key open the clip menu.
- ← and → nudge the clip.
- Space plays.

**Silent when disabled:**

- `S` at a clip boundary.
- Ctrl+G and Ctrl+Shift+G with one clip.
- F6 with no guide open.
- Escape with nothing open.

These keys do nothing and give no reason. The reason lives only in toolbar `title` tooltips, so a keyboard user never sees it (finding 9). Keys that are not bound (Home, End, J, K, L, I, O, M, Ctrl+A) are also not advertised anywhere, so they are not defects.

## Prioritised true no-ops and dead ends

1. **Refused edits are invisible everywhere (systemic SILENT-FAIL).**
   - **What happens:** `editorProject.execute` → `handleExecuteError` puts Rust's refusal in `lastError`, and no always-mounted surface renders `editorProject.lastError`. Only a few dialogs do (ChecksDestination, DiscardProject, WebcamDialog, the close guard, recovery).
   - **Dynamic evidence:** every edit control produced **0 DOM mutations** after its refused `editor_execute`, across 124 of the 126 control and scenario pairs that sent one (the other two show only an unrelated inspector field re-render after Cut).
   - **1a. Most likely to bite in normal use:** Titles **Intro / Chapter / Outro / Blank** and the Media **"+"** insert. Both insert at the playhead on the first track of the right kind, and Rust refuses the overlap. At playhead 0 in the fixture the card targets v3, which already has c5, so clicking does nothing at all. Caption import refusals, caption and chapter add, Detach audio for a silent source, and Split/Delete refusals behave the same way.
   - **1b. Inspector fields:** they quietly revert the text (useInspectorDraft.ts:195) with no message.
   - **Fix:** render `editorProject.lastError` from a shell-level surface. `EditorShell` already mounts `NotificationHost`, so a `watch(() => editorProject.lastError)` that raises `notifications.error(error.message)` is enough. Clear it on the next success (the store already does). No backend change is needed. Separately, make "+" and the Title cards pick a free track, or add a track (`addTrack` exists and `TimelineView.addTrackThenInsert` already does this), instead of always taking the first one.
2. **Focus preview, and the toolbar's Library / Inspector toggles, do nothing at 1180 px and wider** (PreviewToolbar.vue:244–256 → EditorShell.vue:141–142,186).
   - These are the default desktop widths. At 1600×1000 the preview box is unchanged, and the two toggles sit in More.
   - Worse, they flip a hidden `libraryOpen`/`inspectorOpen` that takes effect later if the window is narrowed.
   - **Fix, either:**
     - make `showLibrary`/`showInspector` honour the toggles at every width, which gives Focus preview a real distraction-free layout (`Workspace.focus_preview` already exists in `editorTypes.ts`/`editorWorkspace`); or
     - hide these three items from `TOOLBAR_ITEMS` while `!isCompact`.
   - No backend change is needed.
3. **Dead registry actions `addTrackVideo` / `addTrackAudio`** (actions.ts:281; actionMeta.ts:240 "arrives in a later update").
   - They are permanently disabled with no surface, even though Rust implements `addTrack` and `TimelineView` already sends it.
   - The user has no way to add an empty track except dragging an asset below the last lane.
   - **Fix:** add "Add video track" and "Add audio track" to the track ⋮ menu, or below the lanes, wired to `{kind:"addTrack", trackKind, name, index}` like `TimelineView.addTrackThenInsert`. Give the two ids real resolvers and builders, or delete them from the registry. The existing command is enough; no new backend command is needed.
4. **A revision conflict silently drops the user's edit.**
   - The store parks it in `conflictIntent` and exposes `retryConflict()` (editorProject.ts:133,414), but no component reads either. The edit disappears and there is no Retry.
   - **Fix:** a notice with a "Retry" button that calls `editorProject.retryConflict()` (render it with finding 1's surface).
5. **Clicking the preview canvas does nothing.**
   - `PreviewSurface` emits `canvas-pointerdown` (PreviewSurface.vue:251) "for the canvas tools that arrive in later tasks", and nobody listens (EditorRoot.vue:397).
   - **Fix, either:** handle it in `EditorRoot` to select the top visible clip under the point (`previewLayers` or `layoutGeometry` already compute the rects), or remove the emit and the stale doc. No backend change is needed.
6. **Track ⋮ Move up (top track) and Move down (bottom track) are disabled with no reason** (TrackHeader.vue:194). This breaks the registry's own "every disabled control carries a reason" rule.
   - **Fix:** set the title to "Already the top track" or "Already the bottom track".
7. **"Save failed" shows no reason.** EditorHeader.vue:94 renders only the words, although `saveError.message` (for example disk full or write denied) is available.
   - **Fix:** show the message in `editor-header-save-reason` or a toast.
8. **Copy gives no feedback** (clipboard.ts:102). The menu closes, and the only trace is that Paste becomes enabled.
   - **Fix (low priority):** an info toast, "Copied 2 clips".
9. **Disabled shortcuts say nothing** (EditorShell.vue:250 `onShellKeydown` lets a disabled action bubble). Examples: `S` at a boundary, Ctrl+G with one clip. Keyboard users never see the reason, which is only a hover tooltip.
   - **Fix (low priority):** on a matched but disabled action, announce `resolveActions(ctx)[id].reason` through the notifications store or an `aria-live` region.

These were **not** defects, and a fix must not "correct" them:

- re-clicking the already-active tab, radio or preset;
- the checkbox no-ops (a harness limitation);
- `aria-disabled` items that carry a reason;
- the Edit actions menu opening with every item disabled when nothing is selected. Each item names why.
