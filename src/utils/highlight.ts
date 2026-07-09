/** One piece of a highlighted string; `match: true` spans are query hits. */
export interface HighlightPart {
  text: string;
  match: boolean;
}

/**
 * Split `text` into parts marking case-insensitive occurrences of `query`.
 * Index-based on purpose — never a RegExp built from user input, which would
 * treat `.`/`(` etc. as pattern syntax. If lowercasing changes either
 * string's length (rare Unicode, e.g. 'İ'), index math against the lowered
 * strings would mis-slice the original, so the helper falls back to a single
 * unhighlighted part rather than corrupt the text. Empty query → single
 * unhighlighted part.
 */
export function highlightParts(text: string, query: string): HighlightPart[] {
  const q = query.trim();
  const qLower = q.toLowerCase();
  const lower = text.toLowerCase();
  if (!qLower || lower.length !== text.length || qLower.length !== q.length) {
    return [{ text, match: false }];
  }
  const parts: HighlightPart[] = [];
  let pos = 0;
  for (;;) {
    const idx = lower.indexOf(qLower, pos);
    if (idx === -1) break;
    if (idx > pos) parts.push({ text: text.slice(pos, idx), match: false });
    parts.push({ text: text.slice(idx, idx + qLower.length), match: true });
    pos = idx + qLower.length;
  }
  if (pos < text.length || parts.length === 0) {
    parts.push({ text: text.slice(pos), match: false });
  }
  return parts;
}
