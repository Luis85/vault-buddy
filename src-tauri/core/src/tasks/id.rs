//! Task ID generation + property-name validation. IDs are short random
//! handles (opt-in per vault) written under a configurable frontmatter
//! property, giving tasks a stable identifier for Dataview/links without a
//! vault scan or a cross-device sequential collision.

/// Reserved frontmatter keys the ID property must never collide with — the
/// structured task fields the surgical writer and reader own. Using one as
/// the ID property would let the ID writer clobber a real field.
const RESERVED_TASK_KEYS: &[&str] = &[
    "type", "status", "title", "created", "due", "priority", "tags", "tag", "order",
];

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
        && !RESERVED_TASK_KEYS.contains(&name.to_ascii_lowercase().as_str())
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
    }

    #[test]
    fn is_valid_id_property_charset_and_reserved() {
        assert!(is_valid_id_property("task-id"));
        assert!(is_valid_id_property("uid_2"));
        assert!(!is_valid_id_property("")); // empty
        assert!(!is_valid_id_property("task id")); // space
        assert!(!is_valid_id_property("task:id")); // colon
        for reserved in [
            "type", "status", "title", "created", "due", "priority", "tags", "tag", "order",
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
