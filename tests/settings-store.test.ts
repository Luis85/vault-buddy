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

  it("re-reads settings when localStorage changes in another window", () => {
    const store = useSettingsStore();
    expect(store.animationsEnabled).toBe(true);
    localStorage.setItem("vault-buddy.animations", "off");
    store.syncFromStorage();
    expect(store.animationsEnabled).toBe(false);
  });

  it("enables buddy messages by default", () => {
    expect(useSettingsStore().buddyMessagesEnabled).toBe(true);
  });

  it("persists the buddy-messages toggle across store instances", () => {
    useSettingsStore().toggleBuddyMessages();
    setActivePinia(createPinia());
    expect(useSettingsStore().buddyMessagesEnabled).toBe(false);
  });

  it("re-reads buddy messages when another window changes them", () => {
    const store = useSettingsStore();
    localStorage.setItem("vault-buddy.messages", "off");
    store.syncFromStorage();
    expect(store.buddyMessagesEnabled).toBe(false);
  });

  it("defaults message duration to normal", () => {
    expect(useSettingsStore().messageDuration).toBe("normal");
  });

  it("persists the message duration across store instances", () => {
    useSettingsStore().setMessageDuration("long");
    setActivePinia(createPinia());
    expect(useSettingsStore().messageDuration).toBe("long");
    expect(localStorage.getItem("vault-buddy.messageDuration")).toBe("long");
  });

  it("falls back to normal for an unknown stored duration", () => {
    localStorage.setItem("vault-buddy.messageDuration", "eternal");
    expect(useSettingsStore().messageDuration).toBe("normal");
  });

  it("re-reads the message duration when another window changes it", () => {
    const store = useSettingsStore();
    localStorage.setItem("vault-buddy.messageDuration", "short");
    store.syncFromStorage();
    expect(store.messageDuration).toBe("short");
  });

  it("checks for updates on start by default", () => {
    expect(useSettingsStore().checkUpdatesOnStart).toBe(true);
  });

  it("persists the check-on-start toggle across store instances", () => {
    useSettingsStore().toggleCheckUpdatesOnStart();
    setActivePinia(createPinia());
    expect(useSettingsStore().checkUpdatesOnStart).toBe(false);
    expect(localStorage.getItem("vault-buddy.checkUpdatesOnStart")).toBe("off");
  });

  it("re-reads the check-on-start toggle when another window changes it", () => {
    const store = useSettingsStore();
    localStorage.setItem("vault-buddy.checkUpdatesOnStart", "off");
    store.syncFromStorage();
    expect(store.checkUpdatesOnStart).toBe(false);
  });
});
