import { beforeEach, describe, expect, it, vi } from "vitest";

const logMocks = vi.hoisted(() => ({
  info: vi.fn(),
  warn: vi.fn(),
  error: vi.fn(),
}));
vi.mock("@tauri-apps/plugin-log", () => ({
  info: logMocks.info,
  warn: logMocks.warn,
  error: logMocks.error,
}));

import {
  initLogging,
  logBreadcrumb,
  logVueError,
  logWarning,
} from "../src/logging";

describe("logging bridge", () => {
  beforeEach(() => {
    logMocks.info.mockReset().mockResolvedValue(undefined);
    logMocks.warn.mockReset().mockResolvedValue(undefined);
    logMocks.error.mockReset().mockResolvedValue(undefined);
    delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
  });

  it("no-ops outside Tauri", () => {
    logBreadcrumb("hi");
    logWarning("uh oh");
    expect(logMocks.info).not.toHaveBeenCalled();
    expect(logMocks.warn).not.toHaveBeenCalled();
  });

  it("forwards breadcrumbs and warnings under Tauri", () => {
    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
    logBreadcrumb("drag start @ 10,20");
    logWarning("panel transition failed: boom");
    expect(logMocks.info).toHaveBeenCalledWith("drag start @ 10,20");
    expect(logMocks.warn).toHaveBeenCalledWith("panel transition failed: boom");
  });

  it("forwards uncaught window errors to the log", () => {
    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
    initLogging();
    window.dispatchEvent(
      new ErrorEvent("error", {
        message: "boom",
        filename: "a.js",
        lineno: 1,
        colno: 2,
      }),
    );
    expect(logMocks.error).toHaveBeenCalledWith(
      "window error: boom @ a.js:1:2",
    );
  });

  // Setting app.config.errorHandler replaces Vue's own console logging, and
  // logError is a no-op outside Tauri — so the console.error is the ONLY
  // trace a plain browser dev session gets. Both halves must survive.
  it("logVueError writes the raw error to the console, then to the log", () => {
    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
    const order: string[] = [];
    const consoleSpy = vi
      .spyOn(console, "error")
      .mockImplementation(() => order.push("console"));
    logMocks.error.mockImplementation(() => {
      order.push("log");
      return Promise.resolve();
    });
    const err = new Error("kaboom");
    try {
      logVueError(err, "render function");
      expect(consoleSpy).toHaveBeenCalledWith(err);
      expect(logMocks.error).toHaveBeenCalledWith(
        "vue error (render function): Error: kaboom",
      );
      expect(order).toEqual(["console", "log"]);
    } finally {
      consoleSpy.mockRestore();
    }
  });

  it("logVueError still reaches the console outside Tauri", () => {
    const consoleSpy = vi.spyOn(console, "error").mockImplementation(() => {});
    try {
      logVueError("plain string", "setup function");
      expect(consoleSpy).toHaveBeenCalledWith("plain string");
      expect(logMocks.error).not.toHaveBeenCalled();
    } finally {
      consoleSpy.mockRestore();
    }
  });
});
