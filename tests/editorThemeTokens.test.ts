import { readdirSync, readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { describe, expect, it } from "vitest";

const EDITOR = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../src/components/editor");

function vueFiles(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((e) =>
    e.isDirectory() ? vueFiles(path.join(dir, e.name)) : e.name.endsWith(".vue") ? [path.join(dir, e.name)] : [],
  );
}

describe("editor theme tokens", () => {
  // GAP-206: a white-opacity literal (`bg-white/5`, `hover:bg-white/10`,
  // `border-white/10`) reads as designed on the dark stage and as nothing
  // at all on the light theme's white panels — white over #fff is 1.00:1.
  // Tasks 14 and its fix rounds moved every one under the editor onto
  // `--color-hover`/`--color-track`/`--color-hover-subtle`, which carry a
  // light override; the e2e sweep measures only the rows it drives, so this
  // pins the rest. (The panel window's own glass stays literal by design.)
  it("no editor component uses a white-opacity literal", () => {
    const offenders = vueFiles(EDITOR).flatMap((file) =>
      readFileSync(file, "utf8")
        .split("\n")
        .flatMap((line, i) =>
          /\bwhite\/\d+/.test(line) ? [`${path.relative(EDITOR, file)}:${i + 1}: ${line.trim()}`] : [],
        ),
    );
    expect(offenders).toEqual([]);
  });
});
