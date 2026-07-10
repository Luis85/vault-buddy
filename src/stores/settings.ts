import { defineStore } from "pinia";

import { getCharacter } from "../characters";

const ANIMATIONS_KEY = "vault-buddy.animations";
const CHARACTER_KEY = "vault-buddy.character";
const DRAGGING_KEY = "vault-buddy.dragging";
const MESSAGES_KEY = "vault-buddy.messages";
const MESSAGE_DURATION_KEY = "vault-buddy.messageDuration";
const CHECK_UPDATES_ON_START_KEY = "vault-buddy.checkUpdatesOnStart";

/** How long the buddy's speech bubbles stay up (the ms tiers live in
 * useBuddyBubble's BUBBLE_MS map). */
export type MessageDuration = "short" | "normal" | "long";

// unknown/stale stored values fall back to normal — the getCharacter pattern
function normalizeDuration(value: string | null): MessageDuration {
  return value === "short" || value === "long" ? value : "normal";
}

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
    // the buddy's spoken acknowledgements (open vault/note, recording +
    // transcription progress); on by default
    buddyMessagesEnabled: localStorage.getItem(MESSAGES_KEY) !== "off",
    // how long bubbles stay up; "normal" preserves the pre-setting timings
    messageDuration: normalizeDuration(
      localStorage.getItem(MESSAGE_DURATION_KEY),
    ),
    // quiet update check at startup (metadata-only; installing always asks);
    // on by default — the toggle is the opt-out
    checkUpdatesOnStart:
      localStorage.getItem(CHECK_UPDATES_ON_START_KEY) !== "off",
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
    toggleBuddyMessages() {
      this.buddyMessagesEnabled = !this.buddyMessagesEnabled;
      localStorage.setItem(
        MESSAGES_KEY,
        this.buddyMessagesEnabled ? "on" : "off",
      );
    },
    setMessageDuration(duration: MessageDuration) {
      this.messageDuration = normalizeDuration(duration);
      localStorage.setItem(MESSAGE_DURATION_KEY, this.messageDuration);
    },
    toggleCheckUpdatesOnStart() {
      this.checkUpdatesOnStart = !this.checkUpdatesOnStart;
      localStorage.setItem(
        CHECK_UPDATES_ON_START_KEY,
        this.checkUpdatesOnStart ? "on" : "off",
      );
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
      this.buddyMessagesEnabled = localStorage.getItem(MESSAGES_KEY) !== "off";
      this.messageDuration = normalizeDuration(
        localStorage.getItem(MESSAGE_DURATION_KEY),
      );
      this.checkUpdatesOnStart =
        localStorage.getItem(CHECK_UPDATES_ON_START_KEY) !== "off";
    },
  },
});
