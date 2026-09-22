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

  it("only port.ts invokes editor commands", () => {
    const offenders: string[] = [];
    for (const file of walkTsFiles(SRC_DIR)) {
      if (file === PORT_FILE) continue;
      const src = codeOnly(readFileSync(file, "utf8"));
      if (/invoke\s*\(\s*["'`]editor_/.test(src)) {
        offenders.push(path.relative(SRC_DIR, file));
      }
    }
    expect(offenders).toEqual([]);
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
