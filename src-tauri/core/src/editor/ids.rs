//! Entity and project id generation/validation (Contract reference: ID
//! regex `^[a-zA-Z0-9_-]{1,100}$`). No regex crate — the charset and length
//! bound are checked directly.

const BASE36: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";

/// True iff `s` matches `^[a-zA-Z0-9_-]{1,100}$` — an ASCII charset check
/// plus a 1..=100 character-count bound, without pulling in a regex crate.
pub fn is_valid_id(s: &str) -> bool {
    let len = s.chars().count();
    (1..=100).contains(&len)
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// `<prefix>-<10 base36 chars>` drawn from the OS CSPRNG. Mirrors
/// `tasks::new_task_id`'s use of `getrandom::fill`, without that helper's
/// letter-first rule — entity ids are never used as frontmatter property
/// values, so there is no Dataview number-vs-string ambiguity to guard.
pub fn new_entity_id(prefix: &str) -> String {
    format!("{prefix}-{}", random_base36(10))
}

/// 16 base36 chars drawn from the OS CSPRNG — a project id, with no prefix.
pub fn new_project_id() -> String {
    random_base36(16)
}

fn random_base36(len: usize) -> String {
    let mut bytes = vec![0u8; len];
    // getrandom only fails on a broken OS RNG; a loud panic is correct here
    // (mirrors tasks::new_task_id / mcp::token::generate_token).
    getrandom::fill(&mut bytes).expect("OS RNG unavailable");
    bytes
        .iter()
        .map(|b| BASE36[*b as usize % 36] as char)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn valid_ids_match_the_schema_pattern() {
        assert!(is_valid_id("c1"));
        assert!(is_valid_id("a_b-C"));
        assert!(!is_valid_id(""));
        assert!(!is_valid_id(&"a".repeat(101)));
        assert!(!is_valid_id("a b"));
        assert!(!is_valid_id("\u{e9}"));
    }

    #[test]
    fn new_ids_are_valid_and_distinct() {
        let mut seen = HashSet::new();
        for _ in 0..1000 {
            let id = new_entity_id("clip");
            assert!(
                is_valid_id(&id),
                "generated id failed the schema pattern: {id}"
            );
            assert!(id.starts_with("clip-"), "missing prefix: {id}");
            assert!(seen.insert(id), "generated a duplicate id");
        }
        let mut project_ids = HashSet::new();
        for _ in 0..1000 {
            let id = new_project_id();
            assert!(
                is_valid_id(&id),
                "generated project id failed the schema pattern: {id}"
            );
            assert!(project_ids.insert(id), "generated a duplicate project id");
        }
    }
}
