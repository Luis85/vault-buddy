/**
 * The shared fake `EditorPort`'s own contract. Every real port method is
 * async (`invoke` never throws synchronously), so an unstubbed method must
 * REJECT, not throw: a sync throw skips the caller's own `.catch` — a path
 * production can never take — and a debounced `saveWorkspace` that
 * outlived its test once threw that way into a LATER test as a Vitest
 * "Unhandled Error" (editorProjectMenu, under coverage timing).
 */
import { describe, expect, it } from "vitest";

import { fakeEditorPort } from "./helpers/fakeEditorPort";

describe("fakeEditorPort", () => {
  it("an unstubbed method rejects, naming itself, instead of throwing synchronously", async () => {
    const port = fakeEditorPort();
    let call!: Promise<void>;
    expect(() => {
      call = port.saveWorkspace("ses-1", {});
    }).not.toThrow();
    await expect(call).rejects.toThrow("fakeEditorPort.saveWorkspace not stubbed for this test");
  });

  it("an override replaces the unstubbed default", async () => {
    const port = fakeEditorPort({ saveWorkspace: () => Promise.resolve() });
    await expect(port.saveWorkspace("ses-1", {})).resolves.toBeUndefined();
  });
});
