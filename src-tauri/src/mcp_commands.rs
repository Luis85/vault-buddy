//! Lifecycle + settings IPC for the embedded MCP server. The protocol/tool
//! logic lives in the Tauri-free `vault_buddy_mcp` crate; this file only
//! starts/stops it, persists its config, and bridges its write events to the
//! buddy's announcements.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use tauri::{AppHandle, Emitter, Manager};
use vault_buddy_core::sync_util::lock_ignoring_poison;
use vault_buddy_core::{capture_config, services, uri};

use crate::capture_commands::ConfigWriteLock;

#[derive(Default)]
pub struct McpServerState(pub Mutex<McpInner>);

#[derive(Default)]
pub struct McpInner {
    running: Option<vault_buddy_mcp::RunningServer>,
    last_error: Option<String>,
    /// Shared with the running server's Deps so an allow-writes flip takes
    /// effect live, without a restart.
    allow_writes: Arc<AtomicBool>,
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpStatusDto {
    pub state: String, // "running" | "stopped" | "error"
    pub port: Option<u16>,
    pub message: Option<String>,
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpConfigDto {
    pub enabled: bool,
    pub port: u16,
    pub allow_writes: bool,
    pub token: String,
    pub status: McpStatusDto,
}

#[derive(Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpConfigInput {
    pub enabled: bool,
    pub port: u16,
    pub allow_writes: bool,
}

fn status_of(inner: &McpInner) -> McpStatusDto {
    match (&inner.running, &inner.last_error) {
        (Some(s), _) => McpStatusDto {
            state: "running".into(),
            port: Some(s.port),
            message: None,
        },
        (None, Some(e)) => McpStatusDto {
            state: "error".into(),
            port: None,
            message: Some(e.clone()),
        },
        (None, None) => McpStatusDto {
            state: "stopped".into(),
            port: None,
            message: None,
        },
    }
}

fn snapshot(app: &AppHandle) -> McpConfigDto {
    let cfg = capture_config::load_config().mcp;
    let state = app.state::<McpServerState>();
    let inner = lock_ignoring_poison(&state.0);
    McpConfigDto {
        enabled: cfg.enabled,
        port: cfg.port,
        allow_writes: cfg.allow_writes,
        token: cfg.token,
        status: status_of(&inner),
    }
}

fn emit_status(app: &AppHandle) {
    let state = app.state::<McpServerState>();
    let status = status_of(&lock_ignoring_poison(&state.0));
    if let Err(e) = app.emit("mcp:status", &status) {
        log::warn!("mcp: could not emit status: {e}");
    }
}

fn deps_for(app: &AppHandle, allow_writes: Arc<AtomicBool>) -> vault_buddy_mcp::Deps {
    let emitter = app.clone();
    vault_buddy_mcp::Deps {
        paths: services::ServicePaths::real(),
        app_version: app.package_info().version.to_string(),
        allow_writes,
        launch: Arc::new(|u: &str| uri::launch(u)),
        // The buddy window's announcer (useBuddyAnnouncements) listens for
        // this and speaks it — gated frontend-side on "Buddy messages", the
        // same gate every other announcement rides.
        on_write: Arc::new(move |ev: vault_buddy_mcp::WriteEvent| {
            if let Err(e) = emitter.emit("mcp:write", &ev) {
                log::warn!("mcp: could not emit write event: {e}");
            }
        }),
    }
}

/// Start the server from the persisted config. Never fails the caller: a
/// bind error lands in `last_error`/`mcp:status` and the log.
fn start_from_config(app: &AppHandle, cfg: &capture_config::McpConfig) {
    let state = app.state::<McpServerState>();
    let mut inner = lock_ignoring_poison(&state.0);
    if inner.running.is_some() {
        return;
    }
    inner
        .allow_writes
        .store(cfg.allow_writes, Ordering::Relaxed);
    let deps = deps_for(app, inner.allow_writes.clone());
    match vault_buddy_mcp::start(deps, cfg.port, cfg.token.clone()) {
        Ok(server) => {
            inner.running = Some(server);
            inner.last_error = None;
        }
        Err(e) => {
            log::error!("mcp: start failed: {e}");
            inner.last_error = Some(e);
        }
    }
    drop(inner);
    emit_status(app);
}

/// Stop off the calling thread's critical path: take the handle under the
/// lock, join outside it.
fn stop_running(app: &AppHandle) {
    let state = app.state::<McpServerState>();
    let server = {
        let mut inner = lock_ignoring_poison(&state.0);
        // Stopping also clears a stale error from a failed earlier start:
        // after a bind failure, disabling must read as "stopped", not as the
        // ghost of the old error (Codex review catch) — a restart that fails
        // re-sets last_error immediately after this.
        inner.last_error = None;
        inner.running.take()
    };
    if let Some(server) = server {
        server.stop();
    }
    emit_status(app);
}

/// Called once from setup. Self-heals an enabled config with no token (the
/// enable normally generates one; a hand-edited file may lack it).
pub fn start_if_enabled(app: &AppHandle) {
    let mut cfg = capture_config::load_config().mcp;
    if !cfg.enabled {
        return;
    }
    if cfg.token.is_empty() {
        cfg.token = vault_buddy_mcp::token::generate_token();
        if let Err(e) = persist(app, cfg.clone()) {
            log::error!("mcp: could not persist a self-healed token: {e}");
            return;
        }
    }
    start_from_config(app, &cfg);
}

fn persist(app: &AppHandle, cfg: capture_config::McpConfig) -> Result<(), String> {
    let lock = app.state::<ConfigWriteLock>();
    let _guard = lock_ignoring_poison(&lock.0);
    let path = capture_config::config_path().ok_or("Cannot resolve the config directory")?;
    capture_config::update_mcp_config_at(&path, cfg)
        .map_err(|e| format!("Could not save MCP settings: {e}"))
}

#[tauri::command]
pub fn get_mcp_config(app: AppHandle) -> McpConfigDto {
    snapshot(&app)
}

/// Async: stopping joins the server thread — that wait belongs on the async
/// runtime, not the main thread (the sync-command rule exists for window
/// APIs, which this never touches).
#[tauri::command]
pub async fn set_mcp_config(app: AppHandle, input: McpConfigInput) -> Result<McpConfigDto, String> {
    if input.port < 1024 {
        return Err("Port must be between 1024 and 65535.".to_string());
    }
    let previous = capture_config::load_config().mcp;
    let mut next = previous.clone();
    next.enabled = input.enabled;
    next.port = input.port;
    next.allow_writes = input.allow_writes;
    if next.enabled && next.token.is_empty() {
        next.token = vault_buddy_mcp::token::generate_token();
    }
    persist(&app, next.clone())?;

    // Mirror the grant into shared state first — the call-time authority for
    // any session that lives through the transition.
    {
        let state = app.state::<McpServerState>();
        let inner = lock_ignoring_poison(&state.0);
        inner
            .allow_writes
            .store(next.allow_writes, Ordering::Relaxed);
    }
    let needs_restart = next.enabled != previous.enabled
        || next.port != previous.port
        || next.token != previous.token
        // A grant flip restarts too: sessions end, clients re-initialize and
        // fetch a fresh tools/list, so newly granted write tools actually
        // become discoverable (clients cache tool lists per session and v1
        // sends no listChanged push).
        || next.allow_writes != previous.allow_writes;
    if needs_restart {
        let app2 = app.clone();
        let next2 = next.clone();
        tauri::async_runtime::spawn_blocking(move || {
            stop_running(&app2);
            if next2.enabled {
                start_from_config(&app2, &next2);
            }
        })
        .await
        .map_err(|e| format!("MCP server restart task failed: {e}"))?;
    }
    Ok(snapshot(&app))
}

#[tauri::command]
pub async fn regenerate_mcp_token(app: AppHandle) -> Result<McpConfigDto, String> {
    let mut cfg = capture_config::load_config().mcp;
    cfg.token = vault_buddy_mcp::token::generate_token();
    persist(&app, cfg.clone())?;
    let app2 = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        stop_running(&app2);
        if cfg.enabled {
            start_from_config(&app2, &cfg);
        }
    })
    .await
    .map_err(|e| format!("MCP server restart task failed: {e}"))?;
    Ok(snapshot(&app))
}
