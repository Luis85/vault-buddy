//! App-global settings for the embedded MCP server (spec:
//! docs/superpowers/specs/2026-07-09-local-mcp-server-design.md). Stored as
//! a top-level `mcp` section beside `vaults` in the same hand-editable
//! config.json; parsing is per-field defensive for the same reason the
//! vault entries are. Split out of `capture_config` for LOC headroom —
//! that module re-exports these names, so callers are unchanged.

/// Default port for the embedded MCP server: 0x5642 = ASCII "VB".
pub const DEFAULT_MCP_PORT: u16 = 22082;

#[derive(Debug, Clone, PartialEq)]
pub struct McpConfig {
    pub enabled: bool,
    pub port: u16,
    /// Bearer token clients must send. Empty until first enable; the shell
    /// self-heals an enabled-but-tokenless config by generating one.
    pub token: String,
    /// The "Allow vault writes" grant: add_task, set_task_status, and the
    /// daily-note create branch.
    pub allow_writes: bool,
}

impl Default for McpConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            port: DEFAULT_MCP_PORT,
            token: String::new(),
            allow_writes: false,
        }
    }
}

pub(crate) fn mcp_entry(entry: &serde_json::Value) -> McpConfig {
    let defaults = McpConfig::default();
    McpConfig {
        enabled: entry
            .get("enabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(defaults.enabled),
        port: entry
            .get("port")
            .and_then(|v| v.as_u64())
            .and_then(|v| u16::try_from(v).ok())
            // Same range the settings command enforces (1024–65535). A
            // hand-edited 0 would bind an ephemeral port while the persisted
            // config and client snippets still say 0 — default it instead.
            .filter(|p| *p >= 1024)
            .unwrap_or(defaults.port),
        token: entry
            .get("token")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .unwrap_or_default(),
        allow_writes: entry
            .get("allowWrites")
            .and_then(|v| v.as_bool())
            .unwrap_or(defaults.allow_writes),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture_config::{parse_config, serialize_config, AppConfig};

    #[test]
    fn mcp_config_defaults_when_absent_or_malformed() {
        let cfg = parse_config(r#"{ "vaults": {} }"#);
        assert_eq!(cfg.mcp, McpConfig::default());
        assert!(!cfg.mcp.enabled);
        assert_eq!(cfg.mcp.port, DEFAULT_MCP_PORT);
        // One malformed field defaults only itself — the file is hand-editable.
        let cfg = parse_config(
            r#"{ "mcp": { "enabled": true, "port": "not-a-number", "token": 5, "allowWrites": true } }"#,
        );
        assert!(cfg.mcp.enabled);
        assert_eq!(cfg.mcp.port, DEFAULT_MCP_PORT);
        assert_eq!(cfg.mcp.token, "");
        assert!(cfg.mcp.allow_writes);
        // Out-of-range ports (hand-edited) fall back too: the parser enforces
        // the same 1024–65535 range the settings command does, or startup
        // would bind port 0 (ephemeral!) while the snippets say otherwise.
        for bad in ["0", "80", "1023", "70000"] {
            let cfg = parse_config(&format!(r#"{{ "mcp": {{ "port": {bad} }} }}"#));
            assert_eq!(cfg.mcp.port, DEFAULT_MCP_PORT, "port {bad} must default");
        }
        let cfg = parse_config(r#"{ "mcp": { "port": 1024 } }"#);
        assert_eq!(cfg.mcp.port, 1024);
    }

    #[test]
    fn mcp_config_round_trips_through_serialize() {
        let cfg = AppConfig {
            mcp: McpConfig {
                enabled: true,
                port: 4321,
                token: "abc_-123".to_string(),
                allow_writes: true,
            },
            ..Default::default()
        };
        let reparsed = parse_config(&serialize_config(&cfg));
        assert_eq!(reparsed.mcp, cfg.mcp);
    }

    #[test]
    fn default_mcp_section_is_omitted_from_the_file() {
        // The hand-editable file stays minimal: users who never enable MCP
        // never see the section.
        let json = serialize_config(&AppConfig::default());
        assert!(!json.contains("mcp"), "got: {json}");
    }
}
