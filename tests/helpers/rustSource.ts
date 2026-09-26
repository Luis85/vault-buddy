/**
 * Shared readers over the Rust source that a Vitest pins against, so a value
 * or a set of variants can never drift silently between languages (Task 20,
 * hardening review M-V9/M-V10, GAP-216). Extracted out of
 * `editorCheckWireSpellings.test.ts` (`rustVariants`) and
 * `editorCaptions.test.ts` (`rustLimit`), which now import from here instead
 * of keeping their own copies — one implementation, every caller.
 */
import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");

/**
 * Every variant of a Rust `pub enum <enumName> { ... }` declared in
 * `filePath` (repo-relative), lowercased on its first letter to match
 * serde's `rename_all = "camelCase"` wire spelling, sorted. Doc-comment
 * (`///`, `//`) and attribute (`#[...]`) lines inside the enum body are
 * skipped, so a variant's own doc comment is never mistaken for a variant.
 */
export function rustVariants(enumName: string, filePath: string): string[] {
  const source = readFileSync(path.resolve(ROOT, filePath), "utf8");
  const body = new RegExp(`pub enum ${enumName} [{]([^}]*)[}]`).exec(source);
  if (!body) throw new Error(`enum ${enumName} not found in ${filePath}`);
  return body[1]
    .split("\n")
    .map((line) => line.trim())
    .filter((line) => line !== "" && !line.startsWith("//") && !line.startsWith("#"))
    .map((line) => line.replace(/,$/, ""))
    .map((variant) => variant[0].toLowerCase() + variant.slice(1))
    .sort();
}

/**
 * A Rust `pub const <name>: <numeric type> = <value>;` declared in
 * `filePath` (repo-relative; defaults to `core::editor`'s own limits
 * module, where every editor size/duration/count limit lives). The type is
 * matched loosely (`usize`, `u64`, ...) — the limit's numeric VALUE is what
 * every caller pins against, never its Rust width.
 */
export function rustLimit(name: string, filePath = "src-tauri/core/src/editor/mod.rs"): number {
  const file = path.resolve(ROOT, filePath);
  const match = new RegExp(`pub const ${name}: \\w+ = ([0-9_]+);`).exec(readFileSync(file, "utf8"));
  if (!match) throw new Error(`${name} not found in ${filePath}`);
  return Number(match[1].replace(/_/g, ""));
}
