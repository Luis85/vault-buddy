# Knowledge Intake — Screen Capture — design

Date: 2026-09-18
Status: accepted (user request: "I want to add a new feature: Screen Capture …
a new Intake function per vault: Record Screen"). Sub-decisions the user made
during brainstorming, each recorded at the point it constrains the design below:

| Decision | Choice |
| --- | --- |
| Scope | **One design covering capture + edit + save**, implemented in separated phases. The agent flagged that this spans four subsystems and recommended decomposition; the user chose a single design. Phasing (§13) is how that concern is answered without splitting the spec. |
| Encoder | **Native Windows APIs, nothing bundled** — no FFmpeg, no GPL, no installer growth |
| Encoder wiring | **Media Foundation `SinkWriter` directly** for both record and export; `windows-capture` acquires frames only |
| Architecture | **New `screen` crate + new `editor` window** |
| Editor depth | **Split + delete + reorder segments**, one track |
| Export | **Re-encode for exact cuts** (no keyframe snapping) |
| Transcription | **Out of scope** for this design |
| Staging | **App staging dir, outside the vault** |
| Abandoned capture | **Keep it, offer to resume** |
| Audio devices | **Multi-select, mixed down to one stereo track** |
| Per-vault settings | Folder, quality preset, frame rate, companion note + template |
| Multi-monitor | **Each monitor listed separately**; no "all screens combined" |

## Context

Knowledge Intake has three Capture Providers today: **Audio Recording**
(meeting / voice note, v0.3.0), **Transcription** as its post-capture step
(v0.4.0), and **Document Import via Pandoc** (v0.6.x). The Knowledge Intake
PRD lists **Screen Recording** as a Version 4 provider. This spec builds it.

What exists to build on, against the real code:

- **A complete audio capture engine.** `vault_buddy_capture` owns cpal device
  enumeration (`devices::list_devices` / `open_sources`, including WASAPI
  loopback for desktop audio on Windows), resampling and mixing
  (`mixer.rs`), and a session worker with a `Control { Stop, Pause, Resume }`
  message on **one** channel so no second signalling path can race the stop.
- **Never-clobber vault write machinery.** `core::capture_paths`
  (`reserve_basename`, `rename_noreplace`, ` (N)` suffix retry) and
  `core::capture_note` (`write_atomic_replacing`, owned `.vault-buddy.tmp`
  temps) back all eight sanctioned vault writes.
- **The flat-vs-dated layout branch.** `capture_paths::capture_dir(root, date,
  dated)` is the one branch point; every read/recovery path is
  layout-agnostic rather than migrated.
- **An additive per-vault template mechanism.** `core::template`'s
  `substitute` + `render_extra_frontmatter` (sentinel round-trip, real YAML
  parse, reserved-key filter, merge-key drop) backs three template surfaces
  already (capture note, document import, task renderer).
- **A three-window system with hard invariants** — position-while-hidden,
  main-thread-only window calls, the `tray::hide_buddy` chokepoint, and the
  panel's focus-out auto-hide with its `PANEL_PIN_UNTIL` / `DIALOG_ACTIVE`
  exceptions.
- **Owned-temp recovery precedent.** Capture recovery (`capture/src/recovery.rs`)
  and import recovery (`document_commands::run_import_recovery`) both sweep
  only our own marker-suffixed files, staleness-gated, postponed while work
  is active, rescheduled for fresh orphans.

Nothing in the repo touches video, D3D11, or any media container other than
MP3.

## 1. Goals & scope

**In scope.**

1. A **Record Screen** intake action per vault, beside Meeting / Voice Note /
   Import Document.
2. A **source picker**: any single monitor, any single open window, or a
   user-drawn **region** rectangle.
3. **Multi-select audio devices** (microphones and desktop-audio loopback
   outputs), mixed down into one stereo track.
4. **Pause / resume / stop** during capture, with the buddy as the indicator.
5. A **lightweight editor** — split at playhead, delete a segment, reorder
   segments, undo/redo — over the just-stopped capture.
6. **Export and save** into the vault as MP4 plus an optional companion note
   embedding it, under the existing never-clobber discipline.
7. **Per-vault settings**: capture folder, flat/dated layout, quality preset,
   frame rate, companion note toggle, and the additive
   extra-frontmatter/body template pair.
8. **Staging and recovery** so an abandoned or crashed capture is never
   silently lost.

**Explicitly out of scope** (each a deliberate non-goal, not an oversight):

- **Transcription of screen captures.** The whisper pipeline decodes MP3 via
  Symphonia; feeding it MP4/AAC is its own change. Deferred by user decision.
- **Screen captures in the Recordings browser.** That surface is built on
  `transcript::capture_mp3s`; widening it to a second media type changes a
  shared walker three domains depend on.
- **Rename-after-save.** `capture::rename` retargets an MP3 plus its note and
  transcript embeds; extending it is a separate increment.
- **Transitions, titles, audio ducking, multi-track.** One video track, one
  audio track.
- **Webcam / picture-in-picture, annotations, cursor highlighting.**
- **"All screens combined" capture.** Mixed-DPI virtual-desktop capture via
  DXGI duplication is materially harder and produces awkward output.
- **macOS / Linux.** The shipped app is Windows-only; the `screen` crate
  stubs elsewhere purely to keep the Linux compile gate green.

**Success criteria.** A user clicks the vault's capture button, picks a window
and two audio devices, records with at least one pause, trims a dead section
out in the editor, saves, and finds a playable MP4 embedded in a note in their
vault — with no third-party binary installed and no file ever written into the
vault before they pressed Save.

## 2. Domain language (additions to CONTEXT.md)

- **Screen Capture** — a Capture whose source is the screen. A fourth Capture
  Provider. The recorded artifact is an MP4.
- **Capture Source** — what is being recorded: a **Screen** (one monitor), a
  **Window** (one top-level window), or a **Region** (a rectangle on one
  monitor).
- **Staged Capture** — a finished-but-unsaved recording living in the app's
  staging directory. Not yet knowledge; not in a vault.
- **Timeline** — the ordered list of **Segments** an editor session produces.
- **Segment** — a half-open span `[source_start_ms, source_end_ms)` of the
  staged capture. Segments never overlap within a timeline but may appear in
  any order.
- **Export** — turning a Timeline plus its Staged Capture into one MP4.

A Screen Capture is a Capture, so it inherits the Capture lifecycle language
already in CONTEXT.md. `CONTEXT.md` gains these terms via the
`domain-modeling` skill when the increment lands.

## 3. Dependency & licensing decision

The repo refused to bundle Pandoc: GPL-2, ~150–200 MB, against an MIT app with
a light installer. FFmpeg raises the same objection plus H.264 patent
licensing. Therefore: **native Windows APIs only.**

| Need | Mechanism |
| --- | --- |
| Frame acquisition | `windows-capture` 2.0.1 (MIT, crates.io, 1.38M downloads) wrapping Windows.Graphics.Capture |
| Encode + mux | Media Foundation `IMFSinkWriter` via the `windows` crate (already transitively present through tauri) |
| Decode for export | Media Foundation `IMFSourceReader` |
| Audio capture | the existing `vault_buddy_capture` cpal/WASAPI path |

`windows-capture` is MIT, which `deny.toml`'s `[licenses] allow` list already
permits, and it is a crates.io release, which satisfies
`[sources] unknown-git = "deny"`. **No new licence category and no new
`deny.toml` entry.** `windows-capture` is a `[target."cfg(windows)".dependencies]`
entry of the `screen` crate, so Linux builds never compile it.

`windows-capture` ships its own `VideoEncoder`. We deliberately do **not** use
it: export needs a `SourceReader` regardless, and feeding our own mixed PCM
through its `AudioEncoderSource` is unverified. Owning the `SinkWriter`
directly gives one encoder code path shared by record and export, full control
over the keyframe interval, and no dependency on an API we could not confirm.

## 4. Architecture

```
┌──────────────────────── Rust shell (src-tauri/src) ─────────────────────────┐
│ screen_commands.rs ──── source enumeration, capture lifecycle, region pick  │
│ screen_editor_commands.rs ── editor window, export, save, discard, recovery │
│ screen_config_commands.rs ── the per-vault settings surface                 │
└───┬──────────────────┬───────────────────┬────────────────────┬────────────┘
    │                  │                   │                    │
┌───┴────┐  ┌──────────┴──────────┐  ┌─────┴──────┐  ┌──────────┴──────────┐
│ main   │  │ panel               │  │ overlay    │  │ editor              │
│ buddy  │  │ ScreenSourcePicker  │  │ RegionRoot │  │ EditorRoot          │
│ = live │  │ ScreenCaptureBar    │  │ rubber     │  │ preview + timeline  │
│ badge  │  │ settings cards      │  │ band       │  │ + export progress   │
└────────┘  └─────────────────────┘  └────────────┘  └─────────────────────┘

        pure + engine crates below the shell:
  core::timeline          segment algebra (split/delete/reorder/undo)
  core::screen_capture_config   per-vault settings parse/serialize
  core::screen_note       companion-note renderer
  core::screen_geometry    DPI scaling + crop clamping
  screen (NEW)            acquisition, crop, pause clock, mux, export
  capture (existing)      audio devices + mixing, generalized to N sources
```

### 4.1 The `screen` crate

`src-tauri/screen/`, a fifth workspace member (`vault_buddy_screen`).

```
screen/src/
├── lib.rs        ScreenCapture handle, Control messages, Outcome
├── source.rs     monitor/window enumeration → CaptureSource
├── session.rs    the capture worker: frames + audio → SinkWriter
├── clock.rs      the shared monotonic pause-aware clock (PURE, tested)
├── select.rs     timeline → ordered (source_ts, output_ts) plan (PURE, tested)
├── sink.rs       #[cfg(windows)] MF SinkWriter wrapper
├── reader.rs     #[cfg(windows)] MF SourceReader wrapper
├── export.rs     decode → reorder → encode, progress + cancellation
└── staging.rs    staging dir layout, .part naming, recovery scan
```

**What compiles where.** `clock.rs`, `select.rs`, `staging.rs` (path logic)
are pure and compile and test **anywhere**. `sink.rs`, `reader.rs`,
`source.rs` and the Windows half of `session.rs` sit behind `#[cfg(windows)]`
with a non-Windows arm returning `Err(ScreenError::Unsupported)`. The
AGENTS.md "what compiles where" table gains:

| `src-tauri/screen/` | Screen capture engine: acquisition (`windows-capture`), Media Foundation encode/decode, export. Pure submodules (`clock`, `select`, `staging`) compile anywhere; the rest is `cfg(windows)` with an `Unsupported` stub. | Pure parts anywhere; engine Windows-only. CI gates `-p vault_buddy_screen` on Linux (stub + pure tests). |

Rationale for a new crate rather than growing `capture`: `capture` compiles
and tests on any platform and is exercised by the `rust-core` Linux job.
Adding D3D11/WGC/Media Foundation to it would make that table false and take
the audio engine's tests hostage to a Windows-only dependency tree.

### 4.2 Why the correctness lives in `core`

Everything that can be wrong *logically* — which frames belong to the output
and in what order, how a pause shifts timestamps, how a region rectangle maps
from logical to physical pixels, what the note says — is a pure function in
`core` or in `screen`'s pure submodules, unit-tested on Linux in CI. The
Windows-only code is reduced to: *get me frames*, *write me a file*, *read me
a file*. That is the only way this feature gets meaningful automated coverage,
because no CI runner can record a screen.

## 5. The window system

### 5.1 `editor` — a fourth window

```jsonc
{ "label": "editor", "title": "Vault Buddy — Edit Capture",
  "width": 960, "height": 640, "minWidth": 720, "minHeight": 480,
  "visible": false, "resizable": true, "decorations": true,
  "transparent": false, "alwaysOnTop": false, "skipTaskbar": false }
```

Deliberately unlike the companion windows: a real application window you
alt-tab to, with a title bar, resizable, not always-on-top. Created hidden at
startup like `panel`/`bubble`; `rootFor()` gains `editor → EditorRoot`; the
`default` capability's `windows` array gains `"editor"`.

**It is exempt from `tray::hide_buddy`.** Hiding to tray is a companion-surface
gesture; hiding a window holding unsaved edits would strand work off-screen
(the same class of bug as the `taskDetailBusy` panel-hide problem, GAP-82). The
editor is closed only by the user, by a successful save, or by an explicit
discard.

**Asset protocol.** Preview playback requires `app.security.assetProtocol`
enabled with `scope` restricted to the **staging directory only** — never any
vault path — and the CSP extended with `media-src 'self' asset:
http://asset.localhost`. Narrow scope is the point: the webview must not be
able to read arbitrary disk through the asset handler.

### 5.2 `overlay` — a fifth window, for region selection

```jsonc
{ "label": "overlay", "title": "Vault Buddy — Select Region",
  "visible": false, "transparent": true, "decorations": false,
  "alwaysOnTop": true, "resizable": false, "skipTaskbar": true,
  "focus": true }
```

Created hidden at startup. `select_capture_region` sizes and positions it over
the **target monitor while it is still hidden**, then shows it — the same
discipline `position_panel` and `show_bubble` follow, for the same
stale-frame reason. Drag paints a rubber band with a live `W × H` readout;
pointer-up resolves; Escape resolves as cancelled. Either way the overlay
hides and the panel returns.

While it is up the panel must not auto-hide: `select_capture_region` sets the
existing `DIALOG_ACTIVE` flag for its duration and clears it in a `finally`,
exactly as `withDialogSuppressed` does for native file pickers.

**Hard-won detail: coordinate spaces.** The overlay webview reports **logical**
CSS pixels; WGC frames are **physical** pixels. `core::screen_geometry`
converts:

```rust
pub fn to_physical(rect: LogicalRect, scale: f64) -> PhysicalRect
pub fn clamp_to_frame(rect: PhysicalRect, frame_w: u32, frame_h: u32)
    -> Option<PhysicalRect>   // None when the intersection is empty
```

Both are pure and unit-tested across scale factors (1.0, 1.25, 1.5, 2.0) and
out-of-bounds rectangles. `clamp_to_frame` runs again **at capture start**, not
only at selection time: a monitor whose resolution changed between selecting
and starting would otherwise index outside the frame buffer.

### 5.3 Excluding ourselves from the recording

`SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE)` (Windows 10 2004+) is
applied to `main`, `panel`, `bubble`, `editor` and `overlay` for the duration
of a screen capture. Vault Buddy's own UI is then **invisible to the recording
while remaining fully visible to the user** — which is what lets the buddy
keep being the recording indicator without appearing in every screen
recording. On an older build where the call fails it is logged and ignored;
the capture proceeds with the buddy visible in frame.

> **Amended 2026-09-21 (GAP-166).** Applied to `main` alone. The first
> hardware pass found that the affinity round-trip on the other four
> windows — all of which host WebView2 — stopped the editor painting and
> stopped other applications' toolbars (Explorer, Notepad) taking pointer
> input until Vault Buddy exited; excluding the buddy alone was clean on the
> same machine, with the buddy absent from both a snip and the recording. So
> the panel, bubble, overlay and editor now appear in a recording when they
> are on screen, by design, and the exclusion set is pinned the inverse way:
> a new window is not excluded until a hardware run shows it can be.

## 6. The capture pipeline

### 6.1 Threads

| Thread | Name | Owns |
| --- | --- | --- |
| Frame worker | `screen-capture` | WGC frame callbacks → crop → `SinkWriter` video stream |
| Audio worker | `screen-audio` | cpal streams (`!Send`) → mix → `SinkWriter` audio stream |
| Control | (caller) | forwards `Control` messages to both |

Every thread is named, per the diagnostics invariant.

### 6.2 The one clock

`screen::clock::CaptureClock` is the single source of output timestamps for
**both** streams:

```rust
pub struct CaptureClock { started: Instant, paused_total: Duration,
                          paused_since: Option<Instant> }
impl CaptureClock {
    pub fn pause(&mut self, now: Instant);
    pub fn resume(&mut self, now: Instant);
    pub fn output_ts(&self, now: Instant) -> Option<Duration>; // None while paused
    pub fn elapsed(&self, now: Instant) -> Duration;           // excludes paused time
}
```

Pure, no I/O, fully unit-tested including pause-during-pause, resume-without-
pause, and multiple pause cycles. Because video and audio both stamp from this
one clock, **A/V sync across a pause is structural rather than incidental** —
there is no second time base to drift from.

### 6.3 Pause

Reuses the `Control { Stop, Pause, Resume }` single-channel pattern from
`capture/src/session.rs`, and for the same documented reason: one forwarding
point means no second signalling path can race the stop. While paused, arriving
frames and audio samples are **drained and discarded** and `output_ts` returns
`None`, so paused wall-clock time never appears in the output. Streams stay
open — tearing down and re-creating a WGC session on every pause would drop
frames on resume and can fail outright if the target window changed state.
Stop-while-paused finalizes normally.

### 6.4 Fragmented MP4 — the crash-safety decision

This is the one place video genuinely differs from audio, and it matters.

MP3 is a raw stream: a crashed audio recording still plays, which is what lets
`capture`'s "never lose captured audio" invariant hold with a plain `.part`
file. A **standard MP4 writes its index (`moov`) at the end** — a crash
mid-capture leaves a file no player can open. Carrying the existing invariant
into video therefore requires a different container strategy.

**The staged file is written as fragmented MP4** (`moof`/`mdat` fragments,
~1 s each, flushed as they close). A crash costs the final fragment, not the
recording. The staged file is named `<base>.mp4.part` and is hidden
(dot-prefixed) in the staging dir, mirroring `.mp3.part`.

**Verification — RESOLVED 2026-09-19. The spike ran; §6.4 holds as written.**
Measured on a `windows-latest` runner (`vault_buddy_screen`, `fmp4-spike`
feature, commit `d8c05a3`), writing 300 synthetic frames and killing the
process mid-capture with `std::process::abort()` — no `Finalize`, no
destructors, no COM teardown:

| Run | On disk | Fragments | Frames decoded back |
| --- | --- | --- | --- |
| fMP4, finalized | 63 479 B | 30 | 300 / 300 |
| **fMP4, crashed** | **59 399 B** | **28** | **280 / 300** |
| Standard MP4, crashed (control) | 50 074 B | 0 | **0** |

The control is what makes this conclusive: both crashed files hold a
comparable amount of data, so the difference is not "one wrote and one did
not" — it is the container. The crashed standard MP4 is `[ftyp, uuid, mdat]`:
every byte of video present, no index, unopenable. The crashed fMP4 is
self-describing per fragment and plays as a prefix, losing ~0.7 s.

`MFCreateFMPEG4MediaSink` (Windows 8+, `mfidl.h`) feeds
`MFCreateSinkWriterFromMediaSink` to give exactly the `IMFSinkWriter` this
design assumed, via the `windows` crate 0.62 already in the lockfile.

**The chunked-rolling fallback is therefore NOT adopted.** It stays documented
here only as the contingency that was not needed: roll the sink every N seconds
into sequential chunk files (`<base>.000.mp4`, `.001.mp4`, …) treated as one
logical source.

Two caveats worth carrying into phase 2:

- **Process crash, not power loss.** `abort()` kills the process; the kernel
  still flushes its own write cache. A power cut is a strictly harder case and
  was not tested. It only shortens the recoverable prefix — it cannot make a
  fragmented file behave like an unindexed one — so the design decision is
  unaffected.
- **Do not infer the sink's write cadence from file size during capture.** A
  per-frame `std::fs::metadata` probe reported 0 bytes for the entire capture
  and then 63 KB after `Finalize`, because Windows updates the directory
  entry's size lazily for a file with an open handle. The crashed run proves
  data reaches the OS regardless. An earlier reading of that probe wrongly
  concluded a per-fragment byte-stream flush was required; it is not.

Because the staged file is fMP4, the untouched-timeline fast path (§8.2)
**remuxes** rather than plain-copies, so what lands in the vault is always a
standard, maximally-compatible MP4.

### 6.5 Audio: generalizing the mixer

`capture::mixer::mix_to_stereo_i16(a: &[f32], b: &[f32])` takes exactly two
sources. Multi-select requires N. This increment adds:

```rust
pub fn mix_n_to_stereo_i16(sources: &[&[f32]]) -> Vec<i16>
```

summing N equal-length mono buffers through the existing `soft_clip` before
scaling to i16, with `mix_to_stereo_i16` reimplemented as the two-source case
so existing capture behaviour is byte-identical (regression-tested). Zero
sources yields silence — a legal, documented outcome, because a silent UI
demo is a real use case and blocking Start on it would be wrong.

Per-source gain normalisation (dividing by N) is **not** applied: it would
change existing two-source meeting-recording levels. `soft_clip` already
handles summed overflow, which is the behaviour the audio domain shipped with.

## 7. The picker and settings surfaces

### 7.1 Entry point

`RecordMode.vue`'s `OPTIONS` gains a third entry, **Record Screen** ("Screen,
window, or region"), placed after Voice Note and before Import Document —
capture actions first, import next, browse last, preserving the documented
ordering. It routes to a new panel view, `screenCapture`, whose parent is
`recordMode`, carrying `screenCaptureVaultId`.

### 7.2 `ScreenSourcePicker.vue`

A `TabGroup` over `Screen | Window | Region`:

- **Screen** — one row per monitor: `Screen 1 · 2560×1440 · Primary`.
- **Window** — visible top-level windows by title and owning process, with
  Vault Buddy's own windows filtered out by HWND.
- **Region** — a "Select region…" button invoking the overlay, then rendering
  the result as `1280 × 720 at (320, 180) on Screen 1` with a Reselect
  affordance.

Below it `ScreenAudioPicker.vue`: a multi-select checklist of inputs and
loopback outputs from an extended `list_audio_devices`, each with a live level
bar reusing the existing `capture:level` meter idiom. Zero selected shows an
inline "No audio will be recorded" note and still permits Start.

Then Start. The view refuses to start when the picked source has vanished
between enumeration and click (window closed, monitor unplugged), with an
inline error and a refreshed list rather than a failed capture.

### 7.3 During capture

`ScreenCaptureBar.vue` renders on the panel's list view beside the existing
`RecordingBar`, with elapsed time (paused time excluded), a pause/resume
button, Stop, and an advisory dropped-frame indicator. The buddy shows the
recording badge and `hide_buddy` no-ops, exactly as for audio.

**Mutual exclusion.** A screen capture and an audio capture cannot run
concurrently in this increment: both contend for the same audio devices, and
WASAPI loopback capture of the same endpoint from two sessions is a
reliability hazard. Starting either while the other runs returns a typed
`alreadyCapturing` error the UI renders as "A recording is already in
progress." A single shared `CaptureKind` guard in the shell owns this, so
neither domain can be started behind the other's back.

### 7.4 Settings

`ScreenCaptureConfigTab.vue`, a fourth tab in `CaptureSettings.vue` beside
Recording / Documents / Tasks:

- Capture folder (the shared `VaultFolderSetting.vue`), default
  `Screen Captures`
- Layout toggle: flat (default) vs dated `YYYY/MM`
- Quality preset: Low / Balanced / High
- Frame rate: 30 / 60
- Create companion note (default on)
- Extra frontmatter + body template textareas (`TaskTemplateSettings.vue`'s
  presentational shape)

## 8. The editor

### 8.1 The timeline model — `core::timeline`

```rust
pub struct Segment { pub source_start_ms: u64, pub source_end_ms: u64 }
pub struct Timeline { pub segments: Vec<Segment> }

impl Timeline {
    pub fn whole(duration_ms: u64) -> Timeline;
    pub fn split_at(&self, output_ms: u64) -> Timeline;   // no-op on a boundary
    pub fn delete(&self, index: usize) -> Timeline;       // no-op out of range
    pub fn reorder(&self, from: usize, to: usize) -> Timeline;
    pub fn output_duration_ms(&self) -> u64;
    pub fn to_source_ms(&self, output_ms: u64) -> Option<u64>;
    pub fn is_untouched(&self, source_duration_ms: u64) -> bool;
}
```

Every operation returns a **new** `Timeline`, which makes undo/redo a stack of
snapshots — trivially correct, and cheap because the structure is a handful of
integer pairs. Invariants enforced by construction and asserted in tests:
segments are non-empty (`start < end`), never overlap, and deleting the last
segment yields an empty timeline that Save refuses (with an inline message,
not a crash).

`split_at` on an exact boundary is a **no-op**, not a zero-length segment —
this is the kind of edge that produces an unplayable file downstream if it is
allowed through.

This module is pure arithmetic and carries the editor's correctness. It is
tested on Linux in CI.

### 8.2 Preview

`EditorRoot` renders an HTML5 `<video>` over the staged file via the
asset protocol, and a timeline strip of segment blocks proportional to
duration. Playback walks segments, seeking the element to the next segment's
start at each boundary.

Seeking a `<video>` element has visible latency, so boundaries are not
gapless. **The UI labels this a preview and treats export as authoritative.**
That is a deliberate honesty choice consistent with the repo's posture
elsewhere (the transcription stats footer reporting effective rather than
intended state): we do not imply frame-exact scrubbing we are not building.

Controls: play/pause, scrub, **Split** at playhead, **Delete** selected
segment, drag-to-reorder, undo/redo (Ctrl+Z / Ctrl+Shift+Z), Discard, and
Save to vault.

### 8.3 Export

`screen::export` on a named `screen-export` worker:

1. `SourceReader` opens the staged file.
2. `select::plan(&timeline)` — **pure, unit-tested** — produces the ordered
   list of source spans and the output timestamp each maps to.
3. Frames and audio samples are read in source order per span, restamped to
   output time, and written through `SinkWriter`.
4. Progress is emitted as `screen:exportProgress { fraction }`, throttled via
   the existing `core::throttle`.
5. Cancellation is a polled atomic checked per span, mirroring the search
   scan-generation pattern.

**The untouched fast path.** `timeline.is_untouched(source_duration)` skips
decode/encode entirely and **remuxes** the fMP4 to a standard MP4 — no quality
loss, near-instant, so "record, glance, save" never pays for a re-encode.

Export writes to a temp inside the staging dir. **The staged capture is deleted
only after the vault write has landed** — the never-lose invariant applied to
the export step.

> **Reconciled after phase 5 (2026-09-20).** The paragraph above is still the
> design; the five numbered steps are not the implementation, because the
> route changed while phase 5 was three tasks in (commit `1b458dd`, a decision
> by the repository owner). The export shells out to a **user-installed
> ffmpeg** — the posture and the reasoning §3 already gave for declining to
> ship a large copyleft binary — instead of building a Media Foundation
> reader, a second sink and a sample pump. Testability decided it: under the
> MF design every one of those was Windows-only and executed in no automated
> test on any platform, whereas the argument vector is a pure function
> (`screen::ffmpeg_args`) and CI now installs ffmpeg and runs a real round
> trip (`screen/tests/export_roundtrip.rs`). What that means for the steps:
> - **1–3 collapse into one ffmpeg pass.** `select::plan(&timeline)` is
>   unchanged and still where the ordering lives; it now feeds
>   `filter_complex` — one `trim`/`atrim` + `setpts`/`asetpts` pair per span,
>   numbered by PLAN index, then `concat`. The order of the concat inputs IS
>   the user's reorder. The fast path is `-c copy -movflags +faststart`.
> - **4 is unchanged in substance.** Progress is
>   `screen:exportProgress { fraction }`, throttled through `core::throttle`,
>   derived from ffmpeg's own `-progress out_time_us`.
> - **5 is a TIMED poll, not a per-span one.** There are no spans to poll
>   between when ffmpeg owns the whole pass — and the fast path is a single
>   span, so a per-span poll would have made the longest, most unattended
>   export the only uncancellable one. `export.rs` blocks on
>   `recv_timeout(CANCEL_POLL)` (200 ms) against the progress channel, so even
>   an export emitting no progress at all answers Cancel promptly; cancelling
>   kills the child, reaps it, joins both reader threads, and **deletes the
>   truncated output** (leaving it would offer a broken file as a saved one).
> - Two deviations this route RETIRED rather than inherited: the per-span
>   versus per-sample cancellation question disappears with the child process,
>   and the fast path's unverifiable passthrough disappears because copying
>   streams is a flag (`-c copy`) rather than an inference from a media type.
>
> Media Foundation keeps the CAPTURE side untouched — §6 is unaffected. The
> app carries two media stacks on purpose, each where it is strongest.

## 9. The vault write

This is the **ninth sanctioned vault write**. AGENTS.md's vault-domain list
gains it. It introduces **no new write capability** — it reuses the same
machinery, and the file it writes is created, never overwritten.

```
<vault>/<screen capture folder>/[YYYY/MM/]YYYY-MM-DD HHmm <title>.mp4
<vault>/<screen capture folder>/[YYYY/MM/]YYYY-MM-DD HHmm <title>.md
```

- `reserve_basename` reserves **both** the `.mp4` and the `.md` up front, the
  pairwise reservation the audio domain uses.
- The dated directory is asserted inside the vault **before and after**
  `create_dir_all` — the pre-check stops `create_dir_all` following a
  pre-existing symlink/junction out of the vault, the post-check closes the
  swap-in race. Same discipline as `start_capture` and the document import.
- The video commits via `rename_noreplace` with ` (N)` suffix retry; the note
  is written atomically afterwards.
- **Video first, note second.** A note-write failure after a successful video
  commit degrades to a *warning*, not a failure — the recording is the
  irreplaceable artifact, exactly as `capture::rename` treats audio first.

### 9.1 The companion note

```markdown
---
type: "Screen Capture"
recorded: "2026-09-18 14:32"
duration: "3:17"
source: "Figma — Design System"
inputs:
  - "Microphone (Yeti)"
resolution: "1920x1080"
vault: "Engineering"
created-by: Vault Buddy
---

![[2026-09-18 1432 Figma walkthrough.mp4]]
```

`core::screen_note::render_note` mirrors `capture_note::render_note`: the
managed keys above are **always emitted and never user-removable**; the
vault's `screenExtraFrontmatter` is rendered through
`core::template::render_extra_frontmatter` with those keys reserved, injected
after `created-by`; `screenBodyTemplate` (placeholders `{{recordedAt}}`,
`{{date}}`, `{{duration}}`, `{{source}}`, `{{resolution}}`, `{{vault}}` — the
same six `vars` `screenExtraFrontmatter` resolves against) replaces the body
below the embed. An empty/unset template reproduces the exact output above
byte-for-byte — regression-tested, the rule every other template surface
follows.

Every string is emitted through `yaml_quote`; a window title can contain a
colon, a quote, or a backslash, any of which would otherwise produce malformed
YAML.

## 10. Staging and recovery

```
%LOCALAPPDATA%\com.vaultbuddy.desktop\screen-captures\
  .2026-09-18 1432 Figma walkthrough.mp4.part   ← capturing / crashed
  2026-09-18 1432 Figma walkthrough.mp4         ← staged, awaiting edit
  2026-09-18 1432 Figma walkthrough.json        ← sidecar: vault id, source,
                                                   inputs, duration, timeline
```

Staging deliberately lives **outside every vault**. An unedited, unapproved
capture is not knowledge; putting it in a vault would make discard leave
litter in the user's notes and would mean the app writes to a vault without
the user having asked.

The JSON sidecar carries what the editor needs to resume — including the
in-progress timeline, saved on each edit — so a crash mid-edit loses at most
the last operation.

`run_screen_recovery`, wired into `setup` after `run_import_recovery`:

- `.part` files older than the staleness window → attempt fMP4 finalize,
  promote to a staged capture, and offer it; on failure, leave it and log.
- Complete staged captures → surface in the resume-or-discard offer.
- Postponed while a capture is active; rescheduled for orphans younger than
  the staleness window (the same constant capture recovery already uses — one
  staleness rule for the whole app, not a second to keep in step) — the same
  retry loop shape as capture and import
  recovery.
- It sweeps **only** the staging dir, and deletes only entries matching our
  own naming pattern. It never follows a symlink to a target outside staging.

**Resume or discard.** Opening Record Screen with staged captures present
shows them first: each with its source, duration and age, offering *Resume
editing* or *Discard*. Discard is confirm-gated, being irreversible. Nothing
is ever deleted silently.

**Disk pressure.** Video is large. At save time the app checks free space
before export and refuses with a clear message rather than filling the disk;
the staging dir's total size is shown in the settings card with a "Clear
staged captures" action.

## 11. IPC surface

15 new commands (73 → 88). Sync **only** where a window API is touched;
everything doing device or disk work is async on the blocking pool, per the
documented rule.

> **Reconciled after phase 4 (2026-09-20).** Both numbers above are now wrong
> and the module names below are partly aspirational — read this table as the
> DESIGN, and AGENTS.md's IPC table as what exists. The shipped surface is 85
> commands (measure it, never increment it: the one-liner is in AGENTS.md),
> and the phases landed differently from this section in three ways:
> - Region selection is its OWN module, `region_commands.rs`, not part of
>   `screen_commands.rs`: it is a window-lifecycle concern that touches no
>   `CaptureGuard`, no `ScreenCaptureState` and no session.
> - The editor's module is `editor_commands.rs`, and phase 4 shipped FOUR
>   commands, none of them the ones named below: `open_capture_editor`
>   *(sync — it stashes the base rather than reading disk)*,
>   `take_editor_request` *(sync — the one-shot drain of that stash)*,
>   `load_staged_capture` *(async)* and `save_capture_timeline` *(async)*.
>   `list_staged_captures`, `export_and_save_capture`, `cancel_export`,
>   `discard_staged_capture` and `open_screen_capture` are all still ahead —
>   they are phase 5/6's, along with the vault write none of them can do yet.
> - `screen_config_commands.rs` does not exist at all: five of the seven
>   `screen_*` config fields are parsed and preserved but read by nothing
>   until phase 6.

> **Reconciled again after phase 5 (2026-09-20).** The shipped surface is now
> **92 commands** — measured with the one-liner in AGENTS.md, never
> incremented; that sentence has been wrong four times. Everything the phase-4
> note listed as "still ahead" has landed, and in different modules from the
> ones named below:
> - **`export_commands.rs`** — the export LIFECYCLE and, with it, all five of
>   the feature's export events through one warning-logging emitter:
>   `export_and_save_capture` *(async)*, `cancel_export` *(sync — one mutex,
>   one flag, no I/O)*.
> - **`staged_commands.rs`** — a staged capture as an OBJECT:
>   `discard_staged_capture` *(async)*, `list_staged_captures` *(async)*,
>   `open_screen_capture` *(sync)*. The split is not a design change; one file
>   carrying both came to 989 nonblank lines against the repo's 800 Rust cap.
>   `screen:discarded` is still emitted from `export_commands`' single
>   emitter, so the one-emitter invariant survives it.
> - **`ffmpeg.rs`** — `detect_ffmpeg` *(async)* and `set_ffmpeg_path`
>   *(async)*, which this section does not name because the MF design needed
>   no external tool. **Neither has a frontend caller** (docs/Gaps.md
>   GAP-144).
> - `screen_config_commands.rs` still does not exist — but the "five of seven
>   read by nothing" half of the note above is now **false**: `export_worker`
>   reads all five, and `screen_quality`/`screen_fps` were already read by the
>   capture worker. **All seven are read; none has a settings surface.** That
>   is what phase 6 owes, and it is a different statement from the one this
>   note used to make.
>
> **The Events table below is one event short.** `screen:discarded { base }`
> is not listed here and ships deliberately: without it, discarding a capture
> leaves the store's `lastStaged` pointing at a base no longer on disk, so the
> panel keeps offering **Edit** and the editor fails with a banner the user
> cannot act on. `screen:exported` also carries more than "export lifecycle"
> suggests — `{ base, videoPath, notePath, vaultId, vaultName, warning }` —
> because the editor window installs no store and cannot turn a vault id into
> a name. AGENTS.md's Events table is the one to read as shipped.

| Defined in | Commands |
| --- | --- |
| `screen_commands.rs` | `list_capture_sources` *(async)*, `select_capture_region` *(async — shows the overlay, resolves to a rect or null)*, `start_screen_capture` *(async)*, `pause_screen_capture`, `resume_screen_capture`, `stop_screen_capture` *(async)*, `screen_capture_status` *(sync)* |
| `screen_editor_commands.rs` | `open_capture_editor` *(sync — window show/focus, main thread)*, `list_staged_captures` *(async)*, `export_and_save_capture` *(async)*, `cancel_export`, `discard_staged_capture` *(async — destructive, confirm-gated)*, `open_screen_capture` *(sync — `uri::launch` handoff to Obsidian)* |
| `screen_config_commands.rs` | `get_screen_capture_config`, `set_screen_capture_config` *(async)* |

### Events

| Event | Meaning | Listened to by |
| --- | --- | --- |
| `screen:started` / `paused` / `resumed` / `stopped` / `failed` | Capture lifecycle | `screenCapture` store (BuddyRoot **and** PanelRoot) |
| `screen:warning` | Non-fatal: source lost, frames dropped, a device vanished | same |
| `screen:frames` | Advisory `{fps, dropped}`, ~2 Hz, lossy by design | same |
| `screen:exportProgress` / `exported` / `exportFailed` / `exportCancelled` | Export lifecycle | EditorRoot + panel |

Both `BuddyRoot` and `PanelRoot` must call `screenCapture.init()` — the
documented per-window wiring rule; a store mirroring Rust state that is
initialised in only one webview leaves the other with a dead indicator.

## 12. Configuration

Seven additive `VaultCaptureConfig` fields, parsed **per-field defensively**
like every other vault entry, so one hand-edited bad value defaults only
itself:

| Field | JSON | Default |
| --- | --- | --- |
| `screen_capture_folder` | `screenCaptureFolder` | `None` → `Screen Captures` |
| `screen_capture_date_folders` | `screenCaptureDateFolders` | `false` |
| `screen_quality` | `screenQuality` | `balanced` (invalid → `balanced`) |
| `screen_fps` | `screenFps` | `30` (anything but 30/60 → 30) |
| `screen_create_note` | `screenCreateNote` | `true` |
| `screen_extra_frontmatter` / `screen_body_template` | same, camelCase | `None` |

Quality presets map to bitrate by output area so a 4K capture is not starved
at the same absolute bitrate as a 720p one:

| Preset | Bitrate |
| --- | --- |
| Low | ~0.08 bits/pixel/frame |
| Balanced | ~0.15 |
| High | ~0.25 |

Keyframe interval is fixed at **1 second** regardless of preset. It costs
little, it bounds seek latency in the editor preview, and it leaves a future
no-re-encode fast-cut path possible without a capture-format change.

`set_screen_capture_config` is its own independent field-save command — the
`set_task_template_config` pattern — async, read-modify-write under the single
`capture_config::config_write_lock()`. `config_merge` gains
`merge_screen_owned`, and `merge_capture_owned` / `merge_documents_owned` are
extended to preserve the screen fields, so no settings surface can reset
another's. `serialize_config` round-trips them — the regression that once
silently deleted the whole `mcp` section is exactly this bug, and it gets the
same test.

## 13. Phasing

Six phases. Each ends at a state that compiles, passes CI, and is coherent.
This is how the single-design decision is reconciled with reviewable
increments — the spec is one document, the landing is not one PR.

| Phase | Contents | Gate |
| --- | --- | --- |
| **1. Foundations** | `core::timeline`, `core::screen_geometry`, `core::screen_capture_config`, `core::screen_note`, `mix_n_to_stereo_i16`, `screen` crate skeleton with stubs, `screen::clock`, `screen::select` | All Linux-testable. Full unit coverage. No user-visible change. |
| **2. Capture engine** | **fMP4 spike first**, then `sink.rs`, `source.rs`, `session.rs`, staging, `screen_commands.rs`, picker + capture bar, mutual-exclusion guard | Records a monitor or window with audio to a staged file. Manual Windows verification. |
| **3. Region overlay** | `overlay` window, `RegionRoot`, DPI conversion wired, `WDA_EXCLUDEFROMCAPTURE` | Region capture works across DPI scales. |
| **4. Editor** | `editor` window, asset protocol + CSP, `EditorRoot`, preview, timeline UI, undo/redo, sidecar persistence | Edits a staged capture; no export yet. |
| **5. Export & save** | `reader.rs`, `export.rs`, fast-path remux, vault write, companion note, `run_screen_recovery`, resume-or-discard | End-to-end: record → edit → save → playable note. |
| **6. Settings & docs** | `ScreenCaptureConfigTab`, staging size + clear action, AGENTS.md / CONTEXT.md / README / Gaps.md updates | Docs reconciled; baselines updated. |

Phase 2's spike is a **hard gate**: if fragmented output from `SinkWriter` is
unavailable, the chunked-rolling fallback (§6.4) is adopted before any further
phase work, because both the editor and export read the staged format.

## 14. Error handling

Every failure names what happened and what the user can do; nothing is
swallowed (`log::warn!`/`log::error!` is the floor, per the diagnostics
invariant).

| Failure | Behaviour |
| --- | --- |
| Source vanished before start (window closed, monitor unplugged) | Typed `sourceGone`; inline error + refreshed source list. Never a started-then-dead capture. |
| Source vanishes **during** capture | `screen:warning`, capture **finalizes cleanly** with what it has. A closed window must not lose the preceding recording. |
| An audio device vanishes mid-capture | `screen:warning`, that source drops out, the rest keeps recording — the existing audio-domain posture. |
| Encoder init fails (no H.264 encoder, MF unavailable) | Typed `encoderUnavailable` with the OS error; Start refused before anything is written. |
| Disk fills during capture | Capture stops and **finalizes**; the partial capture is staged and offered, with an explicit "stopped: disk full". |
| Export fails or is cancelled | Staged capture and timeline are **kept**; the editor stays open with the error. |
| Vault write fails | Staged capture **kept**, exported temp kept, error surfaced with retry. Nothing is deleted on a failed save. |

> **Reconciled after phase 5 (2026-09-20): the exported temp is DELETED, not
> kept.** The staged capture and the timeline are kept, which is the half that
> matters and which this row gets right — but a kept temp is a promise
> `screen_recovery` breaks 60 s later, when the staleness sweep removes an
> abandoned `.export.mp4.part` as litter. That would make the same Retry
> button behave differently depending on how long the user spent reading the
> error message. A retry re-exports from the staged capture, which is the
> artifact that must never be lost; the temp is a half-written transcode with
> no independent value. Every `Err` return in `export_worker::export_blocking`
> / `run_export` and in `screen::export` removes it, and the same is true of a
> cancel (`remove_output`). The row's intent — "nothing the user could want is
> deleted on a failed save" — holds; its letter does not.
| Both capture kinds requested at once | Typed `alreadyCapturing`; neither is disturbed. |
| Empty timeline at save | Save disabled with an inline explanation. |

## 15. Testing

**Rust, runs on Linux in CI** (`rust-core` gains `-p vault_buddy_screen`):

- `core::timeline` — split (mid-segment, on a boundary, out of range), delete
  (including last), reorder, `output_duration_ms`, `to_source_ms` round-trip,
  `is_untouched`, non-overlap and non-empty invariants after every operation.
- `core::screen_geometry` — DPI conversion at 1.0/1.25/1.5/2.0, clamping,
  empty-intersection → `None`, resolution-changed-since-selection.
- `core::screen_capture_config` — per-field defensive parse (each field
  malformed in isolation), serialize round-trip, and the cross-surface
  preservation test: a capture save, a documents save and a tasks save each
  leave the screen fields untouched.
- `core::screen_note` — golden output; the empty-template byte-for-byte
  regression; reserved-key filtering; YAML-quoting of titles containing `:`,
  `"`, `\`; merge-key (`<<`) rejection.
- `screen::clock` — pause/resume cycles, double-pause, resume-without-pause,
  elapsed excludes paused time, timestamps are monotonic across pauses.
- `screen::select` — the frame plan for reordered and deleted segments;
  output timestamps contiguous and monotonic.
- `capture::mixer` — `mix_n_to_stereo_i16` for N=0/1/2/5; the N=2 case is
  asserted **byte-identical** to today's `mix_to_stereo_i16`.
- The Linux stub arm returns `Unsupported` rather than panicking.

**Vitest** (happy-dom + `mockIPC`): source picker rendering and refusal on a
vanished source, audio multi-select, the zero-devices notice, capture bar
states, editor timeline interactions against a mocked store, undo/redo,
export progress and cancel, resume-or-discard, settings autosave.

**Manual Windows verification** — an honest, documented limit: no CI runner
can record a screen. A checklist file
(`docs/superpowers/specs/2026-09-18-screen-capture-windows-verification.md`),
following the precedent of the earlier increments, covers: each source kind;
multi-monitor at mixed DPI; pause/resume A/V sync; window closed mid-capture;
kill the process mid-capture and confirm the staged file still plays; export
exactness; the untouched fast path; `WDA_EXCLUDEFROMCAPTURE`; and playback of
the saved note inside Obsidian.

## 16. Quality gates & docs

- LOC baseline (`scripts/loc-baseline.json`) and quality baseline updated in
  the same PR as the code that moves them, per the shrink-only ratchet policy.
- Coverage floors: the new `core` modules are pure and must not lower the 94%
  line floor; `vault_buddy_screen` joins the `llvm-cov` set for its pure
  modules.
- `cargo machete`, `cargo deny`, workspace clippy `-D warnings`,
  `cargo fmt --check` all unchanged in shape. `deny.toml` needs **no** new
  entry — `windows-capture` is MIT and from crates.io.
- AGENTS.md gains: the screen-capture domain section, the `screen` row in
  "what compiles where", the two new windows in the window-system section, the
  ninth sanctioned vault write, the 15 commands and their events, the new
  config fields, and the `editor`/`overlay` labels in `rootFor()`.
- CONTEXT.md gains the §2 terms. README gains a Screen Capture feature entry.
- docs/Gaps.md gains the known residuals: preview seek latency at segment
  boundaries; no transcription of screen captures; screen captures absent from
  the Recordings browser; no rename-after-save; the mutual-exclusion
  restriction between audio and screen capture; and export time on long
  recordings.

## 17. Open questions for implementation

1. **Fragmented MP4 from `IMFSinkWriter`** — the phase 2 spike. Fallback:
   chunked rolling files (§6.4).
2. **`windows-capture` 2.0 frame buffer access** — whether cropping can be
   done on the GPU texture before CPU readback (cheaper) or must happen after.
   Either works; the former is preferable for 4K at 60 fps.
3. **Hardware vs software H.264** — MF picks automatically, but a machine
   whose hardware encoder is busy may fall back to software and drop frames.
   The `screen:frames` dropped-frame counter exists to make that visible; the
   response (drop to 30 fps, warn) is decided from real measurements, not
   guessed here.
