# Vault Buddy

An AI-native desktop companion for knowledge work — starting as the best
desktop companion for Obsidian and evolving into an extensible, local-first
desktop operating layer.

A small animated character lives on your desktop, always within reach. Click
it and your Obsidian vaults are one action away — no window hunting, no
context switching. Your knowledge stays yours: everything runs locally.

- **Platform:** Windows (MVP)
- **Status:** Increment 1 — desktop companion with Obsidian vault access

See the [Product Requirements Document](docs/PRD.md) for the full vision,
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

Vault Buddy never writes into your vaults — opening notes and creating
daily notes is delegated to Obsidian itself via `obsidian://` URIs, and
every launched URI is logged.

## Contributing

Building from source, tests, CI, and the release flow are documented in
[docs/DEVELOPMENT.md](docs/DEVELOPMENT.md).
