import { defineStore } from "pinia";
import { getCharacter } from "../characters";

const ANIMATIONS_KEY = "vault-buddy.animations";
const CHARACTER_KEY = "vault-buddy.character";
const DRAGGING_KEY = "vault-buddy.dragging";
const FACING_KEY = "vault-buddy.facing";

export type Facing = "right" | "left";

export const useSettingsStore = defineStore("settings", {
  state: () => ({
    animationsEnabled: localStorage.getItem(ANIMATIONS_KEY) !== "off",
    // lets the user pin the buddy where it is — clicks still work
    draggingEnabled: localStorage.getItem(DRAGGING_KEY) !== "off",
    // the buddy's home view direction; sprites are drawn facing right
    facing: (localStorage.getItem(FACING_KEY) === "left"
      ? "left"
      : "right") as Facing,
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
    setFacing(facing: Facing) {
      this.facing = facing;
      localStorage.setItem(FACING_KEY, facing);
    },
    setCharacter(id: string) {
      this.character = getCharacter(id).id;
      localStorage.setItem(CHARACTER_KEY, this.character);
    },
  },
});
