import type { Component } from "vue";

import BubbleRoot from "./BubbleRoot.vue";
import BuddyRoot from "./BuddyRoot.vue";
import PanelRoot from "./PanelRoot.vue";
import RegionRoot from "./RegionRoot.vue";

/** Which root component a given window label renders. */
export function rootFor(label: string): Component {
  if (label === "panel") return PanelRoot;
  if (label === "bubble") return BubbleRoot;
  if (label === "overlay") return RegionRoot;
  return BuddyRoot; // "main" and any unexpected label
}
