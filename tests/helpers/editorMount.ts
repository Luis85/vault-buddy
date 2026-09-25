import { mockIPC } from "@tauri-apps/api/mocks";
import { flushPromises, mount } from "@vue/test-utils";

import EditorRoot from "../../src/roots/EditorRoot.vue";

/**
 * Shared mount fixtures for the editor-window suites (`editorRoot.test.ts`,
 * `editorCloseGuard.test.ts`, `editorCutSaveReopen.test.ts`): what the
 * Rust-owned stash hands back on each drain. The session itself opens
 * through each suite's injected fake `EditorPort`.
 *
 * Task 59 retired the phase-4 editor, and with it this helper's replies for
 * its sidecar read and timeline write and its strip/preview accessors.
 *
 * The `vi.mock` calls stay in each suite: they are hoisted per file, so they
 * cannot live here.
 */

export type Call = Record<string, unknown> & { cmd: string };

/** `take_editor_request`'s reply (Task 37 Part B): every base this helper
 * takes is wrapped into the `{kind: "staged", value}` shape Rust sends. */
function stagedRequest(base: string | null): { kind: "staged"; value: string } | null {
  return base === null ? null : { kind: "staged", value: base };
}

/** Serve `take_editor_request` one queued base per drain (then `null`), and
 * record every command the window sent. */
export function mockEditor(requests: (string | null)[] = ["cap one"]) {
  const seen: Call[] = [];
  const queue = [...requests];
  mockIPC((cmd, args) => {
    seen.push({ cmd, ...(args as object) });
    if (cmd === "take_editor_request") {
      return queue.length > 0 ? stagedRequest(queue.shift() ?? null) : null;
    }
    return undefined;
  });
  return seen;
}

export async function open(requests?: (string | null)[]) {
  mockEditor(requests);
  const w = mount(EditorRoot, { attachTo: document.body });
  await flushPromises();
  return w;
}
