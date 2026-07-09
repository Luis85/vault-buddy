//! Embedded MCP server for Vault Buddy (streamable HTTP on 127.0.0.1).
//! Spec: docs/superpowers/specs/2026-07-09-local-mcp-server-design.md.
//! Tauri-free by design: the shell wires lifecycle + events, this crate owns
//! protocol, tools, and the HTTP guard — all testable on Linux.

pub mod http;
pub mod token;
