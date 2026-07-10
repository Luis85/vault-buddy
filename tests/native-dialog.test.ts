import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));
vi.mock("../src/logging", () => ({ logWarning: vi.fn() }));

import { logWarning } from "../src/logging";
import { withDialogSuppressed } from "../src/utils/nativeDialog";

describe("withDialogSuppressed", () => {
  beforeEach(() => {
    mocks.invoke.mockReset();
    mocks.invoke.mockResolvedValue(undefined);
    vi.mocked(logWarning).mockClear();
  });

  it("sets the dialog flag true before running and false after, returning the result", async () => {
    const order: string[] = [];
    mocks.invoke.mockImplementation((_cmd: string, args: { active: boolean }) => {
      order.push(`flag:${args.active}`);
      return Promise.resolve(undefined);
    });
    const result = await withDialogSuppressed(async () => {
      order.push("run");
      return "picked.docx";
    });
    expect(result).toBe("picked.docx");
    expect(order).toEqual(["flag:true", "run", "flag:false"]);
    expect(mocks.invoke).toHaveBeenCalledWith("set_dialog_active", { active: true });
    expect(mocks.invoke).toHaveBeenCalledWith("set_dialog_active", { active: false });
  });

  it("clears the flag even when the dialog throws", async () => {
    await expect(
      withDialogSuppressed(async () => {
        throw new Error("picker failed");
      }),
    ).rejects.toThrow("picker failed");
    // The false-clear still ran in the finally.
    expect(mocks.invoke).toHaveBeenLastCalledWith("set_dialog_active", { active: false });
  });

  it("still runs the dialog when the suppress flag invoke fails", async () => {
    mocks.invoke.mockRejectedValue(new Error("no tauri"));
    const result = await withDialogSuppressed(async () => "ok");
    expect(result).toBe("ok");
    expect(logWarning).toHaveBeenCalled();
  });
});
