import { beforeEach, describe, expect, it } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import { useSettingsStore } from "../src/stores/settings";

describe("settings store", () => {
  beforeEach(() => {
    localStorage.clear();
    setActivePinia(createPinia());
  });

  it("enables animations by default", () => {
    expect(useSettingsStore().animationsEnabled).toBe(true);
  });

  it("persists the toggle across store instances", () => {
    useSettingsStore().toggleAnimations();
    // a fresh pinia simulates an app restart reading localStorage
    setActivePinia(createPinia());
    expect(useSettingsStore().animationsEnabled).toBe(false);
  });

  it("toggles back on and persists that too", () => {
    const store = useSettingsStore();
    store.toggleAnimations();
    store.toggleAnimations();
    setActivePinia(createPinia());
    expect(useSettingsStore().animationsEnabled).toBe(true);
  });

  it("uses the classic buddy by default", () => {
    expect(useSettingsStore().character).toBe("classic");
  });

  it("persists the chosen character across store instances", () => {
    useSettingsStore().setCharacter("knight");
    setActivePinia(createPinia());
    expect(useSettingsStore().character).toBe("knight");
  });

  it("falls back to classic for an unknown stored character", () => {
    localStorage.setItem("vault-buddy.character", "retired-hero");
    expect(useSettingsStore().character).toBe("classic");
  });

  it("normalizes unknown ids passed to setCharacter", () => {
    const store = useSettingsStore();
    store.setCharacter("nope");
    expect(store.character).toBe("classic");
    expect(localStorage.getItem("vault-buddy.character")).toBe("classic");
  });
});
