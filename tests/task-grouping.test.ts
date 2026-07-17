import { afterEach, describe, expect, it } from "vitest";

import { loadGrouping, saveGrouping } from "../src/utils/taskGrouping";

afterEach(() => localStorage.clear());

describe("taskGrouping", () => {
  it("defaults to lists and round-trips per view", () => {
    expect(loadGrouping("v1")).toBe("lists");
    saveGrouping("v1", "dates");
    saveGrouping("all", "tags");
    expect(loadGrouping("v1")).toBe("dates");
    expect(loadGrouping("all")).toBe("tags");
    expect(loadGrouping("v2")).toBe("lists"); // unset
  });
  it("degrades a corrupt value to lists", () => {
    localStorage.setItem("vault-buddy:task-grouping", "not json");
    expect(loadGrouping("v1")).toBe("lists");
  });
});
