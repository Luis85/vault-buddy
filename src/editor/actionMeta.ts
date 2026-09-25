/**
 * Static data behind the tutorial editor's action registry (Task 17, fix
 * round 1 — split out of `actions.ts` at that file's own 500-line cap, so
 * later tasks adding more actions have room). Every `ActionId`, its label/
 * reason text and the wire-kind lookup tables `resolveActions`/`commandFor`
 * (still in `actions.ts`) read live here. Nothing in this file is logic —
 * no function here takes an `ActionContext` — so it can be read as a plain
 * reference table.
 *
 * `actions.ts` imports everything it needs from here and re-exports the
 * public names (`ActionId`, `ACTION_IDS`, `SHORTCUT_DISPLAY`,
 * `UNIMPLEMENTED_KINDS`), so no external caller (`shortcuts.ts`, the two
 * components, both test files) has to change which module it imports from.
 */

/** Every action id a caller can resolve/execute (this task's own brief —
 * the exact 38-member list, in the order it lists them). */
export type ActionId =
  | "split"
  | "delete"
  | "deleteClose"
  | "undo"
  | "redo"
  | "copy"
  | "cut"
  | "paste"
  | "duplicate"
  | "group"
  | "ungroup"
  | "earlier"
  | "later"
  | "addText"
  | "addArrow"
  | "addHighlight"
  | "addSpotlight"
  | "addZoom"
  | "addStep"
  | "addMask"
  | "addCaption"
  | "addMarker"
  | "addTrackVideo"
  | "addTrackAudio"
  | "fadeIn"
  | "fadeOut"
  | "transition"
  | "detachAudio"
  | "save"
  | "render"
  | "checks"
  | "help"
  | "importMedia"
  | "webcam"
  | "toggleLibrary"
  | "toggleInspector"
  | "focusPreview"
  | "guideFocus"
  | "ratio";

/** Every `ActionId`, once, in the union's own declared order — the one
 * place `resolveActions` iterates from, and what `editorActions.test.ts`
 * checks every action id resolves against. */
export const ACTION_IDS: readonly ActionId[] = [
  "split", "delete", "deleteClose", "undo", "redo", "copy", "cut", "paste",
  "duplicate", "group", "ungroup", "earlier", "later",
  "addText", "addArrow", "addHighlight", "addSpotlight", "addZoom", "addStep", "addMask",
  "addCaption", "addMarker", "addTrackVideo", "addTrackAudio",
  "fadeIn", "fadeOut", "transition", "detachAudio",
  "save", "render", "checks", "help", "importMedia", "webcam",
  "toggleLibrary", "toggleInspector", "focusPreview", "guideFocus", "ratio",
];

// ---- human-text reasons (Behavior section's own literal values) -----------

export const NO_CLIP = "Select a clip first";
export const CLIP_BOUNDARY = "The playhead is at a clip boundary";
export const NO_PROJECT = "No project is open.";
/** Review (Task 47) renders part of the output; an empty timeline has none. */
export const NOTHING_TO_REVIEW = "Place a clip on the timeline to review a render.";

export function lockedReason(trackName: string): string {
  return `Track ${trackName} is locked`;
}

// ---- static labels/wire kinds ----------------------------------------------

export const ACTION_LABELS: Record<ActionId, string> = {
  split: "Split", delete: "Delete", deleteClose: "Delete (close gap)",
  undo: "Undo", redo: "Redo", copy: "Copy", cut: "Cut", paste: "Paste",
  duplicate: "Duplicate", group: "Group", ungroup: "Ungroup",
  earlier: "Move earlier", later: "Move later",
  addText: "Text", addArrow: "Arrow", addHighlight: "Highlight",
  addSpotlight: "Spotlight", addZoom: "Zoom", addStep: "Step", addMask: "Privacy cover",
  addCaption: "Add caption", addMarker: "Add marker",
  addTrackVideo: "Add video track", addTrackAudio: "Add audio track",
  fadeIn: "Fade in", fadeOut: "Fade out", transition: "Add transition",
  detachAudio: "Detach audio",
  save: "Save project", render: "Review", checks: "Checks", help: "Help",
  importMedia: "Import media", webcam: "Webcam",
  toggleLibrary: "Library", toggleInspector: "Inspector", focusPreview: "Focus preview",
  guideFocus: "Guide focus", ratio: "Aspect ratio",
};

/** The one `EditorCommand` wire `kind` each action maps to, when it maps to
 * exactly one — see `actions.ts`'s module doc for the actions deliberately
 * absent. DIAGNOSTIC data only: never shown to the user directly (see
 * `unavailableReason` below) — it is Rust's own error vocabulary
 * (`core::editor::commands::mod.rs`'s `unimplemented_commands()` table),
 * not a word a person reading a button labelled "Text" would recognize. */
export const ACTION_KIND: Partial<Record<ActionId, string>> = {
  split: "splitClip", delete: "deleteClips", deleteClose: "deleteClips",
  undo: "undo", redo: "redo", cut: "cutClips", paste: "pasteFragment",
  duplicate: "duplicateClips", group: "groupClips", ungroup: "ungroupClips",
  earlier: "reorderClip", later: "reorderClip",
  addText: "addEffect", addArrow: "addEffect", addHighlight: "addEffect",
  addSpotlight: "addEffect", addZoom: "addEffect", addStep: "addEffect", addMask: "addEffect",
  addCaption: "addCaption", addMarker: "addMarker",
  addTrackVideo: "addTrack", addTrackAudio: "addTrack",
  fadeIn: "setFades", fadeOut: "setFades", transition: "addTransition",
  detachAudio: "detachAudio",
  // `ratio` deliberately carries NO entry here (Task 32): it needs an
  // extra user choice (which of the four presets) this table cannot
  // pre-build, so it is resolved by its own `RESOLVERS` entry
  // (`resolveProjectGated`, `actions.ts`) and sends `setCanvas` directly
  // from `PreviewToolbar.vue`'s own ratio control -- never through
  // `commandFor`/`BUILDERS`. See that file's module doc.
};

/** The human-readable shortcut shown beside an action's label/tooltip —
 * `shortcuts.ts`'s `SHORTCUTS` map's DISPLAY twin (both `ctrl+shift+z` and
 * `ctrl+y` resolve to `redo`, but `redo` shows only one). `shortcuts.ts`
 * does NOT re-export this (fix round 1, finding 4c corrected a stale claim
 * that it did) — a caller imports it from here (via `actions.ts`'s
 * re-export) directly, the same as every other name in this file. */
export const SHORTCUT_DISPLAY: Partial<Record<ActionId, string>> = {
  split: "S", delete: "Delete", deleteClose: "Shift+Delete",
  undo: "Ctrl+Z", redo: "Ctrl+Shift+Z", copy: "Ctrl+C", cut: "Ctrl+X",
  paste: "Ctrl+V", duplicate: "Ctrl+D", group: "Ctrl+G", ungroup: "Ctrl+Shift+G",
  save: "Ctrl+S", render: "Ctrl+E", help: "F1", guideFocus: "F6",
};

/**
 * The exact sixteen wire kinds this registry still gates. `undo`/
 * `redo`/`splitClip`/`deleteClips`/`cutClips`/`pasteFragment`/
 * `duplicateClips`/`groupClips`/`ungroupClips`/`reorderClip` are
 * deliberately absent (those ten of the sixteen pre-Task-23 kinds are the
 * ones an action in `ACTION_KIND` maps to), and so are `renameTrack`/
 * `moveTrack`/`setTrackFlags`/`deleteTrack` as of Task 23 — Rust's
 * `commands::tracks` implements all five track kinds, but no `ActionId`
 * maps to any of those four (`TrackHeader.vue` calls
 * `editorProject.execute` directly, never through this registry), so
 * removing them changes nothing here and keeps `mod.rs`'s own invariant
 * ("delete the matching entry or every action that maps to that kind stays
 * wrongly disabled") satisfied for the part of it that applies. Task 27
 * removed `setClipMix`/`setMasterGain`/`detachAudio` the same way, WITH a
 * consumer each: `AudioSection`/`MixerPopover` call `editorProject.execute`
 * directly for the two mix kinds (no `ActionId` maps to either), and the
 * `detachAudio` action got its own `RESOLVERS`/`BUILDERS` entries.
 *
 * **`addTrack` was the one exception through Task 23** — kept gated even
 * though Rust already implemented it, because the two `ActionId`s that map
 * to it (`addTrackVideo`/`addTrackAudio`, `ACTION_KIND` above) had no
 * `RESOLVERS`/`BUILDERS` entry in `actions.ts`: nobody had built an "add a
 * new track" UI surface yet (Task 23's own brief scoped `TrackHeader` to an
 * EXISTING track's controls only), and ungating it without those two
 * entries would have rendered `addTrackVideo`/`addTrackAudio` as ENABLED
 * buttons `commandFor` still returned `null` for. **Task 26 is that
 * surface's first consumer** — dropping a media asset below the timeline's
 * last lane sends `addTrack` directly from `TimelineView.vue`
 * (`editorProject.execute`, the `TrackHeader.vue` direct-call precedent
 * above, never through this registry) — so the "no consuming UI yet"
 * condition no longer holds for `addTrack` ITSELF, and it is removed from
 * the set below in the same commit, per `mod.rs`'s own rule.
 * `addTrackVideo`/`addTrackAudio` still have no keyboard/menu/toolbar
 * surface of their own (a future one is still a later task's), so
 * `actions.ts` gives them their own permanent `RESOLVERS` entry
 * (`resolveNoTrackSurfaceYet`) reproducing the exact disabled-with-reason
 * text this gate used to supply — never `BUILDERS`, since there is still
 * nothing for either to build a command FOR. **Task 29 removed
 * `setFades` the same way, WITH a consumer**: `resolveFade`/`buildFade`
 * (`actions.ts`) give `fadeIn`/`fadeOut` their own quick-toggle command
 * (0 <-> a default duration), independent of `FadesSection`/`ClipItem`'s
 * own gold-handle drag and numeric-entry paths, which call
 * `editorProject.execute` directly (the `AudioSection`/`MixerPopover`
 * precedent) and never go through this registry at all. **Task 30 removed
 * `addTransition`/`setTransitionDuration`/`removeTransition`, WITH
 * consumers**: the `transition` action ("Add transition", in the clip
 * context menu and the Fades inspector) got its own `RESOLVERS`/`BUILDERS`
 * entries over `transitionRules.ts`, and the Fades inspector's
 * `TransitionRow.vue` sends the other two directly (no `ActionId` maps to
 * either). **Task 31 removed `setSpeed`/`setLayout`, WITH consumers**: the
 * Speed and Layout inspector sections and the preview's `LayoutHandles`
 * send them directly (no `ActionId` maps to either). **Task 33 removed
 * `addCard`/`updateCard`/`insertIntro`**: no `ActionId` has ever mapped to
 * any of the three (`TitlesLibrary.vue` sends `addCard` directly through
 * `editorProject.execute`, the `MediaLibrary.vue` "+" precedent), so unlike
 * every other removal above there was no gate to lift here either way —
 * this is bookkeeping only, keeping this table's rows matching Rust's
 * implemented set per `mod.rs`'s own rule.
 *
 * **This is a hand-copy with nothing keeping it in sync with Rust's own
 * table**, and Rust's `commands/mod.rs` module doc names this exact
 * constant back — a task that implements a kind there (deletes its row
 * from `unimplemented_commands()` and adds an explicit `apply()` arm) MUST
 * delete the matching entry here in the SAME commit, or every action that
 * maps to that kind stays wrongly disabled after Rust can already accept
 * it. There is no test or build step that catches a divergence in either
 * direction. **Task 32 removed `setAdjustments`/`setCanvas`, WITH
 * consumers**: no `ActionId` maps to `setAdjustments` at all (`ColorSection`
 * calls `editorProject.execute` directly, the `AudioSection`/
 * `MixerPopover` precedent), and `ratio` — the one `ActionId` that used to
 * name `setCanvas` here purely to stay gated — now has its own `RESOLVERS`
 * entry and sends `setCanvas` directly from its ratio control in
 * `PreviewToolbar.vue`, never through `commandFor`. **Task 34 removed
 * `addEffect`/`updateEffect`/`removeEffect`**, and Task 35 gave them their
 * consumers: the seven teaching tools map to `addEffect` through
 * `cueActions.ts`'s resolver/builder pair, while `updateEffect`/
 * `removeEffect` are sent directly by `CueHandles.vue` and
 * `EffectSection.vue` (the `ColorSection` precedent), never through this
 * registry. **Task 36 emptied the set**: the last eight kinds (the five
 * caption kinds and the three marker kinds) are implemented, sent directly
 * by `CaptionsLibrary`/`ChaptersLibrary`, and the `addCaption`/`addMarker`
 * actions resolve through `captionRules.ts`. It stays, empty, as the place
 * a future kind that ships ahead of its Rust arm is gated.
 */
export const UNIMPLEMENTED_KINDS: ReadonlySet<string> = new Set<string>([]);

/**
 * The human copy shown for an action `UNIMPLEMENTED_KINDS` gates — NEVER
 * the raw wire kind (fix round 1, finding 1: a button labelled "Text"
 * showing the literal string "addEffect" leaks Rust's own error vocabulary
 * onto a control whose label is a completely different word the user
 * never typed or clicked). Built from the action's own `ACTION_LABELS`
 * entry so every gated action reads a consistent, honest sentence in the
 * same register as `NOTHING_TO_REVIEW` above, with no per-action hand-written
 * string to keep in sync as `UNIMPLEMENTED_KINDS` shrinks. The wire kind
 * itself stays available to whoever is debugging via `ACTION_KIND`/
 * `resolveActions`'s own gating code, which keeps a local `kind` binding
 * for exactly that — it is diagnostic data, just never UI copy.
 */
export function unavailableReason(actionId: ActionId): string {
  return `${ACTION_LABELS[actionId]} arrives in a later update.`;
}
