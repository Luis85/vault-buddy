import { describe, expect, it } from "vitest";

import { CHECK_ACTIONS, CHECK_CODES, CHECK_SEVERITIES, CHECK_TARGET_KINDS } from "../src/editor/editorCheckTypes";
import { rustVariants } from "./helpers/rustSource";

/**
 * Final whole-branch review I5: `decodeChecks.ts` refuses a finding whose
 * `code` (or severity, target kind, action) is not in these lists, and one
 * refused finding fails the WHOLE `editor_get_checks` reply — so a code
 * Rust gains and this file does not would silently break Checks for every
 * project. The unions are read from `core::editor::checks` itself
 * (`rustVariants`, `tests/helpers/rustSource.ts` — Task 20 pulled it out of
 * this file so `editorWireEnums.test.ts` shares the one reader): every
 * variant, in its serde `rename_all = "camelCase"` spelling, must be listed
 * here and nothing else. Rust's own
 * `every_code_and_action_uses_the_contract_spelling` pins that the serde
 * spelling is the camelCased variant name.
 */
const CHECKS_RS = "src-tauri/core/src/editor/checks.rs";

describe("the before-you-share checks' wire spellings match core::editor::checks", () => {
  it.each([
    ["CheckCode", CHECK_CODES],
    ["Severity", CHECK_SEVERITIES],
    ["TargetKind", CHECK_TARGET_KINDS],
    ["CheckAction", CHECK_ACTIONS],
  ] as const)("%s", (enumName, listed) => {
    const variants = rustVariants(enumName, CHECKS_RS);
    expect(variants.length).toBeGreaterThan(2);
    expect([...listed].sort()).toEqual(variants);
  });
});
