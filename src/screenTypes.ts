/**
 * The SCREEN-CAPTURE domain's wire types (spec §5-§12).
 *
 * Split out of `types.ts` when that file reached 521 nonblank against the
 * 500 cap. The seam is the domain, not the line count: every type here
 * describes something the screen feature alone puts on the wire — a capture
 * source, a region pick, a staged capture, a timeline, an export.
 * `types.ts` re-exports the whole module, so every existing
 * `from "../types"` import keeps working and no call site moved.
 *
 * Several of these are pinned key-for-key against their Rust counterparts by
 * structural tests (`export_commands.rs` for `ExportResult`,
 * `screen_config_commands.rs` for `ScreenCaptureConfig`), because a JSON
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

/** One cut of the source, in SOURCE milliseconds. Mirrors
 * `core::timeline::Segment`. */
export interface SegmentDto {
  sourceStartMs: number;
  sourceEndMs: number;
}

/** The editor's in-progress edit. Mirrors `core::timeline::Timeline`, and is
 * what the staging sidecar's `timeline` field holds. */
export interface TimelineDto {
  segments: SegmentDto[];
}

/** What `load_staged_capture` returns. `assetPath` is the staged `.mp4`'s own
 * ABSOLUTE path; the editor hands it to `convertFileSrc(path, "asset")`.
 *
 * It was a bare file name until P-5. `convertFileSrc` JOINS NOTHING — it
 * percent-encodes its argument onto the asset origin — so a bare name became
 * a URL naming no file on disk, matching no entry in the asset protocol's
 * `$APPLOCALDATA/screen-captures/*` scope, and the preview never loaded. The
 * scope, not the opacity of this string, is what confines the webview: it
 * lives in `tauri.conf.json` and is enforced by Tauri on every request, and
 * nothing on this side of the wire can widen it. */
export interface StagedCaptureDetail {
  base: string;
  assetPath: string;
  durationMs: number;
  sourceTitle: string;
  width: number;
  height: number;
  recordedAt: string;
  /** Whatever the sidecar held — NOT a guarantee. `editor_commands.rs`
   * returns `Option<serde_json::Value>` and never inspects its shape, and the
   * sidecar is hand-editable, so this annotation is a description of the
   * happy path rather than a contract (docs/Gaps.md GAP-134, which also
   * carries the fix: parse it into `core::timeline::Timeline` on the Rust
   * side, where the untrusted value already crosses a typed boundary). */
  timeline: TimelineDto | null;
}

/** The `screen:exportProgress` payload.
 *
 * `fraction` is 0..1, NEVER 0..100. Rust derives it from the same whole
 * percent its emit throttle gates on (`progress_payload_fraction`), so the
 * number that passed the gate and the number rendered here are one number;
 * a 0..100 payload would render a bar that is full from the first tick. */
export interface ExportProgress {
  base: string;
  fraction: number;
}

/** The `screen:exported` payload — the ninth sanctioned vault write landing.
 *
 * `notePath` is null exactly when the vault has companion notes turned off.
 * `warning` is set when the video landed but its note did not: a degraded
 * SUCCESS, never a failure.
 *
 * `vaultId` is what `open_screen_capture` needs; `vaultName` is what a human
 * reads. Both are carried because the editor window installs no store and has
 * no vault list, so it cannot turn one into the other. */
export interface ExportResult {
  base: string;
  videoPath: string;
  notePath: string | null;
  vaultId: string;
  vaultName: string;
  warning: string | null;
}

/** The `screen:exportFailed` payload. A cancel is not a failure and carries
 * no message at all (`screen:exportCancelled`, spec §14). */
export interface ExportFailure {
  base: string;
  message: string;
}

/** One row of `list_staged_captures` (`StagedCaptureSummaryDto`) — a capture
 * that has been recorded but not yet saved into a vault or discarded.
 *
 * `durationMs` is what was RECORDED; `outputDurationMs` is what an export
 * would PRODUCE. They differ exactly when the editor cut something out, and
 * the second is the number the user is really deciding about.
 *
 * `edited` is Rust's own `Timeline::is_untouched` answer, not "the sidecar
 * has a timeline field" — the editor writes a timeline on every operation
 * and never writes null, so the field's mere presence marks every capture
 * the editor was ever OPENED on. A capture whose source duration is unknown
 * (`recovered`, below) is never `edited`: unknown is not an edit.
 *
 * `recovered` means `screen_recovery` rebuilt this capture's sidecar after
 * an interrupted session. It therefore records neither the vault it belongs
 * to nor its duration — nothing on disk remembers either — so Save refuses
 * it outright (`export_worker::prepare`) and the row must not offer one. */
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
}
