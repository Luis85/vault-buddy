import { describe, expect, it } from "vitest";

import { rootFor } from "../src/roots";
import BubbleRoot from "../src/roots/BubbleRoot.vue";
import BuddyRoot from "../src/roots/BuddyRoot.vue";
import PanelRoot from "../src/roots/PanelRoot.vue";

describe("rootFor", () => {
  it("maps window labels to root components", () => {
    expect(rootFor("main")).toBe(BuddyRoot);
    expect(rootFor("panel")).toBe(PanelRoot);
    expect(rootFor("bubble")).toBe(BubbleRoot);
  });
  it("defaults an unknown label to the buddy", () => {
    expect(rootFor("whatever")).toBe(BuddyRoot);
  });
});
