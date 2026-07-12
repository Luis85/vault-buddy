import { defineStore } from "pinia";

type SaveState = "idle" | "saving" | "saved" | "error";

// How long the "Saved" acknowledgement lingers before fading to idle.
const SAVED_LINGER_MS = 2000;

// Module-scoped (not reactive state) so storing a timer handle never trips
// Pinia's reactivity — the same module-constant idiom the settings store uses.
let fadeTimer: ReturnType<typeof setTimeout> | null = null;
function clearFade() {
  if (fadeTimer !== null) {
    clearTimeout(fadeTimer);
    fadeTimer = null;
  }
}

// The panel header's transient save indicator, shared across every auto-saving
// settings field so one indicator covers the whole view. Because the Vault
// settings tabs stay mounted (v-show), several useAutosave instances report
// here at once, so in-flight saves AND failures are tracked PER OWNER: a
// success from one field can't clear another field's still-unresolved error,
// and an unmounted field can be retired cleanly (Codex PR #55). `state` and
// `error` are the public fields the header reads; the bookkeeping below derives
// them via recompute().
export const useSettingsStatusStore = defineStore("settingsStatus", {
  state: () => ({
    state: "idle" as SaveState,
    error: null as string | null,
    // In-flight saves and outstanding failures, each keyed by the reporting
    // useAutosave instance's owner id. Per-owner (not a global count) so
    // release() can drop exactly one owner's markers without a late completion
    // leaving a stranded "Saving…" or error.
    savingOwners: {} as Record<number, true>,
    errorsByOwner: {} as Record<number, string>,
    // Drives the transient "Saved ✓" once nothing is failing or in flight.
    savedFlash: false,
  }),
  actions: {
    // Priority: any outstanding error > a save in flight > a recent success >
    // idle. An unresolved failure therefore outranks a later unrelated success.
    recompute() {
      const messages = Object.values(this.errorsByOwner);
      if (messages.length > 0) {
        this.state = "error";
        this.error = messages[0];
        return;
      }
      this.error = null;
      if (Object.keys(this.savingOwners).length > 0) this.state = "saving";
      else this.state = this.savedFlash ? "saved" : "idle";
    },
    saving(owner: number) {
      clearFade();
      this.savingOwners[owner] = true;
      // A retry drops this owner's prior failure before it re-attempts.
      delete this.errorsByOwner[owner];
      this.savedFlash = false;
      this.recompute();
    },
    saved(owner: number) {
      clearFade();
      delete this.savingOwners[owner];
      delete this.errorsByOwner[owner];
      this.savedFlash = true;
      this.recompute();
      // Fade only when "Saved" is actually showing (not masked by another
      // owner's error or an in-flight save).
      if (this.state === "saved") {
        fadeTimer = setTimeout(() => {
          this.savedFlash = false;
          this.recompute();
          fadeTimer = null;
        }, SAVED_LINGER_MS);
      }
    },
    // Sticky until THIS owner's next saving()/saved() or a reset() — a failure
    // the user isn't looking at (its tab may be hidden) must not disappear
    // because a different field saved.
    failed(owner: number, message: string) {
      clearFade();
      delete this.savingOwners[owner];
      this.errorsByOwner[owner] = message;
      this.savedFlash = false;
      this.recompute();
    },
    // Retire an unmounted owner: drop its in-flight AND error markers so a
    // late-settling save can't strand status in the header (the component and
    // its inline error are gone). useAutosave also stops reporting for a retired
    // owner, so no later saved()/failed() re-adds a marker.
    release(owner: number) {
      delete this.savingOwners[owner];
      delete this.errorsByOwner[owner];
      this.recompute();
    },
    reset() {
      clearFade();
      this.savingOwners = {};
      this.errorsByOwner = {};
      this.savedFlash = false;
      this.recompute();
    },
  },
});
