# Increment 1 — Windows Manual Verification Checklist

Run on a Windows machine with Obsidian installed. This is the manual gate for
the spec's success criteria (the automated gates are `cargo test` in
`src-tauri/core` and `npm run test` / `npm run build`).

Setup: `npm install`, then `npm run tauri dev`.

- [ ] App launches; the companion appears in a transparent window (no frame,
      no background rectangle), always on top, with a tray icon.
- [ ] Character idles (bobbing), wiggles on hover.
- [ ] Pressing the character and moving the mouse drags the window around the
      desktop (no separate handle); releasing without movement still counts as
      a click and does not accidentally start a drag.
- [ ] After dropping the buddy from a drag, the panel does not pop open from
      the release; the next deliberate click opens it.
- [ ] With the buddy against the right screen edge, clicking it opens the
      panel to the LEFT of the buddy, fully on-screen, and the buddy does not
      visibly move; closing restores everything.
- [ ] Same near the bottom edge (panel unfolds upward) and in the
      bottom-right corner (left + up).
- [ ] Open the panel near the right edge, drag the window somewhere else
      while it is open, close the panel: the buddy stays where it was
      dropped (no teleporting).
- [ ] With the panel open, drag the buddy: the panel stays open and moves
      along; the drag is not cancelled and the buddy does not jump to the
      panel's old corner.
- [ ] Park the buddy over/near the taskbar, hover a taskbar item until the
      window preview opens, close it: if the buddy dropped behind the
      taskbar it comes back on top within about a second.
- [ ] Move the buddy somewhere distinctive, quit via the tray, relaunch:
      the buddy reappears where it was left (position persists; window size
      starts collapsed regardless of how it was closed).
- [ ] Clicking the character grows the window and shows the dark panel listing
      your real vaults (names match Obsidian's vault switcher) with a count
      badge and an avatar initial per row.
- [ ] Clicking a vault row brings that vault up in Obsidian; a spinner shows
      on the row while it launches, then the panel closes by itself.
- [ ] The calendar button with an existing daily note opens it in the right
      vault.
- [ ] With more than 5 vaults a "Filter vaults…" box appears; typing narrows
      the list by name and path; Escape clears the filter first, a second
      Escape closes the panel.
- [ ] Escape closes the panel; clicking anywhere on the desktop (window loses
      focus) also closes it.
- [ ] With Windows "reduced motion" enabled (Settings > Accessibility >
      Visual effects > Animation effects off), the buddy stops bobbing and
      blinking.
- [ ] Delete/rename today's note, retry: Obsidian creates it (empty — template
      not applied is a known limitation).
- [ ] Vault with a custom daily-note folder/format (e.g. `Journal`,
      `YYYY/MM-DD`) resolves the correct file.
- [ ] While an action runs the character pulses (working state); afterwards the
      panel stays responsive.
- [ ] Rename `%APPDATA%\obsidian\obsidian.json` temporarily and restart: the
      panel shows the "Obsidian not found" message; no crash. Restore the file.
- [ ] While Vault Buddy is running, remove a vault from Obsidian (so it leaves
      `obsidian.json`), then trigger one of its actions from the still-open
      panel: an inline "vault not found" error banner appears; no crash.
- [ ] Right-clicking the buddy shows a native menu with "Hide to tray" and
      "Quit Vault Buddy"; Hide makes the companion disappear and tray
      "Show / Hide" brings it back; Quit exits (and still saves position).
- [ ] Right-clicking the panel or empty areas shows no browser context menu;
      right-clicking inside the filter box still shows the native text menu.
- [ ] Tray "Show / Hide" toggles the companion; "Quit Vault Buddy" exits the
      app.
- [ ] `tauri dev` console shows a `launching URI: obsidian://...` log line for
      every action performed.
