import type { Component } from "vue";

import BubbleRoot from "./BubbleRoot.vue";
import BuddyRoot from "./BuddyRoot.vue";
import EditorRoot from "./EditorRoot.vue";
import PanelRoot from "./PanelRoot.vue";
import RegionIndicatorRoot from "./RegionIndicatorRoot.vue";
import RegionRoot from "./RegionRoot.vue";

/** Which root component a given window label renders. */
export function rootFor(label: string): Component {
  if (label === "panel") return PanelRoot;
  if (label === "bubble") return BubbleRoot;
  if (label === "overlay") return RegionRoot;
  if (label === "editor") return EditorRoot;
  if (label === "region-indicator") return RegionIndicatorRoot;
  return BuddyRoot; // "main" and any unexpected label
}
