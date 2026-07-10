import { describe, expect, it } from "vitest";

import {
  CHARACTERS,
  DEFAULT_CHARACTER_ID,
  getCharacter,
} from "../src/characters";

describe("character registry", () => {
  it("puts the classic buddy first as the default", () => {
    expect(CHARACTERS[0].id).toBe("classic");
    expect(CHARACTERS[0].sprite).toBeNull();
    expect(DEFAULT_CHARACTER_ID).toBe("classic");
  });

  it("offers animated sprite characters alongside the classic one", () => {
    const sprites = CHARACTERS.filter((c) => c.sprite !== null);
    expect(sprites.length).toBeGreaterThanOrEqual(4);
    for (const c of sprites) {
      // every sprite character has both an idle and a working animation
      expect(c.sprite?.idle).toBeTruthy();
      expect(c.sprite?.run).toBeTruthy();
      expect(c.sprite?.idle).not.toBe(c.sprite?.run);
    }
  });

  it("has unique ids and human-readable names", () => {
    const ids = CHARACTERS.map((c) => c.id);
    expect(new Set(ids).size).toBe(ids.length);
    for (const c of CHARACTERS) expect(c.name).toBeTruthy();
  });

  it("resolves known ids and falls back to classic for unknown ones", () => {
    expect(getCharacter("knight").id).toBe("knight");
    expect(getCharacter("does-not-exist").id).toBe("classic");
    expect(getCharacter("").id).toBe("classic");
  });
});
