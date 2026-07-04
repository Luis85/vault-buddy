# Increment 2 — Windows manual verification

Companion checklist to the increment 2 design; run on a Windows machine
with Obsidian and a microphone. Development happens on Linux, so every
device-dependent behavior below must be verified here before release.

## Happy path
- [ ] Click 🎙 Capture on a vault: recording starts < 2 s; buddy pulses red
      with the dot; tray icon shows the red dot; tray menu gains "⏹ Stop
      recording"; toast is NOT shown on start (panel/buddy are the signal).
- [ ] During a Teams (or any) call: stop after ≥ 2 min → MP3 in
      `Meetings/YYYY/MM/` inside the vault, saved toast with filename,
      both your voice and the other side audible, duration matches.
- [ ] Companion note sits beside the MP3, embed plays inside Obsidian,
      frontmatter lists both devices.
- [ ] Stop → file present in < 5 s regardless of recording length.

## Collisions and modes
- [ ] Two captures in the same minute → second file gets " (2)".
- [ ] Pre-create `<name>.md` in the target folder → capture uses " (2)"
      for BOTH mp3 and note; the user note is untouched.
- [ ] Set `"mode": "voice-note"` in config.json → recording contains mic
      only; works with no output device connected.
- [ ] Set `"createNote": false` → MP3 only, no .md.

## Indicator hardening
- [ ] While recording: tray "Show / Hide" does nothing (buddy stays);
      buddy right-click → Hide does nothing.
- [ ] Start capture while the buddy is hidden in the tray → buddy shows.
- [ ] Tray "⏹ Stop recording" stops and saves.
- [ ] Quit (tray and buddy menu) mid-recording → recording is saved
      (toast) before the app exits. Alt+F4 likewise.

## Reliability
- [ ] Unplug the headset mic mid-meeting → warning in panel, recording
      continues (desktop audio side), note metadata records the event.
- [ ] Voice-note mode + unplug mic → recording finalizes immediately,
      "ended early" toast, partial audio saved.
- [ ] Kill Vault Buddy in Task Manager mid-recording → relaunch →
      within ~2 min a "Recording recovered" toast; `… (recovered).mp3`
      plays, containing audio up to ~the kill moment.
- [ ] Kill + relaunch + immediately start a new capture in the same vault
      and minute → new capture gets a suffixed name; the orphan is still
      recovered afterwards.
- [ ] Launch a second Vault Buddy instance while recording → no second
      process; existing buddy focused; recording unaffected.

## Audit
- [ ] App log contains started/saved/recovered lines with vault + path
      for every case above.
