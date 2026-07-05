import type { Component } from "vue";
import BuddyRoot from "./BuddyRoot.vue";
import PanelRoot from "./PanelRoot.vue";
import BubbleRoot from "./BubbleRoot.vue";

/** Which root component a given window label renders. */
export function rootFor(label: string): Component {
  if (label === "panel") return PanelRoot;
  if (label === "bubble") return BubbleRoot;
  return BuddyRoot; // "main" and any unexpected label
}
