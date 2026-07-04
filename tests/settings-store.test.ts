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
});
