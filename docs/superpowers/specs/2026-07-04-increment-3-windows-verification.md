# Increment 3 — Windows Verification Checklist

Companion checklist to the increment 3 design (local speech-to-text); run
on a Windows machine after the `windows-app` build, with Obsidian and a
microphone available (per the increment 2 checklist). Development happens
on Linux, so every device- and network-dependent behavior below — the
whisper.cpp engine build, the model download, Obsidian's embed resolution —
must be verified here before release.

## Setup

- [ ] In `%APPDATA%\vault-buddy\config.json`, set a vault's
      `transcribe: true` (leave `transcriptionModel` at its default,
      `small`).

## Happy path

- [ ] Record a short (~30 s) clip with some Spanish and English speech;
      Stop.
- [ ] Confirm the MP3 still saves within ~5 s (the save toast fires first,
      independent of transcription).
- [ ] Confirm the first transcription downloads the `small` model once,
      with a visible progress indicator in the panel; later recordings
      reuse it with **no network**.
- [ ] Open the meeting note in Obsidian: the `## Transcript` section
      renders the transcript **inline** (embedded sidecar), with
      `[HH:MM:SS]` timestamps that line up with the audio player.
- [ ] Confirm `<name>.transcript.md` exists beside the MP3 and carries
      `vault-buddy-transcript: complete`.

## Embed resolution (spec risk)

- [ ] Confirm the dotted embed `![[<name>.transcript]]` resolves in
      Obsidian (no "file not found"). If it does not, rename the sidecar
      scheme to `<name> transcript.md` / `![[<name> transcript]]` (see
      `2026-07-04-increment-3-local-speech-to-text-design.md`).

## Resilience + failure

- [ ] Start a recording, Stop, and quit the app **while "Transcribing…" is
      showing**; relaunch → the transcript completes (startup scan resumes
      it).
- [ ] Kill the app mid-recording; relaunch → the recording is recovered
      **and** transcribed.
- [ ] Go offline before the first model download (or point the model URL
      at nothing): confirm the audio + note are untouched, the transcript
      embed shows a retryable "failed" note, a toast fires, and the
      panel's **Retry** works once back online.
- [ ] Hand-edit a completed `.transcript.md`; trigger a rescan (relaunch):
      confirm your edits are **not** overwritten.

## No-cloud audit

- [ ] With the model already downloaded, transcribe with the network
      disconnected → succeeds fully offline.
