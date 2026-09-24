/**
 * The Screen tab of Vault settings (spec §12, docs/Gaps.md GAP-103).
 *
 * Until it landed, all seven (now eight) `screen_*` fields were `config.json` hand-edits:
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
  screenAudioStems?: boolean;
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
    screenAudioStems: false,
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

/** Drive a SelectMenu: open the trigger, then click the option.
 *
 * The popup is Teleported to document.body, so it is NOT inside the wrapper —
 * the same helper RecordingConfigTab's suite uses, for the same reason. */
const pick = async (
  wrapper: ReturnType<typeof mount>,
  testid: string,
  value: string | number,
) => {
  await wrapper.get(`[data-testid="${testid}"]`).trigger("click");
  (
    document.body.querySelector(`[data-testid="${testid}-option-${value}"]`) as HTMLElement
  ).click();
  await flushPromises();
};

/** The `cfg` payload of the last set call. */
function lastSaved(calls: Array<{ cmd: string; args: unknown }>) {
  // Indexed rather than `.at(-1)`: the project's tsconfig lib predates
  // es2022, so `.at` does not type-check here.
  const set = calls.filter((c) => c.cmd === "set_screen_capture_config");
  const last = set[set.length - 1];
  return (last?.args as { cfg: Record<string, unknown> } | undefined)?.cfg;
}

describe("ScreenCaptureConfigTab", () => {
  // Task 53: stems are a property of HOW a capture is recorded, so the toggle
  // must say it changes nothing already staged -- a user flipping it and
  // reopening yesterday's capture would otherwise look for tracks that were
  // never recorded. It loads what is on disk and saves on the click.
  it("the stems toggle states it applies to new recordings", async () => {
    const { wrapper, calls } = mountTab({ screenAudioStems: true });
    await flushPromises();
    const toggle = wrapper.get("[data-testid='screen-audio-stems-toggle']");
    expect((toggle.element as HTMLInputElement).checked).toBe(true);
    const label = wrapper.get("label[for='screen-audio-stems']").text();
    expect(label).toContain("Keep each audio input as a separate track (new recordings only)");

    (toggle.element as HTMLInputElement).checked = false;
    await toggle.trigger("change");
    await flushPromises();
    expect(lastSaved(calls)?.screenAudioStems).toBe(false);
  });

  it("loads all eight fields from disk", async () => {
    const { wrapper } = mountTab({
      screenCaptureFolder: "Recordings/Screen",
      screenCaptureDateFolders: true,
      screenQuality: "high",
      screenFps: 60,
      screenCreateNote: false,
      screenExtraFrontmatter: "area: Demos",
      screenBodyTemplate: "## Notes",
      screenAudioStems: true,
    });
    await flushPromises();

    expect(
      (wrapper.get("[data-testid='screen-audio-stems-toggle']")
        .element as HTMLInputElement).checked,
    ).toBe(true);
    expect(
      (wrapper.get("[data-testid='screen-capture-folder-input']")
        .element as HTMLInputElement).value,
    ).toBe("Recordings/Screen");
    expect(
      (wrapper.get("[data-testid='screen-date-folders-toggle']")
        .element as HTMLInputElement).checked,
    ).toBe(true);
    // A SelectMenu renders its trigger as a button showing the option LABEL
    // (a native <select> is deliberately not used — see the component).
    expect(wrapper.get("[data-testid='screen-quality-select']").text()).toContain(
      "High",
    );
    expect(wrapper.get("[data-testid='screen-fps-select']").text()).toContain(
      "60 fps",
    );
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

    await pick(wrapper, "screen-quality-select", "low");

    // Every field travels on every save: the command is a whole-DTO
    // read-modify-write, so a partial payload would write serde's defaults
    // over the seven fields the user did not touch.
    expect(lastSaved(calls)).toEqual({
      screenCaptureFolder: null,
      screenCaptureDateFolders: false,
      screenQuality: "low",
      screenFps: 30,
      screenCreateNote: true,
      screenExtraFrontmatter: null,
      screenBodyTemplate: null,
      screenAudioStems: false,
    });
  });

  // screenFps crosses IPC as a u32 and serde rejects "60" rather than
  // coercing it, so a string here fails every save while the quality control
  // beside it works -- which reads as the setting being broken. SelectMenu
  // emits the option's OWN value, so a number stays a number; that is a
  // property of the options table, which is what this pins.
  it("sends the frame rate as a number, never a string", async () => {
    const { wrapper, calls } = mountTab();
    await flushPromises();

    await pick(wrapper, "screen-fps-select", 60);

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
