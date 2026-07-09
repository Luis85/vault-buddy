import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  clearRecentSearches,
  loadRecentSearches,
  MAX_RECENT_SEARCHES,
  pushRecentSearch,
} from "../src/utils/recentSearches";

vi.mock("../src/logging", () => ({ logWarning: vi.fn() }));
import { logWarning } from "../src/logging";

describe("recentSearches", () => {
  beforeEach(() => {
    localStorage.clear();
    vi.clearAllMocks();
  });

  it("loads an empty list from empty storage", () => {
    expect(loadRecentSearches()).toEqual([]);
  });

  it("push adds to the front and returns the updated list", () => {
    expect(pushRecentSearch("alpha")).toEqual(["alpha"]);
    expect(pushRecentSearch("beta")).toEqual(["beta", "alpha"]);
    expect(loadRecentSearches()).toEqual(["beta", "alpha"]);
  });

  it("dedups case-insensitively keeping the latest casing", () => {
    pushRecentSearch("Alpha");
    pushRecentSearch("beta");
    expect(pushRecentSearch("ALPHA")).toEqual(["ALPHA", "beta"]);
  });

  it("caps the list at MAX_RECENT_SEARCHES", () => {
    for (const q of ["a1", "a2", "a3", "a4", "a5", "a6"]) pushRecentSearch(q);
    const list = loadRecentSearches();
    expect(list).toHaveLength(MAX_RECENT_SEARCHES);
    expect(list[0]).toBe("a6");
    expect(list).not.toContain("a1");
  });

  it("clear empties the list", () => {
    pushRecentSearch("alpha");
    clearRecentSearches();
    expect(loadRecentSearches()).toEqual([]);
  });

  it("corrupted storage degrades to an empty list with a warning", () => {
    // Failure mode: a hand-edited/corrupted value must not throw into the
    // component — degrade + warn (no swallowed errors).
    localStorage.setItem("vault-buddy:recent-searches", "{not json");
    expect(loadRecentSearches()).toEqual([]);
    expect(logWarning).toHaveBeenCalled();
  });

  it("non-array JSON degrades to an empty list", () => {
    localStorage.setItem("vault-buddy:recent-searches", '{"a":1}');
    expect(loadRecentSearches()).toEqual([]);
  });
});
