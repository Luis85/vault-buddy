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
  "toggleLibrary", "toggleInspector", "focusPreview", "ratio",
];

// ---- human-text reasons (Behavior section's own literal values) -----------

export const NO_CLIP = "Select a clip first";
export const CLIP_BOUNDARY = "The playhead is at a clip boundary";
export const NO_PROJECT = "No project is open.";
export const RENDER_REASON = "Rendering a video arrives in a later update.";

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
  ratio: "Aspect ratio",
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
  detachAudio: "detachAudio", ratio: "setCanvas",
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
  save: "Ctrl+S", render: "Ctrl+E", help: "F1", focusPreview: "F6",
};

/**
 * The exact thirty wire kinds `core::editor::commands::mod.rs`'s `apply()`
 * still rejects. `undo`/`redo`/`splitClip`/`deleteClips`/`cutClips`/
 * `pasteFragment`/`duplicateClips`/`groupClips`/`ungroupClips`/
 * `reorderClip` are deliberately absent: those ten (of the sixteen
 * implemented kinds) are the ones an action in `ACTION_KIND` maps to.
 *
 * **This is a hand-copy with nothing keeping it in sync with Rust's own
 * table**, and Rust's `commands/mod.rs` module doc names this exact
 * constant back — a task that implements a kind there (deletes its row
 * from `unimplemented_commands()` and adds an explicit `apply()` arm) MUST
 * delete the matching entry here in the SAME commit, or every action that
 * maps to that kind stays wrongly disabled after Rust can already accept
 * it. There is no test or build step that catches a divergence in either
 * direction.
 */
export const UNIMPLEMENTED_KINDS: ReadonlySet<string> = new Set([
  "addTrack", "renameTrack", "moveTrack", "setTrackFlags", "deleteTrack",
  "setClipMix", "setMasterGain", "detachAudio", "setFades",
  "addTransition", "setTransitionDuration", "removeTransition",
  "setSpeed", "setLayout", "setAdjustments", "setCanvas",
  "addCard", "updateCard", "insertIntro",
  "addEffect", "updateEffect", "removeEffect",
  "setCaptionSettings", "addCaption", "updateCaption", "splitCaption", "removeCaptions",
  "addMarker", "updateMarker", "removeMarker",
]);

/**
 * The human copy shown for an action `UNIMPLEMENTED_KINDS` gates — NEVER
 * the raw wire kind (fix round 1, finding 1: a button labelled "Text"
 * showing the literal string "addEffect" leaks Rust's own error vocabulary
 * onto a control whose label is a completely different word the user
 * never typed or clicked). Built from the action's own `ACTION_LABELS`
 * entry so every gated action reads a consistent, honest sentence in the
 * same register as `RENDER_REASON` above, with no per-action hand-written
 * string to keep in sync as `UNIMPLEMENTED_KINDS` shrinks. The wire kind
 * itself stays available to whoever is debugging via `ACTION_KIND`/
 * `resolveActions`'s own gating code, which keeps a local `kind` binding
 * for exactly that — it is diagnostic data, just never UI copy.
 */
export function unavailableReason(actionId: ActionId): string {
  return `${ACTION_LABELS[actionId]} arrives in a later update.`;
}
