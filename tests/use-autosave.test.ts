import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { defineComponent, h } from "vue";

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

  it("one field's success does not clear another field's error in the shared status (Codex #55)", async () => {
    // Two auto-saving fields report to the same header. A failing save on one
    // must stay visible even when a different field saves successfully just
    // after (its inline error may be on a now-hidden tab).
    const status = useSettingsStatusStore();
    const failing = useAutosave(vi.fn().mockRejectedValue("bad folder"));
    const ok = useAutosave(vi.fn().mockResolvedValue(undefined));
    failing.saveNow();
    await flushPromises();
    expect(status.state).toBe("error");
    ok.saveNow();
    await flushPromises();
    expect(status.state).toBe("error"); // still error, not "saved"
    expect(status.error).toBe("bad folder");
  });

  it("ignores a save completion after its component unmounts (Codex #55)", async () => {
    // On a slow vault a save can still be awaiting when the user navigates away.
    // Once the component unmounts (owner retired), its late saved()/failed()
    // must NOT strand a stale "Saved ✓"/"Couldn't save" in a different view's
    // header, and its in-flight marker must be dropped so nothing sticks on
    // "Saving…".
    const status = useSettingsStatusStore();
    let resolveSave!: () => void;
    const save = vi.fn().mockImplementation(() => new Promise<void>((r) => (resolveSave = r)));
    let api!: ReturnType<typeof useAutosave>;
    const Comp = defineComponent({
      setup() {
        api = useAutosave(save);
        return () => h("div");
      },
    });
    const wrapper = mount(Comp);
    api.saveNow(); // slow save starts and hangs
    expect(status.state).toBe("saving");
    wrapper.unmount(); // component gone → owner retired + released
    expect(status.state).toBe("idle"); // in-flight marker dropped
    resolveSave(); // the save finally settles, after unmount
    await flushPromises();
    expect(status.state).toBe("idle"); // no stale report leaked into the header
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
