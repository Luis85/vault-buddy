import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { describe, expect, it } from "vitest";

import { CHECK_ACTIONS, CHECK_CODES, CHECK_SEVERITIES, CHECK_TARGET_KINDS } from "../src/editor/editorCheckTypes";

/**
 * Final whole-branch review I5: `decodeChecks.ts` refuses a finding whose
 * `code` (or severity, target kind, action) is not in these lists, and one
 * refused finding fails the WHOLE `editor_get_checks` reply — so a code
 * Rust gains and this file does not would silently break Checks for every
 * project. The unions are read from `core::editor::checks` itself (the
 * `rustLimit` precedent, `editorCaptions.test.ts`): every variant, in its
 * serde `rename_all = "camelCase"` spelling, must be listed here and
 * nothing else. Rust's own `every_code_and_action_uses_the_contract_spelling`
 * pins that the serde spelling is the camelCased variant name.
 */
const CHECKS_RS = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "../src-tauri/core/src/editor/checks.rs",
);

function rustVariants(enumName: string): string[] {
  const source = readFileSync(CHECKS_RS, "utf8");
  const body = new RegExp(`pub enum ${enumName} \{([^}]*)\}`).exec(source);
  if (!body) throw new Error(`enum ${enumName} not found in core::editor::checks`);
  return body[1]
    .split("\n")
    .map((line) => line.trim())
    .filter((line) => line !== "" && !line.startsWith("//") && !line.startsWith("#"))
    .map((line) => line.replace(/,$/, ""))
    .map((variant) => variant[0].toLowerCase() + variant.slice(1))
    .sort();
}

describe("the before-you-share checks' wire spellings match core::editor::checks", () => {
  it.each([
    ["CheckCode", CHECK_CODES],
    ["Severity", CHECK_SEVERITIES],
    ["TargetKind", CHECK_TARGET_KINDS],
    ["CheckAction", CHECK_ACTIONS],
  ] as const)("%s", (enumName, listed) => {
    const variants = rustVariants(enumName);
    expect(variants.length).toBeGreaterThan(2);
    expect([...listed].sort()).toEqual(variants);
  });
});
