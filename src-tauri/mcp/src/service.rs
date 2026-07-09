//! The MCP service: seven tools over core::services. Vaults are addressed by
//! ID, never name. Write tools ride two gates: session-construction router
//! filtering (advisory — clients cache tools/list) and a live atomic check on
//! every call (authoritative).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{
    CallToolResult, ContentBlock, Implementation, ProtocolVersion, ServerCapabilities, ServerInfo,
};
use rmcp::{tool, tool_handler, tool_router, ErrorData as McpError, ServerHandler};
use vault_buddy_core::services::{self, ServicePaths};

pub const WRITES_DISABLED: &str = "Vault writes are disabled in Vault Buddy settings.";

const INSTRUCTIONS: &str = "Vault Buddy exposes the user's Obsidian vaults. Always call \
list_vaults first and address vaults by their `id` (never by name). Write tools (add_task, \
set_task_status, and creating a missing daily note) work only while the user has enabled \
'Allow vault writes' in Vault Buddy settings.";

#[derive(Clone, Copy, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum WriteKind {
    AddTask,
    SetTaskStatus,
    CreateDailyNote,
}

/// Emitted after every successful vault write so the shell can have the buddy
/// announce what an AI client just did.
#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WriteEvent {
    pub kind: WriteKind,
    pub title: String,
    pub vault_name: String,
}

/// Everything the tools need from the outside world, injectable for tests:
/// real paths + uri::launch + a Tauri event emitter in the app, temp dirs +
/// recording closures in tests.
// The two Arc<dyn Fn...> callback fields are the documented Deps shape (Task
// 6 and the shell consume them exactly) — a type alias would only rename,
// not simplify, the interface, so the clippy complexity lint is silenced
// rather than the shape changed.
#[derive(Clone)]
#[allow(clippy::type_complexity)]
pub struct Deps {
    pub paths: ServicePaths,
    pub app_version: String,
    pub allow_writes: Arc<AtomicBool>,
    pub launch: Arc<dyn Fn(&str) -> Result<(), String> + Send + Sync>,
    pub on_write: Arc<dyn Fn(WriteEvent) + Send + Sync>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct VaultIdParams {
    /// The vault's id from list_vaults.
    pub vault_id: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AddTaskParams {
    /// The vault's id from list_vaults.
    pub vault_id: String,
    /// The task's title.
    pub title: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SetTaskStatusParams {
    /// The vault's id from list_vaults.
    pub vault_id: String,
    /// The task file's path, from list_tasks.
    pub path: String,
    /// One of: new, done, archived.
    pub status: String,
}

#[derive(Clone)]
pub struct VaultBuddyMcp {
    deps: Deps,
    pub(crate) tool_router: ToolRouter<Self>,
}

impl VaultBuddyMcp {
    /// Router chosen at construction — the HTTP layer constructs one service
    /// per SESSION, so a toggled grant shows up in tools/list on the next
    /// connect; the live atomic check in each write tool covers sessions that
    /// initialized before the flip.
    pub fn new(deps: Deps) -> Self {
        let mut tool_router = Self::read_tools_router();
        if deps.allow_writes.load(Ordering::Relaxed) {
            // Brief specifies `tool_router + write_tools_router()` (ToolRouter
            // implements Add); clippy::assign_op_pattern wants the `+=` form
            // for a self-reassignment — same merge, mechanical rewrite.
            tool_router += Self::write_tools_router();
        }
        Self { deps, tool_router }
    }

    fn writes_allowed(&self) -> bool {
        self.deps.allow_writes.load(Ordering::Relaxed)
    }

    /// Uniform result shape: pretty JSON as text content — every target
    /// client renders text blocks. Domain failures are tool errors
    /// (isError: true), never protocol errors.
    fn ok_json<T: serde::Serialize>(value: &T) -> Result<CallToolResult, McpError> {
        let json = serde_json::to_string_pretty(value)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        Ok(CallToolResult::success(vec![ContentBlock::text(json)]))
    }

    fn tool_error(message: impl Into<String>) -> Result<CallToolResult, McpError> {
        Ok(CallToolResult::error(vec![ContentBlock::text(
            message.into(),
        )]))
    }

    /// Audit line for every tool call — names + outcome, never argument
    /// values (title lengths only), per the spec's redaction rule.
    fn audit(tool: &str, vault_id: &str, outcome: &Result<(), String>) {
        match outcome {
            Ok(()) => log::info!("mcp: tool={tool} vault={vault_id} ok"),
            Err(e) => log::warn!("mcp: tool={tool} vault={vault_id} failed: {e}"),
        }
    }

    fn today() -> chrono::NaiveDate {
        chrono::Local::now().date_naive()
    }
}

#[tool_router(router = read_tools_router, vis = "pub")]
impl VaultBuddyMcp {
    #[tool(
        description = "List the user's Obsidian vaults: id, name, path, and whether the vault is open now. Call this first — every other tool takes a vault id from here.",
        annotations(read_only_hint = true)
    )]
    pub async fn list_vaults(&self) -> Result<CallToolResult, McpError> {
        let vaults = services::list_vaults(&self.deps.paths);
        Self::audit("list_vaults", "-", &Ok(()));
        Self::ok_json(&vaults)
    }

    #[tool(
        description = "List a vault's tasks (todo documents): path, title, status, created date, done.",
        annotations(read_only_hint = true)
    )]
    pub async fn list_tasks(
        &self,
        Parameters(p): Parameters<VaultIdParams>,
    ) -> Result<CallToolResult, McpError> {
        let tasks = services::list_tasks(&self.deps.paths, &p.vault_id);
        Self::audit("list_tasks", &p.vault_id, &Ok(()));
        Self::ok_json(&tasks)
    }

    #[tool(
        description = "List a vault's recordings (meetings and voice notes): title, recorded time, duration, type, transcript status. Metadata only.",
        annotations(read_only_hint = true)
    )]
    pub async fn list_recordings(
        &self,
        Parameters(p): Parameters<VaultIdParams>,
    ) -> Result<CallToolResult, McpError> {
        let recordings = services::list_recordings(&self.deps.paths, &p.vault_id);
        Self::audit("list_recordings", &p.vault_id, &Ok(()));
        Self::ok_json(&recordings)
    }

    #[tool(description = "Open a vault in Obsidian (focuses/launches the Obsidian app).")]
    pub async fn open_vault(
        &self,
        Parameters(p): Parameters<VaultIdParams>,
    ) -> Result<CallToolResult, McpError> {
        let outcome = services::open_vault(&self.deps.paths, &p.vault_id, &*self.deps.launch);
        Self::audit("open_vault", &p.vault_id, &outcome);
        match outcome {
            Ok(()) => Self::ok_json(&serde_json::json!({ "opened": true })),
            Err(e) => Self::tool_error(e),
        }
    }

    #[tool(
        description = "Open today's daily note in Obsidian. If the note doesn't exist yet, creating it counts as a vault write and requires the 'Allow vault writes' grant in Vault Buddy settings."
    )]
    pub async fn open_daily_note(
        &self,
        Parameters(p): Parameters<VaultIdParams>,
    ) -> Result<CallToolResult, McpError> {
        let allow_create = self.writes_allowed();
        let date = Self::today();
        // Peek at existence first so a create (a vault write) can fire on_write.
        let vault = match services::find_vault(&self.deps.paths, &p.vault_id) {
            Ok(v) => v,
            Err(e) => {
                Self::audit("open_daily_note", &p.vault_id, &Err(e.clone()));
                return Self::tool_error(e);
            }
        };
        let (_, existed) =
            vault_buddy_core::daily_note_target(std::path::Path::new(&vault.path), date);
        let outcome = services::open_daily_note(
            &self.deps.paths,
            &p.vault_id,
            date,
            allow_create,
            &*self.deps.launch,
        );
        Self::audit("open_daily_note", &p.vault_id, &outcome);
        match outcome {
            Ok(()) => {
                if !existed {
                    (self.deps.on_write)(WriteEvent {
                        kind: WriteKind::CreateDailyNote,
                        title: date.format("%Y-%m-%d").to_string(),
                        vault_name: vault.name,
                    });
                }
                Self::ok_json(&serde_json::json!({ "opened": true, "created": !existed }))
            }
            Err(e) => Self::tool_error(e),
        }
    }
}

#[tool_router(router = write_tools_router, vis = "pub")]
impl VaultBuddyMcp {
    #[tool(
        description = "Create a task document in a vault's tasks folder. Requires the 'Allow vault writes' grant."
    )]
    pub async fn add_task(
        &self,
        Parameters(p): Parameters<AddTaskParams>,
    ) -> Result<CallToolResult, McpError> {
        // Denied attempts are audited too (Codex review catch): the spec says
        // EVERY tool call logs its outcome, and the revoked-grant path — a
        // session that cached the write tools before the user flipped the
        // toggle off — is exactly where the log matters most.
        if !self.writes_allowed() {
            Self::audit("add_task", &p.vault_id, &Err(WRITES_DISABLED.to_string()));
            return Self::tool_error(WRITES_DISABLED);
        }
        let vault = match services::find_vault(&self.deps.paths, &p.vault_id) {
            Ok(v) => v,
            Err(e) => {
                Self::audit("add_task", &p.vault_id, &Err(e.clone()));
                return Self::tool_error(e);
            }
        };
        let today = Self::today().format("%Y-%m-%d").to_string();
        let outcome = services::add_task(&self.deps.paths, &p.vault_id, &p.title, &today);
        Self::audit(
            "add_task",
            &p.vault_id,
            &outcome.as_ref().map(|_| ()).map_err(Clone::clone),
        );
        match outcome {
            Ok(task) => {
                (self.deps.on_write)(WriteEvent {
                    kind: WriteKind::AddTask,
                    title: task.title.clone(),
                    vault_name: vault.name,
                });
                Self::ok_json(&task)
            }
            Err(e) => Self::tool_error(e),
        }
    }

    #[tool(
        description = "Set a task's status to new, done, or archived. Requires the 'Allow vault writes' grant."
    )]
    pub async fn set_task_status(
        &self,
        Parameters(p): Parameters<SetTaskStatusParams>,
    ) -> Result<CallToolResult, McpError> {
        // Same audit-before-deny rule as add_task: a gate denial or a
        // failed vault lookup is still a tool-call outcome.
        if !self.writes_allowed() {
            Self::audit(
                "set_task_status",
                &p.vault_id,
                &Err(WRITES_DISABLED.to_string()),
            );
            return Self::tool_error(WRITES_DISABLED);
        }
        let vault = match services::find_vault(&self.deps.paths, &p.vault_id) {
            Ok(v) => v,
            Err(e) => {
                Self::audit("set_task_status", &p.vault_id, &Err(e.clone()));
                return Self::tool_error(e);
            }
        };
        let outcome = services::set_task_status(&self.deps.paths, &p.vault_id, &p.path, &p.status);
        Self::audit(
            "set_task_status",
            &p.vault_id,
            &outcome.as_ref().map(|_| ()).map_err(Clone::clone),
        );
        match outcome {
            Ok(title) => {
                (self.deps.on_write)(WriteEvent {
                    kind: WriteKind::SetTaskStatus,
                    title: title.clone(),
                    vault_name: vault.name,
                });
                Self::ok_json(
                    &serde_json::json!({ "path": p.path, "status": p.status, "title": title }),
                )
            }
            Err(e) => Self::tool_error(e),
        }
    }
}

// API drift from the brief: bare `#[tool_handler]` defaults its router
// expression to `Self::tool_router()` (an associated fn call) — see
// rmcp-macros-2.2.0/src/tool_handler.rs `ToolHandlerAttribute::default`.
// This service has no such method (its two `#[tool_router(router = ...)]`
// blocks are custom-named `read_tools_router`/`write_tools_router`), and
// even if it did, that would rebuild a fresh, always-read-only router on
// every call instead of reading the per-session grant-filtered router
// `new()` cached in `self.tool_router`. Naming the router expression
// explicitly (`router = self.tool_router`) is the SDK's own documented
// pattern for a stateful/dynamic router — see
// rmcp-2.2.0/tests/test_tool_macros.rs `Server`, which pairs a
// `#[tool_router(router = tool_router)]` block with a `tool_router` field
// and `#[tool_handler(router = self.tool_router)]`.
#[tool_handler(router = self.tool_router)]
impl ServerHandler for VaultBuddyMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("vault-buddy", &self.deps.app_version))
            .with_protocol_version(ProtocolVersion::LATEST)
            .with_instructions(INSTRUCTIONS.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[allow(clippy::type_complexity)] // test-only fixture tuple, verbatim from the brief
    fn fixture_deps(
        dir: &std::path::Path,
        allow_writes: bool,
    ) -> (Deps, Arc<Mutex<Vec<String>>>, Arc<Mutex<Vec<WriteEvent>>>) {
        let vault = dir.join("MyVault");
        std::fs::create_dir_all(&vault).unwrap();
        let obsidian_json = dir.join("obsidian.json");
        std::fs::write(
            &obsidian_json,
            serde_json::json!({
                "vaults": { "deadbeef01234567": { "path": vault.to_string_lossy() } }
            })
            .to_string(),
        )
        .unwrap();
        let config_json = dir.join("config.json");
        std::fs::write(&config_json, "{}").unwrap();
        let launched = Arc::new(Mutex::new(Vec::new()));
        let writes = Arc::new(Mutex::new(Vec::new()));
        let l2 = launched.clone();
        let w2 = writes.clone();
        let deps = Deps {
            paths: vault_buddy_core::services::ServicePaths {
                obsidian_json: Some(obsidian_json),
                config_json: Some(config_json),
            },
            app_version: "0.0.0-test".to_string(),
            allow_writes: Arc::new(AtomicBool::new(allow_writes)),
            launch: Arc::new(move |u: &str| {
                l2.lock().unwrap().push(u.to_string());
                Ok(())
            }),
            on_write: Arc::new(move |ev| w2.lock().unwrap().push(ev)),
        };
        (deps, launched, writes)
    }

    fn text_of(result: &rmcp::model::CallToolResult) -> String {
        serde_json::to_string(&result.content).unwrap_or_default()
    }

    #[test]
    fn write_tools_are_listed_only_with_the_grant() {
        let dir = tempfile::tempdir().unwrap();
        let with = VaultBuddyMcp::new(fixture_deps(dir.path(), true).0);
        let without = VaultBuddyMcp::new(fixture_deps(dir.path(), false).0);
        let names = |s: &VaultBuddyMcp| {
            let mut n: Vec<String> = s
                .tool_router
                .list_all()
                .into_iter()
                .map(|t| t.name.to_string())
                .collect();
            n.sort();
            n
        };
        assert_eq!(
            names(&with),
            [
                "add_task",
                "list_recordings",
                "list_tasks",
                "list_vaults",
                "open_daily_note",
                "open_vault",
                "set_task_status"
            ]
        );
        assert_eq!(
            names(&without),
            [
                "list_recordings",
                "list_tasks",
                "list_vaults",
                "open_daily_note",
                "open_vault"
            ]
        );
    }

    #[tokio::test]
    async fn add_task_writes_fires_the_hook_and_respects_the_live_gate() {
        let dir = tempfile::tempdir().unwrap();
        let (deps, _launched, writes) = fixture_deps(dir.path(), true);
        let svc = VaultBuddyMcp::new(deps.clone());
        let result = svc
            .add_task(rmcp::handler::server::wrapper::Parameters(AddTaskParams {
                vault_id: "deadbeef01234567".into(),
                title: "Buy milk".into(),
            }))
            .await
            .unwrap();
        assert_ne!(result.is_error, Some(true), "{}", text_of(&result));
        assert!(dir.path().join("MyVault/Tasks").is_dir());
        assert_eq!(writes.lock().unwrap().len(), 1);
        assert_eq!(writes.lock().unwrap()[0].vault_name, "MyVault");

        // Grant revoked AFTER session construction: the call-time gate is
        // authoritative even though the tool is still in this session's list.
        deps.allow_writes.store(false, Ordering::Relaxed);
        let result = svc
            .add_task(rmcp::handler::server::wrapper::Parameters(AddTaskParams {
                vault_id: "deadbeef01234567".into(),
                title: "Nope".into(),
            }))
            .await
            .unwrap();
        assert_eq!(result.is_error, Some(true));
        assert!(text_of(&result).contains(WRITES_DISABLED));
    }

    #[tokio::test]
    async fn open_daily_note_gates_creation_and_reports_it_as_a_write() {
        let dir = tempfile::tempdir().unwrap();
        let (deps, launched, writes) = fixture_deps(dir.path(), false);
        let svc = VaultBuddyMcp::new(deps.clone());
        let params = || {
            rmcp::handler::server::wrapper::Parameters(VaultIdParams {
                vault_id: "deadbeef01234567".into(),
            })
        };
        let result = svc.open_daily_note(params()).await.unwrap();
        assert_eq!(result.is_error, Some(true));
        assert!(text_of(&result).contains("enable vault writes"));
        assert!(launched.lock().unwrap().is_empty());

        deps.allow_writes.store(true, Ordering::Relaxed);
        let result = svc.open_daily_note(params()).await.unwrap();
        assert_ne!(result.is_error, Some(true), "{}", text_of(&result));
        assert!(launched.lock().unwrap()[0].starts_with("obsidian://new?"));
        assert_eq!(writes.lock().unwrap().len(), 1); // create counted as a write
    }

    #[tokio::test]
    async fn list_vaults_returns_the_registry() {
        let dir = tempfile::tempdir().unwrap();
        let (deps, _, _) = fixture_deps(dir.path(), false);
        let svc = VaultBuddyMcp::new(deps);
        let result = svc.list_vaults().await.unwrap();
        let text = text_of(&result);
        assert!(text.contains("deadbeef01234567") && text.contains("MyVault"));
    }
}
