import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { onMounted, type Ref,ref } from "vue";

import { logWarning } from "../logging";
import { withDialogSuppressed } from "../utils/nativeDialog";

// The settings-card half of an external-tool integration, shared by the two
// cards that have one: Pandoc (document import) and ffmpeg (the screen-capture
// export). Both tools are USER-INSTALLED and detected, never bundled, so both
// cards do the identical four things — probe on mount, Recheck, persist a path
// override, and Browse for the executable — around a detection status whose
// only common field is the override the card seeds itself from.
//
// It exists because the second card was written by copying the first, which is
// how the repo's clone gate found it. Rust made the same move one layer down:
// `external_tool.rs` is the tool-agnostic half of the Pandoc process
// machinery, which `ffmpeg.rs` consumes rather than re-implementing.

/** The one field every tool status must carry: the raw configured override
 * (null → resolving through PATH), so the card seeds its input without a
 * second command. */
export interface ExternalToolStatus {
  configuredPath: string | null;
}

/** The cached-probe store half (`usePandocStore` / `useFfmpegStore`). The card
 * runs its OWN probe, so it claims a token at probe start and writes through
 * at the end — a probe that resolves after a newer one holds a stale token and
 * is dropped rather than clobbering the fresher result. */
export interface ExternalToolProbeStore<S> {
  beginProbe: () => number;
  markDetected: (status: S, token: number) => void;
}

export interface ExternalToolOptions<S extends ExternalToolStatus> {
  /** The IPC detect command (`detect_pandoc` / `detect_ffmpeg`). */
  detectCommand: string;
  /** The IPC override setter, and the single argument it takes. */
  setPathCommand: string;
  setPathArg: string;
  /** Label for the native picker's filter. */
  filterName: string;
  /** The tool's install page, opened in the OS browser through the logged
   * `open_external_url` command — a raw target="_blank" in a Tauri v2 webview
   * either no-ops or replaces the app UI. */
  installUrl: string;
  /** Prefix for this card's log breadcrumbs. */
  label: string;
  /** The shared cache the card writes its probe through to. */
  store: ExternalToolProbeStore<S>;
}

export interface ExternalToolCard<S extends ExternalToolStatus> {
  status: Ref<S | null>;
  pathOverride: Ref<string>;
  error: Ref<string | null>;
  /** Single in-flight guard shared by recheck/savePath/browse — two concurrent
   * detect/save calls could otherwise land out of order and leave a stale
   * status showing. */
  saving: Ref<boolean>;
  /** Mark the override field user-touched (the `@input` handler). */
  markDirty: () => void;
  recheck: () => Promise<void>;
  savePath: () => Promise<void>;
  browse: () => Promise<void>;
  openInstall: () => Promise<void>;
}

export function useExternalTool<S extends ExternalToolStatus>(
  opts: ExternalToolOptions<S>,
): ExternalToolCard<S> {
  const status = ref(null) as Ref<S | null>;
  const pathOverride = ref("");
  // Set once the user has touched the override field. The input stays enabled
  // while the initial detect() is in flight, so a user who types during a slow
  // probe must not have their edit clobbered by the on-mount seed below.
  const dirtied = ref(false);
  const error = ref<string | null>(null);
  const saving = ref(false);

  // Monotonic ticket so out-of-order detect responses can't regress the
  // status: a slow initial probe must not overwrite the fresher result of a
  // save/browse re-detect that resolved first.
  let detectTicket = 0;

  async function detect() {
    const ticket = ++detectTicket;
    // Claim the store's probe token at the START, not at resolution: if the
    // user leaves settings and an intake surface runs a newer probe, this
    // one's token goes stale and markDetected drops the write-through instead
    // of clobbering the fresher result.
    const token = opts.store.beginProbe();
    try {
      const s = await invoke<S>(opts.detectCommand);
      if (ticket === detectTicket) {
        status.value = s;
        // Keep the shared intake cache fresh after a settings-side probe, so a
        // fix made here is reflected where the tool is consulted — but only
        // while this probe is still the newest across the store.
        opts.store.markDetected(s, token);
      }
    } catch (e) {
      // Not running under Tauri (unit tests) or an IPC failure — leave the
      // card rendered: its error line, Recheck and override are the exact
      // recovery affordances, so hiding them would strand a user whose probe
      // is what broke.
      if (ticket === detectTicket) error.value = String(e);
      logWarning(`${opts.label}: ${opts.detectCommand} failed: ${String(e)}`);
    }
  }

  onMounted(async () => {
    await detect();
    // Seed the override field from the resolved status, not a second command —
    // but never over a value the user already typed while detect was in flight.
    if (!dirtied.value) {
      pathOverride.value = status.value?.configuredPath ?? "";
    }
  });

  async function recheck() {
    if (saving.value) return;
    saving.value = true;
    error.value = null;
    try {
      await detect();
    } finally {
      saving.value = false;
    }
  }

  async function savePath() {
    if (saving.value) return;
    saving.value = true;
    error.value = null;
    try {
      const trimmed = pathOverride.value.trim();
      // null, never "": an empty override means "use PATH", which is how Rust
      // stores it too — "" would be a second spelling of the same state.
      await invoke(opts.setPathCommand, { [opts.setPathArg]: trimmed || null });
      // Re-detect so the new (or cleared) override resolves immediately — the
      // card must not keep showing the pre-save status.
      await detect();
    } catch (e) {
      error.value = String(e);
      logWarning(`${opts.label}: ${opts.setPathCommand} failed: ${String(e)}`);
    } finally {
      saving.value = false;
    }
  }

  async function browse() {
    if (saving.value) return;
    saving.value = true;
    error.value = null;
    try {
      // withDialogSuppressed keeps the panel's focus-out auto-hide from firing
      // while the OS picker holds focus.
      const selected = await withDialogSuppressed(() =>
        open({ multiple: false, filters: [{ name: opts.filterName, extensions: ["exe", ""] }] }),
      );
      if (typeof selected === "string") {
        // Browse assigns programmatically (no @input fires), so mark the field
        // dirty explicitly — otherwise a still-pending initial detect could
        // reseed pathOverride from the old configuredPath and savePath would
        // persist the stale value instead of the picked executable.
        dirtied.value = true;
        pathOverride.value = selected;
        // savePath() self-guards on `saving`; release it first so its own
        // setter + re-detect run, then `finally` restores the guard.
        saving.value = false;
        await savePath();
      }
    } catch (e) {
      error.value = String(e);
      logWarning(`${opts.label}: browse failed: ${String(e)}`);
    } finally {
      saving.value = false;
    }
  }

  // A failed launch only warns: the URL is visible in the card's href for a
  // right-click copy, so there is nothing for an error line to add.
  async function openInstall() {
    try {
      await invoke("open_external_url", { url: opts.installUrl });
    } catch (e) {
      logWarning(`${opts.label}: open_external_url failed: ${String(e)}`);
    }
  }

  return {
    status,
    pathOverride,
    error,
    saving,
    markDirty: () => {
      dirtied.value = true;
    },
    recheck,
    savePath,
    browse,
    openInstall,
  };
}
