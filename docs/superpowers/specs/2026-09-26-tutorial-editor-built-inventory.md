# Tutorial editor UI inventory (built, as of this branch)

Scope: `src/components/editor/**`, `src/roots/EditorRoot.vue`, `src/editor/**`,
`src/stores/editor*.ts`, `src/style.css`, `src/components/ui/**`.
All LOC counts are **nonblank** lines (`grep -cve '^\s*$'`), matching
`scripts/check-loc.mjs`'s counting method (cap 500/file). No editor `.vue`
file appears in `scripts/loc-baseline.json`'s allowlist, so every file below
is already under the 500-line cap; headroom = `500 - LOC`.

---

## 1. Component table

Legend for "Built from": **TW** = Tailwind utility classes only (the norm);
**UI** = imports one of `src/components/ui/*`; **scoped** = has a `<style
scoped>` block. Only **2 of 91** editor `.vue` files use scoped CSS at all —
`EditorShell.vue` (grid-template-columns via CSS custom properties,
`src/components/editor/shell/EditorShell.vue:367-371`) and `GuideCoach.vue`
(the highlight ring, `src/components/editor/guide/GuideCoach.vue`, `<style
scoped>` block, `.guide-ring`/`.guide-ring-dim`/`.guide-ring-animated` +
forced-colors override). Everything else is inline Tailwind class strings.
`ui/` primitive usage is sparse and lopsided: 27 files import `AppButton`,
3 import `IconButton`, 1 imports `Field`, 1 imports `CountBadge` — zero
files import `Banner`, `Chip`, `StatusDot`, `Avatar`, `SectionHeader`,
`EmptyState` or `Spinner`. The editor mostly hand-rolls its own
button/input/badge markup with Tailwind rather than consuming the shared
`src/components/ui/` vocabulary the panel window uses (grep: `grep -rohE
"(AppButton|IconButton|Chip|CountBadge|StatusDot|Avatar|SectionHeader|
Banner|Field|EmptyState|Spinner)\.vue" src/components/editor`).

| Path | LOC | Headroom | Region | Built from |
|---|---:|---:|---|---|
| `src/roots/EditorRoot.vue` | 424 | 76 | window root — mounts shell, gates on session match, hosts CloseGuard/Discard/Recovery dialogs, OpenFailureNotice, empty state | TW |
| **shell/** | | | | |
| `shell/EditorShell.vue` | 346 | 154 | grid layout, keyboard dispatcher, drawer state, theme apply | TW + scoped (grid-template-columns) |
| `shell/EditorHeader.vue` | 232 | 268 | header row: title/rename, duration, status, vault id, Help/Checks/theme/Save/Render, drawer toggles (compact) | TW, UI(AppButton×2, IconButton×2) |
| `shell/PreviewToolbar.vue` | 419 | 81 | preview toolbar row: teaching-tool buttons, ratio select, Review, focus/library/inspector toggles, roving tabindex, overflow "More" | TW |
| `shell/RatioSelect.vue` | 69 | 431 | preview toolbar — canvas ratio `<select>` | TW |
| `shell/ToolbarOverflowMenu.vue` | 65 | 435 | preview toolbar — overflow "More" menu popup | TW |
| `shell/RenderVideoButton.vue` | 91 | 409 | header — Render video button + disabled reason (`editor-header-render`) | UI(AppButton) |
| `shell/ChecksButton.vue` | 64 | 436 | header — Checks button + CountBadge | UI(AppButton, CountBadge) |
| `shell/GuideHelpButton.vue` | 129 | 371 | header — Help menu (learning center / resume / shortcuts) | UI(AppButton) |
| `shell/DialogHost.vue` | 165 | 335 | dialogs (generic) — focus trap/backdrop/stack host used by most dialogs | TW |
| `shell/MixerPopover.vue` | 162 | 338 | preview toolbar/transport — the mixer popover shell | TW |
| `shell/MixerTrackRow.vue` | 72 | 428 | mixer popover — one track row (mute/solo/gain) | TW |
| `shell/MixerSlider.vue` | 54 | 446 | mixer popover — gain slider | TW |
| `shell/MixerPeakMeter.vue` | 52 | 448 | mixer popover — peak meter bar | TW |
| `shell/OpenFailureNotice.vue` | 112 | 388 | root-level empty/failure state when no session is open | TW |
| **timeline/** | | | | |
| `timeline/TimelineView.vue` | 368 | 132 | timeline: scroll container, ruler, playhead, track lanes, resize handle, one ContextMenu instance, DnD | TW |
| `timeline/TimelineToolbar.vue` | 209 | 291 | timeline toolbar row (Fit, zoom, snap, Edit actions "More") | TW |
| `timeline/TimelineRuler.vue` | 107 | 393 | timeline ruler (ticks/labels), scrolls with content | TW |
| `timeline/TrackLane.vue` | 168 | 332 | one track's row: label column (`track-lane-header-*`) + clip body (`track-lane-body-*`) | TW |
| `timeline/TrackHeader.vue` | 358 | 142 | track header controls: name/eye/lock/mute/solo/volume/⋮ menu | TW |
| `timeline/ClipItem.vue` | 424 | 76 | one clip: body, fade wedges/handles, trim handles, waveform/thumbnail, label | TW |
| `timeline/ClipWaveform.vue` | 84 | 416 | clip — waveform SVG polyline | TW (raw `<svg>`) |
| `timeline/ClipThumbnail.vue` | 51 | 449 | clip — poster-frame `<img>` via asset protocol | TW |
| **preview/** | | | | |
| `preview/PreviewSurface.vue` | 313 | 187 | preview stage: canvas letterbox, layer host, cue/caption overlays, layout/cue handles | TW |
| `preview/TransportBar.vue` | 137 | 363 | preview — play/pause, scrub, volume, rate | TW |
| `preview/CueOverlay.vue` | 92 | 408 | preview stage — teaching-cue SVG render (burned-in look) | TW (raw `<svg>`) |
| `preview/CueHandles.vue` | 231 | 269 | preview stage sibling — cue selection outline/grab handles/zoom focal marker | TW |
| `preview/LayoutHandles.vue` | 205 | 295 | preview stage sibling — picture-in-picture position/size/rotate handles | TW |
| `preview/CaptionOverlay.vue` | 69 | 431 | preview stage — burned-in caption text layer | TW |
| `preview/ProductPlayer.vue` | 79 | 421 | dialogs(RenderOutcome)/library(ProductCard) — plain `<video>` playback of a rendered product | TW |
| `preview/RenderProgress.vue` | 35 | 465 | dialogs(Render/Review) — render job progress bar | TW |
| **inspector/** | | | | |
| `inspector/InspectorPanel.vue` | 177 | 323 | inspector shell: 6-tab tablist (Clip/Layout/Fades/Audio/Speed/Color) + effect override | TW |
| `inspector/ClipSection.vue` | 225 | 275 | inspector › Clip tab: name/start/in/out + `ClipTransitions` | TW |
| `inspector/ClipTransitions.vue` | 72 | 428 | inspector › Clip tab — cross-fade transition list | TW |
| `inspector/TransitionRow.vue` | 84 | 416 | inspector › Clip tab — one transition row | TW |
| `inspector/LayoutSection.vue` | 263 | 237 | inspector › Layout tab: position/size/presets/crop/rotate/mirror/opacity | TW |
| `inspector/FadesSection.vue` | 158 | 342 | inspector › Fades tab: duration/curve numeric entry | TW |
| `inspector/AudioSection.vue` | 84 | 416 | inspector › Audio tab: volume/mute/Detach audio | TW |
| `inspector/AudioVolumeField.vue` | 83 | 417 | inspector › Audio tab — dB-displayed linear volume slider | TW |
| `inspector/AudioDetachControl.vue` | 53 | 447 | inspector › Audio tab — Detach audio control | TW |
| `inspector/AudioStemsNote.vue` | 35 | 465 | inspector › Audio tab — per-input stems note | TW |
| `inspector/SpeedSection.vue` | 118 | 382 | inspector › Speed tab: presets/numeric/pitch toggle | TW |
| `inspector/ColorSection.vue` | 168 | 332 | inspector › Color tab: 6 presets + 5 sliders | TW |
| `inspector/EffectSection.vue` | 218 | 282 | inspector — selected teaching-cue property editor (replaces the 6 tabs) | TW |
| `inspector/InspectorNumberInput.vue` | 48 | 452 | inspector (shared) — numeric field used by most sections | TW |
| **library/** | | | | |
| `library/LibraryPanel.vue` | 121 | 379 | library: 5-tab tablist (Media/Titles/Captions/Chapters/Products) | TW |
| `library/MediaLibrary.vue` | 226 | 274 | library › Media tab: search, asset cards, Import, "+" insert | TW |
| `library/LibraryAssetCard.vue` | 73 | 427 | library › Media tab — one asset card (draggable) | TW |
| `library/ImportStatus.vue` | 76 | 424 | library › Media tab — import job progress/results | TW |
| `library/TitlesLibrary.vue` | 97 | 403 | library › Titles tab — title-card presets | TW |
| `library/CaptionsLibrary.vue` | 208 | 292 | library › Captions tab: cue list + toolbar + settings + export | TW |
| `library/CaptionCueRow.vue` | 104 | 396 | library › Captions tab — one caption cue row | TW |
| `library/CaptionsToolbar.vue` | 63 | 437 | library › Captions tab — import/replace toolbar | TW |
| `library/CaptionSettingsPanel.vue` | 120 | 380 | library › Captions tab — burn-in/position settings | TW |
| `library/CaptionsExport.vue` | 60 | 440 | library › Captions tab — Export .srt/.vtt | TW |
| `library/CaptionNotices.vue` | 34 | 466 | library › Captions tab — skipped-cue notice | TW |
| `library/ChaptersLibrary.vue` | 105 | 395 | library › Chapters tab — chapter list | TW |
| `library/ProductLibrary.vue` | 96 | 404 | library › Products tab — rendered-product grid | TW |
| `library/ProductCard.vue` | 134 | 366 | library › Products tab — one product card (Watch/Restore) | TW |
| **menus/** | | | | |
| `menus/ContextMenu.vue` | 190 | 310 | timeline right-click / Shift+F10 context menu (registry-driven) | TW |
| `menus/SaveProjectMenu.vue` | 89 | 411 | header — Save project ▾ dropdown | TW |
| **dialogs/** | | | | |
| `dialogs/CloseGuardDialog.vue` | 135 | 365 | dialog — window-close guard (Save/Keep/Discard) | TW, uses DialogHost |
| `dialogs/DiscardProjectDialog.vue` | 122 | 378 | dialog — Discard project confirm | TW, uses DialogHost |
| `dialogs/RecoveryDialog.vue` | 92 | 408 | dialog — Resume/Discard unsaved-journal offer | TW, uses DialogHost |
| `dialogs/SaveProjectDialog.vue` | 177 | 323 | dialog — Save as portable/lightweight project file | TW, uses DialogHost |
| `dialogs/RenderDialog.vue` | 251 | 249 | dialog — start a render + settings + outcome | TW, uses DialogHost, RenderProgress |
| `dialogs/RenderSettingsForm.vue` | 155 | 345 | dialog(Render) — name/range/quality form | TW |
| `dialogs/RenderOutcome.vue` | 93 | 407 | dialog(Render) — success/failure outcome + ProductPlayer | TW |
| `dialogs/ReviewDialog.vue` | 142 | 358 | dialog — quick Review render + playback | TW, uses DialogHost |
| `dialogs/PublishDialog.vue` | 270 | 230 | dialog — publish a product into a vault | TW, uses DialogHost |
| `dialogs/PublishForm.vue` | 40 | 460 | dialog(Publish) — vault/folder/options form | TW |
| `dialogs/DestinationFields.vue` | 61 | 439 | dialog(Publish) — folder/dated/note fields | TW |
| `dialogs/ReconnectDialog.vue` | 142 | 358 | dialog — relink missing media | TW, uses DialogHost |
| `dialogs/ReconnectRow.vue` | 62 | 438 | dialog(Reconnect) — one missing-asset row | TW |
| `dialogs/ChecksDialog.vue` | 128 | 372 | dialog — before-you-share findings | TW, uses DialogHost |
| `dialogs/ChecksDestination.vue` | 95 | 405 | dialog(Checks) — publish-destination summary | TW |
| `dialogs/ChecksFindingList.vue` | 76 | 424 | dialog(Checks) — grouped finding list | TW |
| `dialogs/ChecksFindingRow.vue` | 35 | 465 | dialog(Checks) — one finding row | TW |
| `dialogs/WebcamDialog.vue` | 232 | 268 | dialog — webcam take recorder shell | TW, uses DialogHost |
| `dialogs/WebcamLive.vue` | 96 | 404 | dialog(Webcam) — live preview + record controls | TW |
| `dialogs/WebcamControls.vue` | 59 | 441 | dialog(Webcam) — record/stop controls | TW |
| `dialogs/WebcamReview.vue` | 63 | 437 | dialog(Webcam) — review a take before Add | TW |
| `dialogs/WebcamCloseConfirm.vue` | 60 | 440 | dialog(Webcam) — close-with-unsaved-take confirm | TW |
| **guide/** | | | | |
| `guide/GuideInvitation.vue` | 72 | 428 | fixed overlay — "Show me around?" invitation card | TW |
| `guide/GuideCoach.vue` | 284 | 216 | fixed overlay — highlight ring + coach card positioning | TW + scoped (ring) |
| `guide/GuideCoachCard.vue` | 162 | 338 | guide overlay — the coach's text/Back/Next/Pause/Close card | TW |
| `guide/GuideStartOver.vue` | 47 | 453 | guide — "Start over" confirm | TW |
| `guide/LearningCenter.vue` | 181 | 319 | dialog — Help ▸ Learning center (tabs: Walkthrough/Answers/Shortcuts/Preferences) | TW, uses DialogHost |
| `guide/LearningWalkthrough.vue` | 80 | 420 | learning center tab — lesson/chapter list, jump-to | TW |
| `guide/LearningAnswers.vue` | 61 | 439 | learning center tab — quick answers | TW |
| `guide/LearningShortcuts.vue` | 42 | 458 | learning center tab — keyboard shortcut table | TW |
| `guide/LearningPreferences.vue` | 137 | 363 | learning center tab — dimming/motion prefs, export/import/restart | TW |

Total: **91** `.vue` files under `src/components/editor/**` (`find
src/components/editor -type f -name "*.vue" | wc -l`) + `EditorRoot.vue` = 92
rows in the table above (verified: `awk` range-count of table rows = 92).

---

## 2. Layout skeleton (`EditorShell.vue`)

- Root: `<div data-testid="editor-shell" class="relative flex grow flex-col
  gap-2 text-fg">` — `src/components/editor/shell/EditorShell.vue:280-284`.
- Children in order: `EditorHeader` (`:285`), a `grid grow gap-2` row
  (`:297-345`), the timeline `<section>` (`:352-359`), `NotificationHost` /
  `GuideInvitation` / `GuideCoach` (`:361-363`).
- The middle row's grid: `class="grid grow gap-2" :class="isCompact ?
  'grid-cols-1' : 'editor-shell-grid'"` (`:297-300`) where
  `.editor-shell-grid { grid-template-columns: var(--editor-sidebar) 1fr
  var(--editor-inspector); }` (`:367-371`, scoped). Tokens:
  `--editor-sidebar: 244px`, `--editor-inspector: 276px` (`src/style.css:100,
  101`) — plain CSS custom properties, not Tailwind-generated classes
  (`src/style.css:96-99` comment).
- Breakpoint: `COMPACT_BREAKPOINT = 1180` (`EditorShell.vue:106`), tracked
  from `window.innerWidth` via a `resize` listener into a plain `ref`
  (`:108-113`), not a CSS media query — because Vitest/happy-dom has no
  layout engine (`:26-31` doc comment). Below 1180px the grid collapses to
  `grid-cols-1` and library/inspector become toggled `<aside v-show>`
  drawers (`libraryOpen`/`inspectorOpen` refs, `:136-142`) instead of
  always-shown columns; above it both columns always show
  (`showLibrary`/`showInspector` computed, `:141-142`).
- The header is a **sibling** of the collapsible row, not inside it
  (`:22-24` doc, confirmed by template structure `:285` vs `:297`) — so no
  drawer state can cover/unmount it ("the route back").
- Timeline: a separate `<section data-testid="editor-shell-timeline">`
  BELOW the grid row, sized by `workspace.timelineHeight` via inline
  `:style="{ height: ... }"` on `TimelineView.vue`'s own root
  (`timeline/TimelineView.vue:333`), not by the shell. No CSS Grid
  row-sizing constant is used for it — `--editor-timeline: 400px` exists in
  `style.css:102` but nothing in the shipped code reads it (grep: only
  `TRACK_LABEL_WIDTH_PX`/`--editor-label` is actually consumed, see below).
- **Splitter**: yes — one draggable horizontal resize handle,
  `data-testid="timeline-resize-handle"` (`TimelineView.vue:341-348`),
  `role="separator"`, a `pointerdown` → `pointermove`/`pointerup` drag that
  calls `workspace.setTimelineHeight(...)` (`TimelineView.vue:169-183`).
  There is **no** vertical splitter between library/preview/inspector
  columns — those three widths are fixed CSS custom properties
  (`--editor-sidebar`/`--editor-inspector`, preview is the remaining `1fr`),
  not user-resizable.
- Preview/timeline split: not proportional — preview `grow`s to fill
  whatever height remains after a FIXED-height timeline section
  (`workspace.timelineHeight`, resizable only via the one handle above);
  the shell's own "Height" doc explains the intent (`EditorShell.vue:52-58`).
- Sizing floor/ceiling: window `minWidth`/`minHeight` 960×640 is set at the
  Tauri window level (AGENTS.md "window system" section, `editor` bullet),
  not in this component; `tests/e2e/editorShell.spec.ts:88-92` pins four
  sizes (960×640, 1280×820, 1600×1000, 1920×1080) at which Save/Render must
  stay `toBeInViewport()` and vertical overflow on `main` must be `<=1px`,
  and a separate test (`:48-64`) pins **zero** horizontal overflow at
  960×640.

---

## 3. Icon system

**There is no icon component system anywhere in the editor** (no `<svg>`
icon library, no icon-font, no `lucide`/`heroicons`-style import). Every
"icon" is either a plain-text glyph/emoji character inline in a template, a
Unicode HTML entity, or a one/two-letter abbreviation. `src/components/ui/
IconButton.vue` is only a styling wrapper (hover/focus/disabled treatment,
sizes `sm`/`md`) around whatever is placed in its default slot — it renders
no icon itself (`src/components/ui/IconButton.vue:1-3` comment).

Full inventory of non-ASCII/glyph "icons" found in editor templates
(exhaustive scan of every `<template>` block under `src/components/editor/`
+ `EditorRoot.vue`):

| Glyph / text | Meaning | File:line |
|---|---|---|
| `📁` | Library drawer toggle (compact header) | `shell/EditorHeader.vue:158` |
| `⚙️` | Inspector drawer toggle (compact header) | `shell/EditorHeader.vue:239` |
| `🌙` / `☀️` | Theme toggle (dark/light) | `shell/EditorHeader.vue:205` |
| `&#128065;` (👁) | Track visibility (eye) | `timeline/TrackHeader.vue` template, `track-header-*-visible` button |
| `&#128274;` (🔒) | Track lock | `timeline/TrackHeader.vue` template, `track-header-*-lock` button |
| `&#8942;` (⋮) | Track menu (Move up/down/Delete) | `timeline/TrackHeader.vue` template, `track-header-*-menu` button |
| `M` / `S` (plain letters) | Track mute / solo | `timeline/TrackHeader.vue` template |
| `▶` / `❚❚` | Transport play/pause | `preview/TransportBar.vue:97` |
| `✕` | Dismiss/close (guide invitation, caption row delete, chapter delete) | `guide/GuideInvitation.vue:11`, `library/CaptionCueRow.vue:98`, `library/ChaptersLibrary.vue:107` |
| `✓` | Lesson "read" checkmark | `guide/LearningWalkthrough.vue:74` |
| `▾` | Save-project menu caret | `menus/SaveProjectMenu.vue` |
| `›` | Checks finding row disclosure | `dialogs/ChecksFindingRow.vue` |
| `→` | Help button inline arrow | `shell/GuideHelpButton.vue` |
| `←` | Inspector "back to clip settings" from an effect | `inspector/InspectorPanel.vue` |
| `·` `×` `°` `–` `—` `…` `""` | Typographic punctuation (separators, multiplication in "1920×1080", degree in rotation fields, en/em dash, ellipsis, quotes) | scattered — `guide/GuideCoach.vue`, `guide/LearningCenter.vue`, `inspector/LayoutSection.vue`, `inspector/ColorSection.vue`, `inspector/SpeedSection.vue`, several dialogs |

Toolbar and menu "buttons" that might look icon-driven are in fact **plain
text labels** read from the action registry (`resolved[id].label`,
`shell/PreviewToolbar.vue:415`) — e.g. the teaching-tool buttons render the
words "Text", "Arrow", "Highlight", etc., not glyphs (verify:
`src/editor/actionMeta.ts` labels feed `PreviewToolbar.vue:372,415` and
`menus/ContextMenu.vue`). Likewise `shell/ChecksButton.vue` and
`shell/GuideHelpButton.vue` render the words "Checks"/"Help", not icons.
One real inline `<svg>` exists per clip for **waveforms**
(`timeline/ClipWaveform.vue`, a `<polyline>`, not an icon) and one real
`<svg>` renders **teaching-cue shapes** on the preview stage
(`preview/CueOverlay.vue:61-99`, arrows/highlights/text/spotlight/zoom/mask
shapes built by `src/editor/cueShapes.ts`) — these are content, not UI
chrome icons.

**Implication for a restyle plan**: introducing a real icon system (SVG
component set) would touch essentially every leaf component in `shell/`,
`timeline/`, `library/`, `guide/`, and `dialogs/` — there is no existing
`Icon.vue` or icon-name prop convention to extend; one would have to be
designed from scratch and then substituted glyph-by-glyph.

---

## 4. Theme mechanism

- Tokens live entirely in `src/style.css` (248 lines total, the whole
  file — see the earlier read). The `@theme` block (`:8-102`) defines the
  DARK/default values for the whole app's semantic ladder
  (`--color-fg*`, `--color-accent*`, `--color-focus`, status colors,
  `--radius-control`, `--text-micro`) plus editor-only tokens introduced at
  `:29-102`: `--color-stage/-panel/-raised/-line`, `--color-video/-video-bg/
  -audio/-audio-bg/-gold/-gold-bg`, `--color-app`, `--color-hover`,
  `--color-track`, `--color-hover-subtle`, and four non-color CSS vars
  `--editor-sidebar/-inspector/-timeline/-label`.
- `[data-theme="light"]` (`style.css:117-159`) overrides the fg ladder,
  accent/danger/focus, `--color-app`, all editor surface/media tokens, and
  `--color-hover`/`--color-track`/`--color-hover-subtle` with light-mode
  values, each with a measured-contrast comment (e.g. 3.99:1/3.85:1/3.76:1/
  3.68:1/3.78:1 boundary contrasts, `:139-149`).
- `[data-theme="dark"]` (`style.css:170-176`) overrides only two tokens
  from the `@theme` defaults inside the editor window specifically:
  `--color-fg-subtle` and `--color-video` (raised for contrast, GAP-209).
  No other window ever sets `data-theme`, so this block is inert elsewhere.
- **Default theme selection**: `editorWorkspace.ts` seeds
  `theme: ref<Theme>(prefersLightTheme() ? "light" : "dark")`
  (`src/stores/editorWorkspace.ts:114`), where `prefersLightTheme()`
  (`:61-67`) reads `window.matchMedia("(prefers-color-scheme: light)")`.
  This seed is overwritten by the persisted `workspace.json` value on the
  next `hydrate()` call if one exists (Task 18's F16 field) — i.e. OS
  preference is only the very-first-run fallback; after that the toggle
  persists per-project via `editor_save_workspace`.
- The actual DOM application is `EditorShell.vue`'s own job: `watch(() =>
  workspace.theme, (t) => { document.documentElement.dataset.theme = t; },
  { immediate: true })` (`EditorShell.vue:163-170`) — sets
  `<html data-theme="...">` inside the editor webview only.
  `toggleTheme()` (`:171-173`) calls `workspace.toggleTheme()`
  (`src/stores/editorWorkspace.ts:388`).
- Forced-colors (Windows contrast themes): `@media (forced-colors: active)`
  in `style.css:187-207`, unlayered so it beats Tailwind's
  `focus:outline-none`. It restyles `:focus-visible` (2px `Highlight`
  outline), `[role="option"][aria-selected="true"]` (the selected clip,
  since box-shadow/ring disappears under forced colors), `.vb-playhead`
  (`background-color: Highlight`) and `.vb-handle` (`background-color:
  ButtonText`) — these two classes are applied on the timeline playhead
  (`timeline/TimelineView.vue:368`, class `vb-playhead`) and on every fade/
  trim handle (`timeline/ClipItem.vue`, class `vb-handle` on all four
  handle spans) specifically so they survive `forced-colors` (box-shadows
  are dropped by the browser under that mode, per the comment at
  `style.css:180-186`).
- Contrast is machine-checked, not just documented:
  `tests/e2e/editorKeyboard.spec.ts` measures per-surface text contrast
  (`:431-459`, "theme text meets 4.5:1 on every surface", both themes) and
  boundary contrast for trim handles against 1.4.11 (`:370-394`,
  `boundaryContrast`/`surfaceContrast` helpers `:315-369`).

---

## 5. Timeline: scroll mechanism, cue rendering, clip anatomy

### 5a. Track header / lane scrolling — the suspected bug, explained

**The header (label) column is inside the same horizontally-scrolling
element as the clip body — it is NOT `position: sticky` and NOT a separate
scroll region.**

- One scroll container for the whole timeline:
  `<div ref="scrollRef" data-testid="timeline-scroll" class="relative
  min-h-0 flex-1 overflow-auto" @scroll="onScroll">`
  (`timeline/TimelineView.vue:356-361`), holding the ruler, the playhead,
  every `TrackLane`, and the below-lanes drop target.
- Each `TrackLane` renders a flex row `class="flex border-b border-line"`
  (`timeline/TrackLane.vue:134-141`) whose FIRST child is the label column
  `<div data-testid="track-lane-header-{id}" class="flex shrink-0 ...">`
  fixed at `TRACK_LABEL_WIDTH_PX` = 196px (`TrackLane.vue:142-146`,
  constant defined `src/editor/timelineLayout.ts:27-30`, mirroring
  `--editor-label` in `style.css:103`), and the SECOND child is the clip
  body `<div data-testid="track-lane-body-{id}" ... :style="{ width:
  widthPx }">` (`TrackLane.vue:158-164`). Both are plain flex children with
  no `position: sticky`/`left` offset anywhere in the file (`grep sticky` on
  the whole editor tree returns zero matches).
- `TrackLane.vue`'s own doc block (`:9-15`) says this explicitly: *"The
  label column deliberately scrolls WITH the lane body rather than staying
  pinned via `position: sticky` — a real product would pin it, but ...
  sticky layout is untestable in this Vitest environment anyway (happy-dom
  has no layout engine) and the CSS interaction with a per-row flex width
  was real added risk for a property nothing here can verify. A follow-up
  gap, not a hidden shortcut."* So this is a **documented, deliberate
  limitation**, not an accidental regression — but it does mean scrolling
  the timeline horizontally at all moves the track names/eye/lock/mute/
  solo/volume/menu controls off-screen along with the ruler and clips.
- `TimelineView.vue` itself has to subtract the label width back out
  whenever it converts the raw `scrollLeft` into content-space math, e.g.
  `bodyScrollLeft = Math.max(0, scrollLeftPx.value -
  TRACK_LABEL_WIDTH_PX)` (`:138`) and its own comment at `:132-137`
  spelling out that `scrollLeftPx` is "measured against the whole scrolled
  row INCLUDING the label column."

**The scroll-into-view code that can trigger it:**

1. **Keyboard nudge** (`ArrowLeft`/`ArrowRight` on a selected clip):
   `ClipItem.vue`'s `onNudge` (`:304-318`) calls `drag.nudge(deltaMs)`, then
   after `nextTick()` calls `restored?.focus()` (`:317`) on the clip
   element (re-queried by `data-testid` in case Vue reused a different DOM
   node). A native `.focus()` call on an element outside the current
   scrollport makes the browser auto-scroll the nearest scrollable ancestor
   (here, `timeline-scroll`) just enough to bring the focused element into
   view — and because the label column is a sibling INSIDE that same
   scrolled container rather than pinned, that auto-scroll can carry the
   label column off the left edge exactly as it reveals a clip further
   right. This is the most likely mechanism behind "selecting a clip
   scrolls the header column out of view" — it is a side effect of the
   native focus-follows-keyboard behavior, not of `onSelect` itself (a
   plain click, `ClipItem.vue:108-120`, does not call `.focus()` or
   `scrollIntoView` and does not fire this path).
2. **`revealScrollLeft`** (`src/editor/timelineLayout.ts:196-199`,
   consumed by `TimelineView.vue`'s `onReveal("timeline", ...)` handler at
   `:106-115`): this is a PROGRAMMATIC scroll used only by the
   before-you-share Checks dialog's "Show it" action
   (`src/editor/revealBus.ts:65-69`, `requestTimelineReveal`) — it is not
   wired to clip *selection* at all. It correctly reserves
   `TRACK_LABEL_WIDTH_PX` when deciding whether to scroll
   (`revealScrollLeft`'s check `x >= scrollLeft + TRACK_LABEL_WIDTH_PX`),
   but once it does scroll, the label column still moves with the content
   for the same structural reason as above — it is just less likely to
   scroll the header far off-screen since it only scrolls the minimum
   needed to bring the target ms into the body's visible span.
3. Manual mouse-wheel/scrollbar dragging of `timeline-scroll` obviously
   also moves the header, by design (`TrackLane.vue:9-15`).

Any restyle that wants to fix this would need to either (a) make the label
column `position: sticky; left: 0` within `timeline-scroll` (the "real
product" approach the doc already anticipates, `TrackLane.vue:10`), or
(b) split the timeline into two independently-scrolled regions (a fixed
label column + a horizontally-scrolling body, synced on `scrollTop` only) —
either is a structural change to `TimelineView.vue`/`TrackLane.vue`, not a
pure color/spacing restyle, and the doc comment already names both the risk
("CSS interaction with a per-row flex width") and the reason it was never
tried (`happy-dom` cannot verify `position: sticky` in Vitest — only
`tests/e2e/*.spec.ts` measures real layout).

### 5b. Teaching Cues (project `effects`) — where they render

Cues are **not** rendered on the timeline at all (`grep` for
`effect`/`Effect`/`cue`/`Cue` across `timeline/*.vue` returns only
unrelated matches — `dropEffect`, `effectiveViewportWidth`, `vector-effect`,
comment prose — confirmed above). There is no cue lane, no cue marker on
the ruler, and no "Effects" tab in the library (`library/` has
Media/Titles/Captions/Chapters/Products only, `library/LibraryPanel.vue`).

Cues render in exactly two places:
1. **Preview stage** — `preview/CueOverlay.vue` (`:60-99`): one `<svg>`
   overlay sized to the letterboxed canvas box (`viewBox` = project canvas
   dimensions), drawing every cue whose output span holds the current
   playhead (`activeCues`, `src/editor/cueGeometry.ts`) via shape
   primitives from `src/editor/cueShapes.ts` (arrows/text/highlight/
   spotlight/zoom/mask). It is `pointer-events-none` and purely visual — "
   what is drawn here is what the render burns in" (`CueOverlay.vue:8-9`).
   A sibling, `preview/CueHandles.vue`, draws the interactive selection
   outline/grab handles/zoom focal marker on top, outside the stage
   proper (`PreviewSurface.vue:43-53`).
2. **Inspector** — `inspector/EffectSection.vue` fills `InspectorPanel`'s
   `#effect` slot, which REPLACES the six category tabs while a cue is
   selected (`InspectorPanel.vue:34-37`; wired in `EditorRoot.vue:377-384`
   `<template #effect="{ effectId }"><EffectSection :key="effectId"
   :effect-id="effectId" /></template>`). Selecting a cue happens either by
   adding one from the preview toolbar (`PreviewToolbar.vue:292-302`
   `addCue`, which calls `editorWorkspace.setSelected({type:"effect",
   id})`) or by interacting with `CueHandles.vue` directly on the stage.

### 5c. Clip anatomy (`ClipItem.vue`)

Full template breakdown (`timeline/ClipItem.vue:360-441`):
- Root `<div role="option" tabindex="0">`, absolutely positioned
  (`left`/`width` from `previewLeftPx`/`previewWidthPx`), colored by asset
  kind — `bg-video-bg border-video text-video` or `bg-audio-bg border-audio
  text-audio` (`:369-372`), plus `ring-2 ring-accent` when selected.
- **Fade wedges**: two `v-if` divs (`clip-{id}-fade-in`/`-fade-out`),
  `bg-gold/40` triangles via `clip-path: polygon(...)`, only rendered when
  the fade duration is nonzero (`:380-391`).
- **Fade handles**: two always-rendered `<span class="vb-handle ...
  bg-gold rounded-full">` circles at the top corners
  (`clip-{id}-fade-in-handle`/`-fade-out-handle`, `:392-405`) — always
  present regardless of fade amount, so a fade can be dragged into
  existence from zero.
- **Waveform or thumbnail** (mutually exclusive, `v-if`/`v-else-if`,
  `:408-424`): `ClipWaveform.vue` for a clip with audio (an inset bar,
  `bg-audio-bg/60`, containing an SVG polyline drawn by
  `timeline/ClipWaveform.vue`), else `ClipThumbnail.vue` for a video clip
  wide enough to show a poster frame (`THUMBNAIL_MIN_WIDTH_PX` gate).
- **Name label**: a plain truncated `<span>` (`:426`).
- **Trim handles**: two 4px-wide edge `<span class="vb-handle ...
  bg-track">` strips at left/right (`clip-{id}-trim-start`/`-trim-end`,
  `:428-441`).
- Keyboard: Enter/Space select, ArrowLeft/Right nudge (33ms/frame,
  1000ms with Shift) then refocus (`:301-318`, see §5a above), Escape
  cancels an in-progress drag/trim/fade preview and swallows the keystroke
  so it does not also dismiss an open guide coach (`:345-353`), Shift+F10
  or the app's context-menu-key opens the context menu (`:333-338`).

---

## 6. Test coupling

### Structural / must-not-break invariants

- **`tests/editorThemeTokens.test.ts:23-32`** — a source-scan test that
  fails if ANY `.vue` file under `src/components/editor/**` contains the
  literal regex `\bwhite/\d+` (i.e. `bg-white/10`, `border-white/5`, etc.).
  **A restyle must never reintroduce a raw white-opacity Tailwind utility
  anywhere in the editor tree** — use the semantic tokens
  (`bg-hover`/`bg-hover-subtle`/`bg-track`) instead.
- **`tests/e2e/editorShell.spec.ts`** — real-browser pixel measurements
  against the built `dist/`:
  - `:41-46` exactly one `data-testid="preview-toolbar"` node at 960×640.
  - `:48-64` zero horizontal overflow (`main.scrollWidth -
    main.clientWidth <= 0`) at 960×640.
  - `:66-81` below 1180px, `editor-header-library-toggle` is a real
    `aria-expanded` drawer control and `editor-shell-library` is
    hidden/shown accordingly, and `editor-header` stays
    `toBeInViewport()` after opening the drawer (the "route back"
    contract, `EditorShell.vue:22-24`).
  - `:84-107` at each of 960×640 / 1280×820 / 1600×1000 / 1920×1080:
    `editor-header-save` and `editor-header-render` must both
    `toBeInViewport()`, and vertical overflow on `main` must be `<=1px`.
- **`tests/e2e/editorKeyboard.spec.ts`** — real-browser assertions:
  a full keyboard-only journey (`:145-203`) tabbing through every control
  by testid; forced-colors selection visibility (`:204-236`); light-theme
  trim-handle 3:1 boundary contrast (`:370-394`); per-theme subtle-row-tint
  discernibility (`:395-430`); per-theme 4.5:1 text contrast on every
  surface (`:431-459`); reduced-motion freezes the guide ring's transition
  (`:461-470`); and a build-hygiene check that the shipped `dist/` carries
  no editor test hook (`:479-484`).
- **`tests/editorEvidence.test.ts`** (229 lines) cross-checks
  `docs/superpowers/specs/2026-09-21-tutorial-editor-acceptance-evidence.md`
  against the real test suite: every `path#test name` the evidence doc
  cites for an F-ID must actually exist, or CI reddens. This doesn't pin
  DOM/CSS directly, but it means **deleting or renaming a test file/test
  name that the evidence doc cites breaks CI** even if the underlying
  behavior still works — a restyle that also refactors test files must
  update that doc in the same change (per AGENTS.md's own documentation
  map entry for that file).
- **Guide target registry** (`src/editor/guide/targets.ts`,
  `src/composables/useGuideTarget.ts`) — 22 `GUIDE_TARGET_KEYS`
  (`targets.ts:27-51`), each registered by whichever component owns that
  control via a template `:ref="someTarget"` binding (e.g.
  `EditorHeader.vue:146` `:ref="projectbarTarget"` for key `"projectbar"`,
  `:208` `:ref="saveTarget"` for `"header.save"`; `TrackHeader.vue`'s menu
  button for `"track.menu"`; `PreviewToolbar.vue:176-185` for
  `"preview.toolstrip"` with an overflow fallback). Resolution is by
  **function reference to a live DOM element**, not by CSS selector or
  class name (`targets.ts:1-23` doc) — so a restyle is free to change
  classes/markup around a registered element, but must keep **something**
  registering each of the 22 keys (or the guide silently "explains rather
  than highlights" for that lesson, `targets.ts:135-136`), and must not
  remove the element the `:ref` binds to. `tests/editorGuideCoach.test.ts`
  and `tests/editorGuideContent.test.ts` exercise this.
- **`tests/editorCheckWireSpellings.test.ts`** / `tests/editorWireEnums.test.ts`
  / `tests/editorCommandWire.test.ts` — pin TS↔Rust wire-type spellings,
  unrelated to visual structure but easy to accidentally touch if inspector
  field names change.

### Class-assertion coupling found (`grep` across all `tests/editor*.test.ts`)

Only 5 of ~55 editor test files assert on Tailwind classes directly:
- `tests/editorMediaLibrary.test.ts:183-184` and
  `tests/editorRenderDialog.test.ts:117-118` — an import/render progress
  bar must carry class `bg-track` and must NOT carry `bg-white/10`
  (mirrors the theme-token test above, at the unit-test level for these
  two specific bars).
- `tests/editorTimelineView.test.ts:262-263` — clip elements must carry
  `bg-video-bg` / `bg-audio-bg` depending on asset kind (the color-coding
  contract).
- `tests/editorPlacement.test.ts:240` and
  `tests/editorTrackHeader.test.ts:442,465` — a locked track's clip body
  must carry `pointer-events-none` (functional, not cosmetic).

No other editor Vitest file asserts on arbitrary Tailwind spacing/color
classes — most assert on **text content**, **`data-testid` presence**,
**`aria-*` attributes**, or **emitted events/store calls**. This means a
restyle that changes Tailwind classes freely (spacing, non-semantic colors,
radii, etc.) is largely safe against the Vitest suite as long as: (1) every
`data-testid` referenced by a test stays present and attached to a
functionally-equivalent element, (2) the 5 specific class assertions above
keep holding, (3) the `no white/N literal` scan stays clean, and (4) the
e2e pixel/contrast assertions in section above keep passing.

---

## 7. Header status text and vault id

- **Title**: `editorProject.snapshot?.title ?? "Untitled"`
  (`shell/EditorHeader.vue:64`), rendered in a button
  (`data-testid="editor-shell-title"`, `:172-181`) that opens an inline
  rename `<input>` on click.
- **Duration**: `formatDuration(editorProject.durationMs)`
  (`EditorHeader.vue:65`, `data-testid="editor-shell-duration"`, `:183-186`).
- **Status text** ("Saved" / "Unsaved changes" / "Saving…" / "Save
  failed"): `EditorHeader.vue:92-96`
  ```
  const status = computed<string>(() => {
    if (editorProject.saving) return "Saving…";
    if (editorProject.saveError) return "Save failed";
    return editorProject.dirty ? "Unsaved changes" : "Saved";
  });
  ```
  rendered at `data-testid="editor-header-status"` (`:187-190`). Derived
  fresh every render from `editorProject`'s own reactive fields
  (`saving`/`saveError`/`dirty`) — never a `setTimeout`, per the doc
  comment `:83-91`. `saveError` is the store's SAVE-only error field,
  deliberately distinct from the shared `lastError` (which also covers a
  refused edit or a refused open) so a failed OPEN is never mis-reported
  as "Save failed" (`:88-91`).
- **Vault id** (currently the raw internal id, e.g. `vault-e2e`):
  `const vault = computed(() => editorProject.project?.destination.vault ??
  null);` (`EditorHeader.vue:66`), rendered verbatim at
  `data-testid="editor-shell-vault"` when non-null (`:191-195`). There is
  **no lookup to a human-readable vault name anywhere in this component or
  its store** — `editorProject.project.destination.vault` is the raw
  vault id string as it comes off the wire from Rust's
  `Project.destination.vault` field (see `editorTypes.ts` for the DTO
  shape) — the same id `list_vaults` returns, never resolved against the
  vault registry's display name (`core::discovery::Vault.name`). This is
  the literal source of the "shows the raw id like `vault-e2e`" behavior
  the prompt describes; fixing it would mean either (a) the store fetching
  `list_vaults` and mapping id→name, or (b) Rust's `editor_open_staged`/
  `editor_open_project` reply carrying a display name alongside the id.
  Neither exists today.

---

## 8. Inspector tabs — what each shows and drives

All six live behind `InspectorPanel.vue`'s `role="tablist"`
(`:11-38` doc), tab selection persisted in `editorWorkspace.propertyTab`
(a view-preference field, `InspectorPanel.vue:69-70`), each rendering a
named slot scoped with `clipIds` — only the active tab's panel is mounted
("don't pay for a hidden tab", `:30-32`). All slots are wired in
`EditorRoot.vue:335-386`. Every mutation in every tab goes through
`editorProject.execute(command)` (Rust is the sole authority, ADR
invariant 1) — nothing here holds local edit state beyond an
uncommitted numeric-field draft (`useInspectorDraft`).

| Tab | Component | Shows | Commands / store reads |
|---|---|---|---|
| **Clip** | `ClipSection.vue` | Name, Start, In, Out numeric fields (the drag alternative); cross-fade transitions list (`ClipTransitions.vue` → `TransitionRow.vue` per transition) | `rename`/`trimClip`/`moveClips`-shaped commands via `editorProject.execute`; transitions via their own add/remove commands (doc `ClipSection.vue:1-10`) |
| **Layout** | `LayoutSection.vue` | Position/size in % of canvas, 4 corner presets, frame shape, fit, crop zoom/focus, rotation, mirror, vertical flip, opacity | `setLayout` over the whole `clipIds` selection (`LayoutSection.vue:1-9`) — one atomic command even for multi-select |
| **Fades** | `FadesSection.vue` | Numeric fade-in/out duration + curve, warns about track-shortening | fade-set command(s) via `execute` (`FadesSection.vue:1-9`) |
| **Audio** | `AudioSection.vue` + `AudioVolumeField.vue` + `AudioDetachControl.vue` + `AudioStemsNote.vue` | Clip volume (stored linear, displayed in dB), mute, Detach audio, a stems informational note | `setLayout`/volume + mute commands, `detachAudio` command (`AudioSection.vue:1-10`) |
| **Speed** | `SpeedSection.vue` | Presets 0.5×/1×/1.5×/2×, numeric speed, preserve-pitch toggle, a derived "resulting duration" line | `setSpeed` (Rust computes output duration, refuses overlap into next clip) |
| **Color** | `ColorSection.vue` | 6 presets + 5 sliders (brightness/contrast/saturation/sepia/grayscale) | `setAdjustments` over the whole selection, atomic |
| *(override)* **Effect** | `EffectSection.vue` | Replaces the 6 tabs while a teaching cue is selected; every property of the cue's kind is editable | cue update command via `editorProject.execute`, fields resolved by `src/editor/effectFields.ts` |

Empty-selection state: the six-tab shell shows teaching copy instructing
the user to select a clip rather than six panels of disabled inputs
(`InspectorPanel.vue:18-20`); a multi-clip selection shows an explicit
"`N` clips selected" statement before the active tab's slot (`:23-25`).

---

## Summary of biggest structural facts for a restyle plan

1. **91 editor `.vue` files (+ EditorRoot.vue = 92), all under the 500-LOC cap** (max 424,
   `ClipItem.vue`/`EditorRoot.vue`) — no LOC-cap pressure blocks a restyle,
   but touching many files means the `bg-white/N` scan
   (`tests/editorThemeTokens.test.ts`) and the LOC guard must both stay
   green after edits.
2. **Only 2 of 92 files use scoped `<style>`** — everything else is inline
   Tailwind utility classes; there is no CSS-module or BEM layer to work
   around, but also no per-component stylesheet to edit centrally.
3. **No icon system exists at all** — every icon is an emoji, HTML entity,
   or 1-2 letter abbreviation inline in a template (full table in §3).
   Introducing SVG icons is a from-scratch design + a glyph-by-glyph swap
   across `shell/`, `timeline/`, `library/`, `guide/`, `dialogs/`.
4. **Shared `ui/` primitives are barely used inside the editor** — 27
   files use `AppButton`, almost nothing else; most controls (track
   header buttons, toolbar buttons, clip handles) are raw `<button>`/
   `<span>` with hand-written Tailwind, so a "restyle via primitives"
   strategy would need new work, not just prop changes.
5. **Theming is token-driven and dual-checked**: `@theme` defaults (dark)
   + `[data-theme="light"]` + `[data-theme="dark"]` overrides in
   `src/style.css`, selected by `editorWorkspace.theme` (persisted,
   OS-preference seed only on first run), applied via a `watch` setting
   `document.documentElement.dataset.theme` in `EditorShell.vue`. A
   structural test forbids raw `white/N` opacity utilities anywhere in the
   editor tree, and real-browser contrast tests (4.5:1 text, 3:1 UI
   boundaries) run in BOTH themes — any new token or color choice needs to
   pass those, not just look right.
6. **Layout is a fixed 3-column CSS Grid** (`--editor-sidebar: 244px`,
   `1fr` preview, `--editor-inspector: 276px`) collapsing to `grid-cols-1`
   with toggle-drawers below 1180px, plus one independently-resizable
   timeline section at the bottom (one horizontal drag handle, no other
   splitters). Four e2e-pinned window sizes (960×640 floor through
   1920×1080) must keep Save/Render visible and near-zero scroll overflow
   — any restyle changing padding/heights must re-run
   `tests/e2e/editorShell.spec.ts`.
7. **The timeline's track-header column deliberately scrolls with the
   body** (documented non-goal, `TrackLane.vue:9-15`) rather than being
   `position: sticky`; a native `.focus()` call after keyboard nudge
   (`ClipItem.vue:317`) is the concrete mechanism most likely to visibly
   "scroll the header out of view" after selecting/nudging a clip, since
   the browser's default focus-scroll has no concept of the reserved label
   width. Fixing it is a structural change (sticky column or dual-scroll
   split), not a color/spacing tweak.
8. **Teaching Cues render in exactly two places** — the preview stage's
   `CueOverlay`/`CueHandles` SVG layers, and the inspector's `EffectSection`
   override — never on the timeline itself; there is no cue lane/marker/list
   to restyle there.
9. **The header's vault line shows the raw internal vault id verbatim**
   (`editorProject.project?.destination.vault`, `EditorHeader.vue:66`) with
   no id→name lookup anywhere in the chain — a cosmetic fix requires new
   data plumbing (a vault-name lookup), not just a template change.
10. Test coupling to visual structure is narrow and enumerable: one
    tree-wide regex scan (no `white/N`), one wire-format cross-check
    (`editorEvidence.test.ts` vs the acceptance-evidence doc), 22 guide
    target keys that need *a* registered element each (not a specific
    class/selector), 4-5 explicit Tailwind-class assertions, and a
    real-browser suite (`editorShell.spec.ts`, `editorKeyboard.spec.ts`)
    that measures pixels/contrast rather than markup — a restyle has real
    room to move as long as `data-testid`s, guide target refs, and those
    specific class/contrast contracts survive.
