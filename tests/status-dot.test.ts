import { mount } from "@vue/test-utils";
import { describe, expect, it } from "vitest";

import StatusDot from "../src/components/ui/StatusDot.vue";

describe("StatusDot", () => {
  it("maps each tone to its palette color", () => {
    const cases: Array<[string, string]> = [
      ["success", "bg-emerald-400"],
      ["recording", "bg-red-500"],
      ["transcribing", "bg-violet-400"],
      ["priority-high", "bg-red-400"],
      ["priority-low", "bg-slate-500"],
    ];
    for (const [tone, cls] of cases) {
      const w = mount(StatusDot, { props: { tone: tone as never } });
      expect(w.get("span").classes()).toContain(cls);
    }
  });

  it("adds animate-pulse only when pulsing and stays aria-hidden", () => {
    const on = mount(StatusDot, { props: { tone: "recording", pulse: true } });
    expect(on.get("span").classes()).toContain("animate-pulse");
    expect(on.get("span").attributes("aria-hidden")).toBe("true");
    const off = mount(StatusDot, { props: { tone: "success" } });
    expect(off.get("span").classes()).not.toContain("animate-pulse");
  });
});
