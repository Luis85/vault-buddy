# Local MCP Server — Windows Verification Checklist

Manual end-to-end verification on a real Windows machine (the CI Windows job
proves the build; this proves the behavior). Run after installing a build of
the `claude/buddy-local-mcp-8th5l0` branch. Spec:
[2026-07-09-local-mcp-server-design.md](2026-07-09-local-mcp-server-design.md).

Prerequisites: Obsidian with at least one vault registered; Claude Code
installed; optionally Cursor / Claude Desktop / MCP Inspector for the
client sweep.

## Enable + connect (Claude Code, the primary client)

- [ ] Buddy settings → *AI integrations — MCP server*: the card shows
      **Stopped**, port 22082, no token, and "Allow vault writes" off.
- [ ] Toggle **Local MCP server** on → status flips to
      **Running on 127.0.0.1:22082**, a token appears, and the *Client
      setup* section unfolds.
- [ ] Copy the Claude Code snippet and run it in a terminal
      (`claude mcp add --transport http vault-buddy … --header
      "Authorization: Bearer …"`), then in a Claude Code session run
      `/mcp` → **vault-buddy shows as connected**.
- [ ] Ask Claude to list your vaults → `list_vaults` returns the real
      registry (ids + names; vaults currently open in Obsidian flagged).
- [ ] `vault-buddy.log` (tray → Open logs folder) shows an
      `mcp: tool=list_vaults … ok dur_ms=…` audit line — and no raw
      argument values anywhere.

## The write gate

- [ ] With "Allow vault writes" **off**: ask Claude to add a task →
      the write tools are absent from the client's tool list and/or the
      call fails with *"Vault writes are disabled in Vault Buddy
      settings."*; the log shows `failed=writes-disabled`.
- [ ] `open_daily_note` on a day with **no** daily note, writes off →
      tool error naming the setting, **no** note created, nothing launched.
- [ ] Toggle "Allow vault writes" **on** → the server restarts (status
      blips), Claude Code reconnects on next use and now lists `add_task`
      and `set_task_status`.
- [ ] Ask Claude to add a task ("Buy milk") to a vault → the file appears
      in that vault's Tasks folder, the panel's Tasks view shows it, **and
      the buddy announces it** ("Added task …" bubble — with Buddy
      messages enabled).
- [ ] Ask Claude to mark it done → `status: done` in the file, checkbox
      checked in the panel, buddy announces the update.
- [ ] `open_daily_note` with writes on and no note → Obsidian creates and
      opens today's note; the buddy announces the created note.

## Lifecycle + security spot-checks

- [ ] **Regenerate** the token → old snippet stops working (client errors /
      401), new snippet connects.
- [ ] Change the **port** → server restarts on the new port; old URL dead.
- [ ] Toggle the server **off** → status **Stopped** within ~3 s; the
      client can no longer connect; `netstat -ano | findstr 22082` shows
      no listener.
- [ ] Quit the buddy (tray → Quit) with the server enabled → relaunch →
      the server auto-starts (status Running without touching settings).
- [ ] Set the port to one that's in use → status shows the bind **error**
      inline; the rest of the app keeps working; picking a free port
      recovers.
- [ ] `curl http://127.0.0.1:22082/mcp -X POST -d "{}"` (no token) → 401.

## Client sweep (secondary targets)

- [ ] **Cursor**: paste the `.cursor/mcp.json` snippet → vault-buddy tools
      appear and `list_vaults` works.
- [ ] **Claude Desktop**: paste the `mcp-remote` snippet into the desktop
      config → tools appear and `list_vaults` works.
- [ ] **MCP Inspector**: connect with the URL + Authorization header →
      initialize succeeds, seven tools listed (five with writes off).

## Regression sanity

- [ ] Vault list, daily note, recording start/stop, transcription, and the
      Tasks view all behave exactly as before with the MCP server disabled
      (the default) — and while it is enabled.
