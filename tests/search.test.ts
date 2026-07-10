import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { nextTick } from "vue";

import { noteOpenedMessage } from "../src/buddyMessages";
import Search from "../src/components/Search.vue";
import { useNotificationsStore } from "../src/stores/notifications";
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
    localStorage.clear(); // recents persist per happy-dom context — isolate tests
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
      args: { id: "v1", file: "Notes/idea", keepOpen: false },
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

  it("ArrowDown/ArrowUp move the selection, clamped, and Enter opens the selected hit", async () => {
    const { wrapper, calls } = mountSearch({
      search_vaults: () =>
        response([hit(), hit({ name: "second", file: "Notes/second" })]),
    });
    await type(wrapper, "alpha");
    const input = wrapper.get('[data-testid="search-input"]');
    expect(input.attributes("aria-activedescendant")).toBe("search-hit-0");
    await input.trigger("keydown", { key: "ArrowDown" });
    expect(input.attributes("aria-activedescendant")).toBe("search-hit-1");
    await input.trigger("keydown", { key: "ArrowDown" }); // clamped at the end
    expect(input.attributes("aria-activedescendant")).toBe("search-hit-1");
    await input.trigger("keydown", { key: "ArrowUp" });
    await input.trigger("keydown", { key: "ArrowUp" }); // clamped at the top
    expect(input.attributes("aria-activedescendant")).toBe("search-hit-0");
    await input.trigger("keydown", { key: "ArrowDown" });
    await input.trigger("keydown", { key: "Enter" });
    await vi.advanceTimersByTimeAsync(0);
    expect(calls.find((c) => c.cmd === "open_search_result")).toEqual({
      cmd: "open_search_result",
      args: { id: "v1", file: "Notes/second", keepOpen: false },
    });
  });

  it("Enter with no results is a no-op and selection resets on a new result set", async () => {
    const { wrapper, calls } = mountSearch({
      search_vaults: (args) =>
        (args as { query: string }).query === "zzz"
          ? response([])
          : response([hit(), hit({ name: "second", file: "Notes/second" })]),
    });
    await type(wrapper, "zzz");
    await wrapper
      .get('[data-testid="search-input"]')
      .trigger("keydown", { key: "Enter" });
    await vi.advanceTimersByTimeAsync(0);
    expect(calls.some((c) => c.cmd === "open_search_result")).toBe(false);
    await type(wrapper, "alpha");
    const input = wrapper.get('[data-testid="search-input"]');
    await input.trigger("keydown", { key: "ArrowDown" });
    await type(wrapper, "alphab"); // new result set → selection back to 0
    expect(input.attributes("aria-activedescendant")).toBe("search-hit-0");
  });

  it("shows the refinement indicator only while refining with results up", async () => {
    const pending: Array<{ resolve: (r: SearchResponse) => void }> = [];
    const { wrapper } = mountSearch({
      search_vaults: () =>
        new Promise<SearchResponse>((resolve) => pending.push({ resolve })),
    });
    await type(wrapper, "alpha"); // first search: no results up yet
    expect(wrapper.find('[data-testid="search-refreshing"]').exists()).toBe(false);
    pending[0].resolve(response([hit()]));
    await vi.advanceTimersByTimeAsync(0);
    await nextTick();
    await type(wrapper, "alphab"); // refinement: results up + in flight
    expect(wrapper.find('[data-testid="search-refreshing"]').exists()).toBe(true);
    pending[1].resolve(response([hit()]));
    await vi.advanceTimersByTimeAsync(0);
    await nextTick();
    expect(wrapper.find('[data-testid="search-refreshing"]').exists()).toBe(false);
  });

  it("group headers show a hit count and rows show a kind icon", async () => {
    const { wrapper } = mountSearch({
      search_vaults: () =>
        response([
          hit(),
          hit({ name: "deck.pdf", file: "deck.pdf", isNote: false, snippet: null }),
        ]),
    });
    await type(wrapper, "alpha");
    expect(wrapper.get('[data-testid="group-count"]').text()).toBe("2");
    expect(wrapper.findAll('[data-testid="hit-icon-note"]')).toHaveLength(1);
    expect(wrapper.findAll('[data-testid="hit-icon-file"]')).toHaveLength(1);
  });

  it("Ctrl+Enter and Ctrl+click open the hit without closing the panel", async () => {
    const { wrapper, calls } = mountSearch();
    await type(wrapper, "alpha");
    await wrapper
      .get('[data-testid="search-input"]')
      .trigger("keydown", { key: "Enter", ctrlKey: true });
    await vi.advanceTimersByTimeAsync(0);
    const opens = calls.filter((c) => c.cmd === "open_search_result");
    expect(opens).toHaveLength(1);
    // keepOpen must reach Rust: skipping close_panel alone is not enough —
    // the panel's focus-out check would hide it when Obsidian grabs focus,
    // so the backend pins the panel open for the grab window.
    expect(opens[0].args).toMatchObject({ keepOpen: true });
    expect(calls.some((c) => c.cmd === "close_panel")).toBe(false);
    await wrapper
      .get('[data-testid="search-hit"]')
      .trigger("click", { ctrlKey: true });
    await vi.advanceTimersByTimeAsync(0);
    expect(calls.filter((c) => c.cmd === "open_search_result")).toHaveLength(2);
    expect(calls.some((c) => c.cmd === "close_panel")).toBe(false);
  });

  it("hovering a row syncs the keyboard selection", async () => {
    const { wrapper } = mountSearch({
      search_vaults: () =>
        response([hit(), hit({ name: "second", file: "Notes/second" })]),
    });
    await type(wrapper, "alpha");
    await wrapper.findAll('[data-testid="search-hit"]')[1].trigger("mousemove");
    expect(
      wrapper
        .get('[data-testid="search-input"]')
        .attributes("aria-activedescendant"),
    ).toBe("search-hit-1");
  });

  it("shows an aria-live summary of matches across vaults", async () => {
    const { wrapper } = mountSearch({
      search_vaults: () =>
        response([
          hit(),
          hit({ vaultId: "v2", vaultName: "P", name: "b", file: "b" }),
        ]),
    });
    await type(wrapper, "alpha");
    const summary = wrapper.get('[data-testid="search-summary"]');
    expect(summary.text()).toBe("2 matches in 2 vaults");
    expect(summary.attributes("role")).toBe("status");
    expect(summary.attributes("aria-live")).toBe("polite");
  });

  it("marks the summary with a plus when results were truncated", async () => {
    const { wrapper } = mountSearch({
      search_vaults: () => response([hit()], true),
    });
    await type(wrapper, "alpha");
    expect(wrapper.get('[data-testid="search-summary"]').text()).toBe(
      "1+ matches in 1 vault",
    );
  });

  it("collapsing a group hides its rows and keyboard navigation skips them", async () => {
    const { wrapper } = mountSearch({
      search_vaults: () =>
        response([
          hit(),
          hit({ vaultId: "v2", vaultName: "P", name: "other", file: "other" }),
        ]),
    });
    await type(wrapper, "alpha");
    await wrapper.findAll('[data-testid="group-toggle"]')[0].trigger("click");
    const rows = wrapper.findAll('[data-testid="search-hit"]');
    expect(rows).toHaveLength(1); // first vault's row hidden
    expect(rows[0].text()).toContain("other");
    // the flat visible list re-indexes: the remaining row is search-hit-0
    expect(
      wrapper
        .get('[data-testid="search-input"]')
        .attributes("aria-activedescendant"),
    ).toBe("search-hit-0");
    // count chip stays visible on the collapsed header
    expect(wrapper.findAll('[data-testid="group-count"]')[0].text()).toBe("1");
  });

  it("kind chips filter rows and show the filtered-empty line", async () => {
    const { wrapper } = mountSearch({
      search_vaults: () =>
        response([
          hit(),
          hit({ name: "deck.pdf", file: "deck.pdf", isNote: false, snippet: null }),
        ]),
    });
    await type(wrapper, "alpha");
    await wrapper.get('[data-testid="search-filter-notes"]').trigger("click");
    expect(wrapper.findAll('[data-testid="search-hit"]')).toHaveLength(1);
    expect(wrapper.findAll('[data-testid="hit-icon-note"]')).toHaveLength(1);
    await wrapper.get('[data-testid="search-filter-files"]').trigger("click");
    expect(wrapper.findAll('[data-testid="hit-icon-file"]')).toHaveLength(1);
    // a filter that empties a non-empty response gets its own line
    const { wrapper: onlyNotes } = mountSearch({
      search_vaults: () => response([hit()]),
    });
    await type(onlyNotes, "alpha");
    await onlyNotes.get('[data-testid="search-filter-files"]').trigger("click");
    expect(onlyNotes.text()).toContain("Nothing matches this filter.");
  });

  it("records successful searches and offers them as recent chips", async () => {
    const { wrapper, calls } = mountSearch();
    await type(wrapper, "alpha");
    await wrapper.get('[data-testid="search-input"]').setValue(""); // back to hint state
    const chip = wrapper.get('[data-testid="recent-chip"]');
    expect(chip.text()).toBe("alpha");
    await chip.trigger("click");
    await vi.advanceTimersByTimeAsync(300);
    const searches = calls.filter((c) => c.cmd === "search_vaults");
    expect(searches).toHaveLength(2);
    expect(searches[1].args).toEqual({ query: "alpha" });
  });

  it("a failed search records no recent and Clear empties the list", async () => {
    let fail = true;
    const { wrapper } = mountSearch({
      search_vaults: () => {
        if (fail) throw new Error("boom");
        return response([hit()]);
      },
    });
    await type(wrapper, "broken");
    await wrapper.get('[data-testid="search-input"]').setValue("");
    expect(wrapper.find('[data-testid="recent-chip"]').exists()).toBe(false); // failure recorded nothing
    fail = false;
    await type(wrapper, "works");
    await wrapper.get('[data-testid="search-input"]').setValue("");
    expect(wrapper.get('[data-testid="recent-chip"]').text()).toBe("works");
    await wrapper.get('[data-testid="recent-clear"]').trigger("click");
    expect(wrapper.find('[data-testid="recent-chip"]').exists()).toBe(false);
  });

  it("ignores Enter and arrows from IME composition", async () => {
    // Failure mode: committing CJK text via an IME delivers Enter as a
    // keydown with isComposing — treating it as an open command launches a
    // note while the user is still typing their query; arrows during
    // composition belong to the IME candidate list, not the result list.
    const { wrapper, calls } = mountSearch({
      search_vaults: () =>
        response([hit(), hit({ name: "second", file: "Notes/second" })]),
    });
    await type(wrapper, "alpha");
    const input = wrapper.get('[data-testid="search-input"]');
    await input.trigger("keydown", { key: "ArrowDown", isComposing: true });
    expect(input.attributes("aria-activedescendant")).toBe("search-hit-0"); // unchanged
    await input.trigger("keydown", { key: "Enter", isComposing: true });
    await vi.advanceTimersByTimeAsync(0);
    expect(calls.some((c) => c.cmd === "open_search_result")).toBe(false);
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
