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

import { initLogging, logBreadcrumb, logWarning } from "../src/logging";

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
});
