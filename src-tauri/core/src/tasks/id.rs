//! Task ID generation + property-name validation. IDs are short random
//! handles (opt-in per vault) written under a configurable frontmatter
//! property, giving tasks a stable identifier for Dataview/links without a
//! vault scan or a cross-device sequential collision.

/// A short random task ID: 8 base36 characters (`0-9a-z`) from the OS CSPRNG.
pub fn new_task_id() -> String {
    const ALPHA: &[u8; 26] = b"abcdefghijklmnopqrstuvwxyz";
    const BASE36: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut bytes = [0u8; 8];
    // getrandom only fails on a broken OS RNG; a loud panic is correct here
    // (mirrors mcp::token::generate_token).
    getrandom::fill(&mut bytes).expect("OS RNG unavailable");
    // First char is always a letter so the id is never all-digits (or
    // scientific-notation-shaped): Obsidian/Dataview must read it as a string,
    // not a number, for `task-id`-keyed queries to match.
    let mut s = String::with_capacity(8);
    s.push(ALPHA[bytes[0] as usize % 26] as char);
    for b in &bytes[1..] {
        s.push(BASE36[*b as usize % 36] as char);
    }
    s
}

/// True iff `name` is a safe frontmatter key for the ID property: non-empty,
/// `[A-Za-z0-9_-]` only, and not a reserved structured task key (case-folded —
/// Obsidian folds frontmatter keys, so `Status`/`DUE` collide with the real
/// fields even though the charset alone would accept them).
pub fn is_valid_id_property(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        && !super::RESERVED_TASK_KEYS.contains(&name.to_ascii_lowercase().as_str())
}

/// The exact YAML text to write as a REFERENCE to `key`'s current value in
/// `content` (a parent's id, copied into a child's `parent-id`) — chosen so a
/// YAML decoder resolves the reference to the SAME value the source line
/// resolves to, not merely the same Rust string (review, PR #77).
///
/// The previous approach — decode the source to a Rust `String`, then
/// heuristically re-encode THAT string (`quote_id_if_needed`, below) — throws
/// away exactly the information that decides two sources' true meaning:
///
/// - **Implicitly typed** (`123`, `true`, `2026-07-25`): YAML resolves the
///   TYPE from the bare token itself. `task-id: 123` is the NUMBER 123, not
///   the string "123" — quoting it for the child (`parent-id: "123"`) makes
///   the two properties resolve to different TYPES, so an equality-based
///   Dataview query between them stops matching.
/// - **Tag-decorated** (`!!str 123`): the tag, not the text, decides the
///   type. The strict decoder does not understand tags — it returns the tag
///   syntax as opaque plain-scalar text — so decoding to the Rust string
///   `"!!str 123"` and re-quoting writes a STRING whose content is literal
///   tag syntax: nonsense, matching neither the parent's resolved value nor
///   anything sensible.
///
/// Mirroring the raw (comment-stripped) source text verbatim fixes both: the
/// child's copy goes through the identical implicit-typing or tag resolution
/// the parent's own line does.
///
/// **Anchor-decorated** (`&stable abc`) is the one raw form that must NOT be
/// mirrored verbatim: copying it would DEFINE A SECOND ANCHOR of the same
/// name in the child's own document — invalid or misleadingly-redefined
/// YAML, not a reference. There is nothing here for a reference to
/// legitimately copy but the VALUE, so the anchor annotation is stripped and
/// the remainder — `abc` — is what gets classified and mirrored (anchor
/// names are matched as `[A-Za-z0-9_-]+`, the same conservative charset this
/// module already uses for id PROPERTY names; an anchor using some other
/// character is a corner this app's line-oriented reader was never a full
/// YAML parser for, and falls back to the safe branch below instead of
/// risking a wrong split).
///
/// A QUOTED source (`"abc"`, `'abc'`) is UNCHANGED: quoting already commits
/// the source to string type, so re-deriving a safe plain-or-quoted spelling
/// from the DECODED string (`quote_id_if_needed`) always resolves back to
/// the identical string — decode-then-reencode loses no meaning a raw copy
/// would have preserved, and it also carries the control-character
/// round-trip fix (`yaml_quote_multiline`, so `task-id: "a\nb"` mirrors a
/// child that decodes back to the same real newline rather than a flattened
/// space). This is also the fallback for a raw form this function declines
/// to handle (an alias, or — defensively — a lookup that somehow finds no
/// raw text at all): `decoded` is always at least a SAFE value to fall back
/// to, even when it is not the most literal one.
///
/// An ALIAS source (`*stable`) has no reachable caller today:
/// `strict_scalar_field` refuses to decode one at all (see its own doc
/// comment), so `ensure_id` never treats it as a usable existing value and
/// `services::tasks::parent`'s validation refuses the assignment before this
/// function would ever be called. It is still classified defensively here
/// (falling back to the quoted-source encoding of `decoded`, never mirrored)
/// so this function can never manufacture a dangling or colliding alias if
/// that upstream invariant is ever weakened.
///
/// **Read-side parity.** `strict_scalar_field` strips the same anchor
/// annotation via the shared `strip_anchor` helper below, so a task's own id
/// (as `scalar_id_ci`/`list_tasks` report it) and this function's mirrored
/// output always resolve an anchored source to the identical value — three-
/// way agreement with js-yaml/Dataview, not just this function being
/// internally reasonable. Read that function's doc comment for the
/// regression this closes: `strip_anchor` used to be this module's own,
/// unshared helper.
pub fn mirror_id_reference(content: &str, key: &str, decoded: &str) -> String {
    match super::parse::top_level_raw_value_ci(content, key) {
        // `strip_scalar_comment`, not the naive `strip_inline_comment`: a
        // tag-decorated QUOTED source (`!!str "abc # def"`) has an embedded
        // `#` that the naive scan truncates ON (task report, case a) —
        // see its own doc comment for why only THIS shape needs the
        // quote-aware handling here.
        Some(raw) => mirror_or_fall_back(super::parse::strip_scalar_comment(raw).trim(), decoded),
        // Unreachable in practice: this is only ever called with a `key`
        // `update_task_fields` (or its own forecast) just confirmed has a
        // usable value under. Kept so a future caller mistake degrades to a
        // safe encoding instead of a panic or a garbled mirror.
        None => quote_id_if_needed(decoded),
    }
}

/// Classify the (already comment-stripped) raw source text and either mirror
/// it verbatim or fall back to the safe, decoded-string encoding — see
/// `mirror_id_reference`'s doc comment for the reasoning behind each branch.
fn mirror_or_fall_back(stripped: &str, decoded: &str) -> String {
    if stripped.starts_with('&') {
        // Never fall through to mirroring THIS branch's own text verbatim —
        // that would either mirror a real anchor annotation (defining a
        // second one of the same name in the child) or, if `strip_anchor`
        // can't cleanly find where the name ends (no separating whitespace —
        // a malformed or unsupported source), risk mirroring a stray `&`
        // into the child. Either way, a value that starts with `&` and isn't
        // cleanly splittable falls back to the safe encoding rather than
        // being classified as an ordinary plain/tag scalar.
        return match strip_anchor(stripped) {
            Some(value) => classify(value, decoded),
            None => quote_id_if_needed(decoded),
        };
    }
    classify(stripped, decoded)
}

/// The final classification, once any leading anchor is out of the way:
/// mirror verbatim, unless it's quoted (unchanged from today — see
/// `mirror_id_reference`), an alias (defensive only), empty (nothing to
/// mirror), or — task report, case b — not actually a valid YAML scalar at
/// all (`abc: def`, `- abc`, …). That last check is a real YAML parse, not
/// another hand-rolled shape check: `is_valid_scalar_source` is the same
/// gate `strict_scalar_field`'s decoder now applies, so the two can never
/// again disagree about what counts as a mirrorable source (the exact way
/// this module's anchor-handling drifted from the read side once already —
/// see `strip_anchor`'s doc comment). In practice a candidate this rejects
/// is unreachable here (the caller's own `ensure_id`/assignability forecast
/// already refuses the write before this function would ever see it — see
/// `services::tasks::parent`'s regression test) — kept anyway so this
/// function can never manufacture invalid YAML if that upstream invariant
/// is ever weakened, matching the alias/malformed-anchor fallbacks above.
fn classify(candidate: &str, decoded: &str) -> String {
    if candidate.is_empty() || candidate.starts_with(['"', '\'', '*']) {
        quote_id_if_needed(decoded)
    } else if super::parse::is_valid_scalar_source(candidate) {
        candidate.to_string()
    } else {
        quote_id_if_needed(decoded)
    }
}

/// Strip a leading YAML anchor annotation (`&name`, then required
/// whitespace) from a scalar's raw text, returning the remainder — or `None`
/// when the text does not start with one (including a malformed `&` with no
/// following name, which this declines to guess at).
///
/// SHARED with the READ side: `tasks::parse::scalar::strict_scalar_field`
/// (the decoder behind `scalar_id_ci` — a task's own id, what `list_tasks`
/// reports — and the `parent-id`/`parent` readers in `tasks::parent`) calls
/// this too, so a raw `&name value` decodes to the identical `value` on
/// both sides. Before that read-side fix landed, only THIS module's write
/// side stripped the anchor: a task's own id, read back via the (then
/// unfixed) strict decoder, still carried the annotation verbatim, so it
/// could never equal what `mirror_id_reference` (below) mirrors into a
/// child's `parent-id` — the hierarchy the app itself had just written was
/// orphaned the instant it landed. One helper, reused rather than copied
/// into a second implementation, is what keeps the two sides from drifting
/// apart again — that is exactly how they drifted apart the first time.
pub(in crate::tasks) fn strip_anchor(raw: &str) -> Option<&str> {
    let rest = raw.strip_prefix('&')?;
    let name_len = rest
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_'))
        .unwrap_or(rest.len());
    let after = &rest[name_len..];
    if name_len == 0 || !after.starts_with(char::is_whitespace) {
        return None;
    }
    Some(after.trim_start())
}

/// Emit an id bare ONLY when the token is provably not implicitly typed by
/// YAML; quote it otherwise. The safe fallback `mirror_id_reference` (above)
/// reaches for whenever a source's raw text cannot be mirrored verbatim: a
/// QUOTED source (quoting already commits to string type, so re-deriving a
/// safe spelling from the decoded string loses no meaning), an ALIAS (no
/// literal value to mirror), or the never-normally-reached "no raw text
/// found" case.
///
/// Inverted on purpose: enumerating YAML's implicit types (null, bool, int,
/// float, hex, sexagesimal, `.inf`/`.nan`, timestamp) is a losing game, but
/// "starts with an ASCII letter" rules out every numeric and date form in one
/// stroke, since all of them begin with a digit, `.`, `-` or `+`. That leaves
/// only the bool/null keywords to name explicitly.
///
/// The quoting encoder is `yaml_quote_multiline`, NOT `yaml_quote`, because it
/// must be the exact inverse of the decoder that produced the value: the strict
/// reader decodes through `yaml_unquote_multiline`, so `task-id: "a\nb"` yields a
/// REAL newline, and `yaml_quote` flattens `\n`/`\r` back to spaces. That
/// round-trip loss is silent and fatal here — `parent-id: "a b"` no longer equals
/// the parent's own id, orphaning the relationship the instant it is written.
/// The two encoders agree on every value that has no control characters, so this
/// changes nothing for ordinary ids (Codex P2, PR #77).
fn quote_id_if_needed(id: &str) -> String {
    let plain_charset = !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    let letter_first = id.chars().next().is_some_and(|c| c.is_ascii_alphabetic());
    let keyword = matches!(
        id.to_ascii_lowercase().as_str(),
        "null" | "nil" | "true" | "false" | "yes" | "no" | "on" | "off" | "y" | "n"
    );
    if plain_charset && letter_first && !keyword {
        id.to_string()
    } else {
        crate::yaml_scalar::yaml_quote_multiline(id)
    }
}

/// The frontmatter property a generated id should be written under, or `None`
/// when id generation is OFF or the configured property is not a safe,
/// non-reserved key. One chokepoint so the create (`add_task`) and edit
/// (`update_task`) paths can never drift on the gate. Logs and skips on an
/// invalid property (a hand-edited config can set one the settings command
/// would reject).
pub fn id_property_for_generation(enabled: bool, property: &str) -> Option<&str> {
    if !enabled {
        return None;
    }
    if is_valid_id_property(property) {
        Some(property)
    } else {
        log::warn!(
            "task id generation: property {property:?} is not a valid frontmatter key; skipping"
        );
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(line: &str) -> String {
        format!("---\ntype: Task\n{line}\n---\n")
    }

    #[test]
    fn mirror_id_reference_writes_implicitly_typed_tokens_bare() {
        // `task-id: 123` is the NUMBER 123 in YAML, not the string "123" — a
        // quoted `parent-id: "123"` would retype it. Mirroring the bare
        // token keeps both properties resolving to the same value.
        for (line, decoded) in [
            ("task-id: 123", "123"),
            ("task-id: true", "true"),
            ("task-id: 2026-07-25", "2026-07-25"),
        ] {
            assert_eq!(
                mirror_id_reference(&doc(line), "task-id", decoded),
                decoded,
                "{line} must mirror bare"
            );
        }
    }

    #[test]
    fn mirror_id_reference_writes_a_tag_decorated_token_verbatim() {
        // `!!str 123` forces the string type via an explicit tag; the
        // decoded Rust string is the opaque "!!str 123" (the strict decoder
        // doesn't interpret tags), so mirroring it verbatim is what lets the
        // child's copy resolve through the SAME tag rather than becoming a
        // quoted string literally containing tag syntax.
        assert_eq!(
            mirror_id_reference(&doc("task-id: !!str 123"), "task-id", "!!str 123"),
            "!!str 123"
        );
    }

    #[test]
    fn mirror_id_reference_mirrors_a_tag_decorated_quoted_scalar_with_an_embedded_hash() {
        // Case (a), task report: `strip_inline_comment` is not quote-aware,
        // so it truncates INSIDE the quotes at the embedded `#` — `!!str
        // "abc # def"` mirrored as the unterminated `!!str "abc`, written
        // straight into the child's frontmatter. A plain quoted source (no
        // tag) survives today only by accident: `classify`'s own
        // quote-prefix check still recognizes the (corrupted) text as
        // quote-fronted and falls back to the safe encoding regardless — the
        // TAG is what hides that leading quote from `classify`, which is why
        // this needs tag + quotes together to actually corrupt anything.
        let doc = "---\ntype: Task\ntask-id: !!str \"abc # def\"\n---\n";
        let mirrored = mirror_id_reference(doc, "task-id", "WRONG-FALLBACK");
        assert_eq!(
            mirrored, "!!str \"abc # def\"",
            "must mirror the FULL tagged+quoted text, comment and all"
        );
        assert_parses_as_yaml_scalar(&mirrored);
    }

    #[test]
    fn mirror_id_reference_never_mirrors_an_invalid_plain_scalar_verbatim() {
        // Case (b), task report: `abc: def` is not a valid YAML plain scalar
        // — an unquoted `key: value` shape is a same-line nested mapping,
        // forbidden outside flow context — yet nothing here classified it as
        // anything other than an ordinary mirrorable token, so
        // `parent-id: abc: def` was written straight into a child,
        // corrupting its frontmatter even though the reference itself
        // "resolved" under this app's own line-oriented reader.
        let doc = "---\ntype: Task\ntask-id: abc: def\n---\n";
        let mirrored = mirror_id_reference(doc, "task-id", "fallback-id");
        assert_ne!(
            mirrored, "abc: def",
            "must never mirror an invalid plain scalar verbatim"
        );
        assert_parses_as_yaml_scalar(&mirrored);
    }

    /// Parse-based validity assertion (task report: "assert by parsing, not
    /// by string comparison" — a string assertion alone would not have
    /// caught this bug class, which is exactly why it shipped). No `---`
    /// fences: wrapping a lone value in a fenced document makes the parser
    /// see TWO documents and error for an unrelated reason — the dead
    /// probe's own harness bug, reproduced and diagnosed fresh for this fix.
    fn assert_parses_as_yaml_scalar(value: &str) {
        let frontmatter = format!("parent-id: {value}\n");
        serde_yaml_ng::from_str::<serde_yaml_ng::Value>(&frontmatter).unwrap_or_else(|e| {
            panic!("mirrored parent-id must parse as valid YAML: {e}\n---\n{frontmatter}")
        });
    }

    #[test]
    fn mirror_id_reference_strips_an_anchor_and_mirrors_the_value() {
        // Copying `&stable abc` verbatim would DEFINE a second anchor named
        // "stable" in the child's own document. Only the value, `abc`, is
        // safe to copy.
        assert_eq!(
            mirror_id_reference(&doc("task-id: &stable abc"), "task-id", "&stable abc"),
            "abc"
        );
        // The stripped value is itself re-classified: an anchored NUMBER-
        // shaped token must still mirror bare, not fall back to quoting.
        assert_eq!(
            mirror_id_reference(&doc("task-id: &stable 123"), "task-id", "&stable 123"),
            "123"
        );
    }

    #[test]
    fn mirror_id_reference_falls_back_safely_for_a_malformed_anchor() {
        // No separating whitespace after the anchor name: this reader can't
        // tell where the name ends and a value would begin, so it declines
        // to guess rather than risk mirroring a stray `&` into the child
        // (which would define an anchor there, not a reference).
        let out = mirror_id_reference(&doc("task-id: &stableabc"), "task-id", "&stableabc");
        assert_eq!(out, "\"&stableabc\"");
        assert!(!out.starts_with('&'));
    }

    #[test]
    fn mirror_id_reference_strips_a_trailing_comment_before_mirroring() {
        // raw_scalar_field (capture_note.rs) does not strip a trailing
        // comment — only mirror_id_reference's own comment-strip does, so
        // the parent's edit-history prose never leaks into the child.
        assert_eq!(
            mirror_id_reference(&doc("task-id: 123 # was xyz"), "task-id", "123"),
            "123"
        );
    }

    #[test]
    fn mirror_id_reference_leaves_a_quoted_source_unchanged() {
        // Quoting already commits the source to string type, so re-deriving
        // a safe spelling from the DECODED string is correct — this is
        // "unchanged from today" by design, not a gap in the fix.
        assert_eq!(
            mirror_id_reference(&doc("task-id: \"abc\""), "task-id", "abc"),
            "abc" // plain-safe decoded string still writes bare
        );
        assert_eq!(
            mirror_id_reference(&doc("task-id: \"123\""), "task-id", "123"),
            "\"123\"" // decoded "123" is not plain-safe -> quoted, matches source's string type
        );
        // The control-character round trip this subsumes: a quoted source
        // with an escaped newline must still round-trip through the SAME
        // control character, not a flattened space.
        assert_eq!(
            mirror_id_reference(&doc("task-id: \"a\\nb\""), "task-id", "a\nb"),
            "\"a\\nb\""
        );
    }

    #[test]
    fn mirror_id_reference_falls_back_safely_for_an_alias_source() {
        // Defensive only: `strict_scalar_field` now refuses to decode an
        // alias at all, so no real caller reaches this with a `decoded`
        // derived from one. Still pinned directly so this function can
        // never manufacture a dangling/colliding alias if that upstream
        // invariant is ever weakened — mirroring "*stable" verbatim would
        // reference an anchor that may not exist (or may be a different
        // node) in the child's own document.
        let out = mirror_id_reference(&doc("task-id: *stable"), "task-id", "*stable");
        assert_eq!(out, "\"*stable\"");
        assert!(!out.starts_with('*'));
    }

    #[test]
    fn mirror_id_reference_falls_back_when_the_key_is_entirely_absent() {
        // Defensive only: `mirror_id_reference` is always called with a key
        // the caller just confirmed has a usable value under. A lookup
        // miss still degrades to a safe encoding rather than panicking.
        assert_eq!(
            mirror_id_reference("---\ntype: Task\n---\n", "task-id", "fallback-id"),
            "fallback-id"
        );
    }

    #[test]
    fn quote_id_if_needed_round_trips_a_value_through_the_strict_reader() {
        // The encoder must be the exact inverse of the decoder that produced the
        // value, or a preserved id changes meaning on its way into `parent-id`.
        // `yaml_quote` FLATTENS \n and \r to spaces, while the reader
        // (`strict_scalar_field` -> `yaml_unquote_multiline`) decodes them to the
        // real control characters — so `task-id: "a\nb"` was re-emitted as
        // `parent-id: "a b"`, which no longer equals the parent's own id and
        // orphans the relationship the instant it is created (Codex P2, PR #77).
        let doc = |v: &str| format!("---\ntype: Task\ntask-id: {v}\n---\n");
        for encoded_on_disk in [
            r#""a\nb""#,   // newline
            r#""a\rb""#,   // carriage return
            r#""a\tb""#,   // tab
            r#""a\x1bb""#, // ESC — a C0 control the encoder must not emit raw
        ] {
            let decoded =
                super::super::parse::strict_scalar_field(&doc(encoded_on_disk), "task-id", false)
                    .expect("the reader accepts a single-line double-quoted scalar");
            let re_encoded = quote_id_if_needed(&decoded);
            let re_decoded =
                super::super::parse::strict_scalar_field(&doc(&re_encoded), "task-id", false)
                    .expect("what we write must be readable again");
            assert_eq!(
                re_decoded, decoded,
                "{encoded_on_disk} must survive decode -> encode -> decode unchanged"
            );
        }
    }

    #[test]
    fn quote_id_if_needed_keeps_generated_ids_bare_and_quotes_typed_tokens() {
        // Generated ids are letter-first base36 — the common case stays clean.
        assert_eq!(quote_id_if_needed("k3m9x2qp"), "k3m9x2qp");
        assert_eq!(quote_id_if_needed("uid_2"), "uid_2");
        // Syntax that would break or retype the value.
        assert_eq!(quote_id_if_needed("[legacy]"), "\"[legacy]\"");
        assert_eq!(quote_id_if_needed("has space"), "\"has space\"");
        for kw in [
            "null", "NULL", "true", "False", "yes", "no", "on", "off", "y", "n",
        ] {
            assert_eq!(
                quote_id_if_needed(kw),
                format!("\"{kw}\""),
                "{kw} must be quoted"
            );
        }
        // Numeric / date / special forms — all caught by the letter-first rule.
        for typed in ["123", "0x1F", "1e3", "2026-07-25", "-5", "1:30", ".inf"] {
            assert_eq!(
                quote_id_if_needed(typed),
                format!("\"{typed}\""),
                "{typed} must be quoted"
            );
        }
    }

    #[test]
    fn new_task_id_is_8_base36_chars_and_unique() {
        let a = new_task_id();
        assert_eq!(a.len(), 8);
        assert!(a
            .chars()
            .all(|c| c.is_ascii_digit() || c.is_ascii_lowercase()));
        // Never number-typed: an all-digit (or \d+e\d+-shaped) id would be read
        // as a NUMBER by Obsidian/Dataview when emitted unquoted, breaking
        // `WHERE task-id = "…"` string-equality queries. Forcing a leading
        // letter rules that out.
        assert!(a.chars().next().unwrap().is_ascii_lowercase());
        // Weak uniqueness over a 26·36^7 space — a collision in 1000 draws is
        // effectively impossible; this pins that the source is actually random.
        let mut seen = std::collections::HashSet::new();
        for _ in 0..1000 {
            assert!(seen.insert(new_task_id()));
        }
    }

    #[test]
    fn id_property_for_generation_gates_on_enabled_and_validity() {
        assert_eq!(id_property_for_generation(false, "task-id"), None); // disabled
        assert_eq!(id_property_for_generation(true, "task-id"), Some("task-id"));
        assert_eq!(id_property_for_generation(true, "uid"), Some("uid"));
        assert_eq!(id_property_for_generation(true, "status"), None); // reserved
        assert_eq!(id_property_for_generation(true, "Status"), None); // case-folded reserved
        assert_eq!(id_property_for_generation(true, ""), None); // empty/invalid charset
        assert_eq!(id_property_for_generation(true, "scheduled"), None); // reserved (do-date)
        assert_eq!(id_property_for_generation(true, "description"), None); // reserved (detail)
    }

    #[test]
    fn is_valid_id_property_charset_and_reserved() {
        assert!(is_valid_id_property("task-id"));
        assert!(is_valid_id_property("uid_2"));
        assert!(!is_valid_id_property("")); // empty
        assert!(!is_valid_id_property("task id")); // space
        assert!(!is_valid_id_property("task:id")); // colon
        for reserved in [
            "type",
            "status",
            "title",
            "created",
            "due",
            "scheduled",
            "priority",
            "tags",
            "tag",
            "order",
            "description",
            "parent-id",
            "parent",
        ] {
            assert!(
                !is_valid_id_property(reserved),
                "{reserved} must be rejected"
            );
        }
        // The reserved check must be case-insensitive: Obsidian folds
        // frontmatter keys, so "Status"/"DUE" collide with the real fields
        // even though the charset check alone would accept them (A-Z allowed).
        assert!(
            is_valid_id_property("Task-ID"),
            "an uppercase NON-reserved name must still be accepted"
        );
        assert!(!is_valid_id_property("Status"));
        assert!(!is_valid_id_property("DUE"));
    }
}
