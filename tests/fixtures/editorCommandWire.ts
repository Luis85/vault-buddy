/**
 * One instance of EVERY `EditorCommand` kind, in its exact wire spelling —
 * final whole-branch review I7. `EditorCommand` is hand-maintained twice:
 * `core::editor::commands::EditorCommand` (+ `payloads.rs`) in Rust and
 * `src/editor/editorCommandTypes.ts` in TypeScript.
 *
 * - TypeScript: the array below `satisfies EditorCommand[]`, so `vue-tsc`
 *   (which checks `tests/`) refuses an entry the union does not accept —
 *   a missing kind, a misspelled field, a field of the wrong type.
 * - Rust: `core::editor::commands`' `the_shared_wire_table_*` test reads
 *   THIS file (`include_str!`), takes the JSON between the two markers,
 *   deserializes every entry as an `EditorCommand`, re-serializes it and
 *   requires the same JSON back (so no field is dropped or renamed on
 *   either side), and requires each of the enum's variants exactly once,
 *   through an exhaustive `match` a new variant cannot compile past.
 *
 * Optional payload fields are all present, so their spelling is pinned too.
 * Keep the part between the markers plain JSON (quoted keys, no trailing
 * commas): the Rust half parses it as JSON.
 */
import type { EditorCommand } from "../../src/editorTypes";

export const EDITOR_COMMAND_WIRE = // BEGIN WIRE
[
  { "kind": "rename", "title": "Walkthrough" },
  { "kind": "undo" },
  { "kind": "redo" },
  { "kind": "insertClip", "assetId": "src", "trackId": "v1", "startMs": 1200, "inMs": 300, "outMs": 9100 },
  { "kind": "updateClip", "clipId": "c1", "name": "Intro shot" },
  { "kind": "splitClip", "clipId": "c1", "atMs": 4400 },
  { "kind": "trimClip", "clipId": "c1", "startMs": 800, "inMs": 250, "outMs": 7300 },
  { "kind": "deleteClips", "clipIds": ["c1", "c2"], "closeGap": true },
  { "kind": "moveClips", "clipIds": ["c3"], "deltaMs": -250, "trackId": "v2" },
  { "kind": "reorderClip", "clipId": "c4", "direction": "earlier" },
  { "kind": "groupClips", "clipIds": ["c1", "c5"] },
  { "kind": "ungroupClips", "groupId": "g1" },
  { "kind": "duplicateClips", "clipIds": ["c6"], "offsetMs": 1500 },
  {
    "kind": "pasteFragment",
    "fragment": { "clips": [], "effects": [], "captions": [], "markers": [], "originMs": 2100 },
    "trackId": "v1",
    "atMs": 6300
  },
  { "kind": "cutClips", "clipIds": ["c7"], "closeGap": false },
  { "kind": "addTrack", "trackKind": "audio", "name": "Voice", "index": 2 },
  { "kind": "renameTrack", "trackId": "a1", "name": "Narration" },
  { "kind": "moveTrack", "trackId": "a1", "toIndex": 0 },
  { "kind": "setTrackFlags", "trackId": "a1", "visible": false, "locked": true, "muted": true, "solo": false, "volume": 0.75 },
  { "kind": "deleteTrack", "trackId": "a2" },
  { "kind": "setClipMix", "clipIds": ["c8"], "volume": 1.25, "muted": false },
  { "kind": "setMasterGain", "gain": 0.8 },
  { "kind": "detachAudio", "clipId": "c9", "audioTrackId": "a3" },
  { "kind": "setFades", "clipId": "c9", "fadeInMs": 400, "fadeOutMs": 650, "fadeCurve": "equal-power" },
  { "kind": "addTransition", "fromClipId": "c1", "toClipId": "c2", "durationMs": 500, "transitionKind": "dissolve" },
  { "kind": "setTransitionDuration", "transitionId": "t1", "durationMs": 700 },
  { "kind": "removeTransition", "transitionId": "t1" },
  { "kind": "setSpeed", "clipId": "c2", "speed": 1.5, "preservePitch": false },
  {
    "kind": "setLayout",
    "clipIds": ["c10"],
    "x": 0.1,
    "y": 0.2,
    "w": 0.35,
    "h": 0.45,
    "opacity": 0.9,
    "fit": "contain",
    "frameShape": "circle",
    "rotation": 90,
    "mirror": true,
    "flipY": false,
    "cropZoom": 1.2,
    "cropX": 0.05,
    "cropY": 0.15
  },
  {
    "kind": "setAdjustments",
    "clipIds": ["c10"],
    "adjustments": { "brightness": 1.1, "contrast": 0.9, "saturation": 1.3, "sepia": 0.2, "grayscale": 0.1 }
  },
  { "kind": "setCanvas", "width": 720, "height": 1280 },
  { "kind": "addCard", "preset": "chapter", "trackId": "v3", "startMs": 3000, "durationMs": 2500, "title": "Part two", "subtitle": "Settings" },
  {
    "kind": "updateCard",
    "clipId": "card-1",
    "title": "Part three",
    "subtitle": "Export",
    "background": "#102030",
    "foreground": "#f0e0d0",
    "accent": "#aa3366"
  },
  { "kind": "insertIntro", "durationMs": 3500, "title": "Welcome", "subtitle": "A short tour" },
  {
    "kind": "addEffect",
    "clipId": "c1",
    "effectKind": "text",
    "startMs": 1000,
    "endMs": 2600,
    "props": { "x": 0.1, "y": 0.8, "w": 0.6, "h": 0.12, "text": "Click Save", "fontSize": 28, "color": "#ffffff", "background": true }
  },
  { "kind": "updateEffect", "effectId": "e1", "startMs": 1100, "endMs": 2700, "props": { "text": "Click Save now" } },
  { "kind": "removeEffect", "effectId": "e2" },
  { "kind": "setCaptionSettings", "enabled": true, "burnIn": false, "fontSize": 34, "position": "top", "background": true },
  { "kind": "addCaption", "clipId": "c1", "startMs": 500, "endMs": 2300, "text": "Open the settings" },
  { "kind": "updateCaption", "captionId": "k1", "startMs": 600, "endMs": 2400, "text": "Open the settings menu" },
  { "kind": "splitCaption", "captionId": "k1", "atMs": 1500 },
  { "kind": "removeCaptions", "captionIds": ["k2", "k3"] },
  { "kind": "addMarker", "clipId": "c1", "sourceMs": 5200, "title": "Chapter two" },
  { "kind": "updateMarker", "markerId": "m1", "sourceMs": 5400, "title": "Chapter 2" },
  { "kind": "removeMarker", "markerId": "m2" },
  { "kind": "setDestination", "vaultId": "a1b2c3d4e5f60718", "folder": "Tutorials", "dated": true }
] satisfies EditorCommand[]; // END WIRE
