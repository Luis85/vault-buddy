// Task 60: the tutorial editor's acceptance-evidence file is a claim about
// the test suite, so the suite checks it. Failure mode this guards: an
// evidence row that names a test file or test which was later renamed,
// moved or deleted keeps reading as "covered" in the release record while
// nothing runs any more — a doc that drifts from the tests must fail CI.
import { existsSync, readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { describe, expect, it } from "vitest";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const evidencePath = path.join(
  root,
  "docs/superpowers/specs/2026-09-21-tutorial-editor-acceptance-evidence.md",
);
const read = (p: string) => (existsSync(p) ? readFileSync(p, "utf8") : "");
const evidence = read(evidencePath);

// `path` or `path#test name`: a repo-relative test source, optionally with
// a test name that must appear verbatim in that file.
const EVIDENCE_TOKEN = /^(tests|src-tauri)\/[^#\s]+\.(ts|rs)(#.+)?$/;

function rows(): Map<string, string> {
  const out = new Map<string, string>();
  for (const line of evidence.split(/\r?\n/)) {
    const m = /^\| (F-\d{2}) \|/.exec(line);
    if (!m) continue;
    expect(out.has(m[1]), `${m[1]} is listed twice`).toBe(false);
    out.set(m[1], line);
  }
  return out;
}

describe("tutorial editor acceptance evidence", () => {
  it("exists", () => {
    expect(existsSync(evidencePath), `missing ${evidencePath}`).toBe(true);
  });

  it("lists all 50 F-IDs exactly once", () => {
    const expected = Array.from({ length: 50 }, (_, i) => `F-${String(i + 1).padStart(2, "0")}`);
    expect([...rows().keys()].sort()).toEqual(expected);
  });

  it("names at least one existing test file per F-ID, and every named test exists", () => {
    const table = rows();
    expect(table.size).toBe(50);
    for (const [id, line] of table) {
      const tokens = [...line.matchAll(/`([^`]+)`/g)].map((m) => m[1]).filter((t) => EVIDENCE_TOKEN.test(t));
      expect(tokens.length, `${id} names no test file`).toBeGreaterThan(0);
      for (const token of tokens) {
        const [file, name] = token.split("#", 2);
        const text = read(path.join(root, file));
        expect(text, `${id}: ${file} does not exist`).not.toBe("");
        // A test FILE: a Vitest/Playwright suite, or a Rust file with tests.
        expect(file.startsWith("tests/") || text.includes("#[test]"), `${id}: ${file} holds no tests`).toBe(true);
        expect(name === undefined || text.includes(name), `${id}: "${name}" is not in ${file}`).toBe(true);
      }
    }
  });

  it("maps every ADR placeholder GAP-N1..N5 to a real docs/Gaps.md entry", () => {
    const gaps = read(path.join(root, "docs/Gaps.md"));
    for (let n = 1; n <= 5; n += 1) {
      const m = new RegExp(`^\\| GAP-N${n} \\| GAP-(\\d+) \\|`, "m").exec(evidence);
      expect(m, `GAP-N${n} is not mapped`).not.toBeNull();
      expect(gaps, `GAP-${m?.[1]} has no docs/Gaps.md heading`).toMatch(new RegExp(`^### GAP-${m?.[1]} `, "m"));
    }
  });
});
