use base64::Engine;

/// 32 random bytes as unpadded base64url — the bearer token MCP clients must
/// present. Generated shell-side on first enable and stored in config.json
/// (user-profile ACLs; same trust level as the rest of that file).
pub fn generate_token() -> String {
    let mut bytes = [0u8; 32];
    // getrandom only fails on broken OS RNG — a panic here is correct
    // (an unguessable token is the whole security model).
    getrandom::fill(&mut bytes).expect("OS RNG unavailable");
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_43_char_base64url_and_unique() {
        let a = generate_token();
        let b = generate_token();
        assert_eq!(a.len(), 43); // 32 bytes, base64url, no padding
        assert_ne!(a, b);
        assert!(a
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
    }
}
