/**
 * "Remove track…" (visual-parity Task 13; concept `editor.js`
 * `deleteTrack`): one rule for every surface that offers it — the track
 * menu (`menuSets.ts`) and the inspector's Track properties. A track with
 * clips asks first (`RemoveTrackDialog`, which answers `trackRemovalRequest`);
 * an empty one goes at once, since there is nothing on it to lose and the
 * removal is one undoable edit either way.
 *
 * A module-level `ref`, not store state: the request is a pending question
 * to the person, the `trackRenameRequest` precedent in `revealBus.ts`. It
 * names the session it was asked in, so a question left over from a closed
 * project is never put to the next one — even one with a track of the same
 * id (fix round 1).
 */
import { ref } from "vue";

import type { Project } from "../editorTypes";
import type { EditorCommand } from "./editorCommandTypes";

/** The track whose removal waits for the person's answer, and the session
 * it belongs to. */
export const trackRemovalRequest = ref<{ sessionId: string | null; trackId: string } | null>(null);

/** What a removal needs from the open session (`editorProject`). */
interface Session {
  sessionId: string | null;
  project: Project | null;
  execute(command: EditorCommand): Promise<boolean>;
}

export function clipsOnTrack(project: Project | null, trackId: string): number {
  return project?.clips.filter((c) => c.track_id === trackId).length ?? 0;
}

/** Asks first when the track holds clips; otherwise sends `deleteTrack`. */
export function requestTrackRemoval(session: Session, trackId: string): void {
  if (clipsOnTrack(session.project, trackId) > 0) {
    trackRemovalRequest.value = { sessionId: session.sessionId, trackId };
    return;
  }
  void session.execute({ kind: "deleteTrack", trackId });
}

/** The confirm's body, the concept's own words. */
export function removalMessage(trackName: string, clipCount: number): string {
  const clips = clipCount === 1 ? "1 clip" : `${clipCount} clips`;
  return `Remove “${trackName}” and its ${clips} from this edit? Original media stays in the library. You can undo this.`;
}
