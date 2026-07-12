import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { useSettingsStatusStore } from "../src/stores/settingsStatus";

describe("settingsStatus store", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.runOnlyPendingTimers();
    vi.useRealTimers();
  });

  it("starts idle", () => {
    expect(useSettingsStatusStore().state).toBe("idle");
  });

  it("saving() sets saving and clears a prior error", () => {
    const s = useSettingsStatusStore();
    s.failed("boom");
    s.saving();
    expect(s.state).toBe("saving");
    expect(s.error).toBeNull();
  });

  it("saved() shows saved then fades to idle after 2s", () => {
    const s = useSettingsStatusStore();
    s.saving();
    s.saved();
    expect(s.state).toBe("saved");
    vi.advanceTimersByTime(1999);
    expect(s.state).toBe("saved");
    vi.advanceTimersByTime(1);
    expect(s.state).toBe("idle");
  });

  it("a new saving() cancels a pending saved fade", () => {
    const s = useSettingsStatusStore();
    s.saved();
    vi.advanceTimersByTime(1000);
    s.saving();
    vi.advanceTimersByTime(2000);
    expect(s.state).toBe("saving"); // the old fade timer was cancelled
  });

  it("failed() holds the error state (no auto-fade) with the message", () => {
    const s = useSettingsStatusStore();
    s.failed("disk full");
    vi.advanceTimersByTime(5000);
    expect(s.state).toBe("error");
    expect(s.error).toBe("disk full");
  });

  it("reset() returns to idle and clears the error", () => {
    const s = useSettingsStatusStore();
    s.failed("boom");
    s.reset();
    expect(s.state).toBe("idle");
    expect(s.error).toBeNull();
  });
});
