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
    const ALPHABET: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut bytes = [0u8; 8];
    // getrandom only fails on a broken OS RNG; a loud panic is correct here
    // (mirrors mcp::token::generate_token).
    getrandom::fill(&mut bytes).expect("OS RNG unavailable");
    bytes
        .iter()
        .map(|b| ALPHABET[*b as usize % 36] as char)
        .collect()
}

/// True iff `name` is a safe frontmatter key for the ID property: non-empty,
/// `[A-Za-z0-9_-]` only, and not a reserved structured task key.
pub fn is_valid_id_property(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        && !RESERVED_TASK_KEYS.contains(&name)
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
        // Weak uniqueness over a 36^8 space — a collision in 1000 draws is
        // effectively impossible; this pins that the source is actually random.
        let mut seen = std::collections::HashSet::new();
        for _ in 0..1000 {
            assert!(seen.insert(new_task_id()));
        }
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
    }
}
