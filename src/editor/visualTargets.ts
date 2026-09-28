/**
 * Which clips have a picture to lay out or colour — the webview's mirror of
 * `core::editor::commands::layout::check_targets` / `check_color_targets`
 * (visual-parity Task 13 fix round 1). `setLayout` and `setAdjustments`
 * refuse a selection if ANY clip is not on a video track, and
 * `setAdjustments` also refuses a title card, so every surface that offers
 * either — the Layout and Color sections, the clip menus' "Color
 * treatment", the inspector's Selection properties — asks here first, in
 * Rust's order, with the Color section's own words.
 */
import type { Project } from "../editorTypes";

export const VISUAL_NOTE = "Colour applies to video and image clips. Select only those to adjust them.";
export const CARD_NOTE = "Colour applies to footage, not title cards.";

/** Every clip resolves and sits on a video track (and there is one). */
export function allOnVideoTracks(project: Project | null, clipIds: readonly string[]): boolean {
  if (!project || clipIds.length === 0) return false;
  return clipIds.every((id) => {
    const clip = project.clips.find((c) => c.id === id);
    return project.tracks.find((t) => t.id === clip?.track_id)?.kind === "video";
  });
}

/** Why `setAdjustments` would refuse these clips, or `null` when it would not
 * (the lock aside, which every caller states on its own). */
export function colorRefusal(project: Project | null, clipIds: readonly string[]): string | null {
  if (!allOnVideoTracks(project, clipIds)) return VISUAL_NOTE;
  const isCard = (id: string) => {
    const clip = project?.clips.find((c) => c.id === id);
    return project?.assets.find((a) => a.id === clip?.asset_id)?.builtin === "card";
  };
  return clipIds.some(isCard) ? CARD_NOTE : null;
}
