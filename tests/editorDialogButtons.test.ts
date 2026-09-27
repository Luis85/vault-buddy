import { existsSync, readdirSync, readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { describe, expect, it } from "vitest";

const SRC = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../src");
const EDITOR = path.join(SRC, "components/editor");

function vueFiles(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((e) =>
    e.isDirectory() ? vueFiles(path.join(dir, e.name)) : e.name.endsWith(".vue") ? [path.join(dir, e.name)] : [],
  );
}

/** The `.vue` files `file` imports, resolved to absolute paths. */
function vueImports(file: string): string[] {
  const source = readFileSync(file, "utf8");
  return [...source.matchAll(/from\s+"(\.{1,2}\/[^"]+\.vue)"/g)]
    .map((m) => path.resolve(path.dirname(file), m[1]))
    .filter((p) => existsSync(p));
}

/** Every component that renders inside an editor dialog: `DialogHost`
 * itself, each file that mounts it, the mixer popover (a `role="dialog"` of
 * its own), and everything those import, transitively. */
function insideDialogs(): string[] {
  const host = path.join(EDITOR, "shell/DialogHost.vue");
  const roots = [
    host,
    path.join(EDITOR, "shell/MixerPopover.vue"),
    ...vueFiles(EDITOR).filter((f) => vueImports(f).includes(host)),
  ];
  const seen = new Set<string>();
  const queue = [...roots];
  while (queue.length > 0) {
    const file = queue.pop() as string;
    if (seen.has(file)) continue;
    seen.add(file);
    queue.push(...vueImports(file).filter((f) => f.startsWith(EDITOR)));
  }
  return [...seen];
}

describe("editor dialog buttons", () => {
  // Ruling T21-3 (visual-parity Task 21, checked by Task 24): the editor's
  // dialogs use `DialogButton`, the concept's `.btn` / `.primary`. The
  // panel window's `AppButton` and `IconButton` draw their secondary, ghost
  // and hover looks with `white/N` literals — invisible on the editor's
  // light theme (white over white), and a second button style inside one
  // dialog. Task 24's sweep found the second through every dialog's Close.
  it("no component rendered inside an editor dialog uses the panel's AppButton or IconButton", () => {
    const offenders = insideDialogs()
      .filter((file) => /import\s+(AppButton|IconButton)\b/.test(readFileSync(file, "utf8")))
      .map((file) => path.relative(EDITOR, file).replace(/\\/g, "/"))
      .sort();
    expect(offenders).toEqual([]);
  });

  it("finds the dialogs it guards", () => {
    const names = insideDialogs().map((f) => path.basename(f));
    expect(names).toEqual(
      expect.arrayContaining([
        "DialogHost.vue",
        "MixerPopover.vue",
        "RenameDialog.vue",
        "LearningWalkthrough.vue",
        "GuideStartOver.vue",
      ]),
    );
  });
});
