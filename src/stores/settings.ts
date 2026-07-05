import { defineStore } from "pinia";
import { getCharacter } from "../characters";

const ANIMATIONS_KEY = "vault-buddy.animations";
const CHARACTER_KEY = "vault-buddy.character";
const DRAGGING_KEY = "vault-buddy.dragging";

// The buddy's view direction is no longer a stored setting — it is derived from
// the buddy's screen position (it looks toward the centre) and pushed from Rust
// via the `buddy-facing` event; see BuddyRoot.vue.
export type Facing = "right" | "left";

export const useSettingsStore = defineStore("settings", {
  state: () => ({
    animationsEnabled: localStorage.getItem(ANIMATIONS_KEY) !== "off",
    // lets the user pin the buddy where it is — clicks still work
    draggingEnabled: localStorage.getItem(DRAGGING_KEY) !== "off",
    // getCharacter normalizes stale/unknown stored ids to the classic buddy
    character: getCharacter(localStorage.getItem(CHARACTER_KEY) ?? "").id,
  }),
  actions: {
    toggleAnimations() {
      this.animationsEnabled = !this.animationsEnabled;
      localStorage.setItem(
        ANIMATIONS_KEY,
        this.animationsEnabled ? "on" : "off",
      );
    },
    toggleDragging() {
      this.draggingEnabled = !this.draggingEnabled;
      localStorage.setItem(DRAGGING_KEY, this.draggingEnabled ? "on" : "off");
    },
    setCharacter(id: string) {
      this.character = getCharacter(id).id;
      localStorage.setItem(CHARACTER_KEY, this.character);
    },
    // re-reads the same keys the state initializer uses, so the buddy
    // window picks up settings changed in the panel window's settings view
    // (separate webviews sharing localStorage — see the `storage` listener
    // installed in BuddyRoot.vue).
    syncFromStorage() {
      this.animationsEnabled = localStorage.getItem(ANIMATIONS_KEY) !== "off";
      this.draggingEnabled = localStorage.getItem(DRAGGING_KEY) !== "off";
      this.character = getCharacter(
        localStorage.getItem(CHARACTER_KEY) ?? "",
      ).id;
    },
  },
});
