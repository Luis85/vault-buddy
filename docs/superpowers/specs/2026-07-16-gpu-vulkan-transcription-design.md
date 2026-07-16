# GPU (Vulkan) Transcription Design — "Use the hardware that's there"

- **Date:** 2026-07-16
- **Status:** Approved (follow-up increment named in
  [the accuracy & speed spec](2026-07-16-transcription-accuracy-and-speed-design.md);
  user directed it onto the same branch/PR #61)
- **Source:** The accuracy & speed increment shipped vocabulary priming,
  the Turbo tier, and real Silero VAD — all CPU-bound. whisper.cpp's
  single biggest remaining speed lever is GPU inference; the original STT
  spec (2026-07-04) already named the intended path: **one Vulkan build**
  covering NVIDIA, AMD, and Intel GPUs on Windows, with CPU fallback.

## Goal

Shipped Windows builds run whisper on any Vulkan-capable GPU when one
exists, fall back to CPU when it doesn't, and give the user one app-global
escape hatch ("Use GPU", default on) for buggy drivers — without breaking
CPU-only contributor builds, Linux CI, or the whisper-rs 0.16 pin.

## Scope

### In scope

1. **A `gpu` build flavor.** transcribe gains
   `whisper-vulkan = ["whisper", "whisper-rs/vulkan"]`; the shell gains
   `gpu = ["vault_buddy_transcribe/whisper-vulkan"]`. Nothing enables it
   by default — the two Windows CI jobs pass the feature explicitly, so
   every SHIPPED build is GPU-capable while a contributor without the
   Vulkan SDK still builds and runs CPU-only.
2. **Explicit `use_gpu` wiring.** whisper-rs 0.16's
   `WhisperContextParameters::default()` sets `use_gpu` **false** (unlike
   whisper.cpp's C default) — today's code could never engage a GPU even
   on a Vulkan build. `WhisperTranscriber::load` gains a `use_gpu: bool`
   and sets it on the context builder. On a non-vulkan build the flag is
   inert (no GPU backend compiled in) — safe on every platform.
3. **App-global "Use GPU" setting, default on.** New app-global
   `transcription` section in `config.json` (`{"useGpu": true}`), split
   module `core/src/transcription_config.rs` (the `mcp_config` /
   `document_import_config` precedent): per-field defensive parse,
   round-tripped by `serialize_config`, preserved by every other
   section's writer. IPC: `get_transcription_config` /
   `set_transcription_config` (sync, under `ConfigWriteLock` — the
   `set_capture_config` shape; app-side file, not a vault write). UI: a
   small **Transcription** card in `BuddySettings.vue` (beside the MCP and
   Document import cards) with the toggle and honest helper copy:
   applies from the next transcription; falls back to CPU when no
   compatible GPU exists; turn off if you hit graphics-driver crashes.
4. **Model cache keyed on `(tier, use_gpu)`.** The worker's cached
   `WhisperTranscriber` reloads when either changes, so flipping the
   toggle takes effect on the next job without a restart.
   `process_transcription` reads the live setting per job.
5. **CI/release plumbing.** `windows-app` (ci.yml) and
   `windows-installer` (release.yml) install a **pinned LunarG Vulkan
   SDK** (cached; includes glslc for whisper.cpp's shader compilation)
   before the build and enable the `gpu` feature. `rust-core` and
   `linux-app` stay CPU-only and SDK-free — the Linux compile gates never
   see the vulkan feature.

### Out of scope

- **Per-transcript device reporting.** Deliberately none: whisper-rs
  exposes no API to observe which backend actually ran, and recording the
  *setting* as if it were the *engagement* is exactly the dishonesty the
  VAD stats row almost shipped (a Vulkan build with no usable GPU silently
  falls back to CPU). whisper.cpp's own device/init log lines already land
  in the app log via `install_logging_hooks` — that is the audit trail.
- `flash_attn` (conflicts with DTW, default off upstream), `gpu_device`
  selection (device 0 only), CUDA/HIP/Metal flavors, and any Linux/macOS
  GPU builds.
- Live progress/perf telemetry changes — the existing progress callback
  is backend-agnostic.

## Key decisions

| Decision | Choice | Why |
| --- | --- | --- |
| Backend | Vulkan only | One binary covers NVIDIA/AMD/Intel on Windows; the 2026-07-04 spec's documented path |
| Feature plumbing | Explicit `gpu` cargo feature, enabled only by Windows CI/release | Target-conditional features would force every local Windows build to need the Vulkan SDK; explicit opt-in keeps contributor builds working and makes CI's intent visible |
| Toggle scope | App-global, default ON | GPU is machine-level, not per-vault; default-on delivers the win, the toggle is the driver-crash escape hatch |
| Toggle effect | Next model load (`(tier, use_gpu)` cache key) | No restart required; no mid-inference switching complexity |
| Device row in stats | **None** | Cannot honestly observe backend engagement through whisper-rs (the VAD lesson); app logs carry whisper.cpp's own device lines |
| Same PR | Yes (#61) | User direction; the branch already carries the increment this builds on |

## Design

### Cargo & build

- `src-tauri/transcribe/Cargo.toml`:
  `whisper-vulkan = ["whisper", "whisper-rs/vulkan"]`.
- `src-tauri/Cargo.toml` (shell): feature
  `gpu = ["vault_buddy_transcribe/whisper-vulkan"]`; no default change.
- Windows CI/release builds pass the feature through tauri's cargo args
  (`npx tauri build -- --features gpu` / the tauri-action `args` input —
  exact syntax pinned at plan time against the tauri v2 CLI).
- `cargo deny`/`machete` implications: none expected (no new crates.io
  deps; vulkan is a whisper-rs-sys build-time backend).

### Engine (`transcribe/src/engine.rs`)

`WhisperTranscriber::load(model_path: &Path, use_gpu: bool)`:

```rust
let mut params = WhisperContextParameters::default();
// whisper-rs (unlike whisper.cpp's C default) ships use_gpu = false, so
// this explicit set is what makes a Vulkan build actually use the GPU.
// On a CPU-only build the flag is inert. Fallback is whisper.cpp's own:
// no usable device -> CPU, logged through install_logging_hooks.
params.use_gpu(use_gpu);
```

The whisper feature-gated engine is the only touchpoint; decode, VAD,
prompt, and callback wiring are backend-agnostic and unchanged.

### Config & IPC

- `core/src/transcription_config.rs`: `TranscriptionConfig { use_gpu: bool }`
  (default true), `parse_transcription_section` (defensive), serializer
  emitting the section only when non-default (`useGpu: false`) — the
  minimal-file discipline. `capture_config::AppConfig` carries it;
  `serialize_config` round-trips it; regression test that a capture-side
  save preserves it (the mcp-section-deletion lesson).
- `src-tauri/src/transcription_config_commands.rs` (or beside the existing
  config commands — plan decides placement against LOC caps):
  `get_transcription_config` / `set_transcription_config`, sync, under
  `ConfigWriteLock`, registered in `lib.rs`'s handler list + AGENTS.md IPC
  table.

### Worker (`src-tauri/src/transcription.rs`)

The cached model becomes `Option<(ModelTier, bool, WhisperTranscriber)>`;
`process_transcription` reads `use_gpu` from the app-global config it
already loads, passes it to `WhisperTranscriber::load`, and reloads when
either key element differs. No queue/cancel/dedup semantics change.

### UI

`TranscriptionAppSettings.vue` (new, self-contained — the
`DocumentImportSettings` idiom): loads via `get_transcription_config` on
mount, one toggle, saves via `set_transcription_config` with the
optimistic/error-toast discipline; mounted as a **Transcription** card in
`BuddySettings.vue`. The per-vault `TranscriptionSettings.vue` is
untouched (GPU is not per-vault).

### CI

Both Windows jobs gain, before the build step:

1. A pinned LunarG Vulkan SDK install (silent installer, `VULKAN_SDK` env
   exported, glslc component included), **cached** by SDK version so the
   ~200 MB download doesn't run on every PR.
2. The `gpu` feature on the build invocation.

`rust-core` (Linux) keeps testing `--features whisper` only; `linux-app`
keeps building the CPU shell. A Vulkan SDK failure on Windows CI is a
visible job failure, not a silent CPU-only release.

### Error handling & risks

- **No GPU / driver too old:** whisper.cpp falls back to CPU at context
  init and logs it — no user-facing failure, no behavior change beyond
  speed.
- **Buggy driver crash:** the engine is in-process, so a driver fault can
  take the app down; the crash handler records it, the toggle is the
  remedy, and the sidecar-process architecture remains the documented
  future fix. Gaps.md gets an entry naming this accepted risk.
- **Model load failure on GPU** (e.g. VRAM exhaustion on `medium`/turbo):
  surfaces exactly like today's corrupt-model load failure (retryable
  `failed` sidecar). The self-heal model-delete stays — a load failure
  with GPU on may be spurious, but re-downloading is merely wasteful, not
  wrong; noted in Gaps.md as a known rough edge.

## Testing

- **core:** `transcription_config` parse/default/round-trip/preservation
  tests (Linux).
- **transcribe:** `load` signature change compile-gated on both feature
  sets; no behavior test can observe GPU engagement (CI runners have no
  GPU) — stated honestly, the compile gate + whisper.cpp's logged
  fallback are the automated surface. The `#[ignore]` real-model test
  gains a `VB_TEST_GPU=1` env path for a manual Windows run.
- **shell:** worker cache-key logic extracted pure enough to unit test
  (reload-on-flip), command tests where the existing config commands have
  them.
- **frontend:** `TranscriptionAppSettings` Vitest coverage (load, toggle
  emit/save, error path) + BuddySettings mount.
- **Manual (release gate):** one Windows machine with a real GPU: confirm
  whisper.cpp's device log line, a visible speed delta on Turbo, and the
  toggle flipping back to CPU on the next job.

## Documentation updates

- AGENTS.md: IPC table (+2 commands), transcription domain section (GPU
  paragraph incl. the use_gpu-default-false gotcha and the no-device-row
  rationale), "what compiles where" (the `gpu` feature note), commands
  section (Windows GPU build invocation).
- docs/DEVELOPMENT.md: contributor note — plain Windows builds need no
  Vulkan SDK; GPU builds (`--features gpu`) and CI do; SDK version pin.
- docs/Gaps.md: the in-process GPU driver-crash risk entry.
