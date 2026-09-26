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

// Task 20 (hardening review M-V9): a row used to pass with `text.includes(name)`,
// which is true for a title inside a `describe(...)` (never a runnable test)
// or inside `it.skip(...)`/`#[ignore]` (a test that never runs) just as much
// as for a real, live declaration. The checks below require the named test
// to be an actual, live `it`/`test`/`#[test] fn` declaration.

/** Finds the index just past `text[openIndex]` (which must be `open`) that
 * closes it, honoring nesting -- so a `.each(...)` argument containing its
 * own parentheses (`table.cases.map((c) => ...)`, the F-05 case) is walked
 * correctly instead of a naive non-greedy regex stopping at the first `)`. */
function findMatchingClose(text: string, openIndex: number, open: string, close: string): number {
  let depth = 0;
  for (let i = openIndex; i < text.length; i += 1) {
    if (text[i] === open) depth += 1;
    else if (text[i] === close) {
      depth -= 1;
      if (depth === 0) return i;
    }
  }
  return -1;
}

/** Byte ranges of every `describe.skip(...)`/`it.skip(...)`/`test.skip(...)`
 * call's own argument list -- any declaration whose match falls inside one
 * of these is skipped, even when (a `describe.skip` wrapping an ordinary
 * `it(...)`) the inner declaration reads like a live one on its own. */
function skippedRanges(text: string): Array<[number, number]> {
  const ranges: Array<[number, number]> = [];
  const re = /\b(?:describe|it|test)\.skip\s*\(/g;
  let m: RegExpExecArray | null;
  while ((m = re.exec(text))) {
    const openIndex = m.index + m[0].length - 1;
    const closeIndex = findMatchingClose(text, openIndex, "(", ")");
    if (closeIndex !== -1) ranges.push([openIndex, closeIndex]);
  }
  return ranges;
}

function isInsideAnyRange(ranges: Array<[number, number]>, index: number): boolean {
  return ranges.some(([start, end]) => index > start && index < end);
}

/** Reads a quoted string literal's own text (unescaping `\<quote>` only —
 * enough for a test title, never a template expression), starting right
 * after its opening `quote` at `start`. `null` for an unterminated literal. */
function readQuotedLiteral(text: string, quote: string, start: number): string | null {
  let out = "";
  let i = start;
  while (i < text.length) {
    const ch = text[i];
    if (ch === "\\" && i + 1 < text.length) {
      out += text[i + 1];
      i += 2;
      continue;
    }
    if (ch === quote) return out;
    if (ch === "\n") return null; // an unterminated single-line literal
    out += ch;
    i += 1;
  }
  return null;
}

function skipWhitespace(text: string, from: number): number {
  let i = from;
  while (i < text.length && /\s/.test(text[i])) i += 1;
  return i;
}

/** From right after an `it`/`test` identifier, finds the index of the
 * DECLARATION CALL's own opening paren — `it(`/`test(` directly, or
 * `it.each(...)(`/`test.each(...)(` past a balanced `.each(...)` argument
 * (which may hold its own parentheses, the F-05 case). `-1` for anything
 * else (`.skip(`, `.only(`, an unterminated `.each(...)`, ...) — deliberately
 * excludes `it.skip(...)`/`test.skip(...)`, since neither is a live
 * declaration. */
function declarationCallOpenIndex(text: string, afterIdent: number): number {
  let i = skipWhitespace(text, afterIdent);
  if (text[i] === ".") {
    if (!text.startsWith(".each", i)) return -1;
    const eachOpen = i + ".each".length;
    if (text[eachOpen] !== "(") return -1;
    const eachClose = findMatchingClose(text, eachOpen, "(", ")");
    if (eachClose === -1) return -1;
    i = skipWhitespace(text, eachClose + 1);
  }
  return text[i] === "(" ? i : -1;
}

/** The call's first argument, when it is a quoted string literal — `null`
 * otherwise (a title function, a bare identifier, ...). */
function declaredTitleAt(text: string, callOpen: number): string | null {
  const j = skipWhitespace(text, callOpen + 1);
  const quote = text[j];
  if (quote !== '"' && quote !== "'" && quote !== "`") return null;
  return readQuotedLiteral(text, quote, j + 1);
}

/** True when `name` is the literal title of a LIVE `it(...)`/`test(...)`/
 * `it.each(...)(...)`/`test.each(...)(...)` declaration in `text` — not
 * merely a substring anywhere (a `describe` title, a comment, another
 * test's body), and not one reached only through a `.skip(...)` call at any
 * level. Matching is done by scanning source characters (never a single
 * regex with `.*?`) so a `.each(...)` argument holding its own parentheses
 * is walked correctly (`declarationCallOpenIndex`). */
function tsDeclaresTest(text: string, name: string): boolean {
  const skipped = skippedRanges(text);
  const idRe = /\b(it|test)\b/g;
  let m: RegExpExecArray | null;
  while ((m = idRe.exec(text))) {
    const start = m.index;
    // Not a suffix of a longer identifier or a property access (`wait.it(`).
    if (start > 0 && /[\w.]/.test(text[start - 1])) continue;
    const callOpen = declarationCallOpenIndex(text, start + m[0].length);
    if (callOpen === -1) continue;
    // EXACT match, never a prefix: a row naming only the start of a longer
    // declared title (review M-V10) must fail, the same as one naming a
    // `describe(...)` title instead of a real `it(...)`/`test(...)` one.
    if (declaredTitleAt(text, callOpen) !== name) continue;
    if (isInsideAnyRange(skipped, start)) continue;
    return true;
  }
  return false;
}

/** True when `name` is a live Rust `fn <name>(` — not one whose immediately
 * preceding non-blank, non-attribute, non-doc-comment line is `#[ignore]`
 * (attributes and doc comments above it, e.g. `#[test]`, are skipped over
 * while looking for `#[ignore]`). */
function rustDeclaresTest(text: string, name: string): boolean {
  const re = new RegExp(`fn ${name}\\(`, "g");
  let m: RegExpExecArray | null;
  while ((m = re.exec(text))) {
    const lines = text.slice(0, m.index).split(/\r?\n/);
    let ignored = false;
    for (let i = lines.length - 1; i >= 0; i -= 1) {
      const line = lines[i].trim();
      if (line === "") continue;
      if (line.startsWith("#[ignore")) {
        ignored = true;
        break;
      }
      if (line.startsWith("#[") || line.startsWith("///") || line.startsWith("//")) continue;
      break;
    }
    if (!ignored) return true;
  }
  return false;
}

/** Dispatches on the token's own file extension — `.rs` for a Rust `fn`,
 * `.ts` for a Vitest/Playwright declaration. */
function declaresLiveTest(text: string, name: string, file: string): boolean {
  return file.endsWith(".rs") ? rustDeclaresTest(text, name) : tsDeclaresTest(text, name);
}

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
        expect(
          name === undefined || declaresLiveTest(text, name, file),
          `${id}: "${name}" is not a live (non-skipped) test declaration in ${file}`,
        ).toBe(true);
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
