import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { useSettingsStatusStore } from "../src/stores/settingsStatus";

// Each auto-saving field reports under its own owner id, so the shared header
// can represent several concurrently-mounted savers (the v-show Vault tabs)
// without one field's success clearing another field's outstanding error.
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

  it("saving() shows saving", () => {
    const s = useSettingsStatusStore();
    s.saving(1);
    expect(s.state).toBe("saving");
  });

  it("saved() shows saved then fades to idle after 2s", () => {
    const s = useSettingsStatusStore();
    s.saving(1);
    s.saved(1);
    expect(s.state).toBe("saved");
    vi.advanceTimersByTime(1999);
    expect(s.state).toBe("saved");
    vi.advanceTimersByTime(1);
    expect(s.state).toBe("idle");
  });

  it("failed() holds the error state (no auto-fade) with the message", () => {
    const s = useSettingsStatusStore();
    s.saving(1);
    s.failed(1, "disk full");
    vi.advanceTimersByTime(5000);
    expect(s.state).toBe("error");
    expect(s.error).toBe("disk full");
  });

  it("a success from a DIFFERENT owner does not clear another owner's error", () => {
    const s = useSettingsStatusStore();
    s.saving(1);
    s.failed(1, "bad folder"); // owner 1 failed
    s.saving(2);
    s.saved(2); // owner 2 succeeded just after
    expect(s.state).toBe("error");
    expect(s.error).toBe("bad folder");
  });

  it("the failing owner's own retry-success clears the error", () => {
    const s = useSettingsStatusStore();
    s.saving(1);
    s.failed(1, "bad folder");
    s.saving(1); // retry
    s.saved(1);
    expect(s.state).toBe("saved");
    expect(s.error).toBeNull();
  });

  it("a new saving() cancels a pending saved fade", () => {
    const s = useSettingsStatusStore();
    s.saving(1);
    s.saved(1);
    vi.advanceTimersByTime(1000);
    s.saving(2);
    vi.advanceTimersByTime(2000);
    expect(s.state).toBe("saving"); // the old fade timer was cancelled
  });

  it("reset() returns to idle and clears every outstanding error", () => {
    const s = useSettingsStatusStore();
    s.saving(1);
    s.failed(1, "boom");
    s.reset();
    expect(s.state).toBe("idle");
    expect(s.error).toBeNull();
  });
});
