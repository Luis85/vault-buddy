import { defineStore } from "pinia";
import { getCharacter } from "../characters";

const ANIMATIONS_KEY = "vault-buddy.animations";
const CHARACTER_KEY = "vault-buddy.character";

export const useSettingsStore = defineStore("settings", {
  state: () => ({
    animationsEnabled: localStorage.getItem(ANIMATIONS_KEY) !== "off",
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
    setCharacter(id: string) {
      this.character = getCharacter(id).id;
      localStorage.setItem(CHARACTER_KEY, this.character);
    },
  },
});
