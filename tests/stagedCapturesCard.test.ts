import { mockIPC } from "@tauri-apps/api/mocks";
import { flushPromises, mount } from "@vue/test-utils";
import { afterEach, describe, expect, it, vi } from "vitest";

import StagedCapturesCard from "../src/components/StagedCapturesCard.vue";
import { formatBytes } from "../src/utils/formatBytes";

let active: ReturnType<typeof mount> | null = null;
afterEach(() => {
  active?.unmount();
  active = null;
  vi.clearAllMocks();
});

type ClearReply = {
  cleared: number;
  bytesFreed: number;
  skippedPinned: number;
  failed: number;
};

function mountWith(
  usage: { captures: number; bytes: number },
  clear: ClearReply | Error = { cleared: 0, bytesFreed: 0, skippedPinned: 0, failed: 0 },
) {
  const calls: string[] = [];
  let current = usage;
  mockIPC((cmd) => {
    calls.push(cmd);
    if (cmd === "staging_usage") return current;
    if (cmd === "clear_staged_captures") {
      if (clear instanceof Error) throw clear;
      current = { captures: usage.captures - clear.cleared, bytes: usage.bytes - clear.bytesFreed };
      return clear;
    }
    return null;
  });
  active = mount(StagedCapturesCard, { attachTo: document.body });
  return { wrapper: active, calls };
}

async function arm(wrapper: ReturnType<typeof mount>) {
  await wrapper.get('[data-testid="staging-clear"]').trigger("click");
  await flushPromises();
}

describe("StagedCapturesCard", () => {
  it("reports how many captures are staged and what they weigh", async () => {
    const { wrapper } = mountWith({ captures: 3, bytes: 2_500_000_000 });
    await flushPromises();
    expect(wrapper.get('[data-testid="staging-summary"]').text()).toBe("3 captures · 2.33 GB");
  });

  it("says plainly when there is nothing staged, and offers no Clear to press", async () => {
    const { wrapper } = mountWith({ captures: 0, bytes: 0 });
    await flushPromises();
    expect(wrapper.get('[data-testid="staging-summary"]').text()).toBe("No staged captures");
    // Disabled rather than absent: the card still explains what staging is.
    expect(wrapper.get('[data-testid="staging-clear"]').attributes("disabled")).toBeDefined();
  });

  it("never clears on the first click — spec §10, nothing is deleted silently", async () => {
    const { wrapper, calls } = mountWith({ captures: 2, bytes: 1_000_000 });
    await flushPromises();
    await arm(wrapper);
    expect(calls).not.toContain("clear_staged_captures");
    expect(wrapper.find('[data-testid="staging-clear-confirm"]').exists()).toBe(true);
  });

  it("can be backed out of once armed, so a stray click cannot delete a recording", async () => {
    const { wrapper, calls } = mountWith({ captures: 2, bytes: 1_000_000 });
    await flushPromises();
    await arm(wrapper);
    const cancel = wrapper.findAll("button").find((b) => b.text() === "Cancel");
    await cancel!.trigger("click");
    await flushPromises();
    expect(calls).not.toContain("clear_staged_captures");
    expect(wrapper.find('[data-testid="staging-clear"]').exists()).toBe(true);
  });

  it("clears on the confirm and re-reads what is left", async () => {
    const { wrapper, calls } = mountWith(
      { captures: 2, bytes: 1_048_576 },
      { cleared: 2, bytesFreed: 1_048_576, skippedPinned: 0, failed: 0 },
    );
    await flushPromises();
    await arm(wrapper);
    await wrapper.get('[data-testid="staging-clear-confirm"]').trigger("click");
    await flushPromises();
    expect(calls).toContain("clear_staged_captures");
    expect(wrapper.get('[data-testid="staging-summary"]').text()).toBe("No staged captures");
    // The readout is re-taken from Rust rather than decremented locally.
    expect(calls.filter((c) => c === "staging_usage").length).toBe(2);
  });

  it("does NOT claim a refused capture was deleted", async () => {
    // The whole reason clear_staged_captures returns more than a success. A
    // capture refused because its leaf is a symlink is not a success and
    // must not read as one.
    const { wrapper } = mountWith(
      { captures: 3, bytes: 3_000_000 },
      { cleared: 2, bytesFreed: 2_000_000, skippedPinned: 0, failed: 1 },
    );
    await flushPromises();
    await arm(wrapper);
    await wrapper.get('[data-testid="staging-clear-confirm"]').trigger("click");
    await flushPromises();
    const text = wrapper.text();
    expect(text).toContain("Cleared 2");
    expect(text).toContain("1 could not be removed");
    // Task 59: nothing is "being saved" any more — the export is retired.
    expect(text).not.toContain("being saved");
  });

  // R6: a capture pinned to a tutorial project is left alone, and the row
  // must say why — kept, not merely skipped — so the user knows to discard
  // the project first.
  it("says a pinned capture was kept, not merely skipped", async () => {
    const { wrapper } = mountWith(
      { captures: 3, bytes: 3_000_000 },
      { cleared: 2, bytesFreed: 2_000_000, skippedPinned: 1, failed: 0 },
    );
    await flushPromises();
    await arm(wrapper);
    await wrapper.get('[data-testid="staging-clear-confirm"]').trigger("click");
    await flushPromises();
    const text = wrapper.text();
    expect(text).toContain("Cleared 2");
    expect(text).toContain("1 kept (used by a tutorial project)");
    expect(text).not.toContain("left alone (being saved)");
  });

  it("surfaces a rejected clear instead of reporting success", async () => {
    const { wrapper } = mountWith({ captures: 1, bytes: 10_000 }, new Error("staging is unreachable"));
    await flushPromises();
    await arm(wrapper);
    await wrapper.get('[data-testid="staging-clear-confirm"]').trigger("click");
    await flushPromises();
    expect(wrapper.text()).toContain("staging is unreachable");
    expect(wrapper.text()).not.toContain("Cleared");
  });
});

describe("formatBytes", () => {
  it("spans the whole range staging can actually hold", () => {
    // Binary units, matching what File Explorer shows for this same folder.
    expect(formatBytes(0)).toBe("0 KB");
    expect(formatBytes(900)).toBe("1 KB");
    expect(formatBytes(1024 * 700)).toBe("700 KB");
    expect(formatBytes(1024 * 1024 * 5)).toBe("5 MB");
    expect(formatBytes(1024 ** 3 * 2.5)).toBe("2.50 GB");
  });

  it("never renders a negative or non-finite size", () => {
    // `bytes` crosses IPC as a u64, but a degraded reading can be absent and
    // a subtraction upstream could go negative; "-1 KB" would read as a bug.
    expect(formatBytes(-5)).toBe("0 KB");
    expect(formatBytes(Number.NaN)).toBe("0 KB");
  });
});
