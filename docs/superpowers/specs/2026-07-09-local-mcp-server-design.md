# Local MCP Server — Vertical Slice 1 Design

- **Date:** 2026-07-09
- **Status:** Approved
- **Source:** First shipped slice of the [AI Platform & Agent Runtime
  PRD](../../prds/ai-platform.md) (Phase 1's "embedded MCP server"), tracked
  as the [MCP Server & Runtime use-case](../../use-cases/mcp-server-and-runtime.md).
  Direction chosen over the [Local MCP Hub PRD](../../prds/local-mcp-hub.md)
  (Vault Buddy as MCP *client* + Ollama assistant) for the first AI
  increment: the server direction reuses the tested core domain, needs no
  LLM runtime or chat UI, and immediately plugs the buddy into AI clients
  the user already runs. The two remain complementary; the shared service
  functions extracted here are the docking point for the hub later.

## Goal

Embed a **local MCP server** in the running buddy so MCP-compatible AI
clients (Claude Code, Claude Desktop, Cursor, MCP Inspector) can act on the
user's vaults through Vault Buddy's existing safe chokepoints: list vaults,
open vaults and daily notes, list tasks and recordings, and — behind an
explicit extra grant — add tasks and set task status. Disabled by default;
enabling it changes nothing about how the rest of the app starts or runs.

Decisions locked during brainstorming:

- **Server, not client hub** (see Source above).
- **Streamable HTTP** on `127.0.0.1:<port>`, default port **22082**
  (`0x5642` = ASCII "VB"), endpoint path `/mcp`. The buddy is a running
  single-instance GUI app — a client-spawned stdio process would collide
  with `tauri-plugin-single-instance` by design. Stdio-only clients bridge
  with the standard `mcp-remote` shim.
- **Reads + the existing sanctioned task writes.** No note-content
  reading — that is a genuinely new vault capability (privacy, size, path
  design) and is out of scope.
- **One global write-grant toggle**, default off, labeled **"Allow vault
  writes"** — it governs everything that can put a file in a vault over
  MCP: `add_task`, `set_task_status`, and the daily-note create branch
  (Codex review catch: a "task writes" label would understate the grant).
  Two explicit opt-ins total (enable server, enable writes). Finer grants
  (per-vault, per-call approval) are future work.
- **Official Rust SDK (`rmcp`)** rather than a hand-rolled protocol layer:
  three real clients must interoperate, so protocol corners (version
  negotiation, sessions, JSON-RPC edge cases) belong to the SDK. Version
  pinned at implementation time; the SDK is pre-1.0, so the dependency is
  isolated inside the new crate.

## Architecture

New workspace crate **`src-tauri/mcp/` (`vault_buddy_mcp`)**, alongside
`core`/`capture`/`transcribe`. Like them it compiles and tests on Linux (CI
+ local); it depends on `vault_buddy_core` and `rmcp` (server + streamable
HTTP transport features, axum-based) and owns:

- the MCP service: tool definitions (input schemas via `schemars` derive on
  input structs living in this crate), handlers, `tools/list` filtering,
  call-time write gating, tool annotations (`readOnlyHint` on reads);
- the HTTP middleware: bearer-token auth, Origin validation, body-size cap;
- the server runner: bind, serve, graceful shutdown.

The **shell crate owns only lifecycle and settings**: a managed
`McpServerState` (current status + running handle), start on setup when
enabled, start/stop/restart from the settings commands, and the
announce-on-write callback. The server runs on **one named thread
(`"mcp-server"`)** hosting a small tokio runtime; axum shuts down via a
cancellation token when the user disables the feature or changes any
contract-bearing setting (port/token/allow-writes — restart with a
bounded drain, below).
App quit needs no special handling — the OS releases the listener; the
thread touches no window state, so none of the main-thread window
invariants are in play. rmcp's default session management (`Mcp-Session-Id`)
is used as-is; nothing persists across restarts because every tool is
stateless request/response, and clients re-initialize.

### Shared service functions (`core/src/services.rs`)

The bodies currently duplicated between IPC commands and (soon) MCP tools
move into core as plain functions — one implementation, one set of
path-safety guards, and the first concrete step toward the AI-platform
PRD's "service layer":

- `list_vaults()` — discovery + open-flag scrub (the `commands.rs` body)
- `list_tasks(vault_id)`, `add_task(vault_id, title, today)`,
  `set_task_status(vault_id, path, status)` — the `task_commands.rs` bodies
  including `tasks_root_for` and every existing guard (vault-dir existence,
  `safe_recording_root`, `assert_path_inside_vault` /
  `assert_root_inside_vault`). `set_task_status` gains a return value: the
  task's title (it already parses the file), for the announce hook.
- `list_recordings(vault_id)` — the recordings-list command body over
  `core::recordings`.
- `open_vault(vault_id)`, `open_daily_note(vault_id, today, allow_create)`
  — URI build + launch. `allow_create: false` refuses to build the
  `obsidian://new` branch for a missing daily note (the MCP caller passes
  the live `allowWrites`; the IPC command passes `true`, preserving UI
  behavior). The **launcher is injectable** (defaults to `uri::launch`) so
  tests assert the built URI without launching anything.

Core stays clock-free: `today` is always passed in (callers use
`chrono::Local`, as today). The functions are parameterized by a
`ServicePaths { obsidian_json, config_json }` source struct with a
`real()` constructor (mirroring how discovery/config already split pure
parsing from real-path wrappers), so the MCP integration test can point
them at a temp dir. The Tauri commands become thin wrappers; the existing
DTO structs (`VaultDto`-equivalent, `TaskDto`, recording rows) **move to
core** with their camelCase serde attributes so IPC and MCP serialize
identically. Frontend-visible behavior does not change.

## Config — new top-level `mcp` section in `config.json`

App-global (not per-vault), stored in the existing
`%APPDATA%\vault-buddy\config.json`:

```json
{
  "mcp": {
    "enabled": false,
    "port": 22082,
    "token": "<base64url, generated on first enable>",
    "allowWrites": false
  },
  "vaults": { "...": {} }
}
```

- Parsed with the same **per-field defensive** style as the vault entries:
  a malformed port falls back to 22082, a malformed flag to `false` — one
  bad value never fails startup or flips other fields.
- `AppConfig` gains the `mcp` field and — the critical part —
  **`serialize_config` round-trips it**. Today the serializer writes only
  the `vaults` section, so any capture/tasks settings save would silently
  delete an `mcp` section. Regression test: parse a config with an `mcp`
  section, save a vault's capture config, assert the `mcp` section
  survives byte-for-byte semantics.
- The token is 32 random bytes (`getrandom`), base64url (no padding),
  generated on first enable; `enabled: true` with a missing/empty token
  self-heals by generating one. It lives in the user-ACL'd config file —
  the same trust level as the rest of that file and the same approach the
  Obsidian Local REST API plugin takes.
- All config writes stay shell-side under the existing `ConfigWriteLock`
  read-modify-write discipline.
- Every settings change that alters the client contract restarts the
  listener: `enabled`, `port`, `token`, and — Codex review catch —
  `allowWrites` too. Streamable-HTTP clients re-initialize when they
  reconnect and fetch a fresh `tools/list`, so a newly granted (or
  revoked) write toolset becomes discoverable without relying on
  `listChanged` push notifications (out of scope). The grant is ALSO
  mirrored into shared state (an `Arc`/atomic) that write tools re-check
  on every call — the authority during drain windows and for any session
  that outlives the flip.

## Security model

- Bind **127.0.0.1 only**, never `0.0.0.0`.
- Every request requires `Authorization: Bearer <token>`; comparison is
  constant-time; anything else is `401` with no body detail.
- **Origin validation** (the MCP spec's DNS-rebinding defense): requests
  with no `Origin` header (CLI clients) pass; an `Origin` of a localhost
  form (`http(s)://localhost[:p]`, `127.0.0.1`, `[::1]`) passes; anything
  else is `403` — checked before auth work.
- Request body size capped (1 MiB) so a misbehaving client can't balloon
  memory.
- Write tools are **hidden from `tools/list` when `allowWrites` is off**
  (advisory — models shouldn't try) *and* **rejected at call time**
  (authoritative — clients cache tool lists) with a clear error:
  "Vault writes are disabled in Vault Buddy settings." Because clients
  cache tool lists per session, flipping the grant restarts the listener —
  sessions end, clients reconnect and re-list — so a newly granted write
  toolset actually appears everywhere (Codex review catch).
- **Audit**: every tool call logs tool name, vault id, outcome, duration
  through the existing log plumbing. Argument *values* are summarized
  (e.g. title length), not logged verbatim — the redaction discipline the
  Local MCP Hub PRD prescribes for logs, adopted here. URI launches keep
  their existing `uri::launch` audit line.

## Tool surface

Seven tools, **vaults addressed by id, never name** (the `uri.rs` rule —
folder names can collide). Server `instructions` and each tool description
tell clients to call `list_vaults` first. Output DTOs are the core DTO
structs (camelCase), so MCP responses match IPC responses field-for-field.

| Tool | Class | Behavior |
| --- | --- | --- |
| `list_vaults` | read | Registry parse + open-flag scrub: `{id, name, path, open}[]` |
| `list_tasks` | read | `{ vaultId }` → the vault's tasks, archived excluded (same as UI) |
| `list_recordings` | read | `{ vaultId }` → recording rows (type, title, transcript status) — metadata only |
| `open_vault` | open | `{ vaultId }` → launches `obsidian://open` |
| `open_daily_note` | open (write-gated create) | `{ vaultId }` → opens today's note; a **missing** note is only created (via `obsidian://new`) when `allowWrites` is on — off → tool error, nothing launched |
| `add_task` | write | `{ vaultId, title }` → collision-safe task create; returns the created task |
| `set_task_status` | write | `{ vaultId, path, status: new\|done\|archived }` → surgical status toggle |

Classes: **read** and **open** tools are available whenever the server is
enabled — opening Obsidian is the buddy's core benign action and writes
nothing. **write** tools are governed by `allowWrites` as above. One
subtlety (Codex review catch): the daily-note path is open-OR-CREATE —
`daily_note_uri` deliberately builds `obsidian://new` when today's note
doesn't exist, which mutates the vault. Over MCP that create branch counts
as a write: with `allowWrites` off, `open_daily_note` on a missing note
returns a tool error ("today's daily note doesn't exist; enable vault
writes in Vault Buddy settings to let clients create it") instead of
launching anything. The human UI path is unchanged (always may create). Failures
return MCP tool errors carrying the same user-facing messages the panel
shows (vault gone, path escape, bad status) — never a panic.

Server identity: name `vault-buddy`, version = app version.

## IPC commands + frontend

New `src-tauri/src/mcp_commands.rs`, registered in `lib.rs`:

- `get_mcp_config()` → `{ enabled, port, allowWrites, token, status }`
  where `status` is `{ state: "running"|"stopped"|"error", port?, message? }`
  from `McpServerState`.
- `set_mcp_config(dto)` — validates (port in 1024–65535), persists under
  `ConfigWriteLock`, starts/stops/restarts the server as needed, returns
  the new snapshot.
- `regenerate_mcp_token()` — new token, persists, restarts if running,
  returns the snapshot.

Both are **async commands**: they never touch window APIs, and the
stop/restart path joins the server thread — that wait belongs on the async
runtime, not the main thread (the synchronous-command rule exists for
window/geometry APIs, which these commands never use).

Status transitions also emit an **`mcp:status`** event so the settings UI
live-updates (e.g. a bind failure after enable).

**`McpSettings.vue`** — a new section in the Buddy-settings view (app-global,
so it sits with `UpdateSettings`/`DiagnosticsSettings`, not per-vault
settings), self-contained like `Tasks.vue` (IPC + local state, no new Pinia
store): enable toggle, port field, an **"Allow vault writes"** toggle whose
helper text names the full grant ("AI clients may add tasks, update task
status, and create today's daily note"), token with
copy + regenerate, live status line, and **copyable client setup snippets**
rendered from the live port/token for the three target clients — the
`claude mcp add --transport http` one-liner (with the Authorization
header), a Cursor `mcp.json` block, and a Claude Desktop `mcp-remote`
block.

### Announce on write (delight, approved)

The mcp crate stays Tauri-free: it exposes a small event callback the shell
implements. On a successful write the buddy **announces** it through the
existing announce chokepoint — "Added task 'Buy milk' to Notes" / "Marked
'Buy milk' done in Notes" — so the companion visibly narrates what AI just
did in the vault. Announce failures are logged, never affect the tool
result.

## Data flow (write case, end to end)

Claude Code → `POST http://127.0.0.1:22082/mcp` (bearer token) → Origin +
auth middleware → rmcp dispatch → `add_task` handler checks the shared
`allowWrites` state → `core::services::add_task` (same guards as the UI
path) → created-task DTO as the tool result → audit log line → announce
callback. The panel's task list picks the new task up on its next open —
the same freshness model as tasks edited by hand in Obsidian; no push sync
in v1.

## Error handling

Nothing in this feature may hurt the core app:

- Startup with `enabled: true` but the bind fails (port taken): log,
  `error` status in `McpServerState` (+ event), app runs normally.
  Enabling from settings surfaces the same failure inline.
- Malformed `mcp` config: per-field defaults, never a startup failure.
- Tool failures are MCP tool errors, logged, never fatal; slow filesystem
  work only ever delays MCP responses.
- Disabling (or any restart) must **prove the listener is closed** before
  reporting success — Codex review catch: an abandoned shutdown wait could
  leave the old endpoint alive and honoring the old token while the UI
  says "stopped". The server thread races graceful shutdown against a
  bounded drain: cancel sessions via the cancellation token, give
  in-flight requests ~3 s, then drop the serve future — which hard-closes
  the listener and every connection by construction (a client pinning an
  SSE stream open cannot keep the socket alive). The join is therefore
  bounded, `stop()` returns only after the thread — and thus the socket —
  is gone, and the async settings command awaits it off the main thread.
  A thread that panicked instead of exiting surfaces as `error` status,
  never a false "stopped".
- Regenerating the token restarts the listener; old clients get `401`.

## Testing

All Linux-runnable except the last item:

- **mcp crate unit tests**: auth (missing/wrong/valid token), Origin
  validation (absent / localhost / evil), body cap, write gating (hidden
  from list when off, rejected at call when off, allowed when on), config
  parse defaults, token shape, open-tool URI assertions via the injected
  launcher — including the daily-note create gate (missing note +
  `allowWrites` off → tool error, launcher never invoked; on → the
  `obsidian://new` URI).
- **mcp crate round-trip integration test**: start the real server on an
  ephemeral port against a temp-dir `ServicePaths` (fake `obsidian.json`,
  fake vault dir), drive `initialize` → `tools/list` → `tools/call` over
  HTTP using rmcp's client side (dev-dependency; raw JSON-RPC via reqwest
  is the fallback), assert the task file actually lands in the temp
  vault and `set_task_status` flips it. This is the client-agnostic
  spec-level validation, running in CI.
- **core tests**: service functions (mostly moved existing coverage) + the
  config round-trip regression (capture save preserves the `mcp` section).
- **Vitest**: `McpSettings.vue` — toggles call IPC, status renders,
  snippets contain live port + token, regenerate flow (mocked IPC).
- Existing suites stay green: command layer becomes thin wrappers with
  unchanged behavior.
- **Windows verification doc** (repo pattern): enable in settings; `claude
  mcp add --transport http --header "Authorization: Bearer …"`; from Claude
  Code list vaults, add a task, watch the buddy announce it and the file
  appear; spot-check Cursor and Claude Desktop with the generated
  snippets; run MCP Inspector against the endpoint.

## Docs to update in this increment

- README feature blurb (opt-in local MCP server).
- DEVELOPMENT.md: the `mcp` config section, port/token, client setup.
- AGENTS.md: new crate in the compile-where table, IPC additions, and the
  invariants — write gate is call-time-authoritative, vaults addressed by
  id, config `mcp` section must round-trip through `serialize_config`.
- Use-case status flip for `mcp-server-and-runtime.md` (vision → first
  slice shipped) noting what this slice covers.
- Version target: minor bump (v0.6.0) when it ships.

## Out of scope for this slice (deferred)

- Reading note contents over MCP (new vault capability — own design).
- Per-vault or per-call write permissions; permission UI beyond the toggle.
- A stdio sidecar/shim binary; bundling `mcp-remote`.
- MCP resources, prompts, sampling, `listChanged` notifications, or any
  server→client push.
- Capture/transcription control over MCP (start/stop recording is
  deliberately excluded: remote-triggering the microphone needs its own
  safety design).
- The Ollama assistant / MCP client hub (separate PRD; docks onto
  `core::services` later).
- Event bus, typed service objects, plugin API from the AI-platform vision.
