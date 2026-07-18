# Real-YAML extra-frontmatter sanitizer — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the line-heuristic `sanitize_extra_frontmatter` with a real YAML parser so per-vault extra-frontmatter templates are made safe by parsing, not pattern-matching — keeping `{{placeholder}}` templates and guaranteeing Obsidian-compatible output.

**Architecture:** One new `core::template::render_extra_frontmatter(template, vars, reserved)` folds the current `substitute`-then-`sanitize` pair into a pipeline: tokenize `{{placeholders}}` → safe sentinels, parse with a real YAML crate, resolve values *inside* the parsed tree, drop reserved top-level keys, re-emit standard YAML mapping lines. The three renderers (`capture_note`, `tasks::disk`, `document_import`) swap their call site to it; the old heuristic and its helpers are deleted.

**Tech Stack:** Rust, `vault_buddy_core` (pure crate, Linux-testable), `serde_yaml_ng` (YAML parse/emit), existing `log` for dropped-block warnings.

## Global Constraints

- **Crate gate (gate-zero):** the YAML crate MUST pass `cargo deny check` (see `src-tauri/deny.toml`: `unmaintained = "workspace"`, `unknown-git = "deny"`, license allowlist). Primary: `serde_yaml_ng`. If it fails the gate, fall back to `saphyr`. NEVER add a `deny.toml` ignore entry to force a crate through.
- **Obsidian-compatible output:** standard `js-yaml`-parseable mapping lines — no `---`/`...` document markers inside the block, spaces never tabs, placeholder values stay strings (a `{{title}}` = `"2026"` is a text property, not a number).
- **Byte-for-byte guarantee:** an unset/empty (`None`/blank-after-trim) template injects nothing, so every renderer's output with no template is unchanged. The `*_byte_identical_with_empty_templates` tests must stay green.
- **Reserved keys are per renderer** (copied verbatim into each call site — see Task 3).
- **No swallowed errors:** every path that drops a block calls `log::warn!` (diagnostics invariant).
- **Purity:** all logic stays in `vault_buddy_core`; no Tauri types, Linux-testable.
- **Quality gates are shrink-only:** `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo deny check`, `cargo machete`, `cargo llvm-cov … --fail-under-lines 94`, `npm run check:loc`, `npm run check:quality`. Update a baseline only when the metric genuinely improves, with a reason.

---

## File Structure

- `src-tauri/core/Cargo.toml` — add the YAML dependency (Task 1).
- `src-tauri/core/src/template.rs` — add `render_extra_frontmatter` + private `expand_placeholders`/`tokenize`/`resolve_value`; refactor `substitute` onto the shared walker; later remove `sanitize_extra_frontmatter`/`emit_line`/`unquote_key` (Tasks 2–3).
- `src-tauri/core/src/capture_note.rs` — swap call site (Task 3).
- `src-tauri/core/src/document_import.rs` — swap call site (Task 3).
- `src-tauri/core/src/tasks/disk.rs` — swap call site (Task 3).
- `AGENTS.md`, `docs/Gaps.md` — reconcile the "substitute-then-sanitize" prose (Task 3).

---

## Task 1: Add the YAML dependency and prove the gate (gate-zero)

**Files:**
- Modify: `src-tauri/core/Cargo.toml`
- Test: `src-tauri/core/src/template.rs` (one temporary smoke test)

**Interfaces:**
- Consumes: nothing.
- Produces: a vetted `serde_yaml_ng` dependency confirmed to (a) pass `cargo deny`, (b) preserve mapping key order on round-trip, (c) emit no document markers for a root mapping — the three properties the whole design rests on.

- [ ] **Step 1: Add the dependency**

In `src-tauri/core/Cargo.toml`, under `[dependencies]` (alphabetical-ish, after `serde_json`), add:

```toml
serde_yaml_ng = "0.10"
```

If `0.10` does not resolve, run `cd src-tauri/core && cargo add serde_yaml_ng` to pin the latest published `0.x`.

- [ ] **Step 2: Fetch + compile**

Run: `cd src-tauri/core && cargo build`
Expected: compiles; `serde_yaml_ng` is downloaded.

- [ ] **Step 3: Gate-zero — cargo deny MUST be green**

Run: `cd src-tauri && cargo deny check 2>&1 | tail -30`
Expected: `advisories ok`, `licenses ok`, `sources ok`, `bans` (warn only). **No new error** attributable to `serde_yaml_ng`.

If `serde_yaml_ng` is flagged (advisory/license/source): **STOP.** Remove it, and instead add `saphyr = "0.0.6"` (or latest published), re-run this step, and adapt Task 2's parse/emit code to `saphyr`'s `Yaml` AST + `YamlEmitter` (same pipeline: tokenize → `saphyr::Yaml::load_from_str` → resolve sentinels in `Yaml::String` nodes → drop reserved top-level `Yaml::Hash` keys → `YamlEmitter` to a String, then strip the leading `---` line the emitter prepends). Record the swap in the task notes.

- [ ] **Step 4: Write the crate-behaviour smoke test**

Add to the `tests` module in `src-tauri/core/src/template.rs`:

```rust
#[test]
fn yaml_crate_preserves_key_order_and_emits_no_doc_markers() {
    // Gate-zero behaviour the design depends on: parse→emit keeps insertion
    // order and does not wrap a root mapping in ---/... markers.
    let v: serde_yaml_ng::Value = serde_yaml_ng::from_str("b: 2\na: 1\n").unwrap();
    let out = serde_yaml_ng::to_string(&v).unwrap();
    assert_eq!(out, "b: 2\na: 1\n");
    assert!(!out.contains("---") && !out.contains("..."));
}
```

- [ ] **Step 5: Run the smoke test**

Run: `cd src-tauri/core && cargo test yaml_crate_preserves_key_order -- --nocapture`
Expected: PASS. (If order is NOT preserved or markers appear, the crate is unsuitable — revisit Step 3's fallback.)

- [ ] **Step 6: Commit**

```bash
git add src-tauri/core/Cargo.toml src-tauri/core/Cargo.lock src-tauri/core/src/template.rs
git commit -m "build(core): add serde_yaml_ng for real-YAML frontmatter sanitizing

Gate-zero: cargo deny green; a smoke test pins the two properties the
sanitizer rewrite depends on — key-order preservation and no document
markers on a root mapping."
```

---

## Task 2: Implement `render_extra_frontmatter` (TDD)

**Files:**
- Modify: `src-tauri/core/src/template.rs`
- Test: `src-tauri/core/src/template.rs` (tests module)

**Interfaces:**
- Consumes: `serde_yaml_ng::{Value, Mapping}` (Task 1).
- Produces: `pub fn render_extra_frontmatter(template: &str, vars: &[(&str, &str)], reserved: &[&str]) -> String` — returns injectable mapping lines (`\n`-terminated) or `""`. Private helpers `expand_placeholders`, `tokenize`, `resolve_value`, `sentinel`. `substitute`'s public signature is unchanged (refactored onto `expand_placeholders`).

Note: the OLD `sanitize_extra_frontmatter`/`emit_line`/`unquote_key` stay in place this task (still used by the three renderers, so the crate keeps compiling). They are removed in Task 3.

- [ ] **Step 1: Write the failing tests**

Add these to the `tests` module in `src-tauri/core/src/template.rs`. (Expected strings reflect `serde_yaml_ng`'s serializer; if the crate's cosmetic quoting differs — single vs double quotes, `null` spelling — update the expected to the observed output. The invariant under test is safety/structure/ordering, and any valid quoting is Obsidian-compatible.)

```rust
#[test]
fn render_resolves_placeholders_and_preserves_key_order() {
    let vars = [("title", "Buy milk"), ("date", "2026-07-18")];
    assert_eq!(
        render_extra_frontmatter("name: {{title}}\nwhen: {{date}}", &vars, &[]),
        "name: Buy milk\nwhen: 2026-07-18\n"
    );
}

#[test]
fn render_keeps_a_colon_value_as_one_safe_scalar() {
    // A substituted value with a colon-space would read as a nested mapping if
    // injected raw; via the parsed tree it is one quoted scalar.
    assert_eq!(
        render_extra_frontmatter("summary: {{t}}", &[("t", "Ship: v1")], &[]),
        "summary: 'Ship: v1'\n"
    );
}

#[test]
fn render_keeps_bracket_and_numeric_values_as_strings() {
    // `[draft]` stays a string (not a flow sequence); a numeric placeholder
    // value stays a string so Obsidian reads it as text, not a number.
    assert_eq!(
        render_extra_frontmatter("label: {{t}}", &[("t", "[draft]")], &[]),
        "label: '[draft]'\n"
    );
    assert_eq!(
        render_extra_frontmatter("year: {{t}}", &[("t", "2026")], &[]),
        "year: '2026'\n"
    );
}

#[test]
fn render_keeps_a_literal_number_as_a_number() {
    assert_eq!(render_extra_frontmatter("count: 5", &[], &[]), "count: 5\n");
}

#[test]
fn render_drops_reserved_keys_plain_quoted_and_case_insensitive() {
    assert_eq!(
        render_extra_frontmatter("type: Evil\nkeep: kept", &[], &["type"]),
        "keep: kept\n"
    );
    assert_eq!(
        render_extra_frontmatter("\"type\": Evil\nkeep: kept", &[], &["type"]),
        "keep: kept\n"
    );
    assert_eq!(
        render_extra_frontmatter("Type: Evil\nkeep: kept", &[], &["type"]),
        "keep: kept\n"
    );
    // A non-reserved key is kept.
    assert_eq!(
        render_extra_frontmatter("project: Alpha", &[], &["type"]),
        "project: Alpha\n"
    );
}

#[test]
fn render_resolves_a_placeholder_inside_a_sequence() {
    // Capability the line heuristic never had: placeholders inside collections.
    assert_eq!(
        render_extra_frontmatter("people: [{{a}}, {{b}}]", &[("a", "Alex"), ("b", "Sam")], &[]),
        "people:\n- Alex\n- Sam\n"
    );
}

#[test]
fn render_drops_injected_markers_sequences_and_bare_scalars() {
    // A stray fence makes it multi-document → dropped; a sequence or scalar
    // root is not a mapping → dropped.
    assert_eq!(render_extra_frontmatter("owner: me\n---\nsneaky: 1", &[], &[]), "");
    assert_eq!(render_extra_frontmatter("- a\n- b", &[], &[]), "");
    assert_eq!(render_extra_frontmatter("just text", &[], &[]), "");
}

#[test]
fn render_empty_and_all_reserved_yield_empty_never_brace() {
    assert_eq!(render_extra_frontmatter("", &[], &["type"]), "");
    assert_eq!(render_extra_frontmatter("   \n  ", &[], &["type"]), "");
    // Every key reserved → empty mapping → "" (never the serializer's `{}`).
    assert_eq!(
        render_extra_frontmatter("type: a\ntags: b", &[], &["type", "tags"]),
        ""
    );
}

#[test]
fn render_malformed_yaml_drops_the_block() {
    assert_eq!(render_extra_frontmatter("key: [unclosed", &[], &[]), "");
}

#[test]
fn render_output_is_obsidian_safe_no_markers_no_tabs() {
    let out = render_extra_frontmatter("a: 1\nb: {{t}}", &[("t", "x")], &["z"]);
    assert!(!out.contains('\t'), "no tabs: {out}");
    for line in out.lines() {
        assert!(line.trim() != "---" && line.trim() != "...", "no markers: {out}");
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd src-tauri/core && cargo test render_ 2>&1 | tail -20`
Expected: FAIL — `cannot find function render_extra_frontmatter in this scope`.

- [ ] **Step 3: Add the imports and the shared placeholder walker**

At the top of `src-tauri/core/src/template.rs`, under the module doc comment, add:

```rust
use serde_yaml_ng::{Mapping, Value};
```

Add the shared walker and refactor `substitute` onto it (behaviour unchanged — the existing `substitute` tests are the regression guard). Replace the current `substitute` body with:

```rust
/// A sentinel wrapping a Private-Use-Area delimiter (U+E000) — a valid YAML
/// plain-scalar character (YAML c-printable includes U+E000–U+FFFD) that cannot
/// occur in real input, so a `{{placeholder}}` parses as opaque structure and
/// its value is spliced back after parsing.
fn sentinel(i: usize) -> String {
    format!("\u{E000}{i}\u{E000}")
}

/// Walk `{{key}}` placeholders (whitespace inside the braces tolerated),
/// pushing `resolve(key)` for each. An unclosed `{{` is emitted literally.
/// UTF-8 safe. Shared by `substitute` (body templates) and `tokenize`
/// (frontmatter), so the two can never disagree on placeholder syntax.
fn expand_placeholders(template: &str, mut resolve: impl FnMut(&str) -> String) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        if let Some(end) = after.find("}}") {
            out.push_str(&resolve(after[..end].trim()));
            rest = &after[end + 2..];
        } else {
            out.push_str("{{");
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

/// Replace every `{{key}}` (whitespace inside the braces tolerated) with its
/// value from `vars`. An unknown key renders empty. Unclosed `{{` is emitted
/// literally. UTF-8 safe. Used for BODY templates (raw markdown); frontmatter
/// templates go through `render_extra_frontmatter`, which parses the result.
pub fn substitute(template: &str, vars: &[(&str, &str)]) -> String {
    expand_placeholders(template, |key| {
        vars.iter()
            .find(|(k, _)| *k == key)
            .map(|(_, v)| (*v).to_string())
            .unwrap_or_default()
    })
}
```

- [ ] **Step 4: Add the tokenize + resolve + render pipeline**

Add below `substitute`:

```rust
/// Replace each KNOWN `{{key}}` with a sentinel scalar (recording its value at
/// that index) so the text parses as the structure the user intended; an
/// unknown key renders empty (matching `substitute`). Returns the tokenized
/// text and the values indexed by sentinel number.
fn tokenize(template: &str, vars: &[(&str, &str)]) -> (String, Vec<String>) {
    let mut values: Vec<String> = Vec::new();
    let text = expand_placeholders(template, |key| {
        match vars.iter().find(|(k, _)| *k == key) {
            Some((_, v)) => {
                let idx = values.len();
                values.push((*v).to_string());
                sentinel(idx)
            }
            None => String::new(),
        }
    });
    (text, values)
}

/// Splice recorded values back in place of their sentinels in every scalar
/// (mapping keys and values, recursively). A value lands as an opaque string,
/// so it can never inject YAML structure and a numeric-looking value stays a
/// string.
fn resolve_value(v: &mut Value, values: &[String]) {
    match v {
        Value::String(s) => {
            for (i, val) in values.iter().enumerate() {
                let token = sentinel(i);
                if s.contains(&token) {
                    *s = s.replace(&token, val);
                }
            }
        }
        Value::Sequence(seq) => seq.iter_mut().for_each(|e| resolve_value(e, values)),
        Value::Mapping(map) => {
            // Rebuild so KEYS are resolved too; the IndexMap-backed Mapping
            // preserves insertion order across the rebuild.
            let taken = std::mem::take(map);
            for (mut k, mut val) in taken {
                resolve_value(&mut k, values);
                resolve_value(&mut val, values);
                map.insert(k, val);
            }
        }
        _ => {}
    }
}

/// Render a per-vault extra-frontmatter template into mapping lines safe to
/// inject before a closing `---`. `{{placeholders}}` are resolved via a
/// sentinel round-trip, reserved top-level keys dropped, and the result
/// re-emitted as standard, Obsidian-compatible YAML (no document markers).
/// Malformed input, a non-mapping root, or an all-reserved block yields `""`
/// (logged) — never a broken fence or a `{}` literal. Replaces the former
/// substitute-then-sanitize pair.
pub fn render_extra_frontmatter(template: &str, vars: &[(&str, &str)], reserved: &[&str]) -> String {
    let (tokenized, values) = tokenize(template, vars);
    if tokenized.trim().is_empty() {
        return String::new();
    }
    let mut root: Value = match serde_yaml_ng::from_str(&tokenized) {
        Ok(v) => v,
        Err(e) => {
            log::warn!("extra frontmatter dropped: invalid YAML ({e})");
            return String::new();
        }
    };
    resolve_value(&mut root, &values);
    let Value::Mapping(map) = root else {
        log::warn!("extra frontmatter dropped: root is not a mapping");
        return String::new();
    };
    let kept: Mapping = map
        .into_iter()
        .filter(|(k, _)| match k {
            Value::String(s) => !reserved.iter().any(|r| r.eq_ignore_ascii_case(s)),
            _ => true,
        })
        .collect();
    if kept.is_empty() {
        return String::new();
    }
    let emitted = match serde_yaml_ng::to_string(&Value::Mapping(kept)) {
        Ok(s) => s,
        Err(e) => {
            log::warn!("extra frontmatter dropped: emit failed ({e})");
            return String::new();
        }
    };
    // The serializer emits no document markers for a root mapping; strip any
    // bare ---/... line defensively (we inject INSIDE our own fence) and
    // normalize to exactly one trailing newline.
    let mut lines: Vec<&str> = emitted
        .lines()
        .filter(|l| {
            let t = l.trim();
            t != "---" && t != "..."
        })
        .collect();
    while lines.last().is_some_and(|l| l.trim().is_empty()) {
        lines.pop();
    }
    if lines.is_empty() {
        return String::new();
    }
    let mut body = lines.join("\n");
    body.push('\n');
    body
}
```

- [ ] **Step 5: Run the new tests**

Run: `cd src-tauri/core && cargo test render_ 2>&1 | tail -30`
Expected: PASS. If a quoting-cosmetic assertion mismatches (e.g. double vs single quotes), update the expected string to the observed valid output and re-run.

- [ ] **Step 6: Run the whole template + substitute suite (regression)**

Run: `cd src-tauri/core && cargo test --lib template 2>&1 | tail -20`
Expected: PASS — the refactored `substitute` still passes its existing tests; the old `sanitize_*` tests still pass (unchanged this task).

- [ ] **Step 7: Commit**

```bash
git add src-tauri/core/src/template.rs
git commit -m "feat(core): render_extra_frontmatter — real-YAML sanitizing

Tokenize {{placeholders}} to sentinels, parse with serde_yaml_ng, resolve
values inside the parsed tree, drop reserved top-level keys, re-emit standard
Obsidian-compatible mapping lines. substitute is refactored onto a shared
placeholder walker (behaviour unchanged). The old heuristic still backs the
renderers until the next task swaps them."
```

---

## Task 3: Swap the three renderers, delete the heuristic, reconcile docs

**Files:**
- Modify: `src-tauri/core/src/capture_note.rs:120-123`
- Modify: `src-tauri/core/src/document_import.rs:83-86`
- Modify: `src-tauri/core/src/tasks/disk.rs:109-112`
- Modify: `src-tauri/core/src/template.rs` (delete `sanitize_extra_frontmatter`, `emit_line`, `unquote_key` and their tests; update the module doc)
- Modify: `AGENTS.md`, `docs/Gaps.md`

**Interfaces:**
- Consumes: `render_extra_frontmatter` (Task 2).
- Produces: no public API surface change for the renderers (`render_note`/`render_task`/`render_frontmatter` signatures unchanged).

- [ ] **Step 1: Swap the capture-note call site**

In `src-tauri/core/src/capture_note.rs`, replace lines 120–123:

```rust
        out.push_str(&crate::template::sanitize_extra_frontmatter(
            &crate::template::substitute(extra, &vars),
            NOTE_RESERVED,
        ));
```

with:

```rust
        out.push_str(&crate::template::render_extra_frontmatter(
            extra,
            &vars,
            NOTE_RESERVED,
        ));
```

- [ ] **Step 2: Swap the document-import call site**

In `src-tauri/core/src/document_import.rs`, replace lines 83–86:

```rust
        fm.push_str(&crate::template::sanitize_extra_frontmatter(
            &crate::template::substitute(ef, &vars),
            DOC_RESERVED,
        ));
```

with:

```rust
        fm.push_str(&crate::template::render_extra_frontmatter(ef, &vars, DOC_RESERVED));
```

- [ ] **Step 3: Swap the task call site**

In `src-tauri/core/src/tasks/disk.rs`, replace lines 109–112:

```rust
        extra.push_str(&crate::template::sanitize_extra_frontmatter(
            &crate::template::substitute(ef, &vars),
            &reserved,
        ));
```

with:

```rust
        extra.push_str(&crate::template::render_extra_frontmatter(ef, &vars, &reserved));
```

- [ ] **Step 4: Delete the dead heuristic and its tests**

In `src-tauri/core/src/template.rs`, delete the entire `sanitize_extra_frontmatter` function, the `emit_line` function, and the `unquote_key` function. In the `tests` module, delete every `sanitize_*` test: `sanitize_quotes_a_value_that_would_break_the_scalar`, `sanitize_drops_scalars_whose_colon_is_not_a_mapping_separator`, `sanitize_drops_fences_and_reserved_keys`, `sanitize_empty_in_empty_out`, `sanitize_blank_line_inside_a_dropped_block_does_not_leak_it`, `sanitize_reserved_is_case_insensitive_and_dotdotdot_fence_drops`, `sanitize_drops_top_level_sequence_entries_and_bare_scalars`, `sanitize_drops_a_quoted_reserved_key`.

Update the module doc comment at the top of the file to describe the new pipeline:

```rust
//! Additive-template primitives shared by the note/task/document renderers.
//! `substitute` fills `{{token}}` placeholders in body templates;
//! `render_extra_frontmatter` renders a frontmatter template safely — it
//! tokenizes placeholders, parses with a real YAML library, drops reserved
//! managed keys, and re-emits Obsidian-compatible mapping lines, so a user's
//! extra frontmatter can never break the fence or redefine a managed key.
```

- [ ] **Step 5: Verify the crate compiles with no dead code**

Run: `cd src-tauri/core && cargo build 2>&1 | tail -20`
Expected: compiles, no `unused function`/`never used` warnings (the old helpers are gone; `substitute` is still used by body templates and `assemble_body`).

- [ ] **Step 6: Run the full core test suite — renderers included**

Run: `cd src-tauri/core && cargo test 2>&1 | tail -30`
Expected: PASS. The three renderer template tests (`note_extra_frontmatter_injected_and_reserved_dropped`, `document_extra_frontmatter_injected_reserved_dropped_and_substituted`, `task_extra_frontmatter_and_body_apply_and_reserved_dropped`) use `.contains()`/ordering assertions and their example values re-emit identically, so they should stay green. If any fails because the serializer normalized a value, update that test's expected substring to the observed valid output and confirm the reserved key is still dropped and the fence intact.

- [ ] **Step 7: Reconcile the docs**

Grep for stale references and update prose to the new mechanism:

Run: `grep -rn "sanitize_extra_frontmatter\|substitute-then-sanitize\|substitute.then.sanitiz" AGENTS.md docs/`
Expected matches in `AGENTS.md` (the capture, document-import, and tasks domain sections describe "substitute-then-sanitize machinery" / "`core::template` substitute-then-sanitize"). For each, replace the description of the heuristic with the real-YAML pipeline — e.g. change "a shared `core::template` module backs all three template surfaces: `substitute(...)` … and `sanitize_extra_frontmatter(text, reserved)` …" to describe `render_extra_frontmatter(template, vars, reserved)` parsing with a YAML library and re-emitting Obsidian-compatible lines. Keep the managed-key guarantees (identity keys always emitted, reserved keys never user-definable) — those are unchanged. If `docs/Gaps.md` has an entry about the sanitizer heuristic's edge cases, mark it resolved.

- [ ] **Step 8: Commit**

```bash
git add src-tauri/core/src/template.rs src-tauri/core/src/capture_note.rs src-tauri/core/src/document_import.rs src-tauri/core/src/tasks/disk.rs AGENTS.md docs/Gaps.md
git commit -m "refactor(core): route renderers through render_extra_frontmatter

Swap all three renderers off the substitute-then-sanitize heuristic onto the
YAML-parsing render_extra_frontmatter; delete sanitize_extra_frontmatter,
emit_line, unquote_key and their tests. Reconcile AGENTS.md/Gaps.md prose."
```

---

## Task 4: Green the quality gates

**Files:**
- Possibly modify: `scripts/loc-baseline.json`, `scripts/quality-baseline.json` (only if a metric legitimately improved).

**Interfaces:**
- Consumes: the finished implementation (Tasks 1–3).
- Produces: a workspace that passes every CI gate locally.

- [ ] **Step 1: Rust format + lint + tests**

Run:
```bash
cd src-tauri && cargo fmt
cd src-tauri && cargo fmt --check
cd src-tauri/core && cargo clippy --all-targets -- -D warnings
cd src-tauri/core && cargo test
```
Expected: fmt clean, clippy clean (`-D warnings`), all tests pass.

- [ ] **Step 2: Dependency + coverage gates**

Run:
```bash
cd src-tauri && cargo deny check
cd src-tauri && cargo machete .
cd src-tauri && cargo llvm-cov -p vault_buddy_core -p vault_buddy_capture -p vault_buddy_transcribe --fail-under-lines 94
```
Expected: `deny` ok; `machete` reports no unused deps (`serde_yaml_ng` is used); coverage ≥ 94. If coverage dips just below 94 because of an uncoverable `log::warn!` emit-failure arm, add a targeted test for a covered warn path (malformed YAML already covers the parse-error arm) or confirm the aggregate still clears 94.

- [ ] **Step 3: Frontend/Rust LOC + quality ratchets**

Run:
```bash
npm run check:loc
npm run check:quality
```
Expected: both pass. `template.rs` net LOC should be near-neutral (added `render_extra_frontmatter` + helpers; removed `sanitize_extra_frontmatter` + `emit_line` + `unquote_key`). If `check:loc` fails on `template.rs` growth, run `npm run check:loc -- --update` and, in the same commit, add the one-line growth reason next to the entry (in-convention). If a metric shrank, `--update` to ratchet it tighter. `check:quality` (clone detector) must stay at zero — the shared `expand_placeholders` prevents a duplicated placeholder-walk clone; if it flags one, factor the duplication rather than baseline it.

- [ ] **Step 4: Commit any baseline updates**

```bash
git add scripts/loc-baseline.json scripts/quality-baseline.json
git commit -m "chore(core): update LOC/quality baselines for the sanitizer rewrite"
```
(Skip this commit if no baseline changed.)

- [ ] **Step 5: Final full-suite confirmation**

Run: `cd src-tauri/core && cargo test 2>&1 | tail -5` and `npm test 2>&1 | tail -15`
Expected: Rust core green; Vitest suite green (frontend untouched, but confirm nothing regressed).

---

## Self-Review

**Spec coverage:**
- Pipeline (tokenize→parse→resolve→drop→emit) → Task 2. ✓
- Sentinel rationale (PUA delimiter, valid YAML char) → Task 2 `sentinel` doc + tests. ✓
- Crate choice + `cargo deny` gate-zero + `saphyr` fallback → Task 1. ✓
- Obsidian-compat (no markers, no tabs, string-vs-number) → Task 2 tests `render_output_is_obsidian_safe_*`, `render_keeps_bracket_and_numeric_values_as_strings`. ✓
- API: add `render_extra_frontmatter`; remove `sanitize_extra_frontmatter`/`emit_line`/`unquote_key`; keep `substitute`/`yaml_quote` → Tasks 2–3. ✓
- Three call-site swaps with exact vars/reserved → Task 3 Steps 1–3. ✓
- Behavior changes (reformatting, key order preserved, malformed→drop) → Task 2 tests. ✓
- Empty/unset byte-for-byte guarantee → covered by existing `*_byte_identical_with_empty_templates` tests (unchanged; Task 3 Step 6 keeps them green). ✓
- `log::warn!` on every drop → Task 2 `render_extra_frontmatter` (3 warn arms). ✓
- Docs reconciliation → Task 3 Step 7. ✓
- Quality gates / baselines → Task 4. ✓

**Placeholder scan:** No TBD/TODO; every code step shows full code; every command has expected output. ✓

**Type consistency:** `render_extra_frontmatter(template: &str, vars: &[(&str,&str)], reserved: &[&str]) -> String` is used identically in Task 2 (definition) and Task 3 (all three call sites). `expand_placeholders`/`tokenize`/`resolve_value`/`sentinel` are defined and used only within Task 2. `serde_yaml_ng::{Value, Mapping}` imported in Task 2 Step 3. ✓
