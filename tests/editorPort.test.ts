import { readdirSync, readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createListenerScope } from "../src/editor/listenerScope";
import { createTauriEditorPort, EditorPortError } from "../src/editor/port";
import type { EditorCommand, ExecuteRequest } from "../src/editorTypes";

const SRC_DIR = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../src");
const PORT_FILE = path.join(SRC_DIR, "editor", "port.ts");

// A minimal but Rust-shaped `EditorProjection` reply — enough for
// `decodeProjection` (inside `port.execute`) to succeed, so these tests
// exercise the WHOLE round trip (encode the request, decode the reply),
// not just the outbound side.
const PROJECTION_REPLY = {
  snapshot: {
    sessionId: "ses-1",
    projectId: "project",
    revision: 2,
    persistedRevision: null,
    title: "New Title",
    durationMs: 0,
    canUndo: true,
    canRedo: false,
    undoLabel: "Rename",
    redoLabel: null,
  },
  project: {
    schema: "vault-buddy-video-project/3",
    id: "project",
    title: "New Title",
    canvas: { width: 1280, height: 720, fps: 30 },
    master_gain: 1.0,
    assets: [],
    tracks: [],
    clips: [],
    effects: [],
    markers: [],
    transitions: [],
    destination: { vault: "", folder: "", dated: false },
  },
};

function walkTsFiles(dir: string, out: string[] = []): string[] {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      walkTsFiles(full, out);
    } else if (/\.(ts|vue)$/.test(entry.name)) {
      out.push(full);
    }
  }
  return out;
}

/** Strips comments before scanning (the `structural_scan::code_only`
 * precedent on the Rust side) — otherwise a doc comment that EXPLAINS this
 * very rule (as this port's own module doc does) would trip the scan it
 * describes. */
function codeOnly(src: string): string {
  return src.replace(/\/\*[\s\S]*?\*\//g, "").replace(/\/\/.*$/gm, "");
}

// Fix round 1, finding 2: the naive pattern required `invoke(` immediately
// followed by the command-name quote, which misses the GENERIC form
// `invoke<T>("editor_…")` — half of this codebase's own `invoke` call
// sites spell it that way (e.g. `src/stores/screenCapture.ts`'s
// `invoke<ScreenCaptureStatus>("start_screen_capture", …)`), so the naive
// pattern would silently pass a second `invoke<EditorOpenResult>("editor_
// open_staged", …)` call site anywhere in `src/`. `NAIVE_INVOKE_PATTERN` is
// kept only to prove that gap in the test below; the scan itself uses
// `INVOKE_PATTERN`.
const NAIVE_INVOKE_PATTERN = /invoke\s*\(\s*["'`]editor_/;
const INVOKE_PATTERN = /invoke\s*(?:<[^>]*>)?\s*\(\s*["'`]editor_/;

describe("EditorPort", () => {
  afterEach(() => clearMocks());

  it("execute sends the exact invoke payload", async () => {
    let captured: { cmd: string; args: unknown } | null = null;
    mockIPC((cmd, args) => {
      captured = { cmd, args };
      if (cmd === "editor_execute") return PROJECTION_REPLY;
      throw new Error(`unexpected command ${cmd}`);
    });

    const port = createTauriEditorPort();
    // Typed explicitly as `EditorCommand` (imported from `editorTypes.ts`,
    // never `editorCommandTypes.ts` directly) — the single-import-site the
    // module doc promises, proven by actually relying on it here.
    const command: EditorCommand = { kind: "rename", title: "New Title" };
    const request: ExecuteRequest = {
      sessionId: "ses-1",
      expectedRevision: 1,
      commandId: "cmd-1",
      command,
    };
    await port.execute(request);

    expect(captured).toEqual({ cmd: "editor_execute", args: { request } });
  });

  it("execute sends the ExecuteRequest Rust literal unchanged (session.rs execute_request_wire_literal)", async () => {
    let captured: { cmd: string; args: unknown } | null = null;
    mockIPC((cmd, args) => {
      captured = { cmd, args };
      if (cmd === "editor_execute") return PROJECTION_REPLY;
      throw new Error(`unexpected command ${cmd}`);
    });

    const port = createTauriEditorPort();
    // Copied verbatim from core/src/editor/session.rs's
    // `execute_request_wire_literal`:
    // {"sessionId":"s","expectedRevision":1,"commandId":"c1","command":
    //  {"kind":"splitClip","clipId":"c","atMs":1200}}
    const request: ExecuteRequest = {
      sessionId: "s",
      expectedRevision: 1,
      commandId: "c1",
      command: { kind: "splitClip", clipId: "c", atMs: 1200 },
    };
    await port.execute(request);

    expect(captured).toEqual({
      cmd: "editor_execute",
      args: {
        request: {
          sessionId: "s",
          expectedRevision: 1,
          commandId: "c1",
          command: { kind: "splitClip", clipId: "c", atMs: 1200 },
        },
      },
    });
  });

  it("execute sends the pasteFragment ClipboardFragment Rust literal unchanged (commands/mod.rs paste_fragment_wire_literal)", async () => {
    let captured: { cmd: string; args: unknown } | null = null;
    mockIPC((cmd, args) => {
      captured = { cmd, args };
      if (cmd === "editor_execute") return PROJECTION_REPLY;
      throw new Error(`unexpected command ${cmd}`);
    });

    const port = createTauriEditorPort();
    // Typed as `EditorCommand` — not a looser inline object literal — so a
    // spelling drift in ClipboardFragment's own fields (document-spelled
    // clip entities vs. the envelope's camelCase trackId/atMs/originMs)
    // fails vue-tsc, not merely this test. Copied verbatim from
    // core/src/editor/commands/mod.rs's `paste_fragment_wire_literal`.
    const command: EditorCommand = {
      kind: "pasteFragment",
      fragment: {
        clips: [
          {
            id: "c1",
            asset_id: "a1",
            track_id: "v1",
            name: "c1",
            start_ms: 0,
            in_ms: 0,
            out_ms: 200,
            fade_in_ms: 0,
            fade_out_ms: 0,
            fade_curve: "linear",
            opacity: 1,
            volume: 1,
            muted: false,
            x: 0,
            y: 0,
            w: 1,
            h: 1,
          },
        ],
        effects: [],
        captions: [],
        markers: [],
        originMs: 0,
      },
      trackId: "t",
      atMs: 0,
    };
    const request: ExecuteRequest = {
      sessionId: "s",
      expectedRevision: 1,
      commandId: "cmd-paste",
      command,
    };
    await port.execute(request);

    expect(captured).toEqual({ cmd: "editor_execute", args: { request } });
  });

  it("sends camelCased, unwrapped arguments for openStaged/getSnapshot/save", async () => {
    const calls: { cmd: string; args: unknown }[] = [];
    mockIPC((cmd, args) => {
      calls.push({ cmd, args });
      if (cmd === "editor_open_staged") {
        return { ...PROJECTION_REPLY, workspace: {}, missing: [], sourceBase: "base", recovered: false };
      }
      if (cmd === "editor_get_snapshot") return PROJECTION_REPLY;
      if (cmd === "editor_save_project") {
        return { sessionId: "ses-1", savedRevision: 2, projectFileId: "project" };
      }
      throw new Error(`unexpected command ${cmd}`);
    });

    const port = createTauriEditorPort();
    await port.openStaged("2026-09-20 1432 Demo");
    await port.getSnapshot("ses-1", 3);
    await port.save("ses-1", 2);

    expect(calls).toEqual([
      { cmd: "editor_open_staged", args: { stagedBase: "2026-09-20 1432 Demo" } },
      { cmd: "editor_get_snapshot", args: { sessionId: "ses-1", knownRevision: 3 } },
      { cmd: "editor_save_project", args: { sessionId: "ses-1", expectedRevision: 2 } },
    ]);
  });

  it("sends camelCased, unwrapped arguments for openProject/closeSession/hideWindow", async () => {
    const calls: { cmd: string; args: unknown }[] = [];
    mockIPC((cmd, args) => {
      calls.push({ cmd, args });
      if (cmd === "editor_open_project") {
        return { ...PROJECTION_REPLY, workspace: {}, missing: [], sourceBase: null, recovered: false };
      }
      if (cmd === "editor_close_session") return null;
      if (cmd === "editor_hide_window") return null;
      throw new Error(`unexpected command ${cmd}`);
    });

    const port = createTauriEditorPort();
    await port.openProject("proj-1", false);
    await port.closeSession("ses-1", "keep");
    await port.hideWindow();

    expect(calls).toEqual([
      { cmd: "editor_open_project", args: { projectFileId: "proj-1", useRecovery: false } },
      { cmd: "editor_close_session", args: { sessionId: "ses-1", disposition: "keep" } },
      { cmd: "editor_hide_window", args: {} },
    ]);
  });

  // Task 18 (F-48/F-25/F-14): the workspace view-preference commands.
  it("sends camelCased, unwrapped arguments for getWorkspace/saveWorkspace", async () => {
    const calls: { cmd: string; args: unknown }[] = [];
    mockIPC((cmd, args) => {
      calls.push({ cmd, args });
      if (cmd === "editor_get_workspace") return { snap: false, theme: "light" };
      if (cmd === "editor_save_workspace") return null;
      throw new Error(`unexpected command ${cmd}`);
    });

    const port = createTauriEditorPort();
    const ws = await port.getWorkspace("ses-1");
    await port.saveWorkspace("ses-1", { snap: false, theme: "light" });

    expect(ws).toEqual({ snap: false, theme: "light" });
    expect(calls).toEqual([
      { cmd: "editor_get_workspace", args: { sessionId: "ses-1" } },
      { cmd: "editor_save_workspace", args: { sessionId: "ses-1", workspace: { snap: false, theme: "light" } } },
    ]);
  });

  it("converts a rejected invoke into EditorPortError", async () => {
    mockIPC(() => {
      throw { code: "revisionConflict", message: "stale", retryable: true, operationId: "op-1" };
    });

    const port = createTauriEditorPort();
    await expect(port.getSnapshot("ses-1", null)).rejects.toBeInstanceOf(EditorPortError);
    await expect(port.getSnapshot("ses-1", null)).rejects.toMatchObject({
      error: {
        code: "revisionConflict",
        message: "stale",
        retryable: true,
        operationId: "op-1",
      },
    });
  });

  it("wraps a non-EditorError rejection (a transport failure) as an internal EditorPortError", async () => {
    mockIPC(() => {
      throw new Error("the webview vanished mid-call");
    });
    const port = createTauriEditorPort();
    await expect(port.listProjects()).rejects.toBeInstanceOf(EditorPortError);
    await expect(port.listProjects()).rejects.toMatchObject({ error: { code: "internal" } });
  });

  it("still throws only EditorPortError when a well-shaped error carries a malformed retainedAssetIds", async () => {
    // Fix round 1, finding 3: `isEditorError` only checks
    // code/message/retryable/operationId, so a payload with those four
    // fields correct but a bad `retainedAssetIds` (here, a non-string
    // element) passes the guard and then makes `decodeEditorError` itself
    // throw `ProtocolError` — which must NOT escape past `toPortError` in
    // place of the `EditorPortError` every `EditorPort` method promises.
    mockIPC(() => {
      throw {
        code: "invalidProject",
        message: "bad state",
        retryable: false,
        operationId: "op-9",
        retainedAssetIds: ["ok", 42],
      };
    });
    const port = createTauriEditorPort();
    await expect(port.getSnapshot("ses-1", null)).rejects.toBeInstanceOf(EditorPortError);
    await expect(port.getSnapshot("ses-1", null)).rejects.toMatchObject({
      error: {
        code: "invalidProject",
        message: "bad state",
        retryable: false,
        operationId: "op-9",
      },
    });
  });

  it("only port.ts invokes editor commands", () => {
    const offenders: string[] = [];
    for (const file of walkTsFiles(SRC_DIR)) {
      if (file === PORT_FILE) continue;
      const src = codeOnly(readFileSync(file, "utf8"));
      if (INVOKE_PATTERN.test(src)) {
        offenders.push(path.relative(SRC_DIR, file));
      }
    }
    expect(offenders).toEqual([]);
  });

  it("the scan pattern catches the generic invoke<T>(...) call form", () => {
    const genericCall = 'await invoke<EditorOpenResult>("editor_open_staged", { stagedBase: base });';
    // RED proof: the naive pattern this scan used to use misses it entirely
    // — a generic-typed invoke call would have sailed straight past the
    // "only port.ts invokes editor commands" test above.
    expect(NAIVE_INVOKE_PATTERN.test(genericCall)).toBe(false);
    expect(INVOKE_PATTERN.test(genericCall)).toBe(true);
    // The plain (non-generic) form the naive pattern already caught must
    // keep matching too.
    const plainCall = 'await invoke("editor_open_staged", { stagedBase: base });';
    expect(INVOKE_PATTERN.test(plainCall)).toBe(true);
  });
});

describe("createListenerScope", () => {
  it("unlistens a registration that resolves after dispose", async () => {
    const scope = createListenerScope();
    const unlisten = vi.fn();
    let resolveRegistration!: (u: () => void) => void;
    const registration = new Promise<() => void>((resolve) => {
      resolveRegistration = resolve;
    });

    const addPromise = scope.add(registration);
    // Dispose BEFORE the registration resolves — the whole point of the
    // scope: an async `listen()` call that completes after unmount must
    // not leak.
    scope.dispose();
    resolveRegistration(unlisten);
    await addPromise;

    expect(unlisten).toHaveBeenCalledTimes(1);
  });

  it("unlistens every tracked listener on dispose, and only once", async () => {
    const scope = createListenerScope();
    const first = vi.fn();
    const second = vi.fn();
    await scope.add(Promise.resolve(first));
    await scope.add(Promise.resolve(second));

    scope.dispose();
    scope.dispose();

    expect(first).toHaveBeenCalledTimes(1);
    expect(second).toHaveBeenCalledTimes(1);
  });
});
