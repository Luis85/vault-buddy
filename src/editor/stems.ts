/**
 * Per-input audio stems in a migrated project (Task 53, F-05).
 *
 * Migration names stem `n`'s asset `stem-<n>` (Rust:
 * `core::editor::migrate::stem_asset_id`). This is the one TypeScript reading
 * of that shape, used to tell whether a capture was recorded with stems.
 */
import type { Asset } from "../editorTypes";

const STEM_ASSET_ID = /^stem-\d+$/;

/** Is `asset` one of a capture's per-input stems? */
export function isStemAsset(asset: Asset): boolean {
  return asset.kind === "audio" && STEM_ASSET_ID.test(asset.id);
}
