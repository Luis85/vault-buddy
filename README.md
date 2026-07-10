# Vault Buddy

An AI-native desktop companion for knowledge work — starting as the best
desktop companion for Obsidian and evolving into an extensible, local-first
desktop operating layer.

A small animated character lives on your desktop, always within reach. Click
it and your Obsidian vaults are one action away — no window hunting, no
context switching. Your knowledge stays yours: everything runs locally.

- **Platform:** Windows (MVP)
- **Status:** vault access · one-click meeting & voice recording · local offline transcription · per-vault tasks · local MCP server for AI clients

See the [Product Requirements Document](PRD%20-%20Product%20Vision.md) for the full vision,
principles, capabilities, and roadmap.

## Install

Grab the latest installer from the
[**Releases**](https://github.com/Luis85/vault-buddy/releases) page:
download `Vault Buddy_*_x64-setup.exe`, run it, done. The installer fetches
the WebView2 runtime automatically if needed (Windows 11 already has it).

> The installers aren't code-signed yet, so SmartScreen may warn — click
> **More info → Run anyway**.

## Usage

The buddy appears as a small, always-on-top character on your desktop.

- **Click** the buddy to open the vault panel. It lists every vault Obsidian
  knows about; vaults currently open in Obsidian appear first under
  "Open now" with a green dot.
- **Click a vault row** to bring that vault up in Obsidian, or hit the
  **calendar button** to jump straight into today's daily note (created via
  Obsidian if it doesn't exist yet).
- **Filter** kicks in automatically above 5 vaults — type to narrow by name
  or path. Escape clears the filter, then closes the panel.
- **Drag** the buddy anywhere; its position is remembered across restarts.
  The panel opens toward free screen space, so edges and corners are fine.
- **Right-click** the buddy for the menu: toggle the idle **animation**,
  **hide to tray**, or **quit**.
- **Tray icon**: Show/Hide the buddy, quit the app.
- The panel gets out of your way on its own: Escape, clicking the desktop,
  or launching a vault all close it.
- **Record** a meeting or voice note into a vault: click the **capture
  button** on a vault row, choose Meeting (desktop audio + mic) or Voice
  Note (mic only), and the buddy shows a red dot while it records. Pause,
  resume, or stop from the recording bar or the tray. Each vault has its own
  capture settings — folder, audio quality, companion note, follow-up
  template, and transcription — in the panel. When the **follow-up template**
  is on (the default), each recording's companion note gets a ready-made
  `## Follow-up` section (action items, decisions, notes) to fill in after.
- **Transcribe** locally, opt-in per vault: after a recording finishes,
  Vault Buddy runs speech-to-text on-device with whisper.cpp and writes a
  transcript that the note embeds. It downloads a small speech model on
  first use; transcription itself is fully offline — no cloud, no API.
- **Browse past recordings**: from the record chooser, hit **Browse
  recordings** to see everything captured in that vault, grouped by type.
  Click a recording to open its note in Obsidian, or **re-transcribe** any
  one on the spot — useful after switching to a larger, more accurate speech
  model.

- **Connect your AI tools (opt-in)**: Vault Buddy can serve a **local MCP
  server** so MCP clients — Claude Code, Claude Desktop, Cursor — can list
  your vaults, open notes, browse tasks and recordings, and (behind a
  separate "Allow vault writes" switch) add tasks or check them off.
  Enable it under Buddy settings → *AI integrations — MCP server*; the
  panel shows ready-to-copy setup snippets for each client. Everything is
  local: the server listens on `127.0.0.1` only, every request needs the
  generated token, and every tool call is logged.

Vault Buddy is careful with your vault. Browsing vaults and opening notes
never writes anything — that stays delegated to Obsidian via `obsidian://`
URIs, and every launched URI is logged. **Recording and tasks are the
features that write into a vault** (plus AI clients over MCP, but only
after you flip the separate vault-writes switch): recordings save audio, an
optional companion note, and (if enabled) a transcript sidecar into a dated
folder you choose; tasks are single Markdown documents in a folder you
pick. Nothing ever overwrites or edits files you already have. Everything
stays on your machine — no account, nothing uploaded.

## Contributing

Building from source, tests, CI, and the release flow are documented in
[docs/DEVELOPMENT.md](docs/DEVELOPMENT.md). Coding agents should start with
[AGENTS.md](AGENTS.md).
