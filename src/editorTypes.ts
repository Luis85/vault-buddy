/**
 * The tutorial editor's TS declaration site (F10; the `screenTypes.ts`
 * precedent for a domain-scoped split of `types.ts`).
 *
 * Created here with only the entity type `core::editor::time` /
 * `src/editor/timeMap.ts` need (`ClipSpan`); Task 8 extends it with the
 * entity/clipboard shapes `src/editor/fragment.ts` needs (`Clip`/
 * `Effect`/`CaptionCue`/`Marker`/`ClipboardFragment`, plus the minimal
 * `Project` slice `buildFragment` reads). Task 13 widens `Project` to the
 * FULL interchange graph (`core::editor::model::Project` field for field)
 * and adds every remaining Contract-reference envelope
 * (`EditorSnapshot`/`EditorProjection`/`EditorOpenResult`/`SaveReceipt`/
 * `ProjectSummaryDto`/`EditorError`/`JobProgressDto`/…) plus the
 * `EditorCommand` union, split out to `src/editor/editorCommandTypes.ts`
 * (F31) and re-exported below so this stays the one import site every
 * caller uses. Later tasks keep growing this file rather than forking a
 * second declaration site or growing `types.ts`.
 *
 * Field spelling: the IPC ENVELOPE wraps everything in camelCase
 * (`sessionId`, `projectId`, …), but the `project` graph it carries keeps
 * the interchange document's OWN spelling, which is snake_case (R3,
 * `core::editor::model`'s module doc: "Field names are the interchange
 * document's own snake_case spelling"). `ClipSpan` below describes a
 * fragment of that graph, so it is snake_case too — not a TypeScript
 * convention violation, but the one place it would be wrong to camelCase.
 * The entity interfaces below (`Clip`/`Effect`/`CaptionCue`/`Marker`) are
 * document spelling for the same reason, mirroring
 * `core::editor::model`/`core::editor::model_cues` field-for-field —
 * `Effect.fontSize` is the ONE field that stays camelCase even there,
 * because the Rust struct itself renames it
 * (`#[serde(rename = "fontSize")]`, pinned by
 * `font_size_keeps_its_camel_case_spelling`).
 */

// `EditorCommand` is declared in the split-out file (F31) and imported
// here so `ExecuteRequest` below can reference it locally; the bare
// `export type {...} from` at the end of this file re-exports the SAME
// import for every other caller — an `export ... from` re-export alone
// does not bring a name into this module's own scope.
import type { EditorCommand } from "./editor/editorCommandTypes";

/** A clip's time-mapping span: output start, half-open source range
 * `[in_ms, out_ms)`, and speed. Mirrors `core::editor::time::ClipSpan`
 * field-for-field so a `Project`'s clip entity, read straight off the
 * wire, can be passed to `timeMap.ts` without any renaming. */
export interface ClipSpan {
  start_ms: number;
  in_ms: number;
  out_ms: number;
  speed: number;
}

/** `core::editor::model::FadeCurve` (kebab-case on the wire). */
export type FadeCurve = "linear" | "smooth" | "equal-power";

/** A clip's quarter-turn rotation in plain degrees (`core::editor::model::Rotation`). */
export type Rotation = 0 | 90 | 180 | 270;

export type FrameShape = "circle" | "rounded" | "rectangle";

export type Fit = "cover" | "contain";

/** Brightness/contrast/saturation/sepia/grayscale
 * (`core::editor::model::Adjustments`) — every field is required once the
 * object is present at all. */
export interface Adjustments {
  brightness: number;
  contrast: number;
  saturation: number;
  sepia: number;
  grayscale: number;
}

export type CardPreset = "intro" | "chapter" | "outro" | "blank";

/** `core::editor::model::TrackKind`. */
export type TrackKind = "audio" | "video";

/** A titles/chapter card's editable content (`core::editor::model::Card`). */
export interface Card {
  preset: CardPreset;
  title: string;
  subtitle: string;
  background: string;
  foreground: string;
  accent: string;
}

/** An instance of a source range at an output time
 * (`core::editor::model::Clip`). Document spelling (snake_case) — see the
 * module doc above. */
export interface Clip {
  id: string;
  asset_id: string;
  track_id: string;
  name: string;
  start_ms: number;
  in_ms: number;
  out_ms: number;
  fade_in_ms: number;
  fade_out_ms: number;
  fade_curve: FadeCurve;
  opacity: number;
  volume: number;
  muted: boolean;
  x: number;
  y: number;
  w: number;
  h: number;
  speed?: number;
  rotation?: Rotation;
  frame_shape?: FrameShape;
  fit?: Fit;
  mirror?: boolean;
  flip_y?: boolean;
  preserve_pitch?: boolean;
  group_id?: string;
  crop_zoom?: number;
  crop_x?: number;
  crop_y?: number;
  adjustments?: Adjustments;
  card?: Card;
}

export type EffectKind = "text" | "arrow" | "highlight" | "spotlight" | "zoom" | "step" | "mask";

/** A clip-linked teaching cue (`core::editor::model_cues::Effect`).
 * `start_ms`/`end_ms` are SOURCE time, like every clip-linked cue. */
export interface Effect {
  id: string;
  clip_id: string;
  kind: EffectKind;
  start_ms: number;
  end_ms: number;
  x: number;
  y: number;
  color: string;
  text?: string;
  w?: number;
  h?: number;
  x2?: number;
  y2?: number;
  factor?: number;
  fontSize?: number;
  stroke?: number;
  dim?: number;
  easing?: number;
  number?: number;
  background?: boolean;
}

export type CaptionPosition = "top" | "bottom";

/** A caption cue (`core::editor::model_cues::CaptionCue`) — clip-linked
 * SOURCE time, like `Effect`. */
export interface CaptionCue {
  id: string;
  clip_id: string;
  start_ms: number;
  end_ms: number;
  text: string;
}

/** Project-level caption presentation plus the source-linked cue records
 * (`core::editor::model_cues::CaptionSettings`). */
export interface CaptionSettings {
  enabled: boolean;
  burn_in: boolean;
  font_size: number;
  position: CaptionPosition;
  background: boolean;
  cues: CaptionCue[];
}

/** A chapter marker (`core::editor::model_cues::Marker`) — clip-linked
 * SOURCE time, like `Effect`/`CaptionCue`. */
export interface Marker {
  id: string;
  clip_id: string;
  source_ms: number;
  title: string;
}

/**
 * The full interchange project graph (`core::editor::model::Project`,
 * field for field, document spelling). Widened in Task 13 from the
 * narrower `{clips, effects, markers, captions}` slice Task 8 introduced —
 * the single-TS-declaration-site rule: extend this interface rather than
 * forking a second, narrower one. `captions` mirrors Rust's own
 * `#[serde(skip_serializing_if = "Option::is_none")]`: the key is OMITTED
 * on the wire when there are no captions, which `decodeProject`
 * normalizes to `null` here rather than `undefined`, so callers can rely
 * on one falsy shape.
 */
export interface Project {
  schema: string;
  id: string;
  title: string;
  canvas: Canvas;
  master_gain: number;
  assets: Asset[];
  tracks: Track[];
  clips: Clip[];
  effects: Effect[];
  markers: Marker[];
  transitions: Transition[];
  captions: CaptionSettings | null;
  destination: Destination;
}

/**
 * `pasteFragment`'s clipboard payload (Task 8, F9): the IPC ENVELOPE's own
 * fields are camelCase (`origin_ms` → `originMs`, per
 * `commands::payloads::ClipboardFragment`'s own
 * `#[serde(rename_all = "camelCase")]`), but the entities it carries
 * (`Clip`/`Effect`/`CaptionCue`/`Marker`) stay in document spelling, same
 * as everywhere else in the project graph.
 */
export interface ClipboardFragment {
  clips: Clip[];
  effects: Effect[];
  captions: CaptionCue[];
  markers: Marker[];
  originMs: number;
}

// ---- the rest of the project graph (Task 13) ------------------------------
// Document spelling throughout — see the module doc above.

/** `core::editor::model_cues::TransitionKind` (kebab-case on the wire). */
export type TransitionKind = "dissolve" | "equal-power";

/** A pairwise relationship between two clips (`core::editor::model_cues::Transition`). */
export interface Transition {
  id: string;
  from: string;
  to: string;
  duration_ms: number;
  kind: TransitionKind;
}

export type AssetKind = "video" | "audio";

/** `core::editor::model::Builtin` — a procedurally-supplied asset. */
export type Builtin = "presenter" | "screen" | "detail" | "cues" | "ambient" | "card";

export type MediaType = "image";

/** An original immutable source or procedural source (`core::editor::model::Asset`). */
export interface Asset {
  id: string;
  kind: AssetKind;
  name: string;
  duration_ms: number;
  width?: number;
  height?: number;
  size?: number;
  builtin?: Builtin;
  media_type?: MediaType;
  linked_asset?: string;
  original_name?: string;
}

/** The output frame (`core::editor::model::Canvas`) — one of the four
 * supported canvases at 30 fps; range validation is Rust's job. */
export interface Canvas {
  width: number;
  height: number;
  fps: number;
}

/** An ordered composition layer or audio lane (`core::editor::model::Track`). */
export interface Track {
  id: string;
  kind: TrackKind;
  name: string;
  visible: boolean;
  locked: boolean;
  muted: boolean;
  solo: boolean;
  volume: number;
}

/** Where "Save into a vault" publishes (`core::editor::model::Destination`).
 * `vault` is the Obsidian vault ID, never a display name (R3). */
export interface Destination {
  vault: string;
  folder: string;
  dated: boolean;
}

// ---- the saved workspace view-preference blob ------------------------------
// `core::editor::workspace` has no `rename_all` — its fields are snake_case
// on the wire too, like the project graph, because it is part of the same
// persisted document (`workspace.json`), not an IPC envelope.

/** `core::editor::workspace::Selected`. */
export interface Selected {
  type: string;
  id: string;
}

/** `core::editor::workspace::DeleteMode`. */
export type DeleteMode = "gap" | "close";

/** `core::editor::workspace::Theme` (F16, Task 18) — the editor header's
 * light/dark toggle, persisted through the saved workspace so it survives a
 * reopen. */
export type Theme = "dark" | "light";

/** The 18 sanitized workspace preference fields plus `theme` (R16 + F16,
 * `core::editor::workspace::Workspace`) — every one optional, since a
 * malformed/stale value degrades to absent rather than failing the read. */
export interface Workspace {
  selection_clip_ids?: string[];
  selected?: Selected;
  playhead_ms?: number;
  library_tab?: string;
  property_tab?: string;
  timeline_zoom?: number;
  timeline_height?: number;
  timeline_scroll_left?: number;
  timeline_scroll_top?: number;
  snap?: boolean;
  delete_mode?: DeleteMode;
  monitor_muted?: boolean;
  playback_rate?: number;
  library_hidden?: boolean;
  properties_hidden?: boolean;
  properties_open?: boolean;
  focus_preview?: boolean;
  caption_settings_open?: boolean;
  theme?: Theme;
}

// ---- IPC envelopes (Contract reference `Envelopes` paragraph) -------------
// camelCase throughout — these wrap the document-spelled graph above, they
// are not part of it (R3).

/** What `editor_get_snapshot`/`editor_execute` return alongside the
 * project graph (`core::editor::session::EditorSnapshot`). `undoLabel`/
 * `redoLabel` are always-present keys (`null`, never omitted) — an absent
 * key is a decoding error, not "none". */
export interface EditorSnapshot {
  sessionId: string;
  projectId: string;
  revision: number;
  persistedRevision: number | null;
  title: string;
  durationMs: number;
  canUndo: boolean;
  canRedo: boolean;
  undoLabel: string | null;
  redoLabel: string | null;
}

/** `editor_execute`/`editor_get_snapshot`'s reply
 * (`core::editor::projection::EditorProjection`). */
export interface EditorProjection {
  snapshot: EditorSnapshot;
  project: Project;
}

/** `editor_import_captions`' reply once a file was picked
 * (`core::editor::projection::CaptionImportResult`, Task 36): the
 * projection after the ONE import edit, the cues that landed on the clip,
 * and the ones that fell outside it (reported, never dropped silently).
 * The command answers `null` instead when the dialog was cancelled. */
export interface CaptionImportResult {
  projection: EditorProjection;
  imported: number;
  skipped: number;
}

/** One source a freshly opened project references but whose file is not on
 * disk (`core::editor::projection::MissingMedia`). Never a path. */
export interface MissingMedia {
  assetId: string;
  name: string;
  expectedSize: number;
  expectedDurationMs: number;
}

/** `editor_relink_media`'s reply (Task 40; ADR §3.3 `RelinkReport`;
 * `core::editor::relink::RelinkReportDto`) — `null` instead when the dialog
 * was dismissed. Files are named by display name only. `ambiguous` and
 * `mismatched` sources were left untouched; `missing` is what is still
 * missing afterwards. Every list is always present. */
export interface RelinkReport {
  projection: EditorProjection;
  missing: MissingMedia[];
  matched: { assetId: string; file: string }[];
  replaced: { assetId: string; file: string }[];
  ambiguous: { assetId: string; files: string[] }[];
  unmatched: string[];
  mismatched: { assetId: string; file: string; reason: string }[];
  /** A matching file that could not be copied in, with the real cause. */
  failed: { assetId: string; file: string; error: string }[];
  /** Picked files no original claimed. */
  unused: string[];
  /** Originals a batch left out because they cannot be reconnected here. */
  excluded: { assetId: string; reason: string }[];
  perFile: { name: string; error: string }[];
}

/** What `editor_open_staged`/`editor_open_project` return
 * (`core::editor::projection::EditorOpenResult`). `sourceBase` and
 * `missing` are always-present keys, even when `null`/`[]`. */
export interface EditorOpenResult {
  snapshot: EditorSnapshot;
  project: Project;
  workspace: Workspace;
  missing: MissingMedia[];
  sourceBase: string | null;
  recovered: boolean;
}

/** The `editor_execute` request envelope
 * (`core::editor::session::ExecuteRequest`). */
export interface ExecuteRequest {
  sessionId: string;
  expectedRevision: number;
  commandId: string;
  command: EditorCommand;
}

/** `editor_save_project`'s reply (`save_commands::SaveReceipt`, shell). */
export interface SaveReceipt {
  sessionId: string;
  savedRevision: number;
  projectFileId: string;
}

/** `editor_export_package`'s `format` (`core::editor::package_plan::
 * PackageFormat`): a portable `.vbproject.zip` carries the available
 * originals, a lightweight `.vbproject.json` the edit alone. */
export type PackageFormat = "portable" | "lightweight";

/** `editor_export_package`'s reply (`package_commands::PackageReceipt`,
 * shell) — `null` on the wire when the save dialog was dismissed. */
export interface PackageReceipt {
  sessionId: string;
  savedRevision: number;
  fileName: string;
  format: PackageFormat;
}

/** One row of `editor_list_projects` (`store_io::ProjectSummaryDto`, shell). */
export interface ProjectSummaryDto {
  projectFileId: string;
  title: string;
  updatedAt: string;
  persistedRevision: number;
  hasRecovery: boolean;
  sourceBase: string | null;
}

/** `editor_media_url`'s `ref` argument (`media_commands::MediaRef`): exactly
 * one registered entity, by id — the frontend never names a path. */
export type MediaRef = { assetId: string } | { productId: string };

/** `editor_close_session`'s disposition (`session_commands::CloseDisposition`). */
export type CloseDisposition = "keep" | "discardRecovery" | "discardProject";

/** The editor's 15 IPC error codes (`core::editor::error::EditorErrorCode`). */
export type EditorErrorCode =
  | "invalidRequest"
  | "invalidProject"
  | "revisionConflict"
  | "sessionGone"
  | "unauthorizedSource"
  | "sourceMissing"
  | "unsupportedMedia"
  | "deviceUnavailable"
  | "permissionDenied"
  | "diskFull"
  | "writeDenied"
  | "destinationUnavailable"
  | "encoderUnavailable"
  | "cancelled"
  | "internal";

/** Every rejected `editor_*` invoke's error shape
 * (`core::editor::error::EditorError`). `operationId` is always present;
 * `retainedAssetIds` only when an operation left assets behind. */
export interface EditorError {
  code: EditorErrorCode;
  message: string;
  retryable: boolean;
  operationId: string;
  retainedAssetIds?: string[];
}

/** A background job's kind/phase (Contract reference `JobProgressDto`).
 * Emitted by `media_jobs.rs`: `import` (Task 25), `peaks` (Task 28) and
 * `render` (Task 46, with its `rendering`/`publishing` phases); `publish`
 * joins with Task 48. */
export type JobKind = "import" | "render" | "peaks" | "publish";
export type JobPhase =
  | "queued"
  | "preparing"
  | "rendering"
  | "publishing"
  | "complete"
  | "cancelled"
  | "failed";

/** A terminal job's outcome payload (Contract reference `JobTerminal`). */
export interface JobTerminal {
  productId?: string;
  assetIds?: string[];
  perFile?: { name: string; error: string }[];
  error?: EditorError;
}

/** Contract reference `JobProgressDto`. */
export interface JobProgressDto {
  sessionId: string;
  jobId: string;
  kind: JobKind;
  sequence: number;
  phase: JobPhase;
  fraction: number;
  terminal: JobTerminal | null;
}

/** One row of `editor_get_jobs` — the authoritative job state a
 * reconcile installs over whatever the Channel said (Contract reference
 * `JobRecordDto`). */
export interface JobRecordDto {
  jobId: string;
  kind: JobKind;
  phase: JobPhase;
  fraction: number;
  terminal: JobTerminal | null;
}

/** `editor_import_media`'s immediate reply. */
export interface JobStarted {
  jobId: string;
}

// The ~50-variant EditorCommand union lives in its own file (F31) so this
// one does not grow toward the 500-line cap as later tasks add arms; every
// caller still imports it from here.
export type { EditorCommand } from "./editor/editorCommandTypes";
// The render/product wire types (Task 46), split out for the same cap.
export type { ProductDto, RenderRange, RenderRequest, RenderStarted } from "./editor/editorRenderTypes";
