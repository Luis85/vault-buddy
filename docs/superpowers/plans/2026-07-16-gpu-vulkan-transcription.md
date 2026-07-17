# GPU (Vulkan) Transcription Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Shipped Windows builds run whisper on any Vulkan-capable GPU (CPU fallback automatic), behind an app-global default-on *Use GPU* escape hatch — per spec `docs/superpowers/specs/2026-07-16-gpu-vulkan-transcription-design.md`, on the same branch/PR #61.

**Architecture:** An explicit `gpu` cargo feature chain (`shell gpu → transcribe whisper-vulkan → whisper-rs/vulkan`) that only the two Windows CI jobs enable; `WhisperTranscriber::load` gains `use_gpu: bool` (whisper-rs defaults it FALSE — the explicit set is the whole feature); an app-global `transcription` config section with get/set IPC; the worker's model cache keyed on `(tier, use_gpu)`; a new Buddy-settings card. No per-transcript device row (backend engagement is unobservable through whisper-rs — the VAD lesson).

**Tech Stack:** whisper-rs 0.16 `vulkan` feature (pinned version unchanged), LunarG Vulkan SDK in CI (pinned version + SHA-256, direct download — no third-party action), Vue 3 + Vitest.

## Global Constraints

- whisper-rs stays pinned at `0.16`; no new crates.io dependencies.
- The `gpu`/`whisper-vulkan` features are enabled ONLY by the two Windows CI/release build invocations — `rust-core`, `linux-app`, local dev builds, and every `cargo test` stay CPU-only and Vulkan-SDK-free. `whisper-vulkan` cannot compile in this container (no Vulkan headers/glslc); its compile gate is the Windows CI job itself.
- `WhisperContextParameters::default()` has `use_gpu = false` in whisper-rs (unlike whisper.cpp's C default) — the engine must call `.use_gpu(flag)` explicitly.
- Wire key: app-global section `"transcription": { "useGpu": bool }`, default `true`, serialized ONLY when non-default (`useGpu: false`), preserved by every other config writer (regression-tested — the mcp-section-deletion lesson).
- New IPC commands `get_transcription_config` / `set_transcription_config`: sync, under `ConfigWriteLock`, registered in `lib.rs`'s `generate_handler!` AND AGENTS.md's IPC table.
- Model cache key: `(ModelTier, bool)` — a toggle flip takes effect on the next job, no restart.
- UI copy for the toggle: label `Use GPU (Vulkan)`, helper `Applies from the next transcription. Falls back to CPU when no compatible GPU is found — turn off if you hit graphics-driver crashes.`
- Vulkan SDK in CI: LunarG direct download, version AND installer SHA-256 pinned in the workflow (resolve the current version via `https://vulkan.lunarg.com/sdk/latest/windows.json` and its SHA via `https://vulkan.lunarg.com/sdk/sha256/<version>/windows/vulkansdk-windows-X64-<version>.exe.txt` at implementation time; hand-verify the hash file's value before pinning). No cache step in v1 (~2 min install accepted; noted in the workflow comment).
- Commit style: Conventional Commits; committer identity `noreply@anthropic.com` / `Claude` (already configured).
- Rust gates: fmt/clippy `-D warnings`/tests per crate; frontend gates in CI order; `npm run check:loc` after EVERY task that adds Rust lines (twice this branch forgot; both times CI caught it).

---

### Task G1: `whisper-vulkan` feature + `use_gpu` through the engine

**Files:**
- Modify: `src-tauri/transcribe/Cargo.toml` (features block)
- Modify: `src-tauri/transcribe/src/engine.rs` (`WhisperTranscriber::load` ~line 179-188; the `#[ignore]` real-model test's `load` call)
- Modify: `src-tauri/src/transcription.rs` (the one shell `load` call site — updated here so the workspace keeps compiling in one commit; the flag is HARDCODED `false` in this task and wired to config in G3)

**Interfaces:**
- Produces: `WhisperTranscriber::load(model_path: &Path, use_gpu: bool)` — consumed by G3; cargo feature `whisper-vulkan = ["whisper", "whisper-rs/vulkan"]` — consumed by G4.

- [ ] **Step 1: Add the feature**

In `src-tauri/transcribe/Cargo.toml` `[features]`, after the `whisper` line:

```toml
# GPU inference via whisper.cpp's Vulkan backend. Compiles ONLY where the
# Vulkan SDK (headers + glslc) exists — the Windows CI/release jobs enable
# it through the shell's `gpu` feature; every other build stays CPU-only.
# Runtime behavior additionally requires use_gpu(true) on the context
# (whisper-rs defaults it false) and falls back to CPU when no device.
whisper-vulkan = ["whisper", "whisper-rs/vulkan"]
```

- [ ] **Step 2: Widen `load` (compile-driven; the existing suite is the test)**

In `src-tauri/transcribe/src/engine.rs` replace `WhisperTranscriber::load`:

```rust
impl WhisperTranscriber {
    /// `use_gpu` maps to whisper.cpp's context flag. whisper-rs's
    /// `WhisperContextParameters::default()` ships use_gpu = FALSE (unlike
    /// whisper.cpp's own C default), so this explicit set is what makes a
    /// Vulkan build actually engage the GPU. On a CPU-only build the flag
    /// is inert (no GPU backend is compiled in), and on a Vulkan build
    /// with no usable device whisper.cpp falls back to CPU at context
    /// init — its own log line (routed through install_logging_hooks) is
    /// the audit trail; deliberately NO per-transcript device claim (the
    /// VAD stats-row lesson: never record intent as engagement).
    pub fn load(model_path: &Path, use_gpu: bool) -> Result<Self, String> {
        let mut params = WhisperContextParameters::default();
        params.use_gpu(use_gpu);
        // Pass the `&Path` straight through rather than round-tripping via
        // `to_string_lossy()` (see the previous comment on this fn).
        let ctx = WhisperContext::new_with_params(model_path, params)
            .map_err(|e| format!("load model {}: {e}", model_path.display()))?;
        Ok(Self { ctx })
    }
}
```

Update the `#[ignore]` real-model test's call to
`WhisperTranscriber::load(&model, std::env::var("VB_TEST_GPU").is_ok())` with a
one-line comment (`VB_TEST_GPU=1 exercises the GPU path on a manual Windows
run`). Update `examples/transcribe_file.rs` if it calls `load` (grep; pass
`false`).

In `src-tauri/src/transcription.rs`, the load site
(`match WhisperTranscriber::load(&model)` ~line 453) becomes
`WhisperTranscriber::load(&model, false)` with the comment
`// use_gpu wired to the app-global setting in the next commit.`

- [ ] **Step 3: Verify**

Run: `cd src-tauri/transcribe && cargo test && cargo test --features whisper && cargo clippy --all-targets -- -D warnings`
Then: `cd src-tauri && cargo test -p vault-buddy --lib && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --check`
Then: `npm run check:loc` (a few added lines — if the ratchet trips, `--update` + append the justification `use_gpu parameter + doc comment (GPU increment)` to the file's reason).
Expected: all green. (Do NOT try `--features whisper-vulkan` here — no Vulkan SDK in this container; Windows CI is its compile gate.)

- [ ] **Step 4: Commit**

```bash
git add src-tauri/transcribe/Cargo.toml src-tauri/transcribe/src/engine.rs src-tauri/transcribe/examples src-tauri/src/transcription.rs scripts/loc-baseline.json
git commit -m "feat(transcribe): whisper-vulkan feature and explicit use_gpu on model load

whisper-rs defaults use_gpu to false (unlike whisper.cpp's C default), so
without this explicit set even a Vulkan build never engages the GPU. The
flag is inert on CPU-only builds; Vulkan builds fall back to CPU when no
device exists, logged by whisper.cpp itself. Shell passes false until the
app-global setting lands."
```

---

### Task G2: app-global `transcription` config section (core)

**Files:**
- Create: `src-tauri/core/src/transcription_config.rs`
- Modify: `src-tauri/core/src/lib.rs` (module declaration — mirror how `mcp_config` is declared/exported)
- Modify: `src-tauri/core/src/capture_config.rs` (`AppConfig` struct + `parse_config` + `serialize_config` — mirror the `mcp` section threading exactly)

**Interfaces:**
- Produces: `TranscriptionConfig { pub use_gpu: bool }` (Default: `use_gpu: true`), `capture_config::AppConfig.transcription: TranscriptionConfig`, parse/serialize round-trip — consumed by G3.

- [ ] **Step 1: Read the precedent, then write the failing tests**

Read `src-tauri/core/src/mcp_config.rs` and the `mcp` threading in
`capture_config.rs` FIRST — G2 mirrors that shape exactly (struct + default +
`parse_*_section(Option<&serde_json::Value>)` + `serialize_*_section` emitting
only-when-non-default + AppConfig field + round-trip tests). Then, in the new
`transcription_config.rs`, tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture_config::{parse_config, serialize_config, AppConfig};

    #[test]
    fn use_gpu_defaults_on_parses_and_defends() {
        assert!(TranscriptionConfig::default().use_gpu, "GPU defaults on");
        let cfg = parse_config(r#"{ "transcription": { "useGpu": false } }"#);
        assert!(!cfg.transcription.use_gpu);
        // Malformed value defaults only itself (hand-editable file).
        let cfg = parse_config(r#"{ "transcription": { "useGpu": "nope" } }"#);
        assert!(cfg.transcription.use_gpu);
        // Absent section → defaults.
        assert!(parse_config("{}").transcription.use_gpu);
    }

    #[test]
    fn transcription_section_round_trips_and_stays_minimal() {
        // Regression class: serialize_config once dropped a whole section
        // (mcp) — a capture save must never delete this one either.
        let mut cfg = AppConfig::default();
        cfg.transcription.use_gpu = false;
        let json = serialize_config(&cfg);
        assert!(json.contains("\"useGpu\": false"), "got: {json}");
        assert!(!parse_config(&json).transcription.use_gpu);
        // Default-on is omitted — the hand-editable file stays minimal.
        let json2 = serialize_config(&AppConfig::default());
        assert!(!json2.contains("transcription"), "got: {json2}");
    }
}
```

- [ ] **Step 2: Run to verify compile failure**

Run: `cd src-tauri/core && cargo test transcription_config`
Expected: COMPILE ERROR (module/struct missing).

- [ ] **Step 3: Implement**

`transcription_config.rs` (module doc: app-global because GPU is
machine-level, not per-vault; the split-module precedent):

```rust
//! App-global transcription settings (`config.json`'s `transcription`
//! section): machine-level knobs — today only the GPU escape hatch —
//! as opposed to the per-vault fields in `vault_config`. Split module,
//! same shape as `mcp_config`/`document_import_config`.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptionConfig {
    /// Ask whisper for GPU inference (Vulkan builds only; CPU fallback is
    /// whisper.cpp's own). Default on — the toggle exists as the escape
    /// hatch for buggy graphics drivers.
    pub use_gpu: bool,
}

impl Default for TranscriptionConfig {
    fn default() -> Self {
        Self { use_gpu: true }
    }
}

/// Per-field defensive parse — one malformed value defaults only itself.
pub fn parse_transcription_section(section: Option<&serde_json::Value>) -> TranscriptionConfig {
    let defaults = TranscriptionConfig::default();
    let Some(section) = section else {
        return defaults;
    };
    TranscriptionConfig {
        use_gpu: section
            .get("useGpu")
            .and_then(|v| v.as_bool())
            .unwrap_or(defaults.use_gpu),
    }
}

/// The section for `serialize_config` — None when everything is default,
/// so the hand-editable file stays minimal.
pub fn serialize_transcription_section(
    cfg: &TranscriptionConfig,
) -> Option<serde_json::Value> {
    if *cfg == TranscriptionConfig::default() {
        return None;
    }
    Some(serde_json::json!({ "useGpu": cfg.use_gpu }))
}
```

Thread through `capture_config.rs` exactly as the `mcp` section is threaded
(AppConfig field `pub transcription: TranscriptionConfig` + Default + the
`parse_config` read of `"transcription"` + the `serialize_config` emit), and
declare the module in `core/src/lib.rs` beside `mcp_config`.

- [ ] **Step 4: Verify**

Run: `cd src-tauri/core && cargo test && cargo clippy --all-targets -- -D warnings`
Then `npm run check:loc` (new file well under cap; capture_config grows a few lines).
Expected: green.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/core/src/transcription_config.rs src-tauri/core/src/lib.rs src-tauri/core/src/capture_config.rs
git commit -m "feat(core): app-global transcription config section (useGpu, default on)

Machine-level knob, not per-vault — the mcp_config split-module shape,
defensively parsed, emitted only when non-default, round-tripped by
serialize_config so no other section's save can delete it."
```

---

### Task G3: shell — IPC commands + worker cache keyed on `(tier, use_gpu)`

**Files:**
- Modify: `src-tauri/src/transcription.rs` (worker cache + load call site + a pure helper + tests)
- Modify: `src-tauri/src/capture_config_commands.rs` (the two new commands live beside the existing config commands — same file unless its LOC cap objects, then a sibling `transcription_config_commands.rs`)
- Modify: `src-tauri/src/lib.rs` (`generate_handler!` list)

**Interfaces:**
- Consumes: G1's `load(model, use_gpu)`, G2's `AppConfig.transcription`.
- Produces: IPC `get_transcription_config() -> TranscriptionConfigDto { useGpu: bool }`, `set_transcription_config(cfg: TranscriptionConfigDto)`; worker reload on flag flip — consumed by G5's UI.

- [ ] **Step 1: Failing test for the cache-key decision**

In `transcription.rs` tests:

```rust
    #[test]
    fn model_reloads_when_tier_or_gpu_changes() {
        // The cached transcriber must be reused ONLY when both the tier
        // and the GPU flag match — a toggle flip takes effect on the next
        // job without a restart (spec: cache key (tier, use_gpu)).
        assert!(!needs_reload(Some((ModelTier::Small, true)), ModelTier::Small, true));
        assert!(needs_reload(Some((ModelTier::Small, true)), ModelTier::Small, false));
        assert!(needs_reload(Some((ModelTier::Small, true)), ModelTier::Turbo, true));
        assert!(needs_reload(None, ModelTier::Small, true));
    }
```

- [ ] **Step 2: Run to verify it fails**

Run: `cd src-tauri && cargo test -p vault-buddy --lib model_reloads`
Expected: COMPILE ERROR — `needs_reload` missing.

- [ ] **Step 3: Implement**

In `transcription.rs`:

```rust
/// Reuse the cached transcriber only when BOTH cache-key elements match.
/// Pure so the (tier, use_gpu) contract is unit-tested; the worker loop
/// applies it below.
fn needs_reload(cached: Option<(ModelTier, bool)>, tier: ModelTier, use_gpu: bool) -> bool {
    cached != Some((tier, use_gpu))
}
```

Worker changes: `let mut loaded: Option<(ModelTier, bool, WhisperTranscriber)>`
(the `run_transcription` local); in `process_transcription`, read the flag
once per job —

```rust
    let use_gpu = capture_config::load_config().transcription.use_gpu;
```

(directly after the existing per-vault `cfg` load; same one-file read), gate
the reload with
`if needs_reload(loaded.as_ref().map(|(t, g, _)| (*t, *g)), tier, use_gpu)`,
store `*loaded = Some((tier, use_gpu, w))`, and pass
`WhisperTranscriber::load(&model, use_gpu)`. The transcriber borrow becomes
`&loaded.as_ref().unwrap().2`.

Commands (beside `get_capture_config`):

```rust
#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptionConfigDto {
    /// Ask whisper for GPU inference (Vulkan builds; CPU fallback is
    /// whisper.cpp's own). Applies from the next transcription.
    pub use_gpu: bool,
}

#[tauri::command]
pub fn get_transcription_config() -> TranscriptionConfigDto {
    TranscriptionConfigDto {
        use_gpu: capture_config::load_config().transcription.use_gpu,
    }
}

#[tauri::command]
pub fn set_transcription_config(
    lock: tauri::State<ConfigWriteLock>,
    cfg: TranscriptionConfigDto,
) -> Result<(), String> {
    let _guard = lock_ignoring_poison(&lock.0);
    // Read-modify-write INSIDE the lock, like every sibling writer, so a
    // concurrent vault-section save can't clobber this section or vice
    // versa.
    let mut app_cfg = capture_config::load_config();
    app_cfg.transcription.use_gpu = cfg.use_gpu;
    let result = capture_config::store_config(&app_cfg);
    if result.is_ok() {
        log::info!("transcription config saved: useGpu={}", cfg.use_gpu);
    }
    result
}
```

(Check the actual whole-config writer's name in `capture_config.rs` —
`update_vault_config` writes one vault; grep for the fn the mcp settings
commands use to persist an app-global section change and use THAT, e.g.
`store_config`/`save_config`. Mirror `mcp_commands.rs`'s write path exactly.)

Register both commands in `lib.rs`'s `generate_handler!`.

- [ ] **Step 4: Verify**

Run: `cd src-tauri && cargo test -p vault-buddy --lib && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --check` (needs the built `dist/` from earlier; rebuild via `npm run build` if the container recycled). Then `npm run check:loc` (ratchet transcription.rs's entry if tripped: append `+(tier,use_gpu) cache key, needs_reload, GPU config commands (GPU increment)`).
Expected: green.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/transcription.rs src-tauri/src/capture_config_commands.rs src-tauri/src/lib.rs scripts/loc-baseline.json
git commit -m "feat(shell): GPU config IPC + model cache keyed on (tier, use_gpu)

A toggle flip reloads the whisper context on the next job — no restart.
Sync commands under ConfigWriteLock, read-modify-write inside the lock so
section writers can't clobber each other."
```

---

### Task G4: `gpu` shell feature + Vulkan SDK in the two Windows jobs

**Files:**
- Modify: `src-tauri/Cargo.toml` (shell `[features]`)
- Modify: `.github/workflows/ci.yml` (`windows-app` job)
- Modify: `.github/workflows/release.yml` (`windows-installer` job)

**Interfaces:**
- Consumes: G1's `whisper-vulkan`.
- Produces: shipped Windows builds are GPU-capable; nothing else changes.

- [ ] **Step 1: Shell feature**

In `src-tauri/Cargo.toml` (add a `[features]` table if absent; check first — Tauri templates usually have one with `custom-protocol`):

```toml
# GPU (Vulkan) inference. Enabled ONLY by the Windows CI/release builds —
# local builds stay CPU-only so contributors never need the Vulkan SDK.
gpu = ["vault_buddy_transcribe/whisper-vulkan"]
```

- [ ] **Step 2: Resolve and pin the SDK**

Fetch `https://vulkan.lunarg.com/sdk/latest/windows.json` (curl) → version, then
`https://vulkan.lunarg.com/sdk/sha256/<version>/windows/vulkansdk-windows-X64-<version>.exe.txt` → SHA-256. Record both; they go verbatim into BOTH workflows.

- [ ] **Step 3: ci.yml `windows-app`**

Insert before the `Build Tauri app and installers` step:

```yaml
      # GPU builds: whisper.cpp's Vulkan backend needs the SDK's headers +
      # glslc at compile time. Pinned version + SHA-256 (the same pinned-
      # download discipline as the app's own model downloads); ~2 min
      # install per run, accepted — revisit with a cache if it hurts.
      - name: Install Vulkan SDK (pinned)
        shell: bash
        run: |
          VK_VERSION="<version>"
          VK_SHA256="<sha256>"
          curl -fsSL -o vulkan-sdk.exe "https://sdk.lunarg.com/sdk/download/${VK_VERSION}/windows/vulkansdk-windows-X64-${VK_VERSION}.exe"
          echo "${VK_SHA256}  vulkan-sdk.exe" | sha256sum -c -
          ./vulkan-sdk.exe --accept-licenses --default-answer --confirm-command install
          echo "VULKAN_SDK=C:\\VulkanSDK\\${VK_VERSION}" >> "$GITHUB_ENV"
```

and add `--features gpu` to BOTH build invocations in that job's run block:

```bash
            npx tauri build --features gpu --config '{"bundle":{"createUpdaterArtifacts":false}}'
          else
            npx tauri build --features gpu
```

The post-build `cargo test` steps (GAP-43) stay CPU-only — do not add the
feature there (tests would need a GPU to mean anything; the build IS the
vulkan compile gate).

- [ ] **Step 4: release.yml `windows-installer`**

Same SDK step before the tauri-action step, plus in the action's `with:`:

```yaml
          args: --features gpu
```

- [ ] **Step 5: Verify (local = lint only; CI proves it)**

Workflow YAML has no local runner; verify: `node -e "require('js-yaml')"` is
NOT a dev dep — instead run `npx --yes yaml-lint .github/workflows/ci.yml .github/workflows/release.yml` or fall back to `python3 -c "import yaml,sys;[yaml.safe_load(open(f)) for f in sys.argv[1:]]" .github/workflows/ci.yml .github/workflows/release.yml`.
Also: `cd src-tauri && cargo metadata --format-version 1 --no-deps | grep -q '"gpu"' && echo FEATURE-OK` (proves the feature parses; it can't build here).
Expected: YAML parses; FEATURE-OK.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/Cargo.toml .github/workflows/ci.yml .github/workflows/release.yml
git commit -m "ci(windows): pinned Vulkan SDK + gpu feature on the shipped builds

Shipped Windows builds compile whisper.cpp's Vulkan backend; every other
job and local builds stay CPU-only and SDK-free. SDK download pinned by
version + SHA-256 — the same discipline as the app's model downloads."
```

---

### Task G5: frontend — Transcription card in Buddy settings

**Files:**
- Create: `src/components/TranscriptionAppSettings.vue`
- Modify: `src/components/BuddySettings.vue` (import + mount beside DocumentImportSettings/McpSettings — read the file's tab structure and place it in the same tab/section as those integration-style cards)
- Modify: `src/types.ts` (add `TranscriptionAppConfig`)
- Test: `tests/transcription-app-settings.test.ts`

**Interfaces:**
- Consumes: G3's commands.
- Produces: the visible toggle.

- [ ] **Step 1: Failing tests**

`tests/transcription-app-settings.test.ts` (mirror the structure of an
existing self-contained settings test — read `tests/` for the
DocumentImportSettings or McpSettings test file and copy its mockIPC set-up
idiom):

```ts
import { flushPromises, mount } from "@vue/test-utils";
import { mockIPC } from "@tauri-apps/api/mocks";
import { afterEach, describe, expect, it, vi } from "vitest";

import TranscriptionAppSettings from "../src/components/TranscriptionAppSettings.vue";

let active: ReturnType<typeof mount> | null = null;
afterEach(() => {
  active?.unmount();
  active = null;
  vi.clearAllMocks();
});

function mountWith(useGpu = true, failSave = false) {
  const calls: { cmd: string; payload?: unknown }[] = [];
  mockIPC((cmd, payload) => {
    calls.push({ cmd, payload });
    if (cmd === "get_transcription_config") return { useGpu };
    if (cmd === "set_transcription_config") {
      if (failSave) throw new Error("disk full");
      return null;
    }
    return null;
  });
  active = mount(TranscriptionAppSettings, { attachTo: document.body });
  return { wrapper: active, calls };
}

describe("TranscriptionAppSettings", () => {
  it("loads the app-global setting on mount and renders the toggle", async () => {
    const { wrapper } = mountWith(false);
    await flushPromises();
    expect(
      wrapper.get<HTMLInputElement>('[data-testid="use-gpu-toggle"]').element.checked,
    ).toBe(false);
  });

  it("saves on toggle with the camelCase payload", async () => {
    const { wrapper, calls } = mountWith(true);
    await flushPromises();
    await wrapper.get('[data-testid="use-gpu-toggle"]').setValue(false);
    await flushPromises();
    const save = calls.find((c) => c.cmd === "set_transcription_config");
    expect(save?.payload).toEqual({ cfg: { useGpu: false } });
  });

  it("reverts the toggle and surfaces an error when the save fails", async () => {
    const { wrapper } = mountWith(true, true);
    await flushPromises();
    await wrapper.get('[data-testid="use-gpu-toggle"]').setValue(false);
    await flushPromises();
    expect(
      wrapper.get<HTMLInputElement>('[data-testid="use-gpu-toggle"]').element.checked,
    ).toBe(true);
    expect(wrapper.get('[data-testid="use-gpu-error"]').text()).toContain("disk full");
  });
});
```

- [ ] **Step 2: Run to verify they fail**

Run: `npx vitest run tests/transcription-app-settings.test.ts`
Expected: FAIL (component missing).

- [ ] **Step 3: Implement**

`src/types.ts`:

```ts
/** App-global transcription settings (machine-level; per-vault knobs live
 * in CaptureConfig). */
export interface TranscriptionAppConfig {
  /** Ask whisper for GPU inference (Vulkan builds; CPU fallback is
   * automatic). Applies from the next transcription. */
  useGpu: boolean;
}
```

`TranscriptionAppSettings.vue` — self-contained (the DocumentImportSettings
idiom: load on mount, optimistic toggle with revert + inline error; match the
Buddy-settings card markup conventions you find in that file):

```vue
<script setup lang="ts">
import { invoke } from "@tauri-apps/api/core";
import { onMounted, ref } from "vue";

import { logWarning } from "../logging";
import type { TranscriptionAppConfig } from "../types";

// App-global GPU escape hatch. Optimistic with revert-on-failure (the
// autostart-toggle pattern in BuddySettings); busy disables the checkbox
// so two writes can't race.
const useGpu = ref<boolean | null>(null); // null = load pending/failed
const busy = ref(false);
const error = ref<string | null>(null);

onMounted(async () => {
  try {
    const cfg = await invoke<TranscriptionAppConfig>("get_transcription_config");
    useGpu.value = cfg.useGpu;
  } catch (e) {
    error.value = String(e);
    logWarning(`get_transcription_config failed: ${String(e)}`);
  }
});

async function toggle(event: Event) {
  const enabled = (event.target as HTMLInputElement).checked;
  const previous = useGpu.value;
  useGpu.value = enabled;
  busy.value = true;
  error.value = null;
  try {
    await invoke("set_transcription_config", { cfg: { useGpu: enabled } });
  } catch (e) {
    useGpu.value = previous;
    error.value = String(e);
    logWarning(`set_transcription_config failed: ${String(e)}`);
  } finally {
    busy.value = false;
  }
}
</script>

<template>
  <section class="flex flex-col gap-1.5">
    <div class="flex items-center justify-between">
      <label
        for="transcription-use-gpu"
        class="text-sm text-slate-200"
      >
        Use GPU (Vulkan)
        <span class="block text-xs text-slate-500">
          Applies from the next transcription. Falls back to CPU when no
          compatible GPU is found — turn off if you hit graphics-driver
          crashes.
        </span>
      </label>
      <input
        id="transcription-use-gpu"
        data-testid="use-gpu-toggle"
        type="checkbox"
        class="h-4 w-4 accent-violet-500"
        :checked="useGpu === true"
        :disabled="useGpu === null || busy"
        @change="toggle"
      >
    </div>
    <p
      v-if="error"
      data-testid="use-gpu-error"
      class="text-xs text-red-300"
    >
      {{ error }}
    </p>
  </section>
</template>
```

Mount in `BuddySettings.vue` as a **Transcription** card beside the
DocumentImportSettings card (same heading/card markup as its siblings — copy
the surrounding structure exactly).

- [ ] **Step 4: Verify**

Run: `npx vitest run && npm run build && npm run lint`
Expected: green (fix any BuddySettings test that snapshots/queries the tab content — extend, don't weaken).

- [ ] **Step 5: Commit**

```bash
git add src/components/TranscriptionAppSettings.vue src/components/BuddySettings.vue src/types.ts tests/transcription-app-settings.test.ts
git commit -m "feat(ui): app-global Use GPU (Vulkan) toggle in Buddy settings

Optimistic with revert + inline error (the autostart pattern); helper copy
is honest about CPU fallback and the next-transcription effect."
```

---

### Task G6: documentation

**Files:**
- Modify: `AGENTS.md` (IPC table row for the two commands; transcription domain section — GPU paragraph incl. the use_gpu-default-false gotcha and the deliberate absence of a device row; `What compiles where` note that `whisper-vulkan`/`gpu` compile only on the Windows CI jobs; Commands section — the Windows GPU build invocation)
- Modify: `docs/DEVELOPMENT.md` (contributor note: plain builds need no Vulkan SDK; `--features gpu` + the pinned SDK version do; where the config key lives)
- Modify: `docs/Gaps.md` (new entry: in-process GPU driver fault can crash the app; toggle is the remedy, sidecar-process architecture the documented future fix)

Write each insertion to match the surrounding format; every factual claim must
match the shipped code (event names, file names, defaults). Commit:

```bash
git add AGENTS.md docs/DEVELOPMENT.md docs/Gaps.md
git commit -m "docs: GPU (Vulkan) transcription — agent guide, dev setup, driver-crash gap"
```

---

### Task G7: full gates, push, PR body update

- [ ] Rust: fmt --check (workspace), core+transcribe clippy/tests (default AND `--features whisper`), workspace clippy, shell `--lib` tests, `cargo machete`, `cargo deny check` (Cargo.toml changed this increment — deny MUST run now even though earlier tasks skipped it).
- [ ] Frontend, CI order: `npm run lint && npm run check:loc && rm -rf coverage && npm run check:quality && npm run test:coverage && npm run build`.
- [ ] Push: `git push -u origin claude/whisper-transcription-features-0njd72` (retry ×4 with 2/4/8/16 s backoff on network failure only).
- [ ] Update PR #61's body (github MCP `update_pull_request`): append a `## GPU (Vulkan) increment` section — feature chain, use_gpu-default-false fix, app-global toggle, SDK pinning, the no-device-row honesty note, and the manual Windows validation line (device log line + speed delta on Turbo). The controller does this step if the implementer lacks the MCP tools.
- [ ] Watch the next `windows-app` CI run — it is the FIRST compile of whisper.cpp's Vulkan backend and the SDK install step; treat its failure as this increment's own bug until proven otherwise.
