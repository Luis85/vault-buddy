import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { type Ref, ref } from "vue";

import { logWarning } from "../logging";
import { useEditorProjectStore } from "../stores/editorProject";
import type { ExportFailure, ExportProgress, ExportResult, StagedCaptureDetail } from "../types";

/**
 * The editor window's view of spec 8.3's EXPORT: the four-state machine that
 * feeds `ExportBar`, the four `screen:export*` listeners that drive it, and
 * the bar's four verbs (Save, Cancel, Discard, Open).
 *
 * Split from `src/roots/EditorRoot.vue` at 478/500 nonblank lines. The seam is
 * the one the window already draws: `useEditorTimeline` owns the segments and
 * the undo stack, `useEditorSelection` owns the highlighted block and the
 * playhead, and this owns what happens to the capture AFTER editing — none of
 * it reads or writes the timeline, and none of the editing verbs read or write
 * it. The root keeps the one thing that couples them: which capture is open
 * (`detail`), handed in here as a ref and read, never replaced, except by
 * Discard, which reports it through `onDiscarded` so the root stays the only
 * writer of its own load state.
 *
 * Mostly no store: this is the editor's own in-flight progress, not state
 * the app owns anywhere else, so there is nothing to `init()` (AGENTS.md,
 * Frontend state) — with ONE exception since fix round 1 (tutorial-editor
 * Task 15, controller ruling): `onDiscard` reads `editorProject` to close a
 * live session BEFORE discarding. `EditorRoot.openStaged`-ing a session
 * alongside every legacy load (Task 15's own Behavior) PINS the staged
 * capture to a tutorial project, and `discard_staged_capture` refuses
 * outright while a capture is pinned ("Discard the project first.") — so
 * without this, legacy Discard was permanently refused for every capture
 * the editor has ever opened (docs/Gaps.md GAP-171).
 */
export function useEditorExport(
  detail: Ref<StagedCaptureDetail | null>,
  onDiscarded: () => void,
) {
  /** EVERY one of these is reset by the root's `load()` (`resetExport`): the
   * editor window is HIDDEN and REUSED, so a ref that survives a capture is a
   * chance to show the previous one's state under the new one's title — and a
   * stale `done` is the sharpest of them, because `done` is the one state with
   * no Save button in it at all. */
  const exportPhase = ref<"idle" | "exporting" | "done" | "failed">("idle");
  const exportFraction = ref(0);
  const exportMessage = ref<string | null>(null);
  /** What Open launches: the NOTE when the vault writes one, the video
   * otherwise. The note is the richer destination — it embeds the video, the
   * way the audio domain's note embeds its audio — and with notes turned off
   * there is none, so the video is the only answer.
   * `staged_commands::capture_file_param` takes either: it drops the extension
   * for exactly-`.md` and keeps it otherwise. */
  const savedPath = ref<string | null>(null);
  const savedVaultId = ref<string | null>(null);
  const discardBusy = ref(false);

  /** Every `screen:export*` event is emitted APP-WIDE and carries the base it
   * is about. This window edits exactly one capture and is reused, so an
   * editor reopened on B while A is still exporting would otherwise render A's
   * progress — and, worse, A's "Saved" under B's title. */
  function addressesOpenCapture(base: string): boolean {
    return detail.value !== null && detail.value.base === base;
  }

  function onExportProgress(p: ExportProgress) {
    if (!addressesOpenCapture(p.base)) return;
    // A progress tick IS an export running, so it drives the phase as well as
    // the number: the bar cannot show a fraction it is not rendering a bar for.
    exportPhase.value = "exporting";
    exportFraction.value = p.fraction;
  }

  function onExported(p: ExportResult) {
    if (!addressesOpenCapture(p.base)) return;
    exportPhase.value = "done";
    exportFraction.value = 1;
    savedPath.value = p.notePath ?? p.videoPath;
    savedVaultId.value = p.vaultId;
    // A warning means the video landed and its note did not: a degraded
    // SUCCESS. Dropping it would leave the user believing in a note that is
    // not there.
    // `vaultName` and not `vaultId`: the id is Obsidian's opaque hex registry
    // key, which names nothing to a human. A payload written by an older build
    // (or a test) carries no name, so fall back rather than render "undefined".
    exportMessage.value =
      p.warning ??
      (p.vaultName ? `Saved ${p.base} to ${p.vaultName}.` : `Saved ${p.base} into your vault.`);
  }

  /** Spec 14: a cancel keeps the staged capture AND its timeline. There is
   * nothing to apologise for, so no message and no banner — the bar returns to
   * exactly the state Save was pressed from. */
  function onExportCancelled(p: { base: string }) {
    if (!addressesOpenCapture(p.base)) return;
    resetExport();
  }

  function onExportFailed(p: ExportFailure) {
    if (!addressesOpenCapture(p.base)) return;
    exportPhase.value = "failed";
    exportFraction.value = 0;
    exportMessage.value = p.message;
  }

  function resetExport() {
    exportPhase.value = "idle";
    exportFraction.value = 0;
    exportMessage.value = null;
    savedPath.value = null;
    savedVaultId.value = null;
  }

  async function onSave() {
    const base = detail.value?.base;
    if (base === undefined) return;
    exportPhase.value = "exporting";
    exportFraction.value = 0;
    exportMessage.value = null;
    try {
      await invoke("export_and_save_capture", { base });
    } catch (e) {
      // The command REJECTS for anything it decides before a worker starts —
      // no ffmpeg, a refused base, no disk space. No `screen:exportFailed`
      // follows one of those, so a root that only listened would sit at
      // "exporting" forever.
      exportPhase.value = "failed";
      exportMessage.value = String(e);
    }
  }

  async function onCancelExport() {
    try {
      await invoke("cancel_export");
    } catch (e) {
      // The terminal state still arrives as `screen:exportCancelled` or, if
      // the export had already finished, as `screen:exported`.
      logWarning(`cancel_export failed: ${String(e)}`);
    }
  }

  /** The bar confirms first; this is the second click. The staged capture is
   * gone afterwards, so the window stops offering it rather than leaving Save
   * pointed at a sidecar that no longer exists.
   *
   * Close-THEN-discard, in that order, and never the reverse (fix round 1,
   * controller ruling): `discard_staged_capture` refuses a capture pinned to
   * a tutorial project, and every capture this window opens is pinned the
   * moment `EditorRoot` opens it (Task 15's own Behavior). `editorProject
   * .close("discardProject")` unpins AND removes the project — never the
   * recording itself, see that action's own doc — so the legacy discard that
   * follows can actually succeed.
   *
   * The close is gated on `sourceBase === base`, not merely `sessionId !==
   * null` (fix round 2): `editorProject` is a SINGLE-session store, so its
   * live session can belong to a DIFFERENT capture than the one being
   * discarded — e.g. capture A opened a real session, then a later
   * capture B's own session-open FAILED (the store never blanks on a
   * failed open, by design), and the legacy surface moved on to B anyway
   * because B's `load_staged_capture` succeeded independently. Guarding on
   * `sessionId !== null` alone closed, unpinned and REMOVED A's project as
   * a side effect of discarding B — a session this Discard click has
   * nothing to do with. When the store's session is for a different
   * capture (or none), this skips straight to `discard_staged_capture`:
   * Rust refuses a B that turns out to be pinned to some OTHER project
   * with its own clear message, which still surfaces through the same
   * catch block below — there is no second copy of that refusal to write.
   * Only attempted when a MATCHING session is really open: calling
   * `close()` on nothing is a documented no-op that would otherwise risk
   * reading a stale `lastError` left over from an unrelated earlier
   * failure. `close()` itself never throws — it reports a failure through
   * `lastError` instead (its own doc) — so a refused close (e.g. the
   * project is still open elsewhere) is read from there and stops this
   * function before the staged capture — the only copy of the recording —
   * is touched at all. */
  async function onDiscard() {
    const base = detail.value?.base;
    if (base === undefined) return;
    discardBusy.value = true;
    try {
      const editorProject = useEditorProjectStore();
      if (editorProject.sessionId !== null && editorProject.sourceBase === base) {
        await editorProject.close("discardProject");
        if (editorProject.lastError) {
          exportPhase.value = "failed";
          exportMessage.value = editorProject.lastError.message;
          return;
        }
      }
      await invoke("discard_staged_capture", { base });
      onDiscarded();
      resetExport();
    } catch (e) {
      exportPhase.value = "failed";
      exportMessage.value = String(e);
    } finally {
      discardBusy.value = false;
    }
  }

  async function onOpenSaved() {
    const path = savedPath.value;
    const id = savedVaultId.value;
    if (path === null || id === null) return;
    try {
      await invoke("open_screen_capture", { id, path });
    } catch (e) {
      exportPhase.value = "failed";
      exportMessage.value = String(e);
    }
  }

  /** The four export subscriptions, awaited in order. The caller owns the
   * returned unlisteners and their unmount, beside its own `editor:open` —
   * see the root's `onMounted` for why they are subscribed before the first
   * drain. */
  async function listenExportEvents(): Promise<UnlistenFn[]> {
    return [
      await listen<ExportProgress>("screen:exportProgress", (e) => onExportProgress(e.payload)),
      await listen<ExportResult>("screen:exported", (e) => onExported(e.payload)),
      await listen<{ base: string }>("screen:exportCancelled", (e) => onExportCancelled(e.payload)),
      await listen<ExportFailure>("screen:exportFailed", (e) => onExportFailed(e.payload)),
    ];
  }

  return {
    exportPhase,
    exportFraction,
    exportMessage,
    discardBusy,
    resetExport,
    onSave,
    onCancelExport,
    onDiscard,
    onOpenSaved,
    listenExportEvents,
  };
}
