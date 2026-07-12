import { defineStore } from "pinia";

export type SaveState = "idle" | "saving" | "saved" | "error";

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

// The panel header's transient save indicator, shared across every
// auto-saving settings field so one indicator covers the whole view.
export const useSettingsStatusStore = defineStore("settingsStatus", {
  state: () => ({
    state: "idle" as SaveState,
    error: null as string | null,
  }),
  actions: {
    saving() {
      clearFade();
      this.state = "saving";
      this.error = null;
    },
    saved() {
      clearFade();
      this.state = "saved";
      this.error = null;
      fadeTimer = setTimeout(() => {
        // Only fade if nothing newer superseded us.
        if (this.state === "saved") this.state = "idle";
        fadeTimer = null;
      }, SAVED_LINGER_MS);
    },
    // Sticky until the next saving()/saved()/reset() — a failure the user isn't
    // looking at must not silently disappear.
    failed(message: string) {
      clearFade();
      this.state = "error";
      this.error = message;
    },
    reset() {
      clearFade();
      this.state = "idle";
      this.error = null;
    },
  },
});
