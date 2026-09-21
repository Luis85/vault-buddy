/**
 * The Screen tab of Vault settings (spec §12, docs/Gaps.md GAP-103).
 *
 * Until it landed, all seven `screen_*` fields were `config.json` hand-edits:
 * READ in production -- quality and fps by the capture worker, the other five
 * by the exporter -- and settable nowhere, so a user could record and export
 * but never choose a folder, a frame rate, or whether a note was written.
 */
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../src/logging", () => ({ logWarning: vi.fn(), logBreadcrumb: vi.fn() }));
import CaptureSettings from "../src/components/CaptureSettings.vue";
import ScreenCaptureConfigTab from "../src/components/ScreenCaptureConfigTab.vue";

let active: ReturnType<typeof mount> | null = null;
beforeEach(() => {
  setActivePinia(createPinia());
  vi.useFakeTimers();
});
afterEach(() => {
  active?.unmount();
  active = null;
  vi.useRealTimers();
  clearMocks();
  document.body.innerHTML = "";
});

type Cfg = {
  screenCaptureFolder?: string | null;
  screenCaptureDateFolders?: boolean;
  screenQuality?: string;
  screenFps?: number;
  screenCreateNote?: boolean;
  screenExtraFrontmatter?: string | null;
  screenBodyTemplate?: string | null;
};

function mountTab(
  opts: Cfg & { onGet?: () => unknown; onSet?: (a: unknown) => unknown } = {},
) {
  // Defaults spread UNDER the caller's opts, the sibling tabs' shape, so
  // adding a field never grows this callback's branch count.
  const defaults = {
    screenCaptureFolder: null,
    screenCaptureDateFolders: false,
    screenQuality: "balanced",
    screenFps: 30,
    screenCreateNote: true,
    screenExtraFrontmatter: null,
    screenBodyTemplate: null,
  };
  const calls: Array<{ cmd: string; args: unknown }> = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    if (cmd === "get_screen_capture_config")
      return opts.onGet ? opts.onGet() : { ...defaults, ...opts };
    if (cmd === "set_screen_capture_config") return opts.onSet?.(args) ?? null;
  });
  active = mount(ScreenCaptureConfigTab, {
    props: { vaultId: "v1" },
    attachTo: document.body,
  });
  return { wrapper: active, calls };
}

/** The `cfg` payload of the last set call. */
function lastSaved(calls: Array<{ cmd: string; args: unknown }>) {
  // Indexed rather than `.at(-1)`: the project's tsconfig lib predates
  // es2022, so `.at` does not type-check here.
  const set = calls.filter((c) => c.cmd === "set_screen_capture_config");
  const last = set[set.length - 1];
  return (last?.args as { cfg: Record<string, unknown> } | undefined)?.cfg;
}

describe("ScreenCaptureConfigTab", () => {
  it("loads all seven fields from disk", async () => {
    const { wrapper } = mountTab({
      screenCaptureFolder: "Recordings/Screen",
      screenCaptureDateFolders: true,
      screenQuality: "high",
      screenFps: 60,
      screenCreateNote: false,
      screenExtraFrontmatter: "area: Demos",
      screenBodyTemplate: "## Notes",
    });
    await flushPromises();

    expect(
      (wrapper.get("[data-testid='screen-capture-folder-input']")
        .element as HTMLInputElement).value,
    ).toBe("Recordings/Screen");
    expect(
      (wrapper.get("[data-testid='screen-date-folders-toggle']")
        .element as HTMLInputElement).checked,
    ).toBe(true);
    expect(
      (wrapper.get("[data-testid='screen-quality-select']")
        .element as HTMLSelectElement).value,
    ).toBe("high");
    expect(
      (wrapper.get("[data-testid='screen-fps-select']")
        .element as HTMLSelectElement).value,
    ).toBe("60");
    expect(
      (wrapper.get("[data-testid='screen-create-note-toggle']")
        .element as HTMLInputElement).checked,
    ).toBe(false);
    expect(
      (wrapper.get("[data-testid='screen-extra-frontmatter']")
        .element as HTMLTextAreaElement).value,
    ).toBe("area: Demos");
    expect(
      (wrapper.get("[data-testid='screen-body-template']")
        .element as HTMLTextAreaElement).value,
    ).toBe("## Notes");
  });

  it("saves a select immediately and sends the whole config", async () => {
    const { wrapper, calls } = mountTab();
    await flushPromises();

    const quality = wrapper.get("[data-testid='screen-quality-select']");
    (quality.element as HTMLSelectElement).value = "low";
    await quality.trigger("change");
    await flushPromises();

    // Every field travels on every save: the command is a whole-DTO
    // read-modify-write, so a partial payload would write serde's defaults
    // over the six fields the user did not touch.
    expect(lastSaved(calls)).toEqual({
      screenCaptureFolder: null,
      screenCaptureDateFolders: false,
      screenQuality: "low",
      screenFps: 30,
      screenCreateNote: true,
      screenExtraFrontmatter: null,
      screenBodyTemplate: null,
    });
  });

  // A <select>'s value is ALWAYS a string. screenFps crosses IPC as a u32,
  // and serde rejects "60" rather than coercing it -- so a missing Number()
  // makes the frame-rate control fail every save while the quality control
  // beside it works, which reads as the setting being broken.
  it("sends the frame rate as a number, never the select's string", async () => {
    const { wrapper, calls } = mountTab();
    await flushPromises();

    const fps = wrapper.get("[data-testid='screen-fps-select']");
    (fps.element as HTMLSelectElement).value = "60";
    await fps.trigger("change");
    await flushPromises();

    expect(lastSaved(calls)?.screenFps).toBe(60);
    expect(typeof lastSaved(calls)?.screenFps).toBe("number");
  });

  // Blank means UNSET on both sides. Sending "" would have Rust store a
  // folder literally named empty rather than falling back to the default.
  it("sends a cleared folder as null rather than an empty string", async () => {
    const { wrapper, calls } = mountTab({ screenCaptureFolder: "Old" });
    await flushPromises();

    await wrapper
      .get("[data-testid='screen-capture-folder-input']")
      .setValue("   ");
    vi.runAllTimers();
    await flushPromises();

    expect(lastSaved(calls)?.screenCaptureFolder).toBeNull();
  });

  // The seeded-default hazard every sibling tab guards: if a failed READ
  // still rendered the fields, the autosave would write defaults over the
  // values we could not read.
  it("shows an error and no fields when the read fails", async () => {
    const { wrapper } = mountTab({
      onGet: () => {
        throw new Error("config.json is unreadable");
      },
    });
    await flushPromises();

    expect(wrapper.find("[data-testid='screen-load-error']").exists()).toBe(true);
    expect(wrapper.find("[data-testid='screen-quality-select']").exists()).toBe(
      false,
    );
  });

  // The tab existing is not the same as the tab being reachable: it is
  // rendered through CaptureSettings' TabGroup, and a tab with no entry in
  // TABS has no way in.
  it("is reachable as a tab in Vault settings", () => {
    mockIPC(() => undefined);
    active = mount(CaptureSettings, { props: { vaultId: "v1" } });
    expect(active.text()).toContain("Screen");
  });
});
