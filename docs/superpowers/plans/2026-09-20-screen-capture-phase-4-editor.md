# Screen Capture Phase 4 — The Editor Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A staged screen capture can be opened in a real editor window, previewed, cut into segments, reordered, undone and redone — with every edit persisted to the staging sidecar so a crash loses at most the last operation. No export, no vault write.

**Architecture:** A fourth OS window (`editor`), deliberately unlike the three companion surfaces: decorated, resizable, alt-tabbable, not always-on-top, and **exempt from hide-to-tray**. It renders `EditorRoot`, which plays the staged `.mp4` through Tauri's asset protocol (scoped to the staging directory only) and drives `core::timeline` — a pure module Phase 1 already built and unit-tested, which this phase gives its first production caller. Every timeline operation returns a new `Timeline`, so undo/redo is a stack of snapshots rather than an inverse-operation log.

**Tech Stack:** Tauri v2 (window lifecycle, asset protocol, CSP), Rust shell commands, Vue 3 + Pinia + Tailwind 4, `core::timeline` (pure Rust), `screen::staging` (sidecar I/O), Vitest + happy-dom.

## Global Constraints

Every task's requirements implicitly include this section.

- **Branch:** `claude/screen-capture-intake-g0j49q`. PR #79 is already open for it. **Never open a second PR, and never one against `main`.** `main` is red for an unrelated reason (a concept drop broke ESLint there); this branch carries the fix and it rides along at merge.
- **Phase 4 writes NOTHING into any vault.** The ninth sanctioned vault write arrives in Phase 5 with export. A staged capture lives entirely under `%LOCALAPPDATA%\com.vaultbuddy.desktop\screen-captures`. Do not add a vault write here.
- **Phase 4 has no export.** `export_and_save_capture`, `cancel_export`, `discard_staged_capture` and `open_screen_capture` are Phase 5/6. Do not add them.
- **`core::timeline` is already complete** (`src-tauri/core/src/timeline.rs`, 282 non-blank lines): `whole`, `split_at`, `delete`, `reorder`, `output_duration_ms`, `to_source_ms`, `is_untouched`, `is_empty`, `Segment::duration_ms`. **Consume it; do not reimplement or widen it** unless a task below says so explicitly.
- **Window rules (AGENTS.md, "The window system"):** window show/hide/position/size and the window getters run on the MAIN thread only; a sync command must never block; position/size a window WHILE HIDDEN, then show.
- **Verified environment limit:** `cargo clippy -p vault-buddy --target x86_64-pc-windows-msvc` is **impossible** in this container at any scope (`ring v0.17.14` fails in cc-rs with "failed to find tool lib.exe"). `-p vault_buddy_screen --all-targets` and `-p vault_buddy_capture --lib` with that target **do** work. Never list the shell one as a gate; never report it as skipped or passing.
- **Baselines are shrink-only.** `scripts/loc-baseline.json` and `scripts/quality-baseline.json` may tighten, never loosen. Current floors: `deadCodeIssues=2`, `complexFunctions=13`, `criticalComplexity=4`, `averageMaintainability=91.5`, frontend LOC cap 500, Rust cap 800. If a metric moves the wrong way, **extract — do not loosen**. No counter metric has been loosened anywhere on this branch; keep it that way.
- **NEVER run `npm run check:loc -- --update`** (it rewrites every `—` escape as a literal em dash across all 11 entries, producing a 20-line diff over unrelated records) **nor `npm run check:quality -- --update`** (it REPLACES the whole `description` field with a generic default, destroying six recorded justifications). Hand-edit the single value.
- **Gate discipline:** never pipe a gate through `grep`/`tail` in a way that swallows its exit code. Redirect to a file and tail the file, or put `; echo "exit=$?"` on the command itself. This has bitten three separate agents on this branch.
- **Commits:** Conventional Commits, `git commit -F <file>` with a heredoc. **Never put a backtick in a commit message** — they shell-expand and have silently deleted words from commit messages on this branch. Trailer exactly:
  ```
  Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01Ud2Yq3jSXMKVdZaMSTeRco
  ```
- **Manual Windows verification is DEFERRED** until after the final phase, by the user's standing decision. Tasks write checklist rows; nobody runs them and nobody is asked to.
- **Every spawned thread is named** (`std::thread::Builder`). No swallowed errors — anything caught-and-hidden goes through `log::warn!`/`log::error!` or `src/logging.ts`.

## File Structure

| File | Responsibility |
| --- | --- |
| `src-tauri/tauri.conf.json` | Declares the `editor` window; enables `security.assetProtocol` scoped to staging; extends the CSP with `media-src`. |
| `src-tauri/capabilities/default.json` | Adds `"editor"` to the `windows` array, or the editor webview gets no permissions at all. |
| `src-tauri/src/tray.rs` | Splits the one window-label list into a destroy list and a hide list, because the editor must be destroyed on quit but must NOT be hidden to tray. |
| `src-tauri/src/capture_exclusion.rs` | `EXCLUDED_LABELS` gains `"editor"` — Phase 3's config-derived test forces this, by design. |
| `src-tauri/src/editor_commands.rs` | **New.** `open_capture_editor` (sync: stash + show + focus), `take_editor_request` (one-shot drain), `load_staged_capture` (async), `save_capture_timeline` (async). |
| `src-tauri/src/lib.rs` | `mod editor_commands;`, `.manage(...)`, four `generate_handler!` entries. |
| `src/roots/EditorRoot.vue` | **New.** The editor window's root: loads its capture, hosts the preview and the timeline strip, owns the undo/redo stack. |
| `src/roots/index.ts` | `rootFor()` gains `editor → EditorRoot`. |
| `src/components/editor/CapturePreview.vue` | **New.** Presentational `<video>` + transport, plus the segment-boundary seek walk. |
| `src/components/editor/TimelineStrip.vue` | **New.** Presentational segment blocks, selection, drag-to-reorder. |
| `src/composables/useEditorTimeline.ts` | **New.** The undo/redo snapshot stack and the persist-on-edit debounce. |
| `src/utils/timelineGeometry.ts` | **New.** Pure: segment widths as percentages, playhead↔output-ms mapping, drop-index from a pointer x. |
| `src/types.ts` | `StagedCaptureDetail`, `TimelineDto`, `SegmentDto`. |

---

### Task 1: The `editor` window, and the label list it splits in two

Spec §5.1. This task adds no UI — it adds a window, and it resolves the
invariant that window breaks.

**Why the list splits.** `tray::ALL_WINDOW_LABELS` currently backs BOTH
`hide_buddy`'s hide walk AND `finish_quit`'s destroy walk, and AGENTS.md calls
it "the ONE list both read". Spec §5.1 makes the editor **exempt from
hide-to-tray** — hiding a window holding unsaved edits strands work off-screen
(the GAP-82 class) — while `finish_quit` MUST destroy it, because every webview
shares WebView2's `Chrome_WidgetWin_0` class and leaving one alive fails the
unregister with `ERROR_CLASS_HAS_WINDOWS (1412)`. One list cannot serve both
rules any more. Two lists, each pinned to `tauri.conf.json`, is the answer —
**not** an inline exception inside `hide_buddy`, which
`no_window_walk_restates_the_label_list` already forbids.

**Files:**
- Modify: `src-tauri/tauri.conf.json` (the window, `assetProtocol`, the CSP)
- Modify: `src-tauri/capabilities/default.json` (`windows` array)
- Modify: `src-tauri/src/tray.rs` (the split + its tests)
- Modify: `src-tauri/src/capture_exclusion.rs` (`EXCLUDED_LABELS`)
- Modify: `src/roots/index.ts` (`editor → EditorRoot`)
- Create: `src/roots/EditorRoot.vue` (a placeholder root this task only needs to mount)
- Modify: `tests/roots.test.ts`

**Interfaces:**
- Consumes: nothing from later tasks.
- Produces, for Tasks 2-7:
  - a hidden `editor` window, label `"editor"`
  - `tray::ALL_WINDOW_LABELS: [&str; 5]` — the DESTROY list, every configured window
  - `tray::COMPANION_LABELS: [&str; 4]` — the HIDE list, everything except the editor
  - `asset.localhost` readable for `$APPLOCALDATA/screen-captures/*` only

- [ ] **Step 1: Write the failing tests for the split**

In `src-tauri/src/tray.rs`'s existing `#[cfg(test)] mod tests`, REPLACE
`the_hide_and_quit_walk_covers_every_configured_window` with the two tests
below, and ADD the third. The old test asserted one list covers both walks;
that is the claim this task retires.

```rust
    // The DESTROY walk must cover every configured window, editor included:
    // all of them share WebView2's Chrome_WidgetWin_0 class, and leaving one
    // alive fails the unregister with ERROR_CLASS_HAS_WINDOWS (1412).
    #[test]
    fn the_quit_walk_destroys_every_configured_window() {
        let configured = configured_labels();
        let mut walked: Vec<String> = ALL_WINDOW_LABELS.iter().map(|s| s.to_string()).collect();
        let mut expected = configured.clone();
        walked.sort();
        expected.sort();
        assert_eq!(
            walked, expected,
            "every window in tauri.conf.json must be destroyed on quit"
        );
        // The buddy is destroyed last: it is the window whose position the
        // quit path saves, and destroying it first would race that save.
        assert_eq!(ALL_WINDOW_LABELS.last(), Some(&"main"));
    }

    // The HIDE walk must cover every configured window EXCEPT the editor.
    // Hide-to-tray is a companion-surface gesture; the editor is a real
    // application window that can hold unsaved edits, and hiding it would
    // strand that work off-screen with no way back (spec 5.1, the GAP-82
    // class). Asserted as a derived set, not a literal, so a window added to
    // the config lands in exactly one of the two lists on purpose.
    #[test]
    fn the_hide_walk_covers_every_companion_window_and_not_the_editor() {
        let mut companions: Vec<String> =
            COMPANION_LABELS.iter().map(|s| s.to_string()).collect();
        let mut expected: Vec<String> = configured_labels()
            .into_iter()
            .filter(|l| l != "editor")
            .collect();
        companions.sort();
        expected.sort();
        assert_eq!(
            companions, expected,
            "the hide walk must cover every companion window and never the editor"
        );
        assert!(
            !COMPANION_LABELS.contains(&"editor"),
            "hiding the editor would strand unsaved edits off-screen"
        );
    }

    // The editor is a real application window: it is NOT transparent, IS
    // decorated and resizable, and is NOT always-on-top. Those four are what
    // make it unlike the three companion surfaces, and a later edit that
    // quietly makes it another always-on-top transparent panel would change
    // what the window IS without changing its name.
    #[test]
    fn the_editor_window_is_a_real_application_window() {
        let conf: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).expect("tauri.conf.json");
        let editor = conf["app"]["windows"]
            .as_array()
            .expect("app.windows")
            .iter()
            .find(|w| w["label"] == "editor")
            .expect("an editor window must be declared");
        assert_eq!(editor["transparent"], false);
        assert_eq!(editor["decorations"], true);
        assert_eq!(editor["resizable"], true);
        assert_eq!(editor["alwaysOnTop"], false);
        assert_eq!(editor["skipTaskbar"], false, "it is alt-tabbable by design");
        assert_eq!(editor["visible"], false, "created hidden, like panel and bubble");
    }
```

Also update `no_window_walk_restates_the_label_list`: it currently asserts
exactly two `in ALL_WINDOW_LABELS` walks. There is now one of each:

```rust
        let destroy_walks = production.matches("in ALL_WINDOW_LABELS").count();
        let hide_walks = production.matches("in COMPANION_LABELS").count();
        assert_eq!(
            destroy_walks, 1,
            "expected exactly one quit-time destroy walk over ALL_WINDOW_LABELS; found {destroy_walks}"
        );
        assert_eq!(
            hide_walks, 1,
            "expected exactly one hide walk over COMPANION_LABELS; found {hide_walks}"
        );
```

```bash
cd /home/user/vault-buddy/src-tauri && cargo test -p vault-buddy --lib tray 2>&1 | tail -20
```
Expected: FAIL — `COMPANION_LABELS` does not exist, and no `editor` window is declared.

- [ ] **Step 2: Declare the window, the asset protocol and the CSP**

`src-tauri/tauri.conf.json`. Add to `app.windows`, AFTER `overlay` and BEFORE
nothing (order in this array is not load-bearing; the lists' order is):

```jsonc
    {
      "label": "editor",
      "title": "Vault Buddy — Edit Capture",
      "width": 960,
      "height": 640,
      "minWidth": 720,
      "minHeight": 480,
      "visible": false,
      "resizable": true,
      "decorations": true,
      "transparent": false,
      "alwaysOnTop": false,
      "skipTaskbar": false,
      "shadow": true
    }
```

Replace `app.security` entirely:

```jsonc
  "security": {
    "csp": "default-src 'self'; connect-src ipc: http://ipc.localhost; style-src 'self' 'unsafe-inline'; img-src 'self' data:; media-src 'self' asset: http://asset.localhost",
    "assetProtocol": {
      "enable": true,
      "scope": ["$APPLOCALDATA/screen-captures/*"]
    }
  }
```

**The scope is the security boundary, not a convenience.** `$APPLOCALDATA`
resolves to `%LOCALAPPDATA%\com.vaultbuddy.desktop` (verified: Tauri's
`FsScope` documents `$APPLOCALDATA` among its base-directory variables), so
this grants the webview read access to the staging directory and nothing else
— never any vault path. Widening it to `$APPLOCALDATA/*` or adding a vault
root would let the editor webview read arbitrary user notes through the asset
handler.

`src-tauri/capabilities/default.json`: add `"editor"` to the `windows` array.
Without it the editor webview gets **no** permissions and every `invoke` from
it fails.

- [ ] **Step 3: Split the list**

`src-tauri/src/tray.rs`. Replace the single constant with two:

```rust
/// Every window the app owns, in destroy order — the buddy LAST, because the
/// quit path saves its position and destroying it first would race that save.
///
/// This is the QUIT walk. It must cover every configured window including the
/// editor: all of them share WebView2's `Chrome_WidgetWin_0` class, and
/// leaving even one alive fails the unregister with
/// `ERROR_CLASS_HAS_WINDOWS (1412)`.
pub const ALL_WINDOW_LABELS: [&str; 5] = ["panel", "bubble", "overlay", "editor", "main"];

/// The companion surfaces — every window hide-to-tray takes down.
///
/// Deliberately NOT `ALL_WINDOW_LABELS`: the editor is a real application
/// window that can hold unsaved edits, and hiding it would strand that work
/// off-screen with no way back (spec 5.1, the same class as GAP-82's
/// panel-hide problem). Hide-to-tray is a companion gesture; the editor is
/// closed by the user, by a successful save, or by an explicit discard.
pub const COMPANION_LABELS: [&str; 4] = ["panel", "bubble", "overlay", "main"];

/// Windows whose position the window-state plugin must NOT persist: every
/// window except the buddy. The panel, bubble and overlay are all positioned
/// fresh — while hidden — every time they are shown, and the editor opens at
/// its configured default, so a restored position is junk that only buys a
/// startup restore and a `Moved` handler holding the plugin's cache lock.
pub const POSITION_DENYLIST: [&str; 4] = ["panel", "bubble", "overlay", "editor"];
```

Change `hide_buddy`'s walk from `ALL_WINDOW_LABELS` to `COMPANION_LABELS`.
Leave `finish_quit`'s walk on `ALL_WINDOW_LABELS`. Add to `hide_buddy`'s doc
comment:

```rust
/// The editor is deliberately absent: see `COMPANION_LABELS`.
```

- [ ] **Step 4: Exclude the editor from recordings**

`src-tauri/src/capture_exclusion.rs`:

```rust
pub(crate) const EXCLUDED_LABELS: &[&str] = &["main", "panel", "bubble", "overlay", "editor"];
```

Phase 3's `every_declared_window_is_excluded_from_capture` reads
`tauri.conf.json` and will already be failing at this point — that is the
mechanism working as designed, not a surprise. The editor is `skipTaskbar:
false`, so unlike the three companion windows it really does enumerate as a
capture source, which makes this the first label whose exclusion is not
inert.

- [ ] **Step 5: Mount a root so the window is not blank**

`src/roots/EditorRoot.vue` — a placeholder this task only needs to render;
Task 4 replaces its body.

```vue
<script setup lang="ts">
/**
 * The editor window's root (spec 8). Unlike the three companion roots this
 * one mirrors NO Rust state and installs no store: it is handed one staged
 * capture and edits it locally, persisting through `save_capture_timeline`.
 */
</script>

<template>
  <main class="flex h-screen w-screen items-center justify-center bg-slate-900 text-fg">
    <p class="text-sm text-fg-muted">Loading capture…</p>
  </main>
</template>
```

`src/roots/index.ts`:

```ts
import EditorRoot from "./EditorRoot.vue";
...
  if (label === "editor") return EditorRoot;
```

Add to `tests/roots.test.ts`, matching that file's existing idiom:

```ts
  it("mounts the editor root for the editor window", () => {
    expect(rootFor("editor")).toBe(EditorRoot);
  });
```

- [ ] **Step 6: Run the gates**

```bash
cd /home/user/vault-buddy/src-tauri && cargo fmt --check; echo "fmt exit=$?"
cd /home/user/vault-buddy/src-tauri && cargo clippy --workspace --all-targets -- -D warnings; echo "clippy exit=$?"
cd /home/user/vault-buddy/src-tauri && cargo test -p vault-buddy --lib; echo "shell exit=$?"
cd /home/user/vault-buddy && rm -rf coverage && npm run lint; echo "lint exit=$?"
cd /home/user/vault-buddy && npm run check:loc; echo "loc exit=$?"
cd /home/user/vault-buddy && npm run check:quality; echo "quality exit=$?"
cd /home/user/vault-buddy && npm run build; echo "build exit=$?"
cd /home/user/vault-buddy && npm run test:coverage; echo "coverage exit=$?"
```
Every one must be `exit=0`.

- [ ] **Step 7: Mutation-verify**

| Mutation | Must fail |
| --- | --- |
| `COMPANION_LABELS` gains `"editor"` | `the_hide_walk_covers_every_companion_window_and_not_the_editor` |
| `ALL_WINDOW_LABELS` drops `"editor"` | `the_quit_walk_destroys_every_configured_window` |
| `hide_buddy` walks `ALL_WINDOW_LABELS` again | `the_hide_walk_…` **and** `no_window_walk_restates_the_label_list` |
| `EXCLUDED_LABELS` drops `"editor"` | `every_declared_window_is_excluded_from_capture` |
| `tauri.conf.json` editor `"transparent": true` | `the_editor_window_is_a_real_application_window` |
| `assetProtocol.scope` widened to `["$APPLOCALDATA/*"]` | *(nothing — see below)* |

The last row has no test and that is the point: **the scope is not pinned by
anything**. Note it in your report; Task 7 files it as a gap. A test would
have to parse `tauri.conf.json` and assert the scope is exactly the staging
glob, which is worth adding if you can do it cheaply.

Restore each mutation byte-identically and confirm with `md5sum`.

- [ ] **Step 8: Commit**

```bash
cd /home/user/vault-buddy
cat > /tmp/msg-p4t1.txt <<'MSG'
feat(shell): add the editor window, and split the walk it cannot share

Spec 5.1's fourth window: decorated, resizable, alt-tabbable, not
always-on-top -- deliberately unlike the three companion surfaces, because
it is a real application window a user works in rather than a transient
panel.

That forces the one window-label list into two. It must be destroyed on
quit, since every webview shares WebView2's Chrome_WidgetWin_0 class and
leaving one alive fails the unregister; it must NOT be hidden to tray,
since hide-to-tray is a companion gesture and hiding a window holding
unsaved edits strands that work off-screen with no way back. One constant
cannot express both rules, so there are now two, each pinned to
tauri.conf.json by its own test -- not an inline exception inside
hide_buddy, which the structural test already forbids.

The asset protocol is enabled with its scope restricted to the staging
directory alone. That narrowness is the security boundary: the editor
webview must be able to read the capture it is editing and nothing else,
never a vault path, or the asset handler becomes a way to read the user's
notes.

The editor is the first excluded label whose exclusion is not inert. The
three companion windows are skipTaskbar, which tao implements as
WS_EX_TOOLWINDOW and windows-capture already rejects; the editor is
alt-tabbable and really does enumerate as a capture source.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01Ud2Yq3jSXMKVdZaMSTeRco
MSG
git add src-tauri/tauri.conf.json src-tauri/capabilities/default.json src-tauri/src/tray.rs src-tauri/src/capture_exclusion.rs src/roots/EditorRoot.vue src/roots/index.ts tests/roots.test.ts
git commit -F /tmp/msg-p4t1.txt
```

---

### Task 2: Opening the editor on a staged capture

Spec §5.1 and §11. `open_capture_editor` is listed **sync** in §11 ("window
show/focus, main thread") — but the editor also needs the capture's sidecar,
which is disk I/O. Those two facts are only compatible one way: the sync
command shows the window and **stashes** the base name; the editor webview
then asks for its data with a separate async command once it has mounted.

That is not an invention — it is exactly the shape
`document_commands::begin_document_import` / `take_pending_import` already
uses (`src-tauri/src/document_commands.rs:436-456`), for the same reason: the
target window has its own Pinia store and cannot be handed state directly.

**Files:**
- Create: `src-tauri/src/editor_commands.rs`
- Modify: `src-tauri/src/lib.rs` (`mod`, `.manage(...)`, three handler entries)
- Modify: `src/types.ts` (`StagedCaptureDetail`, `SegmentDto`, `TimelineDto`)
- Test: inline `#[cfg(test)]` in `editor_commands.rs` (runs in the `linux-app` CI job)

**Interfaces:**
- Consumes: `tray::ALL_WINDOW_LABELS` (Task 1); `vault_buddy_screen::staging::{StagedSidecar, read_sidecar, sidecar_file_name, staging_dir, mp4_file_name}`; `vault_buddy_core::sync_util::lock_ignoring_poison`.
- Produces, for Tasks 3-7:
  - `open_capture_editor(base: String) -> Result<(), String>` *(sync)*
  - `take_editor_request() -> Option<String>` *(sync, one-shot drain)*
  - `load_staged_capture(base: String) -> Result<StagedCaptureDetail, String>` *(async)*
  - the wire DTO `StagedCaptureDetail { base, assetPath, durationMs, sourceTitle, width, height, recordedAt, timeline: TimelineDto | null }`

- [ ] **Step 1: Write the failing tests for the pure core**

Create `src-tauri/src/editor_commands.rs` with the module doc, the DTOs, a
`todo!()` body for `detail_from_sidecar` and `is_safe_base`, and this test
module:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use vault_buddy_screen::staging::StagedSidecar;

    fn sidecar(base: &str) -> StagedSidecar {
        StagedSidecar {
            base: base.to_string(),
            vault_id: "v1".into(),
            source_title: "Screen 1".into(),
            source_kind: "screen".into(),
            inputs: vec!["Mic".into()],
            duration_ms: 42_000,
            paused_ms: 0,
            width: 1920,
            height: 1080,
            recorded_at: "2026-09-20T10:00:00Z".into(),
            timeline: None,
        }
    }

    // A base name travels from the frontend, so it is untrusted input that
    // becomes a PATH. `..` or a separator in it would read a file outside
    // the staging directory — the `sanitize_title` lesson from phase 2,
    // applied at the other end of the same pipe.
    #[test]
    fn a_base_that_could_escape_staging_is_refused() {
        assert!(is_safe_base("2026-09-20 1432 Figma walkthrough"));
        assert!(!is_safe_base("../../../etc/passwd"));
        assert!(!is_safe_base("a/b"));
        assert!(!is_safe_base("a\\b"));
        assert!(!is_safe_base(".."));
        assert!(!is_safe_base(""));
        // A leading dot is how our own in-progress `.part` files are named;
        // the editor must never be pointed at one.
        assert!(!is_safe_base(".hidden"));
    }

    // The detail the editor renders is derived, not echoed: the webview gets
    // an ASSET url it can actually load, never a raw disk path, because the
    // asset protocol is the only way it can read the file at all.
    #[test]
    fn the_detail_carries_an_asset_url_not_a_disk_path() {
        let d = detail_from_sidecar(&sidecar("cap one"), "cap one.mp4");
        assert_eq!(d.base, "cap one");
        assert_eq!(d.asset_path, "cap one.mp4");
        assert_eq!(d.duration_ms, 42_000);
        assert_eq!(d.width, 1920);
        assert_eq!(d.height, 1080);
        assert_eq!(d.source_title, "Screen 1");
        assert!(d.timeline.is_none(), "an untouched capture has no timeline yet");
    }

    // A sidecar written by a previous editing session must come back as a
    // timeline, or every crash would silently discard the edit it promised
    // to preserve.
    #[test]
    fn a_saved_timeline_round_trips_into_the_detail() {
        let mut s = sidecar("cap");
        s.timeline = Some(serde_json::json!({
            "segments": [{"sourceStartMs": 0, "sourceEndMs": 1000}]
        }));
        let d = detail_from_sidecar(&s, "cap.mp4");
        let t = d.timeline.expect("a saved timeline must survive the round trip");
        assert_eq!(t["segments"][0]["sourceEndMs"], 1000);
    }
}
```

```bash
cd /home/user/vault-buddy/src-tauri && cargo test -p vault-buddy --lib editor_commands 2>&1 | tail -20
```
Expected: FAIL (the `todo!()` bodies panic). Add `mod editor_commands;` to
`lib.rs` first or it will not compile at all.

- [ ] **Step 2: Implement the pure core**

```rust
//! The capture editor's IPC surface (spec 5.1, 8, 11).
//!
//! **Why opening the editor takes two commands.** Spec 11 lists
//! `open_capture_editor` as SYNC, because it shows and focuses a window and
//! the window APIs are main-thread-only. But the editor also needs its
//! capture's sidecar, which is disk I/O a sync command must never do. So the
//! sync command shows the window and stashes the base name, and the editor
//! webview drains that stash and fetches its own data asynchronously once it
//! has mounted — the same split `document_commands::begin_document_import`
//! and `take_pending_import` already use, for the same reason: the target
//! window has its own Pinia store and cannot be handed state directly.

use std::sync::Mutex;

use tauri::{AppHandle, Manager};
use vault_buddy_core::sync_util::lock_ignoring_poison;
use vault_buddy_screen::staging;

const EDITOR_LABEL: &str = "editor";

/// The base name the editor window should open when it next mounts.
/// Rust-owned because the panel and the editor are separate webviews with
/// separate stores; a one-shot slot, drained by `take_editor_request`.
#[derive(Default)]
pub struct EditorRequest(pub Mutex<Option<String>>);

/// What the editor renders. `asset_path` is the file name RELATIVE to the
/// staging directory — the webview joins it onto the asset origin itself.
/// Deliberately not an absolute disk path: the asset protocol's scope is the
/// only thing that lets the webview read the file, and handing it a raw path
/// would invite a future caller to widen that scope.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StagedCaptureDetail {
    pub base: String,
    pub asset_path: String,
    pub duration_ms: u64,
    pub source_title: String,
    pub width: u32,
    pub height: u32,
    pub recorded_at: String,
    pub timeline: Option<serde_json::Value>,
}

/// Is this base name safe to turn into a path inside the staging directory?
///
/// The base travels from the frontend, so it is untrusted input that becomes
/// a path. A separator or `..` would read a file outside staging entirely;
/// a leading dot is how our own in-progress `.part` files are named and must
/// never be opened as a finished capture. This is `sanitize_title`'s lesson
/// (phase 2, spec 10) applied at the other end of the same pipe.
fn is_safe_base(base: &str) -> bool {
    !base.is_empty()
        && !base.starts_with('.')
        && !base.contains('/')
        && !base.contains('\\')
        && !base.contains("..")
        && base.chars().all(|c| !c.is_control())
}

fn detail_from_sidecar(s: &staging::StagedSidecar, asset_path: &str) -> StagedCaptureDetail {
    StagedCaptureDetail {
        base: s.base.clone(),
        asset_path: asset_path.to_string(),
        duration_ms: s.duration_ms,
        source_title: s.source_title.clone(),
        width: s.width,
        height: s.height,
        recorded_at: s.recorded_at.clone(),
        timeline: s.timeline.clone(),
    }
}
```

Re-run Step 1's command: all three tests pass.

- [ ] **Step 3: Write the three commands**

Append to `editor_commands.rs`:

```rust
/// SYNC: shows and focuses the editor window and stashes which capture it
/// should open. Window APIs are main-thread-only and this command touches no
/// disk, so sync is correct — see the module doc for why the data arrives
/// separately.
#[tauri::command]
pub fn open_capture_editor(app: AppHandle, base: String) -> Result<(), String> {
    if !is_safe_base(&base) {
        return Err("That capture name is not one of ours.".to_string());
    }
    *lock_ignoring_poison(&app.state::<EditorRequest>().0) = Some(base);
    let window = app
        .get_webview_window(EDITOR_LABEL)
        .ok_or_else(|| "The editor window is missing.".to_string())?;
    window
        .show()
        .map_err(|e| format!("Could not open the editor: {e}"))?;
    // An editor already open on another capture is raised, not duplicated:
    // there is one editor window and the stash it just read is the new one.
    let _ = window.unminimize();
    let _ = window.set_focus();
    Ok(())
}

/// SYNC, one-shot: the editor webview drains this on mount. Returns `None`
/// when the window was opened with nothing staged, which is not an error —
/// a user can alt-tab back to an editor that is already showing a capture.
#[tauri::command]
pub fn take_editor_request(app: AppHandle) -> Option<String> {
    lock_ignoring_poison(&app.state::<EditorRequest>().0).take()
}

/// ASYNC: reads the sidecar off disk, so it must not sit on the main thread.
#[tauri::command]
pub async fn load_staged_capture(
    app: AppHandle,
    base: String,
) -> Result<StagedCaptureDetail, String> {
    if !is_safe_base(&base) {
        return Err("That capture name is not one of ours.".to_string());
    }
    let dir = staging::staging_dir(
        &app.path()
            .app_local_data_dir()
            .map_err(|e| format!("Could not resolve the staging directory: {e}"))?,
    );
    tauri::async_runtime::spawn_blocking(move || {
        let sidecar_path = dir.join(staging::sidecar_file_name(&base));
        let sidecar = staging::read_sidecar(&sidecar_path)
            .ok_or_else(|| "That capture's details could not be read.".to_string())?;
        let mp4 = staging::mp4_file_name(&base);
        if !dir.join(&mp4).is_file() {
            return Err("That capture's video file is missing.".to_string());
        }
        Ok(detail_from_sidecar(&sidecar, &mp4))
    })
    .await
    .map_err(|e| format!("Loading the capture failed: {e}"))?
}
```

- [ ] **Step 4: Register**

`src-tauri/src/lib.rs`: `mod editor_commands;` in alphabetical position;
`.manage(editor_commands::EditorRequest::default())` beside the other state;
and in `generate_handler![…]`, after the `region_commands` entries:

```rust
            editor_commands::open_capture_editor,
            editor_commands::take_editor_request,
            editor_commands::load_staged_capture,
```

- [ ] **Step 5: Add the wire types**

`src/types.ts`:

```ts
/** One cut of the source, in SOURCE milliseconds. Mirrors
 * `core::timeline::Segment`. */
export interface SegmentDto {
  sourceStartMs: number;
  sourceEndMs: number;
}

/** The editor's in-progress edit. Mirrors `core::timeline::Timeline`, and is
 * what the staging sidecar's `timeline` field holds. */
export interface TimelineDto {
  segments: SegmentDto[];
}

/** What `load_staged_capture` returns. `assetPath` is the file name RELATIVE
 * to the staging directory — the editor joins it onto the asset origin
 * itself, because the asset protocol's staging-only scope is the only thing
 * that lets the webview read it. */
export interface StagedCaptureDetail {
  base: string;
  assetPath: string;
  durationMs: number;
  sourceTitle: string;
  width: number;
  height: number;
  recordedAt: string;
  timeline: TimelineDto | null;
}
```

- [ ] **Step 6: Gates and mutation verification**

Run Task 1 Step 6's full gate list. Then:

| Mutation | Must fail |
| --- | --- |
| `is_safe_base` drops the `contains("..")` check | `a_base_that_could_escape_staging_is_refused` |
| `is_safe_base` drops the `starts_with('.')` check | same |
| `is_safe_base` returns `true` unconditionally | same |
| `detail_from_sidecar` sets `timeline: None` always | `a_saved_timeline_round_trips_into_the_detail` |
| `detail_from_sidecar` swaps `width`/`height` | `the_detail_carries_an_asset_url_not_a_disk_path` |

The width/height row is why that test asserts 1920 and 1080 rather than a
square fixture: a symmetric fixture cannot distinguish a swap, which this
branch has shipped before.

Restore each byte-identically, confirm with `md5sum`.

- [ ] **Step 7: Commit**

```bash
cd /home/user/vault-buddy
cat > /tmp/msg-p4t2.txt <<'MSG'
feat(shell): open the editor on a staged capture

Spec 11 lists open_capture_editor as sync, because it shows and focuses a
window and the window APIs are main-thread only. The editor also needs its
capture's sidecar, which is disk work a sync command must never do. Those
two facts are compatible exactly one way: the sync command shows the window
and stashes the base name, and the editor webview drains that stash and
fetches its own data once it has mounted. That is the split
begin_document_import and take_pending_import already use, for the same
reason -- the target window has its own store and cannot be handed state.

The base name travels from the frontend and becomes a path, so it is
validated before it is joined: a separator or a dot-dot would read outside
the staging directory, and a leading dot is how our own in-progress part
files are named and must never open as a finished capture. That is
sanitize_title's lesson applied at the other end of the same pipe.

The detail carries the file NAME, not a disk path. The webview can only
read that file through the asset protocol's staging-scoped origin, and
handing it an absolute path would invite a later caller to widen the scope
to match.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01Ud2Yq3jSXMKVdZaMSTeRco
MSG
git add src-tauri/src/editor_commands.rs src-tauri/src/lib.rs src/types.ts
git commit -F /tmp/msg-p4t2.txt
```

---

### Task 3: Persisting the edit — `save_capture_timeline`

Spec §10: "The JSON sidecar carries what the editor needs to resume —
including the in-progress timeline, saved on each edit — so a crash mid-edit
loses at most the last operation." The sidecar field already exists
(`StagedSidecar.timeline`, documented "saved on each editor operation (phase
4)"); nothing writes it yet.

**A spec gap this task closes:** §11's command table does not name a
save-the-timeline command, but §10 requires the behaviour. `save_capture_timeline`
is that command. Record it in the report so Task 7 adds it to the IPC table
with the count.

**Files:**
- Modify: `src-tauri/src/editor_commands.rs`
- Modify: `src-tauri/src/lib.rs` (one handler entry)

**Interfaces:**
- Consumes: Task 2's `is_safe_base`, `EditorRequest`, `staging` helpers.
- Produces: `save_capture_timeline(base: String, timeline: TimelineDto | null) -> Result<(), String>` *(async)*

- [ ] **Step 1: Write the failing test**

Append to `editor_commands.rs`'s test module:

```rust
    // The sidecar is read-modify-written, never rebuilt: everything except
    // the timeline is the capture's own recorded truth (duration, inputs,
    // the vault it belongs to), and a save that reconstructed those fields
    // from what the editor happens to know would quietly lose whatever the
    // editor does not carry.
    #[test]
    fn saving_a_timeline_preserves_every_other_sidecar_field() {
        let mut s = sidecar("cap");
        s.inputs = vec!["Mic".into(), "Speakers".into()];
        s.paused_ms = 7_000;
        let updated = with_timeline(
            s.clone(),
            Some(serde_json::json!({"segments": [{"sourceStartMs": 5, "sourceEndMs": 9}]})),
        );
        assert_eq!(updated.inputs, s.inputs);
        assert_eq!(updated.paused_ms, 7_000);
        assert_eq!(updated.duration_ms, s.duration_ms);
        assert_eq!(updated.vault_id, s.vault_id);
        assert_eq!(updated.recorded_at, s.recorded_at);
        assert_eq!(updated.timeline.unwrap()["segments"][0]["sourceStartMs"], 5);
    }

    // Clearing is how "revert to the whole capture" is expressed, and it
    // must remove the field rather than store an empty timeline — an empty
    // segment list is a capture Save refuses (spec 8.1), which is not the
    // same thing as an untouched one.
    #[test]
    fn clearing_a_timeline_removes_it_rather_than_storing_an_empty_one() {
        let mut s = sidecar("cap");
        s.timeline = Some(serde_json::json!({"segments": []}));
        let updated = with_timeline(s, None);
        assert!(updated.timeline.is_none());
    }
```

```bash
cd /home/user/vault-buddy/src-tauri && cargo test -p vault-buddy --lib editor_commands 2>&1 | tail -15
```
Expected: FAIL — `with_timeline` does not exist.

- [ ] **Step 2: Implement**

```rust
/// Return the sidecar with only its timeline replaced.
///
/// Read-modify-write, never rebuild. Everything else in the sidecar is the
/// capture's own recorded truth — its duration, its inputs, the vault it was
/// recorded for — and the editor does not know all of it. A save that
/// reconstructed the struct from what the editor carries would silently drop
/// whatever it does not.
fn with_timeline(
    mut sidecar: staging::StagedSidecar,
    timeline: Option<serde_json::Value>,
) -> staging::StagedSidecar {
    sidecar.timeline = timeline;
    sidecar
}

/// ASYNC: an fsync'd sidecar rewrite on every editor operation must not sit
/// on the main thread.
#[tauri::command]
pub async fn save_capture_timeline(
    app: AppHandle,
    base: String,
    timeline: Option<serde_json::Value>,
) -> Result<(), String> {
    if !is_safe_base(&base) {
        return Err("That capture name is not one of ours.".to_string());
    }
    let dir = staging::staging_dir(
        &app.path()
            .app_local_data_dir()
            .map_err(|e| format!("Could not resolve the staging directory: {e}"))?,
    );
    tauri::async_runtime::spawn_blocking(move || {
        let sidecar_path = dir.join(staging::sidecar_file_name(&base));
        let existing = staging::read_sidecar(&sidecar_path)
            .ok_or_else(|| "That capture's details could not be read.".to_string())?;
        staging::write_sidecar(&dir, &with_timeline(existing, timeline))
            .map_err(|e| format!("Could not save the edit: {e}"))?;
        Ok(())
    })
    .await
    .map_err(|e| format!("Saving the edit failed: {e}"))?
}
```

Register in `lib.rs` beside the other three:

```rust
            editor_commands::save_capture_timeline,
```

- [ ] **Step 3: Gates and mutation verification**

Run Task 1 Step 6's gate list.

| Mutation | Must fail |
| --- | --- |
| `with_timeline` rebuilds the struct, defaulting `inputs` to `vec![]` | `saving_a_timeline_preserves_every_other_sidecar_field` |
| `with_timeline` ignores its `timeline` argument | both new tests |
| `with_timeline` maps `None` to `Some(json!({"segments": []}))` | `clearing_a_timeline_removes_it_rather_than_storing_an_empty_one` |

Restore byte-identically; `md5sum`.

- [ ] **Step 4: Commit**

```bash
cd /home/user/vault-buddy
cat > /tmp/msg-p4t3.txt <<'MSG'
feat(shell): persist the editor's timeline to the staging sidecar

Spec 10 requires the in-progress edit to be saved on each operation so a
crash mid-edit loses at most the last one. The sidecar field has existed
since phase 2, documented as phase 4's to fill; this fills it.

Spec 11's command table does not name this command, which is a gap in the
spec rather than in the phasing -- the behaviour it implements is spec 10's
and is not optional.

The sidecar is read-modify-written rather than rebuilt. Everything else in
it is the capture's own recorded truth, including fields the editor never
sees, and a save that reconstructed the struct from what the editor happens
to carry would drop them silently.

Clearing removes the field rather than storing an empty segment list. An
empty timeline is a capture that Save refuses; an absent one is a capture
nobody has edited. Collapsing the two would make a reverted capture
unsaveable.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01Ud2Yq3jSXMKVdZaMSTeRco
MSG
git add src-tauri/src/editor_commands.rs src-tauri/src/lib.rs
git commit -F /tmp/msg-p4t3.txt
```

---

### Task 4: The pure timeline geometry

Everything the timeline strip needs to be correct is arithmetic, and it is
extracted here for the same reason `core::screen_geometry` was: it is the
only part of the editor a CI runner can execute.

**Files:**
- Create: `src/utils/timelineGeometry.ts`
- Create: `tests/timelineGeometry.test.ts`

**Interfaces:**
- Consumes: `SegmentDto`, `TimelineDto` (Task 2).
- Produces, for Tasks 5-6:
  - `outputDurationMs(t: TimelineDto): number`
  - `segmentWidths(t: TimelineDto): number[]` — percentages summing to 100
  - `toSourceMs(t: TimelineDto, outputMs: number): number | null`
  - `toOutputMs(t: TimelineDto, sourceMs: number): number | null` — the inverse
  - `segmentAtOutputMs(t: TimelineDto, outputMs: number): number | null`
  - `dropIndex(widths: number[], fractionX: number): number`

- [ ] **Step 1: Write the failing tests**

```ts
import { describe, expect, it } from "vitest";

import {
  dropIndex,
  outputDurationMs,
  segmentAtOutputMs,
  segmentWidths,
  toOutputMs,
  toSourceMs,
} from "../src/utils/timelineGeometry";

const T = {
  segments: [
    { sourceStartMs: 0, sourceEndMs: 1000 },
    { sourceStartMs: 5000, sourceEndMs: 8000 },
  ],
};

describe("timeline geometry", () => {
  it("sums segment durations, not source span", () => {
    // 1000 + 3000 — the 4000 ms cut out between them is gone.
    expect(outputDurationMs(T)).toBe(4000);
    expect(outputDurationMs({ segments: [] })).toBe(0);
  });

  it("renders widths as percentages of the OUTPUT duration", () => {
    expect(segmentWidths(T)).toEqual([25, 75]);
    // An empty timeline has no blocks rather than a divide-by-zero.
    expect(segmentWidths({ segments: [] })).toEqual([]);
  });

  // The whole point of the model: output time is not source time.
  it("maps output time back into the source", () => {
    expect(toSourceMs(T, 0)).toBe(0);
    expect(toSourceMs(T, 999)).toBe(999);
    // The first millisecond of segment 2 is source 5000, not 1000.
    expect(toSourceMs(T, 1000)).toBe(5000);
    expect(toSourceMs(T, 2500)).toBe(6500);
    // Past the end is not a source time at all.
    expect(toSourceMs(T, 4000)).toBeNull();
    expect(toSourceMs(T, 9999)).toBeNull();
    expect(toSourceMs({ segments: [] }, 0)).toBeNull();
  });

  // The INVERSE, and the preview cannot work without it: the video element
  // reports SOURCE time, while the strip, the playhead and the scrubber all
  // speak output time. A source moment that was cut out has no output time
  // at all, which is a real answer, not an error.
  it("maps source time back into the output", () => {
    expect(toOutputMs(T, 0)).toBe(0);
    expect(toOutputMs(T, 999)).toBe(999);
    expect(toOutputMs(T, 5000)).toBe(1000);
    expect(toOutputMs(T, 6500)).toBe(2500);
    // 1000..5000 was cut out — it is nowhere in the output.
    expect(toOutputMs(T, 2000)).toBeNull();
    expect(toOutputMs(T, 9000)).toBeNull();
    expect(toOutputMs({ segments: [] }, 0)).toBeNull();
  });

  // Round-trip: the two must agree, or the playhead drifts from the frame
  // on screen every time playback crosses a cut.
  it("round-trips output through source and back", () => {
    for (const ms of [0, 1, 999, 1000, 2500, 3999]) {
      expect(toOutputMs(T, toSourceMs(T, ms) as number)).toBe(ms);
    }
  });

  it("names the segment an output time falls in", () => {
    expect(segmentAtOutputMs(T, 0)).toBe(0);
    expect(segmentAtOutputMs(T, 999)).toBe(0);
    // The boundary belongs to the segment it STARTS, not the one it ends.
    expect(segmentAtOutputMs(T, 1000)).toBe(1);
    expect(segmentAtOutputMs(T, 3999)).toBe(1);
    expect(segmentAtOutputMs(T, 4000)).toBeNull();
  });

  // Drag-to-reorder: where does a drop at this fraction of the strip land?
  // Asymmetric widths on purpose — equal widths cannot distinguish a
  // midpoint rule from an accumulate-until-exceeded rule.
  it("picks a drop index from where the pointer is along the strip", () => {
    const w = [25, 75];
    expect(dropIndex(w, 0)).toBe(0);
    expect(dropIndex(w, 0.1)).toBe(0); // before block 0's midpoint (12.5%)
    expect(dropIndex(w, 0.2)).toBe(1); // past it
    expect(dropIndex(w, 0.6)).toBe(1); // before block 1's midpoint (62.5%)
    expect(dropIndex(w, 0.9)).toBe(2); // past it — dropped at the end
    expect(dropIndex([], 0.5)).toBe(0);
  });
});
```

```bash
cd /home/user/vault-buddy && npx vitest run tests/timelineGeometry.test.ts 2>&1 | tail -10
```
Expected: FAIL — cannot resolve the module.

- [ ] **Step 2: Implement**

```ts
import type { TimelineDto } from "../types";

/** The output duration: the sum of the segments' own lengths, NOT the span
 * of the source they were cut from. Everything the strip renders is a
 * fraction of this. */
export function outputDurationMs(t: TimelineDto): number {
  return t.segments.reduce((sum, s) => sum + (s.sourceEndMs - s.sourceStartMs), 0);
}

/** Each segment's share of the strip, as a percentage. Empty in, empty out —
 * an empty timeline must not divide by zero. */
export function segmentWidths(t: TimelineDto): number[] {
  const total = outputDurationMs(t);
  if (total === 0) return [];
  return t.segments.map((s) => ((s.sourceEndMs - s.sourceStartMs) / total) * 100);
}

/** Which segment an output time falls in, or `null` past the end.
 *
 * A boundary belongs to the segment it STARTS. The alternative puts the
 * playhead in the segment that just ended, which makes the preview seek
 * backwards at every cut. */
export function segmentAtOutputMs(t: TimelineDto, outputMs: number): number | null {
  if (outputMs < 0) return null;
  let acc = 0;
  for (let i = 0; i < t.segments.length; i += 1) {
    acc += t.segments[i].sourceEndMs - t.segments[i].sourceStartMs;
    if (outputMs < acc) return i;
  }
  return null;
}

/** Output time → source time. `null` past the end.
 *
 * This is the function the preview seeks on, and the reason the editor can
 * cut at all: the two clocks are different, and only the timeline knows the
 * mapping. Mirrors `core::timeline::to_source_ms`; keep them in step. */
export function toSourceMs(t: TimelineDto, outputMs: number): number | null {
  if (outputMs < 0) return null;
  let acc = 0;
  for (const s of t.segments) {
    const len = s.sourceEndMs - s.sourceStartMs;
    if (outputMs < acc + len) return s.sourceStartMs + (outputMs - acc);
    acc += len;
  }
  return null;
}

/** Source time -> output time, or `null` when that source moment was cut
 * out and is nowhere in the output.
 *
 * The inverse of `toSourceMs`, and the preview depends on it: the `<video>`
 * element reports SOURCE time while everything the user sees — the strip,
 * the playhead, the scrubber — speaks output time. Without this the two
 * clocks drift apart at the first cut. */
export function toOutputMs(t: TimelineDto, sourceMs: number): number | null {
  let acc = 0;
  for (const s of t.segments) {
    if (sourceMs >= s.sourceStartMs && sourceMs < s.sourceEndMs) {
      return acc + (sourceMs - s.sourceStartMs);
    }
    acc += s.sourceEndMs - s.sourceStartMs;
  }
  return null;
}

/** Where a drop at `fractionX` (0-1 along the strip) inserts.
 *
 * Midpoint rule: past a block's centre means after it. Returns a value in
 * `[0, widths.length]` — the upper bound is "dropped at the end", which is a
 * real destination, not an overflow. */
export function dropIndex(widths: number[], fractionX: number): number {
  const x = fractionX * 100;
  let acc = 0;
  for (let i = 0; i < widths.length; i += 1) {
    if (x < acc + widths[i] / 2) return i;
    acc += widths[i];
  }
  return widths.length;
}
```

Re-run: all six pass.

- [ ] **Step 3: Gates and mutation verification**

Run the frontend half of Task 1 Step 6 (`lint`, `check:loc`, `check:quality`,
`build`, `test:coverage`).

| Mutation | Must fail |
| --- | --- |
| `outputDurationMs` sums `sourceEndMs` instead of the difference | `sums segment durations, not source span` |
| `segmentAtOutputMs` uses `<=` instead of `<` | `names the segment an output time falls in` (the boundary case) |
| `toSourceMs` returns `outputMs` unchanged | `maps output time back into the source` |
| `toSourceMs` drops the `+ (outputMs - acc)` offset | same |
| `dropIndex` compares `x < acc + widths[i]` (no midpoint) | `picks a drop index…` |
| `segmentWidths` divides by `t.segments.length` | `renders widths as percentages…` |
| `toOutputMs` returns `sourceMs` unchanged | `maps source time back into the output`, `round-trips…` |
| `toOutputMs` uses `<=` on `sourceEndMs` | `maps source time back into the output` (the cut-out case) |

Restore byte-identically; `md5sum`.

- [ ] **Step 4: Commit**

```bash
cd /home/user/vault-buddy
cat > /tmp/msg-p4t4.txt <<'MSG'
feat(ui): add the editor's pure timeline geometry

Everything the timeline strip needs to be correct is arithmetic, so it is
extracted where a CI runner can execute it -- the same reasoning that put
screen_geometry in the core crate, applied to the frontend half.

The function that matters is output-to-source mapping. Output time is not
source time the moment anything is cut, and only the timeline knows the
offset; the preview seeks on this and export will plan on its Rust twin.

A boundary belongs to the segment it starts, not the one it ends. The
alternative leaves the playhead in the segment that just finished, which
makes the preview seek backwards at every cut.

The drop-index fixtures are asymmetric on purpose. Equal widths cannot
distinguish a midpoint rule from accumulate-until-exceeded, and a fixture
that cannot fail its own mutation has been shipped on this branch before.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01Ud2Yq3jSXMKVdZaMSTeRco
MSG
git add src/utils/timelineGeometry.ts tests/timelineGeometry.test.ts
git commit -F /tmp/msg-p4t4.txt
```

---

### Task 5: The undo/redo stack and the persist-on-edit composable

Spec §8.1: "Every operation returns a **new** `Timeline`, which makes
undo/redo a stack of snapshots — trivially correct, and cheap because the
structure is a handful of integer pairs."

**Files:**
- Create: `src/composables/useEditorTimeline.ts`
- Create: `tests/useEditorTimeline.test.ts`

**Interfaces:**
- Consumes: `TimelineDto`, `SegmentDto` (Task 2); `save_capture_timeline` (Task 3); `outputDurationMs`, `segmentAtOutputMs` (Task 4).
- Produces, for Tasks 6-7: `useEditorTimeline(base, initial)` returning `{ timeline, canUndo, canRedo, isDirty, splitAt, deleteSegment, reorder, undo, redo, revert, flushPending }`.

- [ ] **Step 1: Write the failing tests**

```ts
import { beforeEach, describe, expect, it, vi } from "vitest";
import { mockIPC } from "@tauri-apps/api/mocks";

import { useEditorTimeline } from "../src/composables/useEditorTimeline";

const WHOLE = { segments: [{ sourceStartMs: 0, sourceEndMs: 10_000 }] };

function calls(): Record<string, unknown>[] {
  const seen: Record<string, unknown>[] = [];
  mockIPC((cmd, args) => {
    seen.push({ cmd, ...(args as object) });
    return undefined;
  });
  return seen;
}

describe("useEditorTimeline", () => {
  beforeEach(() => vi.useRealTimers());

  it("splits at the playhead and can undo back to the whole capture", () => {
    calls();
    const t = useEditorTimeline("cap", WHOLE);
    expect(t.canUndo.value).toBe(false);
    t.splitAt(4000);
    expect(t.timeline.value.segments).toHaveLength(2);
    expect(t.timeline.value.segments[1].sourceStartMs).toBe(4000);
    expect(t.canUndo.value).toBe(true);
    t.undo();
    expect(t.timeline.value.segments).toHaveLength(1);
    expect(t.canRedo.value).toBe(true);
    t.redo();
    expect(t.timeline.value.segments).toHaveLength(2);
  });

  // Spec 8.1: a split exactly on a boundary is a no-op, NOT a zero-length
  // segment — a zero-length segment produces an unplayable file downstream.
  // A no-op must also not push an undo entry, or Ctrl+Z appears to do
  // nothing while actually consuming a step.
  it("refuses to split on a boundary and does not record an undo step", () => {
    calls();
    const t = useEditorTimeline("cap", WHOLE);
    t.splitAt(0);
    expect(t.timeline.value.segments).toHaveLength(1);
    expect(t.canUndo.value).toBe(false);
    t.splitAt(10_000);
    expect(t.timeline.value.segments).toHaveLength(1);
    expect(t.canUndo.value).toBe(false);
  });

  // A new edit after an undo discards the redo branch. Keeping it would let
  // Ctrl+Y jump to a timeline that never followed from what is on screen.
  it("drops the redo branch once a new edit lands", () => {
    calls();
    const t = useEditorTimeline("cap", WHOLE);
    t.splitAt(3000);
    t.undo();
    expect(t.canRedo.value).toBe(true);
    t.splitAt(7000);
    expect(t.canRedo.value).toBe(false);
  });

  it("deletes a segment and refuses to delete the last one", () => {
    calls();
    const t = useEditorTimeline("cap", WHOLE);
    t.splitAt(5000);
    t.deleteSegment(0);
    expect(t.timeline.value.segments).toHaveLength(1);
    expect(t.timeline.value.segments[0].sourceStartMs).toBe(5000);
    // Spec 8.1: an empty timeline is a capture Save refuses. Do not let the
    // editor reach that state by clicking Delete one more time.
    t.deleteSegment(0);
    expect(t.timeline.value.segments).toHaveLength(1);
  });

  it("reorders segments", () => {
    calls();
    const t = useEditorTimeline("cap", WHOLE);
    t.splitAt(5000);
    t.reorder(0, 1);
    expect(t.timeline.value.segments[0].sourceStartMs).toBe(5000);
    expect(t.timeline.value.segments[1].sourceStartMs).toBe(0);
  });

  // Spec 10: saved on each edit, so a crash loses at most the last one.
  it("persists every landed edit and sends the base it was opened with", async () => {
    const seen = calls();
    const t = useEditorTimeline("cap one", WHOLE);
    t.splitAt(4000);
    await t.flushPending();
    const save = seen.find((c) => c.cmd === "save_capture_timeline");
    expect(save).toMatchObject({ base: "cap one" });
    expect((save as { timeline: { segments: unknown[] } }).timeline.segments).toHaveLength(2);
  });

  // A no-op must not write. An fsync'd sidecar rewrite per rejected click is
  // wasted disk, and it makes "saved on each edit" a claim about clicks
  // rather than about edits.
  it("does not persist an operation that changed nothing", async () => {
    const seen = calls();
    const t = useEditorTimeline("cap", WHOLE);
    t.splitAt(0);
    await t.flushPending();
    expect(seen.find((c) => c.cmd === "save_capture_timeline")).toBeUndefined();
  });

  // Revert clears the sidecar's timeline rather than storing the whole
  // capture as a one-segment edit: absent means untouched, which is what
  // phase 5's fast-path remux keys on.
  it("reverts by clearing the stored timeline", async () => {
    const seen = calls();
    const t = useEditorTimeline("cap", WHOLE);
    t.splitAt(4000);
    t.revert();
    await t.flushPending();
    expect(t.timeline.value.segments).toHaveLength(1);
    const last = [...seen].reverse().find((c) => c.cmd === "save_capture_timeline");
    expect((last as { timeline: unknown }).timeline).toBeNull();
  });
});
```

```bash
cd /home/user/vault-buddy && npx vitest run tests/useEditorTimeline.test.ts 2>&1 | tail -10
```
Expected: FAIL — cannot resolve the module.

- [ ] **Step 2: Implement**

```ts
import { invoke } from "@tauri-apps/api/core";
import { computed, ref } from "vue";

import type { TimelineDto } from "../types";
import { logWarning } from "../logging";
import { outputDurationMs, segmentAtOutputMs } from "../utils/timelineGeometry";

/** Undo/redo as a stack of whole-timeline snapshots.
 *
 * Spec 8.1 makes every operation return a NEW timeline, which is what lets
 * this be a snapshot stack rather than a log of inverse operations. A
 * timeline is a handful of integer pairs, so a snapshot costs nothing and
 * the correctness argument is "we kept a copy" rather than "our undo of
 * reorder is right".
 *
 * Persistence is spec 10's: saved on each edit, so a crash loses at most the
 * last operation. A no-op writes nothing — an fsync'd sidecar rewrite per
 * rejected click is wasted disk, and it would make "saved on each edit" a
 * claim about clicks rather than edits. */
export function useEditorTimeline(base: string, initial: TimelineDto) {
  const timeline = ref<TimelineDto>(structuredClone(initial));
  const past = ref<TimelineDto[]>([]);
  const future = ref<TimelineDto[]>([]);
  const dirty = ref(false);
  let pending: Promise<void> = Promise.resolve();

  function persist(value: TimelineDto | null) {
    pending = pending.then(() =>
      invoke("save_capture_timeline", { base, timeline: value }).then(
        () => undefined,
        (e) => {
          // Never throw out of an edit: the edit already landed on screen and
          // undo still works. A failed save means this one operation is not
          // crash-safe, which is worth a log, not a lost interaction.
          logWarning(`save_capture_timeline failed: ${String(e)}`);
        },
      ),
    );
  }

  /** Apply an operation, recording undo only if it CHANGED something. */
  function apply(next: TimelineDto) {
    if (JSON.stringify(next) === JSON.stringify(timeline.value)) return;
    past.value.push(structuredClone(timeline.value));
    future.value = [];
    timeline.value = next;
    dirty.value = true;
    persist(next);
  }

  function splitAt(outputMs: number) {
    const index = segmentAtOutputMs(timeline.value, outputMs);
    if (index === null) return;
    const seg = timeline.value.segments[index];
    let acc = 0;
    for (let i = 0; i < index; i += 1) {
      acc += timeline.value.segments[i].sourceEndMs - timeline.value.segments[i].sourceStartMs;
    }
    const cut = seg.sourceStartMs + (outputMs - acc);
    // Spec 8.1: a split on a boundary is a no-op, not a zero-length segment.
    if (cut <= seg.sourceStartMs || cut >= seg.sourceEndMs) return;
    const segments = [
      ...timeline.value.segments.slice(0, index),
      { sourceStartMs: seg.sourceStartMs, sourceEndMs: cut },
      { sourceStartMs: cut, sourceEndMs: seg.sourceEndMs },
      ...timeline.value.segments.slice(index + 1),
    ];
    apply({ segments });
  }

  function deleteSegment(index: number) {
    // Spec 8.1: deleting the last segment yields an empty timeline that Save
    // refuses. Refuse it here instead, so the editor cannot reach a state
    // whose only exit is Discard.
    if (timeline.value.segments.length <= 1) return;
    if (index < 0 || index >= timeline.value.segments.length) return;
    apply({ segments: timeline.value.segments.filter((_, i) => i !== index) });
  }

  function reorder(from: number, to: number) {
    const n = timeline.value.segments.length;
    if (from < 0 || from >= n || to < 0 || to > n || from === to) return;
    const segments = [...timeline.value.segments];
    const [moved] = segments.splice(from, 1);
    segments.splice(to > from ? to - 1 : to, 0, moved);
    apply({ segments });
  }

  function undo() {
    const prev = past.value.pop();
    if (!prev) return;
    future.value.push(structuredClone(timeline.value));
    timeline.value = prev;
    persist(prev);
  }

  function redo() {
    const next = future.value.pop();
    if (!next) return;
    past.value.push(structuredClone(timeline.value));
    timeline.value = next;
    persist(next);
  }

  /** Back to the whole capture, and CLEAR the stored timeline rather than
   * storing a one-segment edit: absent means untouched, which is what phase
   * 5's fast-path remux keys on (`is_untouched`). */
  function revert() {
    past.value.push(structuredClone(timeline.value));
    future.value = [];
    timeline.value = structuredClone(initial);
    dirty.value = false;
    persist(null);
  }

  return {
    timeline,
    canUndo: computed(() => past.value.length > 0),
    canRedo: computed(() => future.value.length > 0),
    isDirty: computed(() => dirty.value),
    outputMs: computed(() => outputDurationMs(timeline.value)),
    splitAt,
    deleteSegment,
    reorder,
    undo,
    redo,
    revert,
    /** Await every queued save. Tests use it; the window-close path will. */
    flushPending: () => pending,
  };
}
```

Re-run: all eight pass.

- [ ] **Step 3: Gates and mutation verification**

Run the frontend gate list.

| Mutation | Must fail |
| --- | --- |
| `apply` pushes undo before the no-change check | `refuses to split on a boundary and does not record an undo step` |
| `apply` does not clear `future` | `drops the redo branch once a new edit lands` |
| `splitAt` drops the `cut <= start \|\| cut >= end` guard | `refuses to split on a boundary…` |
| `deleteSegment` drops the `length <= 1` guard | `deletes a segment and refuses to delete the last one` |
| `revert` persists `timeline.value` instead of `null` | `reverts by clearing the stored timeline` |
| `persist` is never called from `apply` | `persists every landed edit…` |
| `reorder` uses `to` instead of `to > from ? to - 1 : to` | `reorders segments` |

Restore byte-identically; `md5sum`.

- [ ] **Step 4: Commit**

```bash
cd /home/user/vault-buddy
cat > /tmp/msg-p4t5.txt <<'MSG'
feat(ui): add the editor's undo stack and persist-on-edit

Spec 8.1 makes every timeline operation return a new timeline, which is
what lets undo be a stack of whole snapshots instead of a log of inverse
operations. A timeline is a handful of integer pairs, so a snapshot costs
nothing and the correctness argument becomes "we kept a copy" rather than
"our undo of reorder is right".

A no-op records nothing and writes nothing. A split on a boundary is
spec 8.1's no-op rather than a zero-length segment, because a zero-length
segment produces an unplayable file downstream -- and if a rejected click
still pushed an undo entry, Ctrl+Z would appear to do nothing while
actually consuming a step. The same reasoning keeps an fsync'd sidecar
rewrite off every rejected click, so "saved on each edit" stays a claim
about edits rather than about clicks.

Deleting the last segment is refused here rather than at save time. Spec
8.1 says an empty timeline is a capture Save rejects; letting the editor
reach that state would leave Discard as its only exit.

Revert clears the stored timeline instead of storing the whole capture as
a one-segment edit. Absent means untouched, and that is exactly what phase
5's fast-path remux keys on.

A failed save is logged, never thrown: the edit is already on screen and
undo still works, so the cost is that one operation is not crash-safe, not
a lost interaction.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01Ud2Yq3jSXMKVdZaMSTeRco
MSG
git add src/composables/useEditorTimeline.ts tests/useEditorTimeline.test.ts
git commit -F /tmp/msg-p4t5.txt
```

---

### Task 6: The editor surface — preview, strip, and the window that hosts them

Spec §8.2. Three files because `EditorRoot` owns state and IPC while the two
children are presentational — the `ScreenSourcePicker` / `ScreenRegionPicker`
split Phase 3 arrived at, for the same reason: an inline template pushed
`complexFunctions` over its floor there, and this surface is larger.

**Files:**
- Create: `src/components/editor/CapturePreview.vue`
- Create: `src/components/editor/TimelineStrip.vue`
- Modify: `src/roots/EditorRoot.vue` (replacing Task 1's placeholder)
- Create: `tests/editorRoot.test.ts`
- Create: `tests/timelineStrip.test.ts`

**Interfaces:**
- Consumes: `take_editor_request`, `load_staged_capture` (Task 2); `useEditorTimeline` (Task 5); `segmentWidths`, `dropIndex`, `toSourceMs` (Task 4); `convertFileSrc` from `@tauri-apps/api/core`.
- Produces: nothing later tasks depend on.

- [ ] **Step 1: Write the failing tests**

`tests/timelineStrip.test.ts` — the presentational half:

```ts
import { mount } from "@vue/test-utils";
import { describe, expect, it } from "vitest";

import TimelineStrip from "../src/components/editor/TimelineStrip.vue";

const T = {
  segments: [
    { sourceStartMs: 0, sourceEndMs: 1000 },
    { sourceStartMs: 5000, sourceEndMs: 8000 },
  ],
};

describe("TimelineStrip", () => {
  it("renders one block per segment, sized by its share of the output", () => {
    const w = mount(TimelineStrip, { props: { timeline: T, selected: null, playheadMs: 0 } });
    const blocks = w.findAll('[data-testid^="segment-"]');
    expect(blocks).toHaveLength(2);
    expect(blocks[0].attributes("style")).toContain("25%");
    expect(blocks[1].attributes("style")).toContain("75%");
  });

  it("marks the selected segment for assistive tech, not just visually", () => {
    const w = mount(TimelineStrip, { props: { timeline: T, selected: 1, playheadMs: 0 } });
    expect(w.get('[data-testid="segment-1"]').attributes("aria-pressed")).toBe("true");
    expect(w.get('[data-testid="segment-0"]').attributes("aria-pressed")).toBe("false");
  });

  it("emits the segment a click selected", async () => {
    const w = mount(TimelineStrip, { props: { timeline: T, selected: null, playheadMs: 0 } });
    await w.get('[data-testid="segment-1"]').trigger("click");
    expect(w.emitted("select")?.[0]).toEqual([1]);
  });

  // The playhead is positioned as a fraction of OUTPUT time. Positioning it
  // by source time would put it in the wrong place the moment anything is
  // cut, which is every moment the editor is useful.
  it("places the playhead by output fraction", () => {
    const w = mount(TimelineStrip, { props: { timeline: T, selected: null, playheadMs: 2000 } });
    expect(w.get('[data-testid="playhead"]').attributes("style")).toContain("50%");
  });

  // Drag-to-reorder (spec 8.2). Pointer events, not HTML5 drag-and-drop:
  // Tauri intercepts the latter, which is why the task list's own reorder
  // is pointer-based too (AGENTS.md, the tasks domain).
  it("emits a reorder when a block is dragged past another's midpoint", async () => {
    const w = mount(TimelineStrip, { props: { timeline: T, selected: null, playheadMs: 0 } });
    const strip = w.get('[data-testid="timeline-strip"]');
    // happy-dom gives every element a zero-size rect, so the component must
    // read the strip's width from getBoundingClientRect and the test must
    // supply one — otherwise the fraction is NaN and any assertion passes
    // for the wrong reason.
    strip.element.getBoundingClientRect = () =>
      ({ left: 0, width: 200, top: 0, height: 64, right: 200, bottom: 64, x: 0, y: 0 }) as DOMRect;
    await w.get('[data-testid="segment-0"]').trigger("pointerdown", { clientX: 10 });
    await strip.trigger("pointermove", { clientX: 180 });
    await strip.trigger("pointerup", { clientX: 180 });
    // 180/200 = 0.9, past block 1's midpoint (62.5%) -> index 2 -> "last".
    expect(w.emitted("reorder")?.[0]).toEqual([0, 2]);
  });

  // A press with no movement is a SELECT, not a zero-distance reorder.
  it("does not emit a reorder when the pointer never moved", async () => {
    const w = mount(TimelineStrip, { props: { timeline: T, selected: null, playheadMs: 0 } });
    const strip = w.get('[data-testid="timeline-strip"]');
    strip.element.getBoundingClientRect = () =>
      ({ left: 0, width: 200, top: 0, height: 64, right: 200, bottom: 64, x: 0, y: 0 }) as DOMRect;
    await w.get('[data-testid="segment-0"]').trigger("pointerdown", { clientX: 10 });
    await strip.trigger("pointerup", { clientX: 10 });
    expect(w.emitted("reorder")).toBeUndefined();
  });

  it("renders nothing but an empty state when every segment is gone", () => {
    const w = mount(TimelineStrip, {
      props: { timeline: { segments: [] }, selected: null, playheadMs: 0 },
    });
    expect(w.findAll('[data-testid^="segment-"]')).toHaveLength(0);
    expect(w.text()).toContain("Nothing left to save");
  });
});
```

`tests/editorRoot.test.ts` — the container:

```ts
import { flushPromises, mount } from "@vue/test-utils";
import { describe, expect, it } from "vitest";
import { mockIPC } from "@tauri-apps/api/mocks";

import EditorRoot from "../src/roots/EditorRoot.vue";

const DETAIL = {
  base: "cap one",
  assetPath: "cap one.mp4",
  durationMs: 10_000,
  sourceTitle: "Screen 1",
  width: 1920,
  height: 1080,
  recordedAt: "2026-09-20T10:00:00Z",
  timeline: null,
};

function mockEditor(detail: unknown = DETAIL, request: unknown = "cap one") {
  const seen: Record<string, unknown>[] = [];
  mockIPC((cmd, args) => {
    seen.push({ cmd, ...(args as object) });
    if (cmd === "take_editor_request") return request;
    if (cmd === "load_staged_capture") return detail;
    return undefined;
  });
  return seen;
}

describe("EditorRoot", () => {
  it("drains the request and loads that capture", async () => {
    const seen = mockEditor();
    const w = mount(EditorRoot);
    await flushPromises();
    expect(seen.find((c) => c.cmd === "load_staged_capture")).toMatchObject({ base: "cap one" });
    expect(w.text()).toContain("Screen 1");
  });

  // An untouched capture opens as ONE segment spanning the whole recording —
  // the timeline the sidecar does not carry yet.
  it("seeds a whole-capture timeline when the sidecar has none", async () => {
    mockEditor();
    const w = mount(EditorRoot);
    await flushPromises();
    expect(w.findAll('[data-testid^="segment-"]')).toHaveLength(1);
  });

  it("resumes a saved timeline instead of reseeding it", async () => {
    mockEditor({
      ...DETAIL,
      timeline: {
        segments: [
          { sourceStartMs: 0, sourceEndMs: 2000 },
          { sourceStartMs: 6000, sourceEndMs: 9000 },
        ],
      },
    });
    const w = mount(EditorRoot);
    await flushPromises();
    expect(w.findAll('[data-testid^="segment-"]')).toHaveLength(2);
  });

  // Opening the window with nothing staged is not an error: a user can
  // alt-tab back to an editor that is already showing a capture.
  it("says so plainly when no capture was requested", async () => {
    mockEditor(DETAIL, null);
    const w = mount(EditorRoot);
    await flushPromises();
    expect(w.text()).toContain("No capture open");
  });

  it("surfaces a load failure inline rather than a blank window", async () => {
    mockIPC((cmd) => {
      if (cmd === "take_editor_request") return "cap one";
      if (cmd === "load_staged_capture") throw new Error("That capture's video file is missing.");
      return undefined;
    });
    const w = mount(EditorRoot);
    await flushPromises();
    expect(w.get('[data-testid="editor-error"]').text()).toContain("video file is missing");
  });

  it("splits at the playhead and undoes", async () => {
    mockEditor();
    const w = mount(EditorRoot);
    await flushPromises();
    await w.get('[data-testid="editor-split"]').trigger("click");
    // Playhead starts at 0, which is a boundary, so this must be refused —
    // and Undo must stay disabled rather than consuming a phantom step.
    expect(w.findAll('[data-testid^="segment-"]')).toHaveLength(1);
    expect(w.get('[data-testid="editor-undo"]').attributes("disabled")).toBeDefined();
  });
});
```

```bash
cd /home/user/vault-buddy && npx vitest run tests/timelineStrip.test.ts tests/editorRoot.test.ts 2>&1 | tail -12
```
Expected: FAIL — neither component exists.

- [ ] **Step 2: Write `TimelineStrip.vue`**

```vue
<script setup lang="ts">
/**
 * The segment strip (spec 8.2). Presentational: it renders what it is given
 * and emits what was clicked. Every width and the playhead are fractions of
 * OUTPUT time, never source time — positioning by source would misplace both
 * the moment anything is cut, which is every moment this surface is useful.
 */
import { computed, ref } from "vue";

import type { TimelineDto } from "../../types";
import { dropIndex, outputDurationMs, segmentWidths } from "../../utils/timelineGeometry";

const props = defineProps<{
  timeline: TimelineDto;
  selected: number | null;
  playheadMs: number;
}>();
const emit = defineEmits<{ select: [index: number]; reorder: [from: number, to: number] }>();

/** Drag-to-reorder (spec 8.2), pointer-based rather than HTML5 drag-and-drop
 * because Tauri intercepts the latter — the same reason the task list's own
 * reorder is pointer-based (AGENTS.md, the tasks domain). */
const dragFrom = ref<number | null>(null);
const strip = ref<HTMLElement | null>(null);

function onDown(index: number) {
  dragFrom.value = index;
}

function onUp(e: PointerEvent) {
  const from = dragFrom.value;
  dragFrom.value = null;
  if (from === null || strip.value === null) return;
  const rect = strip.value.getBoundingClientRect();
  if (rect.width === 0) return;
  const to = dropIndex(widths.value, (e.clientX - rect.left) / rect.width);
  // A press that did not move is a select, not a zero-distance reorder;
  // `reorder` itself also no-ops, but emitting would push an undo entry
  // for a click that changed nothing.
  if (to === from || to === from + 1) return;
  emit("reorder", from, to);
}

const widths = computed(() => segmentWidths(props.timeline));
const playheadPct = computed(() => {
  const total = outputDurationMs(props.timeline);
  if (total === 0) return 0;
  return Math.min(100, Math.max(0, (props.playheadMs / total) * 100));
});
</script>

<template>
  <div
    ref="strip"
    data-testid="timeline-strip"
    class="relative h-16 w-full rounded-control bg-white/5 p-1"
    @pointerup="onUp"
    @pointercancel="dragFrom = null"
  >
    <p
      v-if="widths.length === 0"
      class="flex h-full items-center justify-center text-xs text-fg-muted"
    >
      Nothing left to save — undo a delete or revert the edit.
    </p>
    <div v-else class="flex h-full gap-1">
      <button
        v-for="(w, i) in widths"
        :key="i"
        type="button"
        :data-testid="`segment-${i}`"
        :aria-pressed="selected === i"
        :aria-label="`Segment ${i + 1} of ${widths.length}`"
        :style="{ width: `${w}%` }"
        class="h-full cursor-pointer rounded border transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-focus"
        :class="selected === i ? 'border-violet-400 bg-accent/25' : 'border-white/10 bg-white/10 hover:bg-white/15'"
        @click="emit('select', i)"
        @pointerdown="onDown(i)"
      />
    </div>
    <div
      v-if="widths.length > 0"
      data-testid="playhead"
      class="pointer-events-none absolute top-0 h-full w-0.5 bg-accent"
      :style="{ left: `${playheadPct}%` }"
    />
  </div>
</template>
```

- [ ] **Step 3: Write `CapturePreview.vue`**

```vue
<script setup lang="ts">
/**
 * Preview playback (spec 8.2).
 *
 * **This is a preview, and the UI says so.** Seeking a `<video>` element has
 * visible latency, so segment boundaries are not gapless: at each boundary
 * the element is seeked to the next segment's source start and there is a
 * hitch. Export is authoritative. Saying that plainly is the same honesty
 * posture as the transcription stats footer reporting the EFFECTIVE state
 * rather than the intended one — we do not imply frame-exact scrubbing we
 * are not building.
 */
import { ref, watch } from "vue";

import AppButton from "../ui/AppButton.vue";
import type { TimelineDto } from "../../types";
import { outputDurationMs, toOutputMs, toSourceMs } from "../../utils/timelineGeometry";

const props = defineProps<{ src: string; timeline: TimelineDto; outputMs: number }>();
const emit = defineEmits<{ "update:outputMs": [ms: number] }>();

const video = ref<HTMLVideoElement | null>(null);
const playing = ref(false);

/** Drive the element from OUTPUT time: ask the timeline where that lands in
 * the source and seek there. A boundary crossing is a seek, not a gap. */
watch(
  () => props.outputMs,
  (ms) => {
    const src = toSourceMs(props.timeline, ms);
    const el = video.value;
    if (el === null || src === null) return;
    if (Math.abs(el.currentTime * 1000 - src) > 250) el.currentTime = src / 1000;
  },
);

function onTimeUpdate() {
  // The element reports SOURCE time; every other surface speaks OUTPUT time,
  // so the raw value never leaves this component. A source moment that was
  // cut out maps to null — which is exactly the boundary case: playback has
  // run past the end of a segment into footage the edit removed, so seek to
  // wherever the NEXT segment starts instead of reporting a position the
  // strip cannot draw.
  const el = video.value;
  if (el === null) return;
  const sourceMs = el.currentTime * 1000;
  const out = toOutputMs(props.timeline, sourceMs);
  if (out !== null) {
    emit("update:outputMs", out);
    return;
  }
  const next = toSourceMs(props.timeline, props.outputMs);
  if (next === null) {
    el.pause();
    playing.value = false;
    return;
  }
  el.currentTime = next / 1000;
}

/** Scrub (spec 8.2). The range is OUTPUT milliseconds, so the control means
 * the same thing as the strip above it; the watcher does the seek. */
function onScrub(e: Event) {
  emit("update:outputMs", Number((e.target as HTMLInputElement).value));
}

function toggle() {
  const el = video.value;
  if (el === null) return;
  if (el.paused) {
    void el.play().catch(() => undefined);
    playing.value = true;
  } else {
    el.pause();
    playing.value = false;
  }
}
</script>

<template>
  <div class="flex flex-col gap-2">
    <video
      ref="video"
      data-testid="preview-video"
      :src="src"
      class="w-full rounded-control bg-black"
      preload="metadata"
      @timeupdate="onTimeUpdate"
    />
    <div class="flex items-center gap-2">
      <AppButton data-testid="preview-toggle" variant="secondary" @click="toggle">
        {{ playing ? "Pause" : "Play" }}
      </AppButton>
      <input
        type="range"
        data-testid="preview-scrub"
        class="grow accent-violet-500"
        min="0"
        :max="outputDurationMs(timeline)"
        step="100"
        :value="outputMs"
        aria-label="Scrub the preview"
        @input="onScrub"
      />
    </div>
    <p class="text-micro text-fg-subtle">
      Preview only — boundaries hitch while seeking. The exported file is exact.
    </p>
  </div>
</template>
```

- [ ] **Step 4: Write `EditorRoot.vue`**

Replace Task 1's placeholder. It owns: the drain, the load, the timeline
composable, the selected segment, the playhead, and the error banner.

```vue
<script setup lang="ts">
/**
 * The editor window's root (spec 8).
 *
 * The one root that mirrors NO Rust state and installs no store: it is handed
 * exactly one staged capture and edits it locally, persisting each operation
 * through `save_capture_timeline`. There is no event stream to subscribe to,
 * which is why `init()`-per-window (the rule the buddy and panel roots follow
 * for the capture stores) does not apply here.
 */
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { computed, onMounted, ref } from "vue";

import CapturePreview from "../components/editor/CapturePreview.vue";
import TimelineStrip from "../components/editor/TimelineStrip.vue";
import AppButton from "../components/ui/AppButton.vue";
import Banner from "../components/ui/Banner.vue";
import { logWarning } from "../logging";
import type { StagedCaptureDetail, TimelineDto } from "../types";
import { useEditorTimeline } from "../composables/useEditorTimeline";

const detail = ref<StagedCaptureDetail | null>(null);
const error = ref<string | null>(null);
const noCapture = ref(false);
const selected = ref<number | null>(null);
const playheadMs = ref(0);

let editor: ReturnType<typeof useEditorTimeline> | null = null;
const timeline = ref<TimelineDto>({ segments: [] });

const src = computed(() =>
  detail.value === null ? "" : convertFileSrc(detail.value.assetPath, "asset"),
);

onMounted(async () => {
  let base: string | null = null;
  try {
    base = await invoke<string | null>("take_editor_request");
  } catch (e) {
    logWarning(`take_editor_request failed: ${String(e)}`);
  }
  if (base === null) {
    noCapture.value = true;
    return;
  }
  try {
    const loaded = await invoke<StagedCaptureDetail>("load_staged_capture", { base });
    detail.value = loaded;
    // An untouched capture opens as ONE segment spanning the whole
    // recording: the timeline the sidecar does not carry yet.
    const seed: TimelineDto = loaded.timeline ?? {
      segments: [{ sourceStartMs: 0, sourceEndMs: loaded.durationMs }],
    };
    editor = useEditorTimeline(loaded.base, seed);
    timeline.value = editor.timeline.value;
  } catch (e) {
    error.value = String(e);
  }
});

function sync() {
  if (editor !== null) timeline.value = { ...editor.timeline.value };
}
function onSplit() {
  editor?.splitAt(playheadMs.value);
  sync();
}
function onDelete() {
  if (selected.value === null) return;
  editor?.deleteSegment(selected.value);
  selected.value = null;
  sync();
}
function onReorder(from: number, to: number) {
  editor?.reorder(from, to);
  selected.value = null;
  sync();
}
function onUndo() {
  editor?.undo();
  sync();
}
function onRedo() {
  editor?.redo();
  sync();
}
</script>

<template>
  <main class="flex h-screen w-screen flex-col gap-3 bg-slate-900 p-4 text-fg">
    <Banner v-if="error" data-testid="editor-error" tone="danger">{{ error }}</Banner>
    <p v-else-if="noCapture" class="m-auto text-sm text-fg-muted">
      No capture open. Pick one from Record Screen.
    </p>
    <template v-else-if="detail">
      <header class="flex items-baseline justify-between">
        <h1 class="truncate text-sm font-medium">{{ detail.sourceTitle }}</h1>
        <p class="text-micro text-fg-subtle">{{ detail.width }}x{{ detail.height }}</p>
      </header>
      <CapturePreview
        :src="src"
        :timeline="timeline"
        :output-ms="playheadMs"
        @update:output-ms="playheadMs = $event"
      />
      <TimelineStrip
        :timeline="timeline"
        :selected="selected"
        :playhead-ms="playheadMs"
        @select="selected = $event"
        @reorder="onReorder"
      />
      <div class="flex gap-2">
        <AppButton data-testid="editor-split" variant="secondary" @click="onSplit">Split</AppButton>
        <AppButton
          data-testid="editor-delete"
          variant="secondary"
          :disabled="selected === null"
          @click="onDelete"
          >Delete</AppButton
        >
        <AppButton
          data-testid="editor-undo"
          variant="ghost"
          :disabled="!editor?.canUndo.value"
          @click="onUndo"
          >Undo</AppButton
        >
        <AppButton
          data-testid="editor-redo"
          variant="ghost"
          :disabled="!editor?.canRedo.value"
          @click="onRedo"
          >Redo</AppButton
        >
      </div>
      <p class="text-micro text-fg-subtle">
        Saving into a vault arrives in a later update.
      </p>
    </template>
    <p v-else class="m-auto text-sm text-fg-muted">Loading capture…</p>
  </main>
</template>
```

**The trailing line is not filler.** Phase 2 shipped a stop notification
saying "Screen capture saved" when nothing was saved anywhere the user could
reach, and it sent people hunting through Obsidian. The editor must not imply
a save it cannot perform.

- [ ] **Step 5: Gates and mutation verification**

Run the frontend gate list. **If `check:quality` reports `complexFunctions`
14, extract rather than loosen** — the natural seam is the load-and-seed
sequence in `onMounted` moving to `src/composables/useStagedCapture.ts`.

| Mutation | Must fail |
| --- | --- |
| `TimelineStrip` sizes blocks by `segments.length` instead of `segmentWidths` | `renders one block per segment, sized by its share…` |
| Playhead positioned by `playheadMs / durationMs` (source, not output) | `places the playhead by output fraction` |
| `aria-pressed` hardcoded `false` | `marks the selected segment for assistive tech…` |
| `onUp` drops the `to === from \|\| to === from + 1` guard | `does not emit a reorder when the pointer never moved` |
| `onUp` divides by a constant instead of `rect.width` | `emits a reorder when a block is dragged past another's midpoint` |
| `EditorRoot` seeds `{segments: []}` when `timeline` is null | `seeds a whole-capture timeline when the sidecar has none` |
| `EditorRoot` always reseeds, ignoring `loaded.timeline` | `resumes a saved timeline instead of reseeding it` |
| `EditorRoot` treats a `null` request as an error | `says so plainly when no capture was requested` |

Restore byte-identically; `md5sum`.

- [ ] **Step 6: Commit**

```bash
cd /home/user/vault-buddy
cat > /tmp/msg-p4t6.txt <<'MSG'
feat(ui): add the capture editor surface

Spec 8.2's preview and segment strip. Three files, not one: the root owns
the load, the timeline and the error, and the two children are
presentational -- the same split the source picker arrived at in phase 3,
where an inline template pushed complexFunctions over its floor.

Every width and the playhead are fractions of OUTPUT time. Positioning by
source time would misplace both the moment anything is cut, which is every
moment this surface is useful.

The preview says it is a preview. Seeking a video element has visible
latency, so boundaries hitch and export is authoritative; the UI states
that rather than implying frame-exact scrubbing we are not building. It
also says plainly that saving into a vault arrives later -- phase 2 shipped
a toast claiming a save that had not happened and sent people hunting
through Obsidian for a file nobody had written.

An untouched capture opens as one segment spanning the whole recording: the
timeline the sidecar does not carry yet. A saved one resumes instead of
reseeding, or every crash would discard the edit the sidecar exists to
preserve.

Opening the window with nothing staged is not an error. A user can alt-tab
back to an editor that is already showing a capture.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01Ud2Yq3jSXMKVdZaMSTeRco
MSG
git add src/components/editor/CapturePreview.vue src/components/editor/TimelineStrip.vue src/roots/EditorRoot.vue tests/editorRoot.test.ts tests/timelineStrip.test.ts
git commit -F /tmp/msg-p4t6.txt
```

---

### Task 7: The way in, the keyboard, and the docs

The editor exists but nothing opens it. This task wires the entry point, adds
spec §8.2's undo/redo shortcuts, and reconciles the documentation — AGENTS.md
is the file the next agent reads before touching any of this.

**Files:**
- Modify: `src/components/ScreenCaptureBar.vue` (an Edit action on the finished capture)
- Modify: `src/roots/EditorRoot.vue` (the keyboard handler)
- Modify: `tests/editorRoot.test.ts`, `tests/screenCaptureBar.test.ts`
- Modify: `AGENTS.md`, `docs/Gaps.md`, `docs/superpowers/specs/2026-09-18-screen-capture-windows-verification.md`
- Modify: `.superpowers/sdd/progress.md` (gitignored; not committed)

**Interfaces:**
- Consumes: `open_capture_editor` (Task 2); the `screenCapture` store's `lastStaged` (`StagedCapture`, already carries `base`).
- Produces: nothing.

- [ ] **Step 1: Establish the real numbers first**

Do not copy a count out of this plan. Measure:

```bash
cd /home/user/vault-buddy
awk '/generate_handler!\[/,/\]\)/' src-tauri/src/lib.rs | grep -E '^\s+[a-z_]+::[a-z_]+,$' | wc -l
grep -n "^### GAP-" docs/Gaps.md | tail -2
grep -c "^### GAP-104" docs/Gaps.md
git diff --stat origin/main...HEAD -- scripts/loc-baseline.json scripts/quality-baseline.json
```

AGENTS.md says 81 at the start of this phase and Phase 4 adds four
(`open_capture_editor`, `take_editor_request`, `load_staged_capture`,
`save_capture_timeline`), so the table should read 85 — **confirm it against
the handler list rather than trusting the arithmetic.** That sentence has been
wrong twice on this branch. GAP-104 is retired and must never be reused.

- [ ] **Step 2: Write the failing tests**

In `tests/editorRoot.test.ts`:

```ts
  // Spec 8.2's shortcuts. Both spellings of redo, because Ctrl+Y is what
  // half of Windows expects and Ctrl+Shift+Z is what the spec names.
  it("undoes and redoes from the keyboard", async () => {
    mockEditor();
    const w = mount(EditorRoot, { attachTo: document.body });
    await flushPromises();
    // Split somewhere real so there is something to undo.
    await w.get('[data-testid="segment-0"]').trigger("click");
    (w.vm as unknown as { playheadMs: number }).playheadMs = 4000;
    await w.get('[data-testid="editor-split"]').trigger("click");
    expect(w.findAll('[data-testid^="segment-"]')).toHaveLength(2);

    window.dispatchEvent(new KeyboardEvent("keydown", { key: "z", ctrlKey: true }));
    await flushPromises();
    expect(w.findAll('[data-testid^="segment-"]')).toHaveLength(1);

    window.dispatchEvent(new KeyboardEvent("keydown", { key: "z", ctrlKey: true, shiftKey: true }));
    await flushPromises();
    expect(w.findAll('[data-testid^="segment-"]')).toHaveLength(2);
    w.unmount();
  });

  // The listener is on `window`, so an unmounted editor that kept listening
  // would keep editing a timeline nobody can see.
  it("stops listening for shortcuts once unmounted", async () => {
    mockEditor();
    const w = mount(EditorRoot, { attachTo: document.body });
    await flushPromises();
    const before = w.findAll('[data-testid^="segment-"]').length;
    w.unmount();
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "z", ctrlKey: true }));
    await flushPromises();
    expect(before).toBe(1);
  });
```

In `tests/screenCaptureBar.test.ts`, matching that file's own mounting idiom:

```ts
  it("opens the editor on the capture that just finished", async () => {
    const calls: Record<string, unknown>[] = [];
    mockIPC((cmd, args) => {
      calls.push({ cmd, ...(args as object) });
      return undefined;
    });
    const store = useScreenCaptureStore();
    store.status = "idle";
    store.lastStaged = {
      base: "cap one",
      path: "C:/staging/cap one.mp4",
      durationMs: 30_000,
      sourceTitle: "Screen 1",
      width: 1920,
      height: 1080,
    };
    const w = mount(ScreenCaptureBar);
    await flushPromises();
    await w.get('[data-testid="screen-edit"]').trigger("click");
    expect(calls.find((c) => c.cmd === "open_capture_editor")).toMatchObject({
      base: "cap one",
    });
  });

  // The Edit action is about a FINISHED capture. Offering it mid-recording
  // would invite a click that opens an editor on a file still being written.
  it("offers no Edit action while a capture is running", async () => {
    mockIPC(() => undefined);
    const store = useScreenCaptureStore();
    store.status = "capturing";
    store.lastStaged = null;
    const w = mount(ScreenCaptureBar);
    await flushPromises();
    expect(w.find('[data-testid="screen-edit"]').exists()).toBe(false);
  });
```

- [ ] **Step 3: Add the keyboard handler**

In `EditorRoot.vue`'s `<script setup>`:

```ts
/** Spec 8.2's shortcuts. Ctrl+Y as well as Ctrl+Shift+Z: the spec names the
 * latter, half of Windows expects the former, and supporting both costs one
 * clause. Bound on `window` because the editor fills its own window and
 * there is no other focus target to scope to — and removed on unmount, or an
 * unmounted editor goes on editing a timeline nobody can see. */
function onKeydown(e: KeyboardEvent) {
  if (!e.ctrlKey && !e.metaKey) return;
  const key = e.key.toLowerCase();
  if (key === "z" && !e.shiftKey) {
    e.preventDefault();
    onUndo();
  } else if ((key === "z" && e.shiftKey) || key === "y") {
    e.preventDefault();
    onRedo();
  }
}
onMounted(() => window.addEventListener("keydown", onKeydown));
onBeforeUnmount(() => window.removeEventListener("keydown", onKeydown));
```

- [ ] **Step 4: Add the entry point**

`src/components/ScreenCaptureBar.vue` — when the store holds a `lastStaged`
and no capture is running, offer an **Edit** action beside the existing copy:

```vue
      <AppButton
        v-if="store.lastStaged && store.status === 'idle'"
        data-testid="screen-edit"
        variant="secondary"
        @click="openEditor"
      >
        Edit
      </AppButton>
```

```ts
async function openEditor() {
  const staged = store.lastStaged;
  if (staged === null) return;
  try {
    await invoke("open_capture_editor", { base: staged.base });
  } catch (e) {
    logWarning(`open_capture_editor failed: ${String(e)}`);
    notifications.push({ tone: "error", text: String(e) });
  }
}
```

**Phase 5 replaces this with the staged-capture browser** (spec §10's
resume-or-discard list). This is the one-capture entry point Phase 4 needs,
not that browser — do not build it here.

- [ ] **Step 5: Reconcile the docs**

`AGENTS.md`:
1. **"Architecture overview"** — the diagram says four windows; it is five.
   Add the `editor` box, labelled `EditorRoot / preview + strip`.
2. **"The window system"** — a bullet for `editor` in the established shape,
   stating: it is NOT positioned-while-hidden like the companions (it is a
   normal resizable window at its configured size); it is **exempt from
   `tray::hide_buddy`** and why (`COMPANION_LABELS` vs `ALL_WINDOW_LABELS`,
   unsaved edits, the GAP-82 class); and that it IS destroyed by
   `finish_quit`, because of `Chrome_WidgetWin_0`.
3. **"The IPC surface"** — the measured count from Step 1, plus a row:
   > | `editor_commands.rs` | `open_capture_editor` *(sync — window show/focus; it STASHES the base rather than loading it, because a sync command must not touch disk)*, `take_editor_request` *(sync — one-shot drain)*, `load_staged_capture` *(async)*, `save_capture_timeline` *(async — an fsync'd sidecar rewrite per edit)* |
4. **"Screen capture (phases 2–3)"** — retitle to **"phases 2–4"** and add
   the editor's own paragraph: the asset protocol's staging-only scope as a
   security boundary; `core::timeline` finally having a production caller;
   preview-not-authoritative; and that Phase 4 still writes nothing into a
   vault.
5. **"Frontend state"** — `rootFor()` gains `editor → EditorRoot`, and note
   that `EditorRoot` installs no store, unlike every other root.
6. **"Repository map"** — `src/components/editor/`, `src/composables/`,
   `src/utils/timelineGeometry.ts`, `src-tauri/src/editor_commands.rs`.

`docs/Gaps.md` — allocate upward from Step 1's measured highest id:

- **The asset-protocol scope is pinned by no test.** A widening to
  `$APPLOCALDATA/*`, or to a vault path, would let the editor webview read
  arbitrary files through `asset.localhost`, and nothing would fail. Fix
  shape: a test that parses `tauri.conf.json` and asserts the scope is
  exactly `["$APPLOCALDATA/screen-captures/*"]`.
- **The preview is not gapless and the export path does not exist yet**, so
  nothing has ever verified that `toSourceMs` and `core::timeline::to_source_ms`
  agree. They are two implementations of one mapping, in two languages, and
  Phase 5's export plans on the Rust one while the user watches the TS one.
  Fix shape: a shared fixture table, or a Phase 5 test that runs both.
- **`EditorRoot` keeps a `timeline` ref in step with the composable by hand**
  (`sync()` after every operation), because the composable returns a plain
  `ref` the template does not track through the nullable `editor` handle. A
  missed `sync()` is a silent stale render. Fix shape: make `editor` a
  `shallowRef` set once and read `editor.value.timeline` directly.

`docs/superpowers/specs/2026-09-18-screen-capture-windows-verification.md` —
add a **"Covered by Phase 4"** table, same shape, **empty Result column**,
and the same measurement discipline. Rows:

| # | Check | Steps |
| --- | --- | --- |
| 19 | **The editor opens on a real capture** | Record ~30 s, Stop, click **Edit**. **Record** whether the window opens with a title bar, is resizable, alt-tabs, and whether the video plays. A black frame means the asset scope is wrong. |
| 20 | **Hide to tray does not take the editor** | With the editor open, tray → Hide. **Record** whether the buddy/panel disappear and whether the editor stays. It must stay. |
| 21 | **Quit destroys the editor** | With the editor open, tray → Quit. **Record** whether the process exits and whether the log carries "Failed to unregister class Chrome_WidgetWin_0". It must not. |
| 22 | **The editor is not in its own recording** | Start a capture of the whole screen with the editor open in frame. **Record** whether it is visible to you (it must be) and whether it appears in the file (it must not). This is the first excluded window that is not `skipTaskbar`. |
| 23 | **Edits survive a crash** | Split twice, then kill the process from Task Manager. Relaunch, reopen the same capture. **Record** how many segments come back. Expected: both splits. |
| 24 | **Split, delete, reorder, undo, redo** | Exercise each, then Ctrl+Z back to the start and Ctrl+Shift+Z forward. **Record** whether the strip and the preview agree at every step, and whether Undo is ever disabled while an edit is still on screen. |

- [ ] **Step 6: Run the whole gate set**

```bash
cd /home/user/vault-buddy/src-tauri
cargo fmt --check; echo "fmt exit=$?"
cargo clippy --workspace --all-targets -- -D warnings; echo "clippy exit=$?"
cargo clippy -p vault_buddy_screen --all-targets --target x86_64-pc-windows-msvc -- -D warnings; echo "screen-win exit=$?"
cargo clippy -p vault_buddy_capture --lib --target x86_64-pc-windows-msvc -- -D warnings; echo "capture-win exit=$?"
cargo test -p vault_buddy_core -p vault_buddy_capture -p vault_buddy_transcribe -p vault_buddy_mcp -p vault_buddy_screen; echo "crates exit=$?"
cargo test -p vault-buddy --lib; echo "shell exit=$?"
cargo machete .; echo "machete exit=$?"
cargo deny check; echo "deny exit=$?"
cargo llvm-cov -p vault_buddy_core -p vault_buddy_capture -p vault_buddy_transcribe -p vault_buddy_screen --fail-under-lines 94; echo "cov exit=$?"
cd /home/user/vault-buddy
rm -rf coverage && npm run lint; echo "lint exit=$?"
npm run check:loc; echo "loc exit=$?"
npm run check:quality; echo "quality exit=$?"
npm run test:coverage; echo "coverage exit=$?"
npm run build; echo "build exit=$?"
```

Every line must print `exit=0`. **Paste the real output into the report**, not
a summary, and never through a `grep`. Note there is deliberately no shell
Windows-target clippy: it is impossible here (see Global Constraints).

- [ ] **Step 7: Commit and push**

```bash
cd /home/user/vault-buddy
cat > /tmp/msg-p4t7.txt <<'MSG'
feat(ui): open the editor from the capture bar, and reconcile the docs

The editor existed but nothing opened it. The capture bar's finished-capture
state gains an Edit action; phase 5 replaces it with the staged-capture
browser spec 10 describes, and this is deliberately not that browser.

Spec 8.2's undo and redo shortcuts, bound on window because the editor
fills its own window and there is no narrower focus target -- and removed
on unmount, or an unmounted editor goes on editing a timeline nobody can
see. Ctrl+Y as well as Ctrl+Shift+Z: the spec names one, Windows users
expect the other, and both cost a single clause.

AGENTS.md described four windows, a phases-2-to-3 screen-capture domain and
a command count this phase moves. It is the file the next agent reads
before touching any of them, so it is reconciled here rather than later --
including the rule that makes the editor different: it is destroyed on quit
but never hidden to tray, and those are two lists now, not one.

Three residuals recorded rather than left to be rediscovered: the asset
protocol's staging-only scope is a security boundary that no test pins; the
output-to-source mapping now exists twice, in Rust and TypeScript, and
nothing has ever checked that the two agree; and the root keeps a timeline
ref in step with its composable by hand, where a missed call renders stale.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01Ud2Yq3jSXMKVdZaMSTeRco
MSG
git add src/components/ScreenCaptureBar.vue src/roots/EditorRoot.vue tests/editorRoot.test.ts tests/screenCaptureBar.test.ts AGENTS.md docs/Gaps.md docs/superpowers/specs/2026-09-18-screen-capture-windows-verification.md
git commit -F /tmp/msg-p4t7.txt
git push -u origin claude/screen-capture-intake-g0j49q
```

**PR #79 is already open for this branch. Do not open a second one, and do
not open one against `main`.**

---

## Phase exit criteria

Verified by running the command, not by reading a report:

- [ ] All seven tasks committed on `claude/screen-capture-intake-g0j49q` and pushed; PR #79 shows them; no second PR exists.
- [ ] `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings` clean.
- [ ] `cargo clippy -p vault_buddy_screen --all-targets --target x86_64-pc-windows-msvc` and `-p vault_buddy_capture --lib` with that target, both clean. **The shell-crate equivalent is impossible here and is not a gate.**
- [ ] All five member crates' tests plus `cargo test -p vault-buddy --lib` pass.
- [ ] `cargo machete .`, `cargo deny check` clean; **no new dependency** — Phase 4 adds none.
- [ ] `cargo llvm-cov … --fail-under-lines 94` passes.
- [ ] `rm -rf coverage && npm run lint && npm run check:loc && npm run check:quality && npm run test:coverage && npm run build` passes in that order, with exactly one pre-existing lint warning in `src/main.ts`.
- [ ] Baselines unchanged, or a moved one tightened with a written justification.
- [ ] `core::timeline` — Phase 1's orphan — has a production caller.
- [ ] The editor is in `EXCLUDED_LABELS`, in `ALL_WINDOW_LABELS`, and **not** in `COMPANION_LABELS`, each pinned by a test.
- [ ] The asset-protocol scope is exactly `["$APPLOCALDATA/screen-captures/*"]`.
- [ ] Phase 4 writes nothing into any vault — verify structurally, not from comments.
- [ ] The verification checklist carries Phase 4's rows AND every open earlier row. **Nobody is asked to run them yet.**

## What Phase 5 needs from this phase

- **`save_capture_timeline` stores `null` for an untouched capture**, not a one-segment timeline. `is_untouched` is what the fast-path remux keys on, and a capture reverted to whole must take that path.
- **`toSourceMs` (TS) and `core::timeline::to_source_ms` (Rust) are two implementations of one mapping.** Export plans on the Rust one while the user watched the TS one. If they disagree, the exported file does not match the preview — check them against a shared fixture table before building export.
- **The editor window is exempt from `hide_buddy` but not from `finish_quit`.** Export must not assume the editor is gone when the app is hiding.
- **`EditorRoot` installs no store.** If Phase 5 adds `screen:exportProgress`, that is the editor's first Rust event stream, and the per-window `init()` rule (AGENTS.md, "Frontend state") starts applying to it.
- **The staged capture is deleted only after the vault write lands** (spec §8.3) — the never-lose invariant. Phase 4 deletes nothing; do not let export delete early.
