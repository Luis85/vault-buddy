import { mount } from "@vue/test-utils";
import { describe, expect, it } from "vitest";

import json from "../docs/superpowers/specs/2026-09-26-tutorial-editor-visual-parity-icons.json";
import { ICONS } from "../src/components/editor/icons/conceptIcons";
import EditorIcon from "../src/components/editor/icons/EditorIcon.vue";

describe("EditorIcon", () => {
  const names = Object.keys(json).filter((k) => k !== "_meta");
  it("carries every concept icon verbatim", () => {
    expect(Object.keys(ICONS).sort()).toEqual(names.sort());
    for (const n of names) {
      expect(ICONS[n as keyof typeof ICONS]).toBe(
        (json as unknown as Record<string, { svgInnerMarkup: string }>)[n]
          .svgInnerMarkup,
      );
    }
  });
  it("renders a decorative currentColor stroke svg at the requested size", () => {
    const w = mount(EditorIcon, { props: { name: "scissors", size: 14 } });
    const svg = w.find("svg");
    expect(svg.attributes()).toMatchObject({
      "aria-hidden": "true",
      width: "14",
      height: "14",
      stroke: "currentColor",
      fill: "none",
      viewBox: "0 0 24 24",
    });
    // The DOM's own HTML serializer always re-closes a self-closing element
    // (`<path .../>` -> `<path ...></path>`) for any non-void tag — true of
    // every browser and of happy-dom alike, and independent of how the
    // markup was inserted (v-html, an innerHTML prop, or a DOM API). So the
    // rendered markup is compared against that closed-tag normalization of
    // ICONS.scissors, via the element's own `.innerHTML` (not test-utils'
    // `.html()`, which additionally re-indents the output through
    // js-beautify and would break an exact-string comparison for a
    // multi-element icon like scissors).
    const closedTags = ICONS.scissors.replace(
      /<(\w+)([^>]*)\/>/g,
      "<$1$2></$1>",
    );
    expect(svg.element.innerHTML).toBe(closedTags);
  });
  it("defaults to 17px", () => {
    expect(
      mount(EditorIcon, { props: { name: "plus" } }).find("svg").attributes(
        "width",
      ),
    ).toBe("17");
  });
});
