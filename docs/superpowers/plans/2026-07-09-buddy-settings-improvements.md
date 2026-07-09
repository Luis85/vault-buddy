# Buddy Settings Improvements Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Regroup the Buddy settings view into carded sections, add a character motion-preview + selected badge, a message-duration setting wired to the live bubble window, and a Start-with-Windows autostart toggle.

**Architecture:** Frontend-heavy Vue/Pinia work (settings store field, bubble duration tiers, BuddySettings layout) plus one small Rust addition: `tauri-plugin-autostart` wrapped in two custom commands. Spec: `docs/superpowers/specs/2026-07-09-buddy-settings-improvements-design.md`.

**Tech Stack:** Vue 3 + Pinia + Tailwind 4, Vitest (happy-dom, `mockIPC`), Tauri v2 (Rust shell crate).

## Global Constraints

- TDD: every step pair is failing-test-first, then minimal code (repo convention, `.claude/skills`).
- Commits: Conventional Commits, imperative subject, body says *why*; committer identity `Claude <noreply@anthropic.com>` (run `git config user.email noreply@anthropic.com && git config user.name Claude` once before committing).
- Every commit message ends with the two trailer lines already used on this branch (`Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>` and the `Claude-Session:` link) — copy them from `git log -1 --format=%B`.
- `tauri-plugin-single-instance` must stay the FIRST plugin registered in `src-tauri/src/lib.rs` (repo invariant).
- Default message duration `"normal"` must preserve today's exact timings: ack 3200 ms, greeting 5000 ms.
- No swallowed errors: every caught failure logs via `logWarning` (frontend) or `log::warn!/error!` (Rust), user-facing failures surface in the UI.
- Existing element ids `#animations-toggle`, `#dragging-toggle`, `#messages-toggle` and the `.character-option` class must survive the relayout (tests depend on them).
- Test commands: `npx vitest run tests/<file>.test.ts`; full gates: `npm test`, `npm run build`, `cd src-tauri && cargo fmt --check`.

---

### Task 1: `messageDuration` in the settings store

**Files:**
- Modify: `src/stores/settings.ts`
- Test: `tests/settings-store.test.ts`

**Interfaces:**
- Produces: `export type MessageDuration = "short" | "normal" | "long"`; store state `messageDuration: MessageDuration`; action `setMessageDuration(d: MessageDuration)`; `syncFromStorage()` re-reads it; localStorage key `vault-buddy.messageDuration`.

- [ ] **Step 1: Write the failing tests** — append inside the `describe` in `tests/settings-store.test.ts`:

```ts
  it("defaults message duration to normal", () => {
    expect(useSettingsStore().messageDuration).toBe("normal");
  });

  it("persists the message duration across store instances", () => {
    useSettingsStore().setMessageDuration("long");
    setActivePinia(createPinia());
    expect(useSettingsStore().messageDuration).toBe("long");
    expect(localStorage.getItem("vault-buddy.messageDuration")).toBe("long");
  });

  it("falls back to normal for an unknown stored duration", () => {
    localStorage.setItem("vault-buddy.messageDuration", "eternal");
    expect(useSettingsStore().messageDuration).toBe("normal");
  });

  it("re-reads the message duration when another window changes it", () => {
    const store = useSettingsStore();
    localStorage.setItem("vault-buddy.messageDuration", "short");
    store.syncFromStorage();
    expect(store.messageDuration).toBe("short");
  });
```

- [ ] **Step 2: Run to verify they fail**

Run: `npx vitest run tests/settings-store.test.ts`
Expected: 4 FAIL — `messageDuration` is `undefined` / `setMessageDuration is not a function`.

- [ ] **Step 3: Implement** in `src/stores/settings.ts`:

Add below `const MESSAGES_KEY … ;`:

```ts
const MESSAGE_DURATION_KEY = "vault-buddy.messageDuration";

/** How long the buddy's speech bubbles stay up (bubble tiers live in
 * useBuddyBubble's BUBBLE_MS map). */
export type MessageDuration = "short" | "normal" | "long";

// unknown/stale stored values fall back to normal — the getCharacter pattern
function normalizeDuration(value: string | null): MessageDuration {
  return value === "short" || value === "long" ? value : "normal";
}
```

Add to `state`: `messageDuration: normalizeDuration(localStorage.getItem(MESSAGE_DURATION_KEY)),`

Add action:

```ts
    setMessageDuration(duration: MessageDuration) {
      this.messageDuration = normalizeDuration(duration);
      localStorage.setItem(MESSAGE_DURATION_KEY, this.messageDuration);
    },
```

Add to `syncFromStorage()`:

```ts
      this.messageDuration = normalizeDuration(
        localStorage.getItem(MESSAGE_DURATION_KEY),
      );
```

- [ ] **Step 4: Run to verify green**

Run: `npx vitest run tests/settings-store.test.ts` — all pass.

- [ ] **Step 5: Commit**

```bash
git add src/stores/settings.ts tests/settings-store.test.ts
git commit -m "feat(ui): messageDuration setting in the settings store"
```

---

### Task 2: bubble duration tiers, wired to the live bubble window

**Files:**
- Modify: `src/composables/useBuddyBubble.ts`, `src/roots/BubbleRoot.vue`
- Test: `tests/use-buddy-bubble.test.ts` (and keep `tests/bubble-root.test.ts` green — it already runs with Pinia)

**Interfaces:**
- Consumes: `useSettingsStore().messageDuration`, `setMessageDuration` (Task 1).
- Produces: `export const BUBBLE_MS: Record<MessageDuration, { ack: number; greeting: number }>` replacing the exported `GREETING_MS`/`ACK_MS` constants. `useBuddyBubble()` signature unchanged.

- [ ] **Step 1: Update/extend the tests.** In `tests/use-buddy-bubble.test.ts`: add Pinia (the composable now reads the settings store), replace the constant imports with `BUBBLE_MS`, and add tier coverage:

```ts
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount } from "@vue/test-utils";
import { defineComponent } from "vue";
import { createPinia, setActivePinia } from "pinia";
import { useBuddyBubble, BUBBLE_MS } from "../src/composables/useBuddyBubble";
```

In `beforeEach`: `localStorage.clear(); setActivePinia(createPinia()); vi.useFakeTimers();`

Replace every `GREETING_MS` with `BUBBLE_MS.normal.greeting` and every `ACK_MS` with `BUBBLE_MS.normal.ack`. Append:

```ts
  it("normal preserves today's exact timings", () => {
    expect(BUBBLE_MS.normal).toEqual({ ack: 3200, greeting: 5000 });
  });

  it("greeting uses the configured duration tier", () => {
    localStorage.setItem("vault-buddy.messageDuration", "long");
    const wrapper = mount(Host);
    vi.advanceTimersByTime(BUBBLE_MS.normal.greeting);
    expect(wrapper.vm.visible).toBe(true); // long outlives the normal timing
    vi.advanceTimersByTime(BUBBLE_MS.long.greeting - BUBBLE_MS.normal.greeting);
    expect(wrapper.vm.visible).toBe(false);
  });
```

- [ ] **Step 2: Run to verify failure**

Run: `npx vitest run tests/use-buddy-bubble.test.ts`
Expected: FAIL — `BUBBLE_MS` is not exported.

- [ ] **Step 3: Implement `useBuddyBubble.ts`.** Replace the two constants and the mount call:

```ts
import { onMounted, onUnmounted, ref, type Ref } from "vue";
import { greetingFor } from "../greeting";
import { useSettingsStore, type MessageDuration } from "../stores/settings";

// How long a message stays before auto-dismissing, per the user's
// messageDuration setting. Acks are quicker than the launch greeting so a
// burst of them never piles up; `normal` is the pre-setting behavior.
export const BUBBLE_MS: Record<MessageDuration, { ack: number; greeting: number }> = {
  short: { ack: 2000, greeting: 3000 },
  normal: { ack: 3200, greeting: 5000 },
  long: { ack: 6000, greeting: 9000 },
};
```

Inside `useBuddyBubble()` add `const settings = useSettingsStore();` and change the mount line to:

```ts
  onMounted(() => show(greetingFor(new Date()), BUBBLE_MS[settings.messageDuration].greeting));
```

- [ ] **Step 4: Wire `BubbleRoot.vue`.** The bubble is its own webview with its own Pinia — without the storage sync a duration picked in the panel never reaches it. Add imports:

```ts
import { useBuddyBubble, BUBBLE_MS } from "../composables/useBuddyBubble";
import { useSettingsStore } from "../stores/settings";
import { useSettingsStorageSync } from "../composables/useSettingsStorageSync";
```

After `useSuppressContextMenu();` add:

```ts
// Duration changes made in the panel arrive via the storage event — resolve
// the tier at each show, never at listener-registration time.
const settings = useSettingsStore();
useSettingsStorageSync();
```

Change the `bubble-message` listener callback to:

```ts
      (event) => show(event.payload.text, BUBBLE_MS[settings.messageDuration].ack),
```

- [ ] **Step 5: Verify green**

Run: `npx vitest run tests/use-buddy-bubble.test.ts tests/bubble-root.test.ts tests/greeting.test.ts` — all pass.

- [ ] **Step 6: Commit**

```bash
git add src/composables/useBuddyBubble.ts src/roots/BubbleRoot.vue tests/use-buddy-bubble.test.ts
git commit -m "feat(ui): message-duration tiers drive bubble auto-dismiss"
```

---

### Task 3: Behavior card + Message duration row in BuddySettings

**Files:**
- Modify: `src/components/BuddySettings.vue`
- Test: `tests/buddy-settings.test.ts`

**Interfaces:**
- Consumes: `settings.messageDuration` / `setMessageDuration` (Task 1); `SelectMenu.vue` (`modelValue`, `options: {value,label}[]`, `id`, `data-testid`).
- Produces: Behavior section header + card wrapping the three existing toggle rows (ids preserved) + a `message-duration-select` SelectMenu row.

- [ ] **Step 1: Write the failing tests** — append to `tests/buddy-settings.test.ts` (mount with `attachTo: document.body` for the Teleported SelectMenu popup, and unmount/clean in the test):

```ts
  it("groups the toggles under a Behavior card with a message-duration select", async () => {
    const wrapper = mount(BuddySettings, { attachTo: document.body });
    expect(wrapper.text()).toContain("Behavior");
    expect(wrapper.find('[data-testid="message-duration-select"]').exists()).toBe(true);
    // the three toggles keep their ids inside the card
    for (const id of ["#animations-toggle", "#dragging-toggle", "#messages-toggle"]) {
      expect(wrapper.find(id).exists()).toBe(true);
    }
    wrapper.unmount();
    document.body.innerHTML = "";
  });

  it("picking a message duration persists it to the store", async () => {
    const wrapper = mount(BuddySettings, { attachTo: document.body });
    await wrapper.get('[data-testid="message-duration-select"]').trigger("click");
    (document.body.querySelector(
      '[data-testid="message-duration-select-option-long"]',
    ) as HTMLElement).click();
    await flush();
    expect(useSettingsStore().messageDuration).toBe("long");
    expect(localStorage.getItem("vault-buddy.messageDuration")).toBe("long");
    wrapper.unmount();
    document.body.innerHTML = "";
  });
```

(`SelectMenu` renders options with `data-testid="<testid>-option-<value>"` — the CaptureSettings tests use the same helper pattern.)

- [ ] **Step 2: Run to verify failure**

Run: `npx vitest run tests/buddy-settings.test.ts`
Expected: 2 FAIL — no "Behavior" text, no `message-duration-select`.

- [ ] **Step 3: Implement.** In `src/components/BuddySettings.vue` script, add:

```ts
import { computed } from "vue";
import SelectMenu from "./SelectMenu.vue";
import type { MessageDuration } from "../stores/settings";

const DURATION_OPTIONS = [
  { value: "short", label: "Short" },
  { value: "normal", label: "Normal" },
  { value: "long", label: "Long" },
] as const;

const messageDuration = computed({
  get: () => settings.messageDuration,
  set: (v: string | number) => settings.setMessageDuration(v as MessageDuration),
});
```

In the template, replace the three top-level toggle `<section>`s with one Behavior section (rows keep their exact label/input markup and ids, only the wrapper changes):

```html
    <section>
      <h2 class="mb-1.5 text-xs font-semibold uppercase tracking-wide text-slate-400">
        Behavior
      </h2>
      <div class="flex flex-col gap-3 rounded-xl border border-white/10 bg-white/5 p-2">
        <div class="flex items-center justify-between">
          <label for="animations-toggle" class="text-sm text-slate-200">
            Animations
          </label>
          <input
            id="animations-toggle"
            type="checkbox"
            class="h-4 w-4 accent-violet-500"
            :checked="settings.animationsEnabled"
            @change="settings.toggleAnimations()"
          />
        </div>
        <div class="flex items-center justify-between">
          <label for="dragging-toggle" class="text-sm text-slate-200">
            Dragging
            <span class="block text-xs text-slate-500">
              Off pins the buddy in place
            </span>
          </label>
          <input
            id="dragging-toggle"
            type="checkbox"
            class="h-4 w-4 accent-violet-500"
            :checked="settings.draggingEnabled"
            @change="settings.toggleDragging()"
          />
        </div>
        <div class="flex items-center justify-between">
          <label for="messages-toggle" class="text-sm text-slate-200">
            Buddy messages
            <span class="block text-xs text-slate-500">
              The buddy comments on what you do
            </span>
          </label>
          <input
            id="messages-toggle"
            type="checkbox"
            class="h-4 w-4 accent-violet-500"
            :checked="settings.buddyMessagesEnabled"
            @change="settings.toggleBuddyMessages()"
          />
        </div>
        <div class="flex items-center justify-between gap-2">
          <label for="message-duration" class="text-sm text-slate-200">
            Message duration
            <span class="block text-xs text-slate-500">
              How long the buddy's bubbles stay up
            </span>
          </label>
          <SelectMenu
            id="message-duration"
            v-model="messageDuration"
            :options="DURATION_OPTIONS"
            data-testid="message-duration-select"
          />
        </div>
      </div>
    </section>
```

- [ ] **Step 4: Verify green**

Run: `npx vitest run tests/buddy-settings.test.ts` — all pass (old toggle tests included).

- [ ] **Step 5: Commit**

```bash
git add src/components/BuddySettings.vue tests/buddy-settings.test.ts
git commit -m "feat(ui): Behavior card with message-duration select in Buddy settings"
```

---

### Task 4: character motion-preview + selected badge

**Files:**
- Modify: `src/components/BuddySettings.vue`
- Test: `tests/buddy-settings.test.ts`

**Interfaces:**
- Consumes: `BuddyAvatar` props `working: boolean`, `animated: boolean` (existing).
- Produces: card-level `previewId` hover/focus state; `data-testid="selected-badge"` on the selected card.

- [ ] **Step 1: Write the failing tests** — append:

```ts
  it("previews a character's motion on hover and stops on leave", async () => {
    const wrapper = mount(BuddySettings);
    const knight = wrapper.get('[aria-label="Choose Knight"]');
    await knight.trigger("pointerenter");
    // BuddyAvatar renders the run loop via the .running class on its sheet
    expect(knight.find(".sheet").classes()).toContain("running");
    await knight.trigger("pointerleave");
    expect(knight.find(".sheet").classes()).not.toContain("running");
  });

  it("does not preview while animations are off", async () => {
    useSettingsStore().toggleAnimations(); // off
    const wrapper = mount(BuddySettings);
    const knight = wrapper.get('[aria-label="Choose Knight"]');
    await knight.trigger("pointerenter");
    expect(knight.find(".sheet").classes()).not.toContain("running");
  });

  it("marks the selected character with a badge", async () => {
    const wrapper = mount(BuddySettings);
    expect(
      wrapper.get('[aria-label="Choose Classic"]').find('[data-testid="selected-badge"]').exists(),
    ).toBe(true);
    expect(
      wrapper.get('[aria-label="Choose Knight"]').find('[data-testid="selected-badge"]').exists(),
    ).toBe(false);
    await wrapper.get('[aria-label="Choose Knight"]').trigger("click");
    expect(
      wrapper.get('[aria-label="Choose Knight"]').find('[data-testid="selected-badge"]').exists(),
    ).toBe(true);
  });
```

- [ ] **Step 2: Run to verify failure**

Run: `npx vitest run tests/buddy-settings.test.ts`
Expected: 3 FAIL — no `running` class on hover, no `selected-badge`.

- [ ] **Step 3: Implement.** Script: add `import { ref } from "vue";` (merge with the existing vue import) and:

```ts
// Card under the pointer/focus — its avatar plays the run loop as a preview.
// Gated on animationsEnabled so animations-off also silences previews.
const previewId = ref<string | null>(null);
```

Character button: add `relative` to the class list, the preview handlers, the badge, and the `working` prop:

```html
        <button
          v-for="c in CHARACTERS"
          :key="c.id"
          type="button"
          role="radio"
          class="character-option relative flex cursor-pointer flex-col items-center rounded-xl border p-1.5 transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400"
          :class="
            settings.character === c.id
              ? 'border-violet-400 bg-violet-500/20'
              : 'border-white/10 bg-white/5 hover:bg-white/10'
          "
          :aria-checked="settings.character === c.id"
          :aria-label="`Choose ${c.name}`"
          @click="settings.setCharacter(c.id)"
          @pointerenter="previewId = c.id"
          @pointerleave="previewId = null"
          @focusin="previewId = c.id"
          @focusout="previewId = null"
        >
          <span
            v-if="settings.character === c.id"
            data-testid="selected-badge"
            class="absolute right-1 top-1 flex h-3.5 w-3.5 items-center justify-center rounded-full bg-violet-500 text-[9px] font-bold text-white"
            aria-hidden="true"
            >✓</span
          >
          <BuddyAvatar
            :character-id="c.id"
            :animated="settings.animationsEnabled"
            :working="previewId === c.id && settings.animationsEnabled"
          />
          <span class="text-xs text-slate-200">{{ c.name }}</span>
        </button>
```

- [ ] **Step 4: Verify green**

Run: `npx vitest run tests/buddy-settings.test.ts` — all pass.

- [ ] **Step 5: Commit**

```bash
git add src/components/BuddySettings.vue tests/buddy-settings.test.ts
git commit -m "feat(ui): character motion-preview on hover + selected badge"
```

---

### Task 5: Rust autostart plugin + commands

**Files:**
- Modify: `src-tauri/Cargo.toml`, `src-tauri/src/lib.rs`, `src-tauri/src/commands.rs`, `src-tauri/Cargo.lock` (regenerated)

**Interfaces:**
- Produces: IPC commands `get_autostart() -> Result<bool, String>` and `set_autostart(enabled: bool) -> Result<(), String>` (camelCase arg `enabled` on the JS side).

There is no Linux-runnable unit test for these thin plugin wrappers — the verification here is the compile gate (this is the repo's documented gate for shell-crate changes).

- [ ] **Step 1: Add the dependency.** In `src-tauri/Cargo.toml` `[dependencies]`, after `tauri-plugin-notification = "2"`:

```toml
tauri-plugin-autostart = "2"
```

- [ ] **Step 2: Register the plugin.** In `src-tauri/src/lib.rs`, directly after `.plugin(tauri_plugin_process::init())` (single-instance stays FIRST — do not touch the plugin order above it):

```rust
        // Launch-at-login registration, surfaced in Buddy settings via the
        // get_autostart/set_autostart commands (registry-backed on Windows).
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
```

- [ ] **Step 3: Add the commands.** At the end of `src-tauri/src/commands.rs`:

```rust
/// Whether the app is registered to start at login. OS-owned state (the
/// registry on Windows) — read fresh by the settings view on mount, never
/// cached app-side, so the UI always reflects what the OS will actually do.
#[tauri::command]
pub fn get_autostart(app: tauri::AppHandle) -> Result<bool, String> {
    use tauri_plugin_autostart::ManagerExt;
    app.autolaunch().is_enabled().map_err(|e| e.to_string())
}

/// Register/unregister launch-at-login. Logged like every other
/// user-initiated config change (audit trail).
#[tauri::command]
pub fn set_autostart(app: tauri::AppHandle, enabled: bool) -> Result<(), String> {
    use tauri_plugin_autostart::ManagerExt;
    let launcher = app.autolaunch();
    let result = if enabled {
        launcher.enable()
    } else {
        launcher.disable()
    };
    match result {
        Ok(()) => {
            log::info!(
                "autostart {}",
                if enabled { "enabled" } else { "disabled" }
            );
            Ok(())
        }
        Err(e) => Err(e.to_string()),
    }
}
```

- [ ] **Step 4: Register the commands.** In `src-tauri/src/lib.rs`'s `generate_handler![…]`, after `commands::rearm_crash_detection,`:

```rust
            commands::get_autostart,
            commands::set_autostart,
```

- [ ] **Step 5: Format + compile gate**

```bash
cd src-tauri && cargo fmt && cargo fmt --check && cd ..
npm run setup:linux   # once per container — installs WebView/GTK/tray system libs
npx tauri build --no-bundle
```

Expected: build completes (compile gate only; Windows CI is the behavior gate). This also regenerates `src-tauri/Cargo.lock` with the new dependency.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/src/lib.rs src-tauri/src/commands.rs
git commit -m "feat(shell): autostart plugin behind get_autostart/set_autostart"
```

---

### Task 6: System card with the Start-with-Windows toggle

**Files:**
- Modify: `src/components/BuddySettings.vue`
- Test: `tests/buddy-settings.test.ts`

**Interfaces:**
- Consumes: `get_autostart` / `set_autostart` (Task 5) via `invoke`; `logWarning` from `src/logging.ts`.

- [ ] **Step 1: Write the failing tests.** Add imports at the top of `tests/buddy-settings.test.ts`:

```ts
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
```

Mock the logging module beside the existing `vi.mock` calls (required so the no-swallowed-errors breadcrumbs don't hit the real bridge):

```ts
vi.mock("../src/logging", () => ({ logWarning: vi.fn(), logBreadcrumb: vi.fn() }));
```

Append the tests:

```ts
  it("loads the OS autostart state into the System card", async () => {
    mockIPC((cmd) => {
      if (cmd === "get_autostart") return true;
    });
    const wrapper = mount(BuddySettings);
    await flush();
    expect(wrapper.text()).toContain("System");
    const toggle = wrapper.get<HTMLInputElement>('[data-testid="autostart-toggle"]');
    expect(toggle.element.checked).toBe(true);
    expect(toggle.element.disabled).toBe(false);
    clearMocks();
  });

  it("disables the autostart toggle and shows the error when the read fails", async () => {
    mockIPC((cmd) => {
      if (cmd === "get_autostart") throw new Error("registry unavailable");
    });
    const wrapper = mount(BuddySettings);
    await flush();
    const toggle = wrapper.get<HTMLInputElement>('[data-testid="autostart-toggle"]');
    expect(toggle.element.disabled).toBe(true);
    expect(wrapper.get('[data-testid="autostart-error"]').text()).toContain(
      "registry unavailable",
    );
    clearMocks();
  });

  it("toggling autostart invokes set_autostart", async () => {
    const calls: Array<{ cmd: string; args: unknown }> = [];
    mockIPC((cmd, args) => {
      calls.push({ cmd, args });
      if (cmd === "get_autostart") return false;
    });
    const wrapper = mount(BuddySettings);
    await flush();
    await wrapper.get('[data-testid="autostart-toggle"]').setValue(true);
    await flush();
    expect(calls.find((c) => c.cmd === "set_autostart")?.args).toEqual({
      enabled: true,
    });
    clearMocks();
  });

  it("reverts the autostart toggle and shows the error when the write fails", async () => {
    mockIPC((cmd) => {
      if (cmd === "get_autostart") return false;
      if (cmd === "set_autostart") throw new Error("access denied");
    });
    const wrapper = mount(BuddySettings);
    await flush();
    const toggle = wrapper.get<HTMLInputElement>('[data-testid="autostart-toggle"]');
    await wrapper.get('[data-testid="autostart-toggle"]').setValue(true);
    await flush();
    expect(toggle.element.checked).toBe(false); // reverted
    expect(wrapper.get('[data-testid="autostart-error"]').text()).toContain(
      "access denied",
    );
    clearMocks();
  });
```

- [ ] **Step 2: Run to verify failure**

Run: `npx vitest run tests/buddy-settings.test.ts`
Expected: 4 FAIL — no `autostart-toggle` testid.

- [ ] **Step 3: Implement.** Script additions in `BuddySettings.vue` (merge imports):

```ts
import { onMounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { logWarning } from "../logging";

// OS-owned state (the registry on Windows): read fresh on mount, never
// stored in localStorage/the settings store. null = unknown (read pending
// or failed) — the toggle stays disabled so it can't write blind.
const autostart = ref<boolean | null>(null);
const autostartBusy = ref(false);
const autostartError = ref<string | null>(null);

onMounted(async () => {
  try {
    autostart.value = await invoke<boolean>("get_autostart");
  } catch (e) {
    autostartError.value = String(e);
    logWarning(`get_autostart failed: ${String(e)}`);
  }
});

async function toggleAutostart(event: Event) {
  const enabled = (event.target as HTMLInputElement).checked;
  const previous = autostart.value;
  // Optimistic with revert-on-failure (the Tasks-toggle pattern); busy
  // disables the checkbox so two writes can't race.
  autostart.value = enabled;
  autostartBusy.value = true;
  autostartError.value = null;
  try {
    await invoke("set_autostart", { enabled });
  } catch (e) {
    autostart.value = previous;
    autostartError.value = String(e);
    logWarning(`set_autostart failed: ${String(e)}`);
  } finally {
    autostartBusy.value = false;
  }
}
```

Template — insert between the Behavior section and `<UpdateSettings />`:

```html
    <section>
      <h2 class="mb-1.5 text-xs font-semibold uppercase tracking-wide text-slate-400">
        System
      </h2>
      <div class="rounded-xl border border-white/10 bg-white/5 p-2">
        <div class="flex items-center justify-between">
          <label for="autostart-toggle" class="text-sm text-slate-200">
            Start with Windows
            <span class="block text-xs text-slate-500">
              Launch the buddy when you log in
            </span>
          </label>
          <input
            id="autostart-toggle"
            data-testid="autostart-toggle"
            type="checkbox"
            class="h-4 w-4 accent-violet-500"
            :checked="autostart === true"
            :disabled="autostart === null || autostartBusy"
            @change="toggleAutostart"
          />
        </div>
        <p
          v-if="autostartError"
          data-testid="autostart-error"
          class="mt-1.5 text-xs text-red-300"
        >
          {{ autostartError }}
        </p>
      </div>
    </section>
```

Note: the pre-existing BuddySettings tests mount without `mockIPC`, so `get_autostart` rejects there — the catch path (disabled toggle + inline error) keeps those tests' text assertions unaffected, and `logWarning` is mocked.

- [ ] **Step 4: Verify green**

Run: `npx vitest run tests/buddy-settings.test.ts` — all pass.

- [ ] **Step 5: Commit**

```bash
git add src/components/BuddySettings.vue tests/buddy-settings.test.ts
git commit -m "feat(ui): Start with Windows toggle in a System settings card"
```

---

### Task 7: docs, full gates, push

**Files:**
- Modify: `AGENTS.md`

- [ ] **Step 1: Update AGENTS.md.** (a) In the IPC-surface paragraph, extend the commands.rs list: `open_logs_folder`, `rearm_crash_detection` → add `get_autostart`, `set_autostart` after `rearm_crash_detection`. (b) In the "Frontend state" section, amend the storage-sync sentence to name all three roots (buddy, panel, **bubble** — the bubble reads `messageDuration` at show time) and mention the settings store's new `messageDuration` field in the store list sentence (`settings` (buddy character/animation, **message duration**, persisted to localStorage)).

- [ ] **Step 2: Run every local gate**

```bash
npm test          # expected: all files pass
npm run build     # expected: vue-tsc + vite green
cd src-tauri && cargo fmt --check && cd ..   # expected: no diff
```

- [ ] **Step 3: Commit + push**

```bash
git add AGENTS.md
git commit -m "docs(agents): autostart commands + bubble-root settings sync"
git push -u origin claude/record-view-improvements-tq5wek
```

- [ ] **Step 4: Update PR #45** — check the test-plan boxes that now hold (`npm test`, `npm run build`, `cargo fmt --check`, Linux compile gate note) and replace the "spec only" line with the implemented scope.

---

## Self-Review

- **Spec coverage:** layout regroup (T3+T6), picker preview+badge (T4), duration setting+tiers+bubble sync (T1+T2), autostart plugin+commands+UI (T5+T6), AGENTS.md (T7). ✓
- **Placeholders:** none — every step carries its code/commands. ✓
- **Type consistency:** `MessageDuration` exported from `src/stores/settings.ts`, consumed by `useBuddyBubble.ts` and `BuddySettings.vue`; `BUBBLE_MS` exported from `useBuddyBubble.ts`, consumed by `BubbleRoot.vue`; `get_autostart`/`set_autostart` names match between commands.rs, generate_handler, and the frontend invokes. ✓
