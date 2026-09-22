# Tutorial Editor — Windows Verification Checklist

Manual end-to-end verification on a real Windows machine. **A running
document across tasks**, not one task's gate — the tutorial-editor increment
lands in many small tasks (see
`docs/superpowers/specs/2026-09-21-tutorial-editor-integration-design.md`),
and each one that touches something no automated gate on any platform can
observe appends its own rows here rather than opening a second file. The
screen-capture feature's own checklist
(`docs/superpowers/specs/2026-09-18-screen-capture-windows-verification.md`)
is the model this file's header, table shape and measurement discipline are
copied from — read that file's own header once if any of the conventions
below are unclear.

**Created by Task 15.** Its first five rows exist for two different reasons
that happen to land in the same task:

- **T1–T3** verify the thing Task 15 itself ships: opening a staged screen
  capture into the new Rust-backed editor session (`editor_open_staged`),
  from both entry points the panel offers, and that a second open of the
  SAME capture reuses the session rather than minting a duplicate one.
- **T4–T5** close out GAP-170 (docs/Gaps.md), a gap Task 11 opened and this
  file did not exist yet to carry: `build.rs`'s `AppManifest::commands(...)`
  now lists every command in `generate_handler!`, not just the `editor_*`
  ones, because leaving any command out of that list silently disables ACL
  enforcement's grant for every command NOT listed — the near-miss GAP-170
  documents at length. A Rust unit test
  (`editor::capability_guard::the_generated_acl_artifact_resolves_the_
  partition_correctly`) replicates Tauri's own resolution algorithm over the
  generated `gen/schemas/{capabilities,acl-manifests}.json` artifact and
  proves the DATA is right; it cannot exercise `RuntimeAuthority::
  resolve_access` itself, `webview/mod.rs`'s dispatch path, or a real IPC
  round trip inside a running app — no automated test in this repo can. T4
  and T5 are that proof, and until both carry a result GAP-170 stays at its
  current severity (High, unverified): a resolution mismatch here would not
  be a scoping gap, it would be the WHOLE APP refusing every command from
  every window.

Re-measure the row count rather than incrementing it — the screen-capture
checklist's own header explains why that discipline exists (it was wrong
twice from incrementing):

```bash
grep -cE '^\| T[0-9]+ \|' docs/superpowers/specs/2026-09-21-tutorial-editor-windows-verification.md
```

This file carries **10 rows** today (T1–T10), of which **0** carry a result. An
empty *Result* column means unrun, which is not the same as failed — never
convert one to the other, and never claim a manual run that was not actually
performed on this host.

Spec: `docs/superpowers/specs/2026-09-21-tutorial-editor-integration-design.md`
(the ADR this task and its siblings implement); GAP-170 in `docs/Gaps.md` (the
gap T4/T5 close).

Run against a build of `claude/vault-buddy-improvement-polish-95f5bc`
(`npx tauri build`, or `npm run test-build` for a dev run).

## Measurement discipline

Same rule the screen-capture checklist established: **prefer instrumentation
that reports over assertions that confirm.** Write the observed value into
the *Result* column, not a tick — which project id opened, whether a second
open reused it or minted a new session id, the exact ACL refusal message,
which commands were exercised and that each one actually returned data
rather than silently no-op'ing.

## Where to look

| What | Where |
| --- | --- |
| A staged capture's editor session | `%LOCALAPPDATA%\com.vaultbuddy.desktop\editor-projects\<projectId>\` — `project.json` is the workspace envelope; the PIN back to the staged capture is `editorProjectId` in the capture's own sidecar (`%LOCALAPPDATA%\com.vaultbuddy.desktop\screen-captures\<base>.json`) |
| Whether a second open reused the session | The editor window's title/duration line (`EditorRoot.vue`'s temporary shell, testid `editor-shell` in the DOM) shows the opened project's own title; `vault-buddy.log` logs each `editor_open_staged` call and, on reuse, does NOT create a new `<projectId>` folder under `editor-projects\` |
| The app-wide IPC ACL | `src-tauri/gen/schemas/{capabilities,acl-manifests}.json`, regenerated on every `cargo build`/`tauri build` of the shell crate (git-ignored, 1:1 derived from `src-tauri/tauri.conf.json` + `src-tauri/capabilities/*.json` + `src-tauri/build.rs`'s `ALL_COMMANDS`) |
| Logs / crash records | `vault-buddy.log` (tray → *Open logs folder*) |

## Task 15's rows

| # | Check | Steps | Result |
| --- | --- | --- | --- |
| T1 | **Open from the capture bar's Edit button** | Record a short screen capture and stop it. On the panel's list view, the finished capture's row offers **Edit** (`ScreenCaptureBar`'s finished-row affordance). Click it. **Record**: whether the editor window opens, whether its temporary shell line (top of the window) shows a title/duration, and whether `vault-buddy.log` shows an `editor_open_staged` call for that capture's base. | |
| T2 | **Open from the staged-capture list** | Capture knowledge → *Record screen* → with at least one staged (unsaved) capture already sitting in staging, the picker's `StagedCaptureList` shows it with a **Resume editing** action. Click it for a capture that is NOT the one T1 already opened. **Record**: same three observations as T1, for this second, independently-opened capture. | |
| T3 | **A second open of the SAME capture reuses the session, not a new one** | With the editor open on the capture from T1 (or T2), go back to the panel and click **Edit** again for that exact same capture (from the capture bar if it is still the most recent, or from the staged list otherwise). **Record**: the `<projectId>` folder under `editor-projects\` before this second click (list the directory, or read the project id off the editor shell / log line), then click, then record it again. Expect: identical — no second folder created, and `vault-buddy.log` shows Rust reusing the live session rather than opening a fresh one (`open_staged_session_reuses_a_live_session_and_reports_missing_media`'s production behavior). A DIFFERENT id here means a duplicate project was minted for one capture. | |
| T4 | **Ordinary commands still dispatch under the exhaustive app ACL** (GAP-170, closing) | With the app freshly launched, drive it as a user would, through windows OTHER than the editor: open the panel and confirm the vault list populates (`list_vaults`); start and stop a short audio recording (`start_capture`/`stop_capture`); open Buddy settings and save any per-vault or app-global setting (e.g. toggle a Screen tab field via `set_screen_capture_config`, or `set_capture_config`). **Record**, per command, whether it worked exactly as before this task's `build.rs` change (which made the app manifest list ALL commands, not just the eight `editor_*` ones) — a regression here means the app is effectively bricked from every window, not a security issue in one feature. | |
| T5 | **An `editor_*` command is refused from a non-editor window** (GAP-170, closing) | With the PANEL window focused, open its devtools console (right-click → Inspect, or the equivalent dev shortcut) and run `window.__TAURI__.core.invoke("editor_execute", { request: { sessionId: "not-a-real-session", expectedRevision: 0, commandId: "manual-check", command: { kind: "undo" } } })`. **Record**: the exact rejection — it must be refused by the ACL (a permission-denied shape) BEFORE `session_commands::editor_execute`'s own body ever runs (which would instead reject with `sessionGone` for a made-up session id — a DIFFERENT failure that would mean the ACL scoping failed silently and only the native `authz::require_editor_window` caught it). The two are distinguishable by the error's own shape/message; write down which one was observed. | |

## Task 21's rows

Task 21 adds direct manipulation to the timeline: pointer-captured drag and
trim, Escape-to-cancel, keyboard nudge and a keyboard shortcut dispatcher on
the editor shell. The Vitest suite drives all of it through happy-dom, which
has no real pointer capture, no real focus model and no second listener
competing for a keystroke; the Playwright layout check is Chromium, not
WebView2. These two rows are what only the shipped window can show.

| # | Check | Steps | Result |
| --- | --- | --- | --- |
| T6 | **Drag, trim and Escape behave in the real WebView2 window** | Open a staged capture in the editor and split it once (`S` with the playhead mid-clip) so the track holds two clips. (a) Drag the second clip's body a few seconds right, moving the pointer fast and past the clip's own edge and outside the timeline before releasing. (b) Drag its left trim handle right, well past the point where it would shrink below a tenth of a second. (c) Start a body drag, press Escape while still holding the button, then move and release. **Record**: for (a), whether the clip tracked the pointer the whole way (pointer capture) and whether exactly one move landed (one Undo reverts it entirely); for (b), whether the clip stopped shrinking at ~100 ms instead of vanishing or inverting, and the committed in/out in the Inspector's Clip section; for (c), whether the clip snapped back on Escape and whether anything was committed (Undo label unchanged). | |
| T7 | **Ctrl+Z in the new timeline does not also undo the legacy strip** | With the legacy capture editor still showing below the new shell (`SHOW_LEGACY_EDITOR`), make one edit in the LEGACY strip (split a block) and one in the new timeline (nudge a clip with the arrow key). Click a clip in the new timeline so focus is inside the new shell, then press Ctrl+Z once. **Record**: which surface changed — expected only the new timeline's nudge is undone and the legacy strip keeps its split. Then click an empty area outside the new shell (focus on the page body) and press Ctrl+Z once more. **Record** which surface changed this time (expected: the legacy strip, since the new shell's dispatcher only sees keystrokes whose focus is inside it). | |

## Task 22's rows

Task 22 adds the layered preview (`PreviewSurface.vue` + the non-reactive
`PreviewController`) and widens the asset protocol's scope to ADR R7's
enumerated list. happy-dom has no decoder and no Web Audio, and the
Playwright check is Chromium serving a same-origin fixture, so neither can
show the three things below, which depend on WebView2 itself: the asset
protocol's real CORS answer feeding a `MediaElementAudioSourceNode`, the
scope as Tauri actually resolves it, and element-seek sync on real media.

| # | Check | Steps | Result |
| --- | --- | --- | --- |
| T8 | **The new preview plays a staged capture, with sound, through `editor_media_url`** | Record a short screen capture WITH an audio input (a microphone), stop it, and open it in the editor. In the new shell's preview (below the preview toolbar — not the legacy preview further down), press the transport's play button (or Space with focus on the page body). **Record**: whether the picture plays; whether the SOUND is audible (every layer is routed through Web Audio for monitoring — a moving picture with no sound means the asset response's `Access-Control-Allow-Origin` did not satisfy the `crossOrigin="anonymous"` element and the `MediaElementAudioSourceNode` is outputting zeros); and whether the transport's time advances and the timeline playhead follows it. | |
| T9 | **Monitor mute, volume and rate are local, and mute/rate survive a reopen** | During T8's playback: click **Sound** (it becomes **Muted**), drag the volume slider, and pick 1.5x in the rate menu. **Record**: that the sound stops/changes level immediately, that playback speeds up, and that the header's Undo label did NOT change (no editor command was sent). Close the editor window and reopen the same capture. **Record** whether Muted and 1.5x are restored (both are `workspace.json` fields) and that the volume came back at full (deliberately not persisted — R16's workspace has no field for it). | |
| T10 | **The widened asset scope still refuses what R7 leaves out** | With the editor open, open its devtools console and call `window.__TAURI__.core.convertFileSrc(path)` then `fetch(url).then(r => r.status)` for three paths: the open project's own `editor-projects\<projectId>\project.json`, a file you create under that project's `jobs\` directory, and any note in one of your vaults. **Record** each status: all three must be refused (403), while the same call for the project's staged `.mp4` under `screen-captures\` returns 200. A 200 for any of the first three means the scope Tauri resolved is wider than the pinned `tauri.conf.json` array says. | |

