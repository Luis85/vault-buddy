import { defineStore } from "pinia";

const ANIMATIONS_KEY = "vault-buddy.animations";

export const useSettingsStore = defineStore("settings", {
  state: () => ({
    animationsEnabled: localStorage.getItem(ANIMATIONS_KEY) !== "off",
  }),
  actions: {
    toggleAnimations() {
      this.animationsEnabled = !this.animationsEnabled;
      localStorage.setItem(
        ANIMATIONS_KEY,
        this.animationsEnabled ? "on" : "off",
      );
    },
  },
});
