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

/// Emit an id bare ONLY when the token is provably not implicitly typed by
/// YAML; quote it otherwise. Shared by every path that writes an id VALUE
/// (`render_task`'s create path and the surgical parent writer), so the two
/// cannot disagree about when quoting is needed.
///
/// Inverted on purpose: enumerating YAML's implicit types (null, bool, int,
/// float, hex, sexagesimal, `.inf`/`.nan`, timestamp) is a losing game, but
/// "starts with an ASCII letter" rules out every numeric and date form in one
/// stroke, since all of them begin with a digit, `.`, `-` or `+`. That leaves
/// only the bool/null keywords to name explicitly.
///
/// This matters beyond our own ids: `ensure_id` preserves ANY usable existing
/// value, so a hand-authored `task-id: "[legacy]"` or `"123"` would otherwise be
/// re-emitted bare as `parent-id: [legacy]` (a flow sequence the strict reader
/// rejects outright, orphaning the child) or `parent-id: 123` (retyped as a
/// NUMBER by Obsidian/Dataview while the source id is still a string, so
/// equality silently stops matching). Every GENERATED id is letter-first base36,
/// so the common case still writes bare (Codex P2 x3, PR #77).
pub(super) fn quote_id_if_needed(id: &str) -> String {
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
        crate::yaml_scalar::yaml_quote(id)
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
