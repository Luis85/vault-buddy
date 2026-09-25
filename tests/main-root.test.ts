import { describe, expect, it } from "vitest";

import { rootFor } from "../src/roots";
import BubbleRoot from "../src/roots/BubbleRoot.vue";
import BuddyRoot from "../src/roots/BuddyRoot.vue";
import EditorRoot from "../src/roots/EditorRoot.vue";
import PanelRoot from "../src/roots/PanelRoot.vue";
import RegionIndicatorRoot from "../src/roots/RegionIndicatorRoot.vue";
import RegionRoot from "../src/roots/RegionRoot.vue";

describe("rootFor", () => {
  it("maps window labels to root components", () => {
    expect(rootFor("main")).toBe(BuddyRoot);
    expect(rootFor("panel")).toBe(PanelRoot);
    expect(rootFor("bubble")).toBe(BubbleRoot);
    expect(rootFor("overlay")).toBe(RegionRoot);
  });
  it("mounts the editor root for the editor window", () => {
    expect(rootFor("editor")).toBe(EditorRoot);
  });
  it("mounts the indicator root for the region-indicator window", () => {
    expect(rootFor("region-indicator")).toBe(RegionIndicatorRoot);
  });
  it("defaults an unknown label to the buddy", () => {
    expect(rootFor("whatever")).toBe(BuddyRoot);
  });
});
