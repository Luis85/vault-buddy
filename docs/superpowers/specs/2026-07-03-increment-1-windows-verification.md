# Increment 1 — Windows Manual Verification Checklist

Run on a Windows machine with Obsidian installed. This is the manual gate for
the spec's success criteria (the automated gates are `cargo test` in
`src-tauri/core` and `npm run test` / `npm run build`).

Setup: `npm install`, then `npm run tauri dev`.

- [ ] App launches; the companion appears in a transparent window (no frame,
      no background rectangle), always on top, with a tray icon.
- [ ] Character idles (bobbing), wiggles on hover, and the ⠿ handle drags the
      window around the desktop.
- [ ] Clicking the character grows the window and shows the panel listing your
      real vaults (names match Obsidian's vault switcher).
- [ ] "Open vault" brings that vault up in Obsidian.
- [ ] "Open today's daily note" with an existing note opens it in the right
      vault.
- [ ] Delete/rename today's note, retry: Obsidian creates it (empty — template
      not applied is a known limitation).
- [ ] Vault with a custom daily-note folder/format (e.g. `Journal`,
      `YYYY/MM-DD`) resolves the correct file.
- [ ] While an action runs the character pulses (working state); afterwards the
      panel stays responsive.
- [ ] Rename `%APPDATA%\obsidian\obsidian.json` temporarily and restart: the
      panel shows the "Obsidian not found" message; no crash. Restore the file.
- [ ] Tray "Show / Hide" toggles the companion; "Quit Vault Buddy" exits the
      app.
- [ ] `tauri dev` console shows a `launching URI: obsidian://...` log line for
      every action performed.
