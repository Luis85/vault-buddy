/**
 * The editor's theme default (visual-parity design D1): the concept opens
 * dark (`shell.html: <html data-theme="dark"`) and light is a View-menu
 * choice, so a fresh window is dark whatever the OS prefers — and a theme
 * the user saved still wins over that default on hydrate.
 */
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { Workspace } from "../src/editorTypes";
import { useEditorWorkspaceStore } from "../src/stores/editorWorkspace";
import { fakeEditorPort as fakePort } from "./helpers/fakeEditorPort";

/** A `matchMedia` answering "yes" to every light query, the way a Windows
 * machine in light mode would. */
function preferLight() {
  vi.stubGlobal("matchMedia", (q: string) => ({
    matches: q.includes("light"),
    media: q,
    addEventListener() {},
    removeEventListener() {},
  }));
}

describe("editor theme default (D1)", () => {
  beforeEach(() => setActivePinia(createPinia()));
  afterEach(() => vi.unstubAllGlobals());

  it("opens dark even when the OS prefers light", () => {
    preferLight();
    expect(useEditorWorkspaceStore().theme).toBe("dark");
  });

  it("a saved light theme still wins over the dark default", async () => {
    preferLight();
    const workspace = useEditorWorkspaceStore();
    workspace.setPort(fakePort({ getWorkspace: () => Promise.resolve({ theme: "light" } as Workspace) }));
    await workspace.hydrate("ses-a");
    expect(workspace.theme).toBe("light");
  });

  it("a saved workspace without a theme keeps the dark default", async () => {
    preferLight();
    const workspace = useEditorWorkspaceStore();
    workspace.setPort(fakePort({ getWorkspace: () => Promise.resolve({ snap: false } as Workspace) }));
    await workspace.hydrate("ses-a");
    expect(workspace.theme).toBe("dark");
  });
});
