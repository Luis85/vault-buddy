# Real-YAML extra-frontmatter sanitizer

**Date:** 2026-07-18
**Status:** Design — awaiting review
**Area:** `core::template` + the three renderers (`capture_note`, `tasks::disk`, `document_import`)

## Problem

The additive per-vault templates (vault-UX increment, PR #66) let a vault add
free-text **extra frontmatter** around each renderer's managed keys. That user
text is made injection-safe by `core::template::sanitize_extra_frontmatter`, a
**line-based heuristic** that tries to recognise YAML mapping entries, drop
reserved keys, and quote unsafe values without actually parsing YAML.

That heuristic has been patched four times in review (Codex P2 rounds):
quoted-key evasion (`"type": …`), top-level list items with inline colons
(`- project: Alpha`), unsafe substituted values (`summary: Ship: v1`), and
non-mapping scalars (`https://x`). Each fix closed one hand-spotted case; the
approach is structurally whack-a-mole because it re-implements a YAML parser
one edge case at a time.

Decision (user-approved): **replace the heuristic with a real YAML library.**
Keep the `{{placeholder}}` templates working, and additionally guarantee the
output is **Obsidian-compatible frontmatter**.

## Why templates and a real parser are not in tension

They run at different stages and compose:

- `{{placeholder}}` substitution is a **pre-processing step** over raw text.
- The YAML parser runs **after**, on real structure, doing the one job the
  heuristic keeps failing: reliably identify top-level keys (to drop reserved
  ones) and guarantee valid, safely-quoted output.

The only real friction is a chicken-and-egg glitch: `key: {{title}}` is not
parseable YAML (the `{{` opens a flow mapping), and a substituted value like
`Acme: Corp` or `[draft]` is not a safe plain scalar either. So neither
"substitute then parse" nor "parse then substitute" works alone. A **sentinel**
between the two stages resolves it.

## Design: tokenize → parse → resolve → drop → emit

A new function replaces the sanitizer and folds substitution in, because
substitution must happen *inside* the parsed tree:

```
render_extra_frontmatter(template: &str, vars: &[(&str, &str)], reserved: &[&str]) -> String
```

Pipeline:

1. **Tokenize.** Walk `{{key}}` exactly as `substitute` does (whitespace inside
   braces tolerated; unclosed `{{` left literal), but a **known** key is
   replaced by a unique sentinel scalar `\u{E000}<i>\u{E000}` (a Private-Use-Area
   delimiter that is a valid YAML plain scalar and cannot occur in real input),
   recording the sentinel→value mapping. An **unknown** key renders empty
   (inline `""`), matching `substitute`. Result: text whose *structure* is what
   the user intended, and which parses.
2. **Parse** the tokenized text with a real YAML library into a document value.
   - Parse failure (genuinely malformed, e.g. `key: [unclosed`) → **drop the
     whole block** (return `""`) and `log::warn!`. This is the approved,
     more-predictable replacement for today's partial line salvage.
   - A non-mapping root (bare scalar, or a top-level sequence) → **drop the
     whole block** (`""`, warn): extra frontmatter must be mapping entries.
3. **Resolve** sentinels → real values inside every scalar (keys and values).
   Because a placeholder value lands as an opaque string node, a value that
   contains YAML metacharacters (`Acme: Corp`, `[draft]`, `2026`) stays a single
   scalar — it can never inject structure, and a numeric-looking value stays a
   **string** (Obsidian reads it as a text property, not a number).
4. **Drop reserved top-level keys** (string keys only, `eq_ignore_ascii_case`
   against `reserved`). The parser already unquoted keys, so `"type"` and `type`
   compare equal with no special handling — the quoted-key evasion class is gone
   by construction. Non-string keys (`123: x`) are not reserved and kept.
5. **Emit** the surviving mapping through the library's serializer → standard,
   guaranteed-valid YAML mapping lines. An **empty mapping after the drop → `""`**
   (never the serializer's `{}` literal). Defensively strip any leading
   `---`/trailing `...` the serializer might add (we inject *inside* our own
   fence). Return the lines, `\n`-terminated, or `""`.

### Obsidian compatibility (hard requirement)

"Obsidian-compatible" here means the emitted block is what Obsidian's
`js-yaml`-based Properties parser accepts:

- Standard scalar quoting; spaces, never tabs (the serializer's defaults).
- No document markers (`---`/`...`) inside the block — guaranteed by step 5.
- Flow or block sequences are both valid; either is accepted.
- Placeholder values stay strings (step 3), so `{{title}}` = `"2026"` becomes a
  text property, not a number — matching user intent for a *title*.
- Reserved managed keys (including `tags` for docs/tasks) are dropped, so the
  extra block can never fight the managed block Obsidian shows.

### Crate choice (gate-zero)

Primary candidate: **`serde_yaml_ng`** — the actively-maintained fork of the
deprecated `serde_yaml`; serde `Value`/`Mapping` where `Mapping` is
insertion-ordered (`indexmap`), so **key order is preserved**; `from_str` /
`to_string` with no root document markers; MIT/Apache-2.0. It is a distinct
crate id from `serde_yaml`, so it is **not** covered by `serde_yaml`'s
unmaintained RustSec advisory.

**The first implementation step is to add the dependency and run
`cargo deny check` green** (advisories + licenses + sources — see
`src-tauri/deny.toml`: `unmaintained = "workspace"`, `unknown-git = "deny"`,
license allowlist). If `serde_yaml_ng` is flagged, fall back to **`saphyr`**
(the maintained yaml-rust successor: `Yaml` AST + emitter, order-preserving,
MIT/Apache-2.0 — more code, same pipeline). No `deny.toml` ignore entry may be
added to force a crate through; the gate picks the crate.

## API changes (`core::template`)

- **Add** `render_extra_frontmatter(template, vars, reserved) -> String` (the
  pipeline above).
- **Remove** `sanitize_extra_frontmatter`, `emit_line`, `unquote_key` — fully
  subsumed.
- **Keep** `substitute` unchanged — it still renders **body** templates (raw
  markdown, no YAML) at all three renderers and `assemble_body`'s `{{content}}`.
- **Keep** `yaml_quote` unchanged — the renderers still hand-build their managed
  frontmatter lines with it. It is re-exported by `capture_note`.

## Call-site changes (3, mechanical)

Each renderer currently does `sanitize_extra_frontmatter(&substitute(t, &vars), RESERVED)`;
each becomes `render_extra_frontmatter(t, &vars, RESERVED)`:

| Renderer | vars | reserved |
| --- | --- | --- |
| `capture_note::render_note` | `recordedAt, duration, vault, type, date` | `recorded, duration, paused, vault, type, inputs, event, created-by` |
| `document_import::render_frontmatter` | `source, format, date` | `type, tags, source, imported, format, created-by` |
| `tasks::disk::render_task` | `title, date, due, priority` | `type, status, title, created, due, priority, tags, tag, order` (+ task-id property when set) |

No renderer's managed-frontmatter construction, body handling, injection point,
or `None`/empty-template guard changes. **An unset/empty template still skips
injection entirely, so its output is byte-for-byte unchanged** (the primary
regression guarantee — unchanged from PR #66).

## Behavior changes (user-visible, all approved)

- A **non-empty** extra-frontmatter template's kept lines are now **normalized**
  by the serializer: e.g. `[Alex, Sam]` may re-emit as a block sequence, and a
  value needing quoting is requoted canonically. **Key order is preserved.**
  YAML **comments in the extra block are dropped** (accepted; comments in a
  2–3 line frontmatter snippet are rare, and the trade buys guaranteed-valid,
  Obsidian-safe output).
- **Malformed** extra frontmatter (unparseable, or a non-mapping root) drops the
  **whole** extra block rather than salvaging lines — more predictable, and
  logged.
- Bonus capability the heuristic lacked: placeholders inside collections
  (`people: [{{a}}, {{b}}]`) now resolve correctly.

## Testing (TDD)

`core::template` unit tests for `render_extra_frontmatter`, replacing the
`sanitize_*` tests and adding:

- known/unknown placeholder resolution; unclosed `{{` literal.
- value with `: ` (`summary: {{title}}`, title `Ship: v1`) → single safely
  quoted scalar; value `[draft]` → stays a string, not a sequence.
- numeric placeholder value stays a string (`title: {{t}}`, t = `2026` →
  quoted); a literal `count: 5` stays an int `5`.
- reserved key dropped — plain, quoted (`"type":`), and case-insensitive
  (`Type:`); a non-reserved quoted key kept.
- injected `---`/`...` and top-level `- item` / bare scalar → whole block
  dropped.
- empty input → `""`; every-key-reserved → `""` (never `{}`).
- malformed YAML → `""` (+ warn).
- **key order preserved** across a multi-key block.
- **Obsidian checks:** output contains no `---`/`...` line and no tab.

Renderer integration tests (`render_note`/`render_task`/`render_frontmatter`)
that assert exact bytes for **non-empty** templates get their expected strings
updated to the serializer's normalized form. The **empty-template byte-for-byte**
tests stay unchanged and must stay green.

## Out of scope

- The managed frontmatter blocks stay hand-built with `yaml_quote` (surgical
  change; keeps the byte-for-byte empty-template guarantee trivially true).
- Body templates and `assemble_body` are untouched.
- No new IPC, config field, or frontend change — this is a pure `core` internal
  swap behind the existing renderers.

## Risks

- **Crate fails `cargo deny`** → gate-zero catches it; fall back to `saphyr`.
- **LOC/quality baselines** — the new function is comparable in size to the
  three helpers it removes; re-run `check:loc`/`check:quality` and only
  `--update` a baseline that genuinely improves (shrink-only rule).
- **Duplicate keys** in a user block → the parser errors → whole block dropped
  (acceptable, predictable, logged).
- **Sentinel collision** — a value literally containing `\u{E000}<n>\u{E000}` is
  astronomically unlikely (PUA delimiter) and comes only from app-supplied vars;
  noted, not guarded.

## Invariants preserved

- Extra frontmatter can never break the fence or redefine a managed key (now by
  construction, not heuristic).
- Empty/unset template → today's exact output, byte-for-byte.
- `core` stays pure and Linux-testable; `log::warn!` on every dropped block (no
  swallowed errors).
