import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount } from "@vue/test-utils";
import { nextTick } from "vue";
import { createPinia, setActivePinia } from "pinia";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import Search from "../src/components/Search.vue";
import { useNotificationsStore } from "../src/stores/notifications";
import { noteOpenedMessage } from "../src/buddyMessages";
import type { SearchHit, SearchResponse } from "../src/types";

vi.mock("../src/logging", () => ({ logWarning: vi.fn(), logBreadcrumb: vi.fn() }));

const hit = (over: Partial<SearchHit> = {}): SearchHit => ({
  vaultId: "v1",
  vaultName: "Work",
  name: "idea",
  folder: "Notes",
  file: "Notes/idea",
  snippet: "Project Alpha kickoff",
  isNote: true,
  ...over,
});

const response = (hits: SearchHit[], truncated = false): SearchResponse => ({
  hits,
  truncated,
});

function mountSearch(
  handlers: Partial<Record<string, (args: unknown) => unknown>> = {},
) {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    if (handlers[cmd]) return handlers[cmd]!(args);
    if (cmd === "search_vaults") return response([hit()]);
  });
  const wrapper = mount(Search);
  return { wrapper, calls };
}

async function type(wrapper: ReturnType<typeof mount>, text: string) {
  await wrapper.get('[data-testid="search-input"]').setValue(text);
  await vi.advanceTimersByTimeAsync(300);
  await nextTick();
}

describe("Search", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
    clearMocks();
  });

  it("debounces: no call until 300ms after the last keystroke", async () => {
    const { wrapper, calls } = mountSearch();
    await wrapper.get('[data-testid="search-input"]').setValue("alp");
    await vi.advanceTimersByTimeAsync(200);
    expect(calls.filter((c) => c.cmd === "search_vaults")).toHaveLength(0);
    await wrapper.get('[data-testid="search-input"]').setValue("alpha");
    await vi.advanceTimersByTimeAsync(300);
    await nextTick();
    const searches = calls.filter((c) => c.cmd === "search_vaults");
    expect(searches).toHaveLength(1); // the "alp" timer was superseded
    expect(searches[0].args).toEqual({ query: "alpha" });
  });

  it("never searches queries under 2 trimmed characters", async () => {
    const { wrapper, calls } = mountSearch();
    await type(wrapper, " a ");
    expect(calls.filter((c) => c.cmd === "search_vaults")).toHaveLength(0);
    expect(wrapper.text()).toContain("Type at least 2 characters");
  });

  it("shows the too-short hint for a single emoji instead of searching", async () => {
    // Regression: '😀'.length === 2 (UTF-16) passed the old gate while the
    // backend counts chars and refused it — the UI then claimed "No matches".
    const { wrapper, calls } = mountSearch();
    await type(wrapper, "😀");
    expect(calls.filter((c) => c.cmd === "search_vaults")).toHaveLength(0);
    expect(wrapper.text()).toContain("Type at least 2 characters");
  });

  it("renders hits grouped under their vault name with the snippet", async () => {
    const { wrapper } = mountSearch({
      search_vaults: () =>
        response([
          hit(),
          hit({
            vaultId: "v2",
            vaultName: "Personal",
            name: "alpha deck.pdf",
            folder: "",
            file: "alpha deck.pdf",
            snippet: null,
            isNote: false,
          }),
        ]),
    });
    await type(wrapper, "alpha");
    const text = wrapper.text();
    expect(text).toContain("Work");
    expect(text).toContain("Personal");
    expect(text).toContain("idea");
    expect(text).toContain("Project Alpha kickoff");
    expect(wrapper.findAll('[data-testid="search-hit"]')).toHaveLength(2);
  });

  it("drops a stale response: an older slow search cannot clobber newer results", async () => {
    // Failure mode: without a request ticket, the "alp" response landing
    // AFTER the "alpha" response would overwrite the newer results.
    const pending: Array<{
      query: string;
      resolve: (r: SearchResponse) => void;
    }> = [];
    const { wrapper } = mountSearch({
      search_vaults: (args) =>
        new Promise<SearchResponse>((resolve) => {
          pending.push({ query: (args as { query: string }).query, resolve });
        }),
    });
    await type(wrapper, "alp");
    await type(wrapper, "alpha");
    expect(pending.map((p) => p.query)).toEqual(["alp", "alpha"]);
    pending[1].resolve(response([hit({ name: "fresh" })]));
    await vi.advanceTimersByTimeAsync(0);
    await nextTick();
    pending[0].resolve(response([hit({ name: "stale" })]));
    await vi.advanceTimersByTimeAsync(0);
    await nextTick();
    expect(wrapper.text()).toContain("fresh");
    expect(wrapper.text()).not.toContain("stale");
  });

  it("opens a hit: open_search_result + announce + close_panel", async () => {
    const { wrapper, calls } = mountSearch();
    await type(wrapper, "alpha");
    await wrapper.get('[data-testid="search-hit"]').trigger("click");
    await vi.advanceTimersByTimeAsync(0);
    expect(calls.find((c) => c.cmd === "open_search_result")).toEqual({
      cmd: "open_search_result",
      args: { id: "v1", file: "Notes/idea" },
    });
    expect(calls.find((c) => c.cmd === "announce")).toEqual({
      cmd: "announce",
      args: { text: noteOpenedMessage("idea") },
    });
    expect(calls.some((c) => c.cmd === "close_panel")).toBe(true);
  });

  it("keeps the panel open and notifies when opening fails", async () => {
    const notifications = useNotificationsStore();
    const { wrapper, calls } = mountSearch({
      open_search_result: () => {
        throw new Error("vault not found");
      },
    });
    await type(wrapper, "alpha");
    await wrapper.get('[data-testid="search-hit"]').trigger("click");
    await vi.advanceTimersByTimeAsync(0);
    expect(calls.some((c) => c.cmd === "close_panel")).toBe(false);
    expect(notifications.items.some((n) => n.kind === "error")).toBe(true);
  });

  it("keeps previous results and shows a banner when a search fails", async () => {
    // A live refinement that errors must not blank a working result list.
    let fail = false;
    const { wrapper } = mountSearch({
      search_vaults: () => {
        if (fail) throw new Error("scan failed");
        return response([hit()]);
      },
    });
    await type(wrapper, "alpha");
    expect(wrapper.text()).toContain("idea");
    fail = true;
    await type(wrapper, "alphab");
    expect(wrapper.text()).toContain("idea"); // previous results kept
    expect(wrapper.text()).toContain("scan failed");
  });

  it("shows the truncation footer when the backend capped results", async () => {
    const { wrapper } = mountSearch({
      search_vaults: () => response([hit()], true),
    });
    await type(wrapper, "alpha");
    expect(wrapper.find('[data-testid="search-truncated"]').exists()).toBe(true);
  });

  it("shows the empty state for a query with no matches", async () => {
    const { wrapper } = mountSearch({ search_vaults: () => response([]) });
    await type(wrapper, "zzz");
    expect(wrapper.text()).toContain('No matches for "zzz"');
  });

  it("Escape clears the query first instead of bubbling", async () => {
    const { wrapper } = mountSearch();
    const input = wrapper.get('[data-testid="search-input"]');
    await input.setValue("alpha");
    const event = new KeyboardEvent("keydown", {
      key: "Escape",
      bubbles: true,
      cancelable: true,
    });
    const stop = vi.spyOn(event, "stopPropagation");
    input.element.dispatchEvent(event);
    await nextTick();
    expect((input.element as HTMLInputElement).value).toBe("");
    expect(stop).toHaveBeenCalled(); // second Escape (empty query) will bubble → PanelRoot closes
  });
});
