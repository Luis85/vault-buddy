/**
 * The SCREEN-CAPTURE domain's wire types (spec §5-§12).
 *
 * Split out of `types.ts` when that file reached 521 nonblank against the
 * 500 cap. The seam is the domain, not the line count: every type here
 * describes something the screen feature alone puts on the wire — a capture
 * source, a region pick, a staged capture, the staging directory.
 * `types.ts` re-exports the whole module, so every existing
 * `from "../types"` import keeps working and no call site moved.
 *
 * Several of these are pinned key-for-key against their Rust counterparts by
 * literal-JSON tests (`screen_config_commands.rs` for `ScreenCaptureConfig`,
 * `staging_commands.rs` for `ClearStagedResult`), because a JSON
 * object literal crossing IPC is a seam no derive enforces — the GAP-135
 * class.
 */

/** One capturable source from `list_capture_sources`
 * (`vault_buddy_screen::source::CaptureSourceInfo`, camelCase on the wire).
 * `detail` is the ready-made secondary line Rust composes — resolution +
 * primary flag for a monitor, the owning process for a window — so the
 * picker never re-derives it and the two can never disagree. */
export interface CaptureSourceInfo {
  id: string;
  kind: "screen" | "window" | "region";
  title: string;
  detail: string;
  width: number;
  height: number;
  isPrimary: boolean;
}

/** One webcam the Record Screen picker offers
 * (`vault_buddy_screen::source::CaptureWebcamInfo`, camelCase on the wire).
 * `id` is `webcam:<hash>` — what `start_screen_capture`'s `webcamId`
 * carries back. */
export interface CaptureWebcamInfo {
  id: string;
  label: string;
}

/** What `RegionRoot` hands back for one drag. All four geometry values are
 * LOGICAL (CSS) pixels relative to the overlay's own viewport, which is the
 * target monitor's origin — Rust converts to physical with that monitor's
 * own scale factor (`core::screen_geometry::to_physical`). `dpr` is
 * `window.devicePixelRatio`, sent so Rust can LOG a disagreement with the
 * monitor's scale factor; it is never used to scale. */
export interface RegionPick {
  x: number;
  y: number;
  width: number;
  height: number;
  dpr: number;
}

/** `select_capture_region`'s reply. `null` means the user cancelled.
 *
 * Declared ahead of its consumer while the command existed and the picker did
 * not; `ScreenRegionPicker` reads it now, so the self-enforcing
 * `fallow-ignore-next-line unused-type` that parked it has been removed —
 * fallow counts a STALE suppression as an issue, which is exactly what made
 * it safe to leave in the tree in the first place. */
export interface RegionSelection {
  sourceId: string;
  x: number;
  y: number;
  width: number;
  height: number;
}

/** `screen_capture_status`' reply and the `screen:started` payload
 * (`ScreenStatusPayload`). `capturing` is a BOOLEAN here; the store maps it
 * onto its own three-valued `status` string — see `statusFrom`. */
export interface ScreenCaptureStatus {
  capturing: boolean;
  vaultId: string | null;
  startedAtMs: number | null;
  paused: boolean;
  pausedTotalMs: number;
  pausedSinceMs: number | null;
  sourceTitle: string | null;
}

/** The `screen:stopped` payload (`StagedCaptureDto`). `path` is the staged
 * `.mp4` in the app's own staging directory — nothing here is in a vault
 * yet. */
export interface StagedCapture {
  base: string;
  path: string;
  durationMs: number;
  sourceTitle: string;
  width: number;
  height: number;
}

/** One row of `list_staged_captures` (`StagedCaptureSummaryDto`) — a capture
 * that has been recorded but not yet published into a vault or discarded.
 *
 * `durationMs` is what was RECORDED; `outputDurationMs` is what the
 * phase-4 editor's saved cut PRODUCES (the cut the tutorial editor
 * migrates). They differ exactly when that editor cut something out.
 *
 * `edited` is Rust's own `Timeline::is_untouched` answer, not "the sidecar
 * has a timeline field" — the phase-4 editor wrote a timeline on every
 * operation and never wrote null, so the field's mere presence marks every
 * capture it was ever OPENED on. A capture whose source duration is unknown
 * (`recovered`, below) is never `edited`: unknown is not an edit.
 *
 * `recovered` means `screen_recovery` rebuilt this capture's sidecar after
 * an interrupted session. It therefore records neither the vault it belongs
 * to nor its duration — nothing on disk remembers either — so the editor
 * refuses to open it (`editor_open_staged`, F7) and the row must not offer
 * Edit.
 *
 * `projectId` is the tutorial project this capture is PINNED to (R6), when
 * one has adopted it by reference — `null` for an ordinary staged capture.
 * A pinned row must not offer Discard (`staged_commands::discard_conflict`
 * refuses it server-side too — this is the UI half, not the only guard). */
export interface StagedCaptureSummary {
  base: string;
  vaultId: string;
  sourceTitle: string;
  durationMs: number;
  outputDurationMs: number;
  recordedAt: string;
  width: number;
  height: number;
  edited: boolean;
  recovered: boolean;
  projectId: string | null;
}

/** Per-vault screen-capture settings — get_screen_capture_config /
 * set_screen_capture_config (spec §12). One DTO for both directions, the
 * CaptureConfig shape: the setter takes `{ id, cfg }`.
 *
 * Every key here is asserted against the Rust DTO by a test in
 * `screen_config_commands.rs`, because a JSON object crossing IPC is a seam
 * no derive enforces — renaming a field on one side alone leaves both
 * compiling and the setting silently unreadable. */
export interface ScreenCaptureConfig {
  /** null/blank = the "Screen Captures" default. */
  screenCaptureFolder: string | null;
  /** Dated `YYYY/MM` subfolder (true) or flat (false), the same toggle the
   * recording and document domains carry. */
  screenCaptureDateFolders: boolean;
  /** The stable KEY — "low" | "balanced" | "high" — never a variant name.
   * It is what config.json stores and what ScreenQuality::from_key parses. */
  screenQuality: string;
  /** 30 or 60. Anything else is REFUSED by the setter rather than
   * normalised: the parse layer normalises so a hand-edited config.json
   * still opens the app, but a control the user is looking at must not
   * quietly become something else. */
  screenFps: number;
  /** Whether an export writes a companion note beside the .mp4. */
  screenCreateNote: boolean;
  /** Additive per-vault templates for that note. null = today's exact
   * output; managed identity keys are always emitted and never removable. */
  screenExtraFrontmatter?: string | null;
  screenBodyTemplate?: string | null;
  /** Keep each audio input as its own stem beside the mixed track (Task
   * 53). Applies to NEW recordings only; absent reads as off. */
  screenAudioStems?: boolean;
}

/** What the staging directory is holding, as Buddy settings reports it
 * (`staging_usage`). App-global, never per-vault: a staged capture records
 * which vault it is FOR, but it lives in one shared directory outside every
 * vault, and the list surfaces are app-wide for the same reason. */
export interface StagingUsage {
  /** Staged captures waiting to be edited, published or discarded. */
  captures: number;
  /** What a Clear would free: the bytes those captures occupy across their
   * video, sidecar and any abandoned export temp. NOT the directory's whole
   * size — a foreign file nobody vouched for is deliberately uncounted,
   * because this number is labelled as space Clear can reclaim. */
  bytes: number;
}

/** The outcome of `clear_staged_captures`. Four numbers, not a bare
 * success: a capture can be removed, left alone because a tutorial project
 * has PINNED it (R6), or refused (a symlinked leaf), and reporting only
 * "done" would claim the kept ones were deleted. (Task 59 dropped a fifth,
 * `skipped`: a capture the retired phase-5 export was writing.) */
export interface ClearStagedResult {
  cleared: number;
  bytesFreed: number;
  skippedPinned: number;
  failed: number;
}
