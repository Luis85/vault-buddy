import { beforeEach, describe, expect, it } from "vitest";

import { loadFavorites, toggleFavorite } from "../src/utils/favoriteVaults";

describe("favoriteVaults", () => {
  beforeEach(() => localStorage.clear());

  it("starts empty and toggles on/off, persisting", () => {
    expect(loadFavorites()).toEqual([]);
    expect(toggleFavorite("v1")).toEqual(["v1"]);
    expect(loadFavorites()).toEqual(["v1"]);
    expect(toggleFavorite("v1")).toEqual([]);
    expect(loadFavorites()).toEqual([]);
  });

  it("degrades to empty on corrupt storage", () => {
    localStorage.setItem("vault-buddy:favorite-vaults", "{not json");
    expect(loadFavorites()).toEqual([]);
  });

  it("degrades to empty on valid but non-array JSON", () => {
    // recentSearches.ts precedent: a hand-edited value that parses but isn't
    // the expected shape must degrade, not throw into the component.
    localStorage.setItem("vault-buddy:favorite-vaults", '{"a":1}');
    expect(loadFavorites()).toEqual([]);
  });
});
