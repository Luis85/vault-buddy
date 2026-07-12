import { flushPromises } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../src/logging", () => ({ logWarning: vi.fn(), logBreadcrumb: vi.fn() }));
import { useAutosave } from "../src/composables/useAutosave";
import { logWarning } from "../src/logging";
import { useSettingsStatusStore } from "../src/stores/settingsStatus";

beforeEach(() => {
  setActivePinia(createPinia());
  vi.useFakeTimers();
  (logWarning as ReturnType<typeof vi.fn>).mockClear();
});
afterEach(() => {
  vi.useRealTimers();
});

// useAutosave calls onBeforeUnmount, which warns without an active component;
// these unit tests exercise it outside a component, which is supported (the
// composable no-ops the lifecycle hook when there's no current instance).
describe("useAutosave", () => {
  it("schedule() debounces: one save after 600ms of quiet", async () => {
    const save = vi.fn().mockResolvedValue(undefined);
    const a = useAutosave(save);
    a.schedule();
    a.schedule();
    a.schedule();
    expect(save).not.toHaveBeenCalled();
    vi.advanceTimersByTime(600);
    await flushPromises();
    expect(save).toHaveBeenCalledTimes(1);
  });

  it("saveNow() saves immediately", async () => {
    const save = vi.fn().mockResolvedValue(undefined);
    const a = useAutosave(save);
    a.saveNow();
    await flushPromises();
    expect(save).toHaveBeenCalledTimes(1);
  });

  it("flush() runs a pending debounced save, and is a no-op when none pending", async () => {
    const save = vi.fn().mockResolvedValue(undefined);
    const a = useAutosave(save);
    a.flush(); // nothing scheduled → no-op
    await flushPromises();
    expect(save).toHaveBeenCalledTimes(0);
    a.schedule();
    a.flush(); // pending → runs now, no need to wait 600ms
    await flushPromises();
    expect(save).toHaveBeenCalledTimes(1);
  });

  it("reports saving then saved to the status store on success", async () => {
    const status = useSettingsStatusStore();
    const a = useAutosave(vi.fn().mockResolvedValue(undefined));
    a.saveNow();
    expect(status.state).toBe("saving");
    await flushPromises();
    expect(status.state).toBe("saved");
  });

  it("reports the error, sets error ref, and logs on failure", async () => {
    const status = useSettingsStatusStore();
    const a = useAutosave(vi.fn().mockRejectedValue("disk full"), { label: "docs" });
    a.saveNow();
    await flushPromises();
    expect(status.state).toBe("error");
    expect(a.error.value).toBe("disk full");
    expect(logWarning).toHaveBeenCalledWith(expect.stringContaining("docs autosave failed"));
  });

  it("coalesces a save requested mid-flight into exactly one trailing run", async () => {
    let resolveFirst!: () => void;
    const save = vi
      .fn()
      .mockImplementationOnce(() => new Promise<void>((r) => (resolveFirst = r)))
      .mockResolvedValue(undefined);
    const a = useAutosave(save);
    a.saveNow(); // first run starts, pending on resolveFirst
    a.saveNow(); // mid-flight #1
    a.saveNow(); // mid-flight #2 — both collapse into one trailing run
    expect(save).toHaveBeenCalledTimes(1);
    resolveFirst();
    await flushPromises();
    expect(save).toHaveBeenCalledTimes(2); // one trailing run, not three
  });
});
