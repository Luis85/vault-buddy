use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};

fn encode(value: &str) -> String {
    utf8_percent_encode(value, NON_ALPHANUMERIC).to_string()
}

// The `vault` parameter is always the Obsidian vault ID (the unique key
// from obsidian.json). The URI scheme accepts name or ID; the ID
// disambiguates vaults whose folders share a name.

pub fn open_vault_uri(vault_id: &str) -> String {
    format!("obsidian://open?vault={}", encode(vault_id))
}

pub fn open_file_uri(vault_id: &str, file: &str) -> String {
    format!(
        "obsidian://open?vault={}&file={}",
        encode(vault_id),
        encode(file)
    )
}

pub fn new_file_uri(vault_id: &str, file: &str) -> String {
    format!(
        "obsidian://new?vault={}&file={}",
        encode(vault_id),
        encode(file)
    )
}

/// Logs and launches a URI via the OS default handler. The log line is the
/// audit trail required by the PRD's transparency principle.
pub fn launch(uri: &str) -> Result<(), String> {
    log::info!("launching URI: {uri}");
    open::that_detached(uri).map_err(|e| format!("failed to launch {uri}: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_open_vault_uri_with_encoding() {
        // Vault IDs from obsidian.json are hex, but the builder must encode
        // whatever it is given.
        assert_eq!(open_vault_uri("a1b2 c3"), "obsidian://open?vault=a1b2%20c3");
    }

    #[test]
    fn builds_open_file_uri() {
        assert_eq!(
            open_file_uri("a1b2c3", "Daily/2026-07-03"),
            "obsidian://open?vault=a1b2c3&file=Daily%2F2026%2D07%2D03"
        );
    }

    #[test]
    fn builds_new_file_uri() {
        assert_eq!(
            new_file_uri("a1b2c3", "2026-07-03"),
            "obsidian://new?vault=a1b2c3&file=2026%2D07%2D03"
        );
    }
}
