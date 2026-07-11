/**
 * The final path segment of a `\`- or `/`-separated path. Splitting on BOTH
 * separators matters: Rust hands back Windows backslash paths (recording MP3s,
 * imported note paths) even though the Vitest suite runs on Unix, so several
 * views independently inlined `p.split(/[\\/]/).pop() ?? p` to show a filename.
 * One place now, so a future caller doesn't grow a seventh copy. An empty
 * string returns an empty string (`"".split` yields `[""]`), never undefined.
 */
export function basename(path: string): string {
  return path.split(/[\\/]/).pop() ?? path;
}
