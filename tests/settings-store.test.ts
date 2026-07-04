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

  it("enables dragging by default", () => {
    expect(useSettingsStore().draggingEnabled).toBe(true);
  });

  it("persists the dragging toggle across store instances", () => {
    useSettingsStore().toggleDragging();
    setActivePinia(createPinia());
    expect(useSettingsStore().draggingEnabled).toBe(false);
  });

  it("faces right by default", () => {
    expect(useSettingsStore().facing).toBe("right");
  });

  it("persists the view direction across store instances", () => {
    useSettingsStore().setFacing("left");
    setActivePinia(createPinia());
    expect(useSettingsStore().facing).toBe("left");
  });

  it("falls back to right for an unknown stored view direction", () => {
    localStorage.setItem("vault-buddy.facing", "up");
    expect(useSettingsStore().facing).toBe("right");
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
