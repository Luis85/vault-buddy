# Settings UI Polish Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the capture settings readable and tidy, let the transcription row be opened or dismissed, and show where/when transcription is happening (vault row + buddy).

**Architecture:** Seven independent slices, six small and one larger (a custom `SelectMenu` component replacing native selects). All frontend except one added field on an existing Rust event.

**Tech Stack:** Vue 3 + Pinia + Tailwind 4, Vitest (happy-dom + @vue/test-utils), one Rust event field in the Tauri shell.

## Global Constraints

- **Config schema + IPC command surface unchanged.** No `get/set_capture_config` payload changes; `transcriptionLanguage` keeps its null↔"" mapping.
- **Only backend change:** add a `vaultId` field to the existing `capture:transcribing` event (Task 5). No new events, no new writes, no vault interaction.
- **What compiles where:** `src/` + `style.css` build/test on Linux (`npm test`, `npm run build`). The shell (`src-tauri/src/*`) compiles on Windows only — verify with `cargo fmt --check`; CI's `windows-app` job is the gate.
- **Recording precedence:** the buddy shows the recording dot OR the transcribing dot, never both — recording wins (Task 6).
- **Commits:** Conventional Commits — `fix(ui)`, `feat(ui)`, `feat(shell)`, `style(ui)`.

---

### Task 1: `color-scheme: dark` baseline

**Files:**
- Modify: `src/style.css`

**Interfaces:** none.

**Why no unit test:** a CSS property has no behavioral assertion; the fix is verified by the suite still passing and by the native dropdowns rendering dark. Manual/visual.

- [ ] **Step 1: Add `color-scheme: dark`**

In `src/style.css`, add `color-scheme: dark;` to the existing `html, body, #app` rule:

```css
html,
body,
#app {
  margin: 0;
  height: 100%;
  background: transparent;
  overflow: hidden;
  user-select: none;
  color-scheme: dark;
}
```

- [ ] **Step 2: Verify the suite + build still pass**

Run: `npm test` → all pass. Run: `npm run build` → builds.

- [ ] **Step 3: Commit**

```bash
git add src/style.css
git commit -m "style(ui): set color-scheme dark so native controls render readably"
```

---

### Task 2: Transcription row — open or dismiss

**Files:**
- Modify: `src/stores/capture.ts` (`openTranscript` clears on success; add `dismissTranscribed`)
- Modify: `src/components/TranscriptionStatus.vue` (add ✕ button)
- Test: `tests/capture-store.test.ts`, `tests/transcription-status.test.ts`

**Interfaces:**
- Consumes: existing `lastTranscribed: { mp3: string } | null`, `openTranscript()`.
- Produces: `dismissTranscribed()` action; `data-testid="dismiss-transcript"` button.

- [ ] **Step 1: Write the failing store tests**

In `tests/capture-store.test.ts`, add:

```ts
  it("openTranscript clears the row on success", async () => {
    mockIPC((cmd) => {
      if (cmd === "open_transcript") return undefined;
    });
    const store = useCaptureStore();
    store.lastTranscribed = { mp3: "/v/m.mp3" };
    await store.openTranscript();
    expect(store.lastTranscribed).toBeNull();
  });

  it("openTranscript keeps the row and warns on failure", async () => {
    mockIPC(() => {
      throw "vault gone";
    });
    const store = useCaptureStore();
    store.lastTranscribed = { mp3: "/v/m.mp3" };
    await store.openTranscript();
    expect(store.lastTranscribed).toEqual({ mp3: "/v/m.mp3" });
    expect(store.warning).toContain("vault gone");
  });

  it("dismissTranscribed clears the row without opening", async () => {
    const calls: string[] = [];
    mockIPC((cmd) => {
      calls.push(cmd);
    });
    const store = useCaptureStore();
    store.lastTranscribed = { mp3: "/v/m.mp3" };
    store.dismissTranscribed();
    expect(store.lastTranscribed).toBeNull();
    expect(calls).not.toContain("open_transcript");
  });
```

- [ ] **Step 2: Run to verify they fail**

Run: `npx vitest run tests/capture-store.test.ts`
Expected: FAIL — the success path doesn't clear `lastTranscribed`; `dismissTranscribed` is undefined.

- [ ] **Step 3: Update `openTranscript` and add `dismissTranscribed`**

In `src/stores/capture.ts`, change `openTranscript` to clear on success, and add `dismissTranscribed` right after it:

```ts
    async openTranscript() {
      if (!this.lastTranscribed) return;
      try {
        await invoke("open_transcript", { path: this.lastTranscribed.mp3 });
        this.lastTranscribed = null;
      } catch (e) {
        // A failed open (recording moved, launch error) is non-fatal — warn
        // and keep the row so the user can retry.
        this.warning = String(e);
        logWarning(`open transcript rejected: ${String(e)}`);
      }
    },
    dismissTranscribed() {
      this.lastTranscribed = null;
    },
```

- [ ] **Step 4: Run store tests to verify they pass**

Run: `npx vitest run tests/capture-store.test.ts`
Expected: PASS.

- [ ] **Step 5: Write the failing status-component test**

In `tests/transcription-status.test.ts`, add:

```ts
  it("dismisses the finished row without opening", async () => {
    const store = useCaptureStore();
    store.lastTranscribed = { mp3: "/v/m.mp3" };
    const w = mount(TranscriptionStatus);
    await w.get('[data-testid="dismiss-transcript"]').trigger("click");
    expect(store.lastTranscribed).toBeNull();
  });
```

- [ ] **Step 6: Run to verify it fails**

Run: `npx vitest run tests/transcription-status.test.ts`
Expected: FAIL — no `dismiss-transcript` button.

- [ ] **Step 7: Add the ✕ button to the completion row**

In `src/components/TranscriptionStatus.vue`, in the `v-else` (done) branch, add a dismiss button after the "Open in Obsidian" button. Replace the done `<div>` with:

```html
    <div
      v-else
      class="flex items-center justify-between gap-2 rounded-lg bg-emerald-500/15 px-2 py-1.5 text-xs text-emerald-100"
      role="status"
    >
      <span>✓ Transcribed</span>
      <span class="flex items-center gap-1">
        <button
          type="button"
          data-testid="open-transcript"
          class="cursor-pointer rounded bg-emerald-500/80 px-2 py-0.5 font-semibold text-white hover:bg-emerald-500 focus:outline-none focus-visible:ring-2 focus-visible:ring-emerald-300"
          @click="capture.openTranscript()"
        >
          Open in Obsidian
        </button>
        <button
          type="button"
          data-testid="dismiss-transcript"
          aria-label="Dismiss"
          class="cursor-pointer rounded px-1 py-0.5 text-emerald-200/80 hover:bg-white/10 hover:text-white focus:outline-none focus-visible:ring-2 focus-visible:ring-emerald-300"
          @click="capture.dismissTranscribed()"
        >
          ✕
        </button>
      </span>
    </div>
```

- [ ] **Step 8: Run tests + full suite + build**

Run: `npx vitest run tests/transcription-status.test.ts tests/capture-store.test.ts` → PASS.
Run: `npm test` and `npm run build` → both green.

- [ ] **Step 9: Commit**

```bash
git add src/stores/capture.ts src/components/TranscriptionStatus.vue tests/capture-store.test.ts tests/transcription-status.test.ts
git commit -m "feat(ui): let the finished-transcript row be opened or dismissed"
```

---

### Task 3: "Saved ✓" clears on edit

**Files:**
- Modify: `src/components/CaptureSettings.vue`
- Test: `tests/capture-settings.test.ts`

**Interfaces:** none new.

- [ ] **Step 1: Write the failing test**

In `tests/capture-settings.test.ts`, add (inside the `describe`):

```ts
  it("clears the Saved confirmation when a field is edited", async () => {
    const { wrapper } = await mountLoaded();
    await wrapper.get("form").trigger("submit");
    await flushPromises();
    expect(wrapper.text()).toContain("Saved");
    await wrapper.get('[data-testid="folder-input"]').setValue("Elsewhere");
    expect(wrapper.text()).not.toContain("Saved ✓");
  });
```

- [ ] **Step 2: Run to verify it fails**

Run: `npx vitest run tests/capture-settings.test.ts`
Expected: FAIL — "Saved ✓" still shows after the edit.

- [ ] **Step 3: Add the watcher**

In `src/components/CaptureSettings.vue`, import `watch` and add a watcher after the refs are declared (e.g. just before `onMounted`). Update the Vue import and add the watch:

```ts
import { computed, onMounted, ref, watch } from "vue";
```

```ts
// Any edit invalidates the "Saved ✓" confirmation. During the initial load
// saveState is already "idle", so the load-time assignments are idle→idle
// no-ops; this only becomes visible after a save set it to "saved".
watch(
  [
    mode,
    recordingFolder,
    createNote,
    bitrateKbps,
    inputDevice,
    outputDevice,
    transcribe,
    transcriptionModel,
    transcriptionLanguage,
    transcriptTimestamps,
  ],
  () => {
    if (saveState.value === "saved") saveState.value = "idle";
  },
);
```

- [ ] **Step 4: Run to verify it passes**

Run: `npx vitest run tests/capture-settings.test.ts`
Expected: PASS.

- [ ] **Step 5: Typecheck + full suite**

Run: `npm run build` and `npm test` → both green.

- [ ] **Step 6: Commit**

```bash
git add src/components/CaptureSettings.vue tests/capture-settings.test.ts
git commit -m "fix(ui): clear the Saved confirmation once settings are edited"
```

---

### Task 4: Group the transcription sub-settings

**Files:**
- Modify: `src/components/CaptureSettings.vue`

**Interfaces:** none. The `data-testid`s inside are unchanged, so existing tests keep passing.

- [ ] **Step 1: Wrap the transcription sub-settings in an indented block**

In `src/components/CaptureSettings.vue`, the `<template v-if="transcribe">` currently holds three `<section>`s (Model, Language, Timestamps). Wrap them in a `<div>` with a left border + padding + gap so they read as belonging to the toggle. Replace the opening `<template v-if="transcribe">` with a `<div>` and close it accordingly:

```html
    <div v-if="transcribe" class="flex flex-col gap-3 border-l border-white/10 pl-3">
```

and change the matching `</template>` (the one after the Timestamps `</section>`) to `</div>`.

- [ ] **Step 2: Verify the suite + build still pass (testids unchanged)**

Run: `npm test` → all pass (the transcribe show/hide tests still find the controls). Run: `npm run build` → builds.

- [ ] **Step 3: Commit**

```bash
git add src/components/CaptureSettings.vue
git commit -m "style(ui): group the transcription sub-settings under the toggle"
```

---

### Task 5: Vault-row transcription indicator

**Files:**
- Modify: `src-tauri/src/capture_commands.rs` (add `vaultId` to the `capture:transcribing` emit)
- Modify: `src/stores/capture.ts` (`transcribingVaultId` state; set on transcribing, clear on transcribed/failed)
- Modify: `src/components/VaultList.vue` (add prop + dot)
- Modify: `src/components/ActionPanel.vue` (bind the prop)
- Test: `tests/capture-store.test.ts`, `tests/vault-list.test.ts`

**Interfaces:**
- Produces: `capture.transcribingVaultId: string | null`; `VaultList` prop `transcribingVaultId: string | null`.

- [ ] **Step 1: Add `vaultId` to the backend event**

In `src-tauri/src/capture_commands.rs`, in `process_transcription`, change the `capture:transcribing` emit to include the vault:

```rust
    let _ = app.emit(
        "capture:transcribing",
        serde_json::json!({ "mp3": job.mp3.to_string_lossy(), "vaultId": job.vault_id }),
    );
```

Verify (shell is Windows-only): `cd src-tauri && cargo fmt --check` → exit 0.

- [ ] **Step 2: Write the failing store tests**

In `tests/capture-store.test.ts`, add:

```ts
  it("tracks which vault is transcribing, then clears it", async () => {
    mockIPC((cmd) => {
      if (cmd === "capture_status") return { recording: false, vaultId: null, startedAtMs: null };
    });
    const store = useCaptureStore();
    await store.init();
    state.eventHandlers["capture:transcribing"]!({ payload: { mp3: "/v/m.mp3", vaultId: "v7" } });
    expect(store.transcribingVaultId).toBe("v7");
    state.eventHandlers["capture:transcribed"]!({
      payload: { mp3: "/v/m.mp3", transcript: "/v/m.transcript.md" },
    });
    expect(store.transcribingVaultId).toBeNull();
  });

  it("clears the transcribing vault on failure too", async () => {
    mockIPC((cmd) => {
      if (cmd === "capture_status") return { recording: false, vaultId: null, startedAtMs: null };
    });
    const store = useCaptureStore();
    await store.init();
    state.eventHandlers["capture:transcribing"]!({ payload: { mp3: "/v/m.mp3", vaultId: "v7" } });
    state.eventHandlers["capture:transcribeFailed"]!({ payload: { mp3: "/v/m.mp3", message: "x" } });
    expect(store.transcribingVaultId).toBeNull();
  });
```

- [ ] **Step 3: Run to verify they fail**

Run: `npx vitest run tests/capture-store.test.ts`
Expected: FAIL — `transcribingVaultId` is undefined.

- [ ] **Step 4: Add store state + wiring**

In `src/stores/capture.ts`:

(a) Add to `state`, next to `lastTranscribed`:

```ts
    /** Which vault is currently transcribing — drives the vault-row dot. */
    transcribingVaultId: null as string | null,
```

(b) Set it in the `capture:transcribing` listener (change the payload type and body):

```ts
      await listen<{ mp3: string; vaultId: string }>("capture:transcribing", (event) => {
        this.transcribing = true;
        this.transcriptError = null;
        this.transcribingVaultId = event.payload.vaultId;
      });
```

(c) Clear it in the `capture:transcribed` listener:

```ts
      await listen<CaptureTranscribed>("capture:transcribed", (event) => {
        this.transcribing = false;
        this.modelDownload = null;
        this.lastTranscribed = { mp3: event.payload.mp3 };
        this.transcribingVaultId = null;
      });
```

(d) Clear it in the `capture:transcribeFailed` listener:

```ts
      await listen<CaptureTranscribeFailed>("capture:transcribeFailed", (event) => {
        this.transcribing = false;
        this.modelDownload = null;
        this.transcriptError = event.payload.message;
        this.transcriptFailedMp3 = event.payload.mp3;
        this.transcribingVaultId = null;
      });
```

- [ ] **Step 5: Run store tests to verify they pass**

Run: `npx vitest run tests/capture-store.test.ts`
Expected: PASS.

- [ ] **Step 6: Write the failing VaultList test**

In `tests/vault-list.test.ts`, extend `mountList` with a `transcribingVaultId` param and add a test:

```ts
const mountList = (
  vaults: Array<{ id: string; name: string; path: string; open: boolean }>,
  busyVaultId: string | null = null,
  busyCommand: Busy = null,
  captureDisabled = false,
  recordingVaultId: string | null = null,
  transcribingVaultId: string | null = null,
) =>
  mount(VaultList, {
    props: { vaults, busyVaultId, busyCommand, captureDisabled, recordingVaultId, transcribingVaultId },
  });
```

```ts
  it("shows a transcribing indicator on the transcribing vault", () => {
    const wrapper = mountList(sample, null, null, false, null, "aaa111");
    const dot = wrapper.get('[title="Transcribing…"]');
    expect(dot.classes()).toContain("bg-violet-400");
  });
```

- [ ] **Step 7: Run to verify it fails**

Run: `npx vitest run tests/vault-list.test.ts`
Expected: FAIL — no element with `title="Transcribing…"`.

- [ ] **Step 8: Add the prop + dot to VaultList**

In `src/components/VaultList.vue`, add the prop:

```ts
const props = defineProps<{
  vaults: Vault[];
  busyVaultId: string | null;
  busyCommand: "open_vault" | "open_daily_note" | null;
  captureDisabled: boolean;
  recordingVaultId: string | null;
  transcribingVaultId: string | null;
}>();
```

Add the dot right after the existing recording dot (`v-if="vault.id === recordingVaultId"` block):

```html
              <span
                v-if="vault.id === transcribingVaultId"
                class="h-1.5 w-1.5 shrink-0 animate-pulse rounded-full bg-violet-400"
                title="Transcribing…"
                aria-hidden="true"
              ></span>
```

- [ ] **Step 9: Bind the prop in ActionPanel**

In `src/components/ActionPanel.vue`, add to the `<VaultList>` props (next to `:recording-vault-id`):

```html
        :transcribing-vault-id="capture.transcribingVaultId"
```

- [ ] **Step 10: Run VaultList tests + full suite + build**

Run: `npx vitest run tests/vault-list.test.ts tests/capture-store.test.ts` → PASS.
Run: `npm test` and `npm run build` → green.

- [ ] **Step 11: Commit**

```bash
git add src-tauri/src/capture_commands.rs src/stores/capture.ts src/components/VaultList.vue src/components/ActionPanel.vue tests/capture-store.test.ts tests/vault-list.test.ts
git commit -m "feat(ui): show which vault is transcribing on its row"
```

---

### Task 6: Buddy transcribing indicator

**Files:**
- Modify: `src/components/CompanionCharacter.vue` (add `transcribing` prop + dot)
- Modify: `src/App.vue` (bind the prop)
- Test: `tests/companion-character.test.ts`

**Interfaces:**
- Produces: `CompanionCharacter` prop `transcribing?: boolean` (default false); `.transcribe-dot` element.

- [ ] **Step 1: Write the failing tests**

In `tests/companion-character.test.ts`, add:

```ts
  it("shows a violet transcribing dot while transcribing", () => {
    const wrapper = mount(CompanionCharacter, {
      props: { working: false, transcribing: true },
    });
    const dot = wrapper.get(".transcribe-dot");
    expect(dot.classes()).toContain("bg-violet-400");
    expect(wrapper.get("button").classes()).toContain("transcribing");
  });

  it("hides the transcribing dot while recording takes precedence", () => {
    const wrapper = mount(CompanionCharacter, {
      props: { working: false, transcribing: true, recording: true },
    });
    expect(wrapper.find(".transcribe-dot").exists()).toBe(false);
    expect(wrapper.find(".rec-dot").exists()).toBe(true);
  });
```

- [ ] **Step 2: Run to verify they fail**

Run: `npx vitest run tests/companion-character.test.ts`
Expected: FAIL — no `transcribing` prop/dot.

- [ ] **Step 3: Add the prop, class, and dot**

In `src/components/CompanionCharacter.vue`:

(a) Add `transcribing?: boolean` to the props and default it:

```ts
    recording?: boolean;
    paused?: boolean;
    transcribing?: boolean;
```

```ts
    recording: false,
    paused: false,
    transcribing: false,
```

(b) Add `transcribing` to the `.buddy` class binding:

```ts
        { working, still: !animated, recording, paused, transcribing },
```

(c) Add the dot inside the `relative` span, after the `rec-dot` span — shown only when transcribing and not recording:

```html
        <span
          v-if="transcribing && !recording"
          class="transcribe-dot absolute -right-1 -top-1 h-3 w-3 animate-pulse rounded-full bg-violet-400 ring-2 ring-slate-900"
          aria-hidden="true"
        ></span>
```

- [ ] **Step 4: Run to verify they pass**

Run: `npx vitest run tests/companion-character.test.ts`
Expected: PASS.

- [ ] **Step 5: Bind the prop in App.vue**

In `src/App.vue`, add to the `<CompanionCharacter>` props (next to `:recording` / `:paused`):

```html
        :transcribing="capture.transcribing"
```

- [ ] **Step 6: Typecheck + full suite**

Run: `npm run build` and `npm test` → green.

- [ ] **Step 7: Commit**

```bash
git add src/components/CompanionCharacter.vue src/App.vue tests/companion-character.test.ts
git commit -m "feat(ui): pulse a buddy indicator while transcribing"
```

---

### Task 7: Custom `SelectMenu` dropdown

**Files:**
- Create: `src/components/SelectMenu.vue`
- Test: `tests/select-menu.test.ts`
- Modify: `src/components/CaptureSettings.vue` (replace the five `<select>`s)
- Test: `tests/capture-settings.test.ts` (interactions move to the custom component)

**Interfaces:**
- Produces: `SelectMenu` — `props { modelValue: string | number; options: {value: string|number; label: string}[]; id?: string; ariaLabel?: string; dataTestid?: string }`, emits `update:modelValue`. Each option row carries `data-testid="${dataTestid}-option-${value}"` when `dataTestid` is set.

- [ ] **Step 1: Write the failing component tests**

Create `tests/select-menu.test.ts`:

```ts
import { afterEach, describe, expect, it } from "vitest";
import { mount } from "@vue/test-utils";
import SelectMenu from "../src/components/SelectMenu.vue";

const OPTIONS = [
  { value: "", label: "Auto-detect" },
  { value: "de", label: "German" },
  { value: "es", label: "Spanish" },
];

// The popup is Teleported to <body>; unmount removes it between tests.
let active: ReturnType<typeof mount> | null = null;
afterEach(() => {
  active?.unmount();
  active = null;
  document.body.innerHTML = "";
});

function mountMenu(modelValue: string, dataTestid = "lang") {
  active = mount(SelectMenu, {
    props: { modelValue, options: OPTIONS, dataTestid },
    attachTo: document.body,
  });
  return active;
}

describe("SelectMenu", () => {
  it("shows the selected option's label on the trigger", () => {
    const w = mountMenu("de");
    expect(w.get('[data-testid="lang"]').text()).toContain("German");
  });

  it("opens on click and lists the options", async () => {
    const w = mountMenu("");
    await w.get('[data-testid="lang"]').trigger("click");
    const options = document.body.querySelectorAll('[role="option"]');
    expect(options.length).toBe(3);
  });

  it("emits the chosen value and closes when an option is clicked", async () => {
    const w = mountMenu("");
    await w.get('[data-testid="lang"]').trigger("click");
    (document.body.querySelector('[data-testid="lang-option-de"]') as HTMLElement).click();
    expect(w.emitted("update:modelValue")).toEqual([["de"]]);
    expect(document.body.querySelector('[role="option"]')).toBeNull();
  });

  it("marks expanded state on the trigger", async () => {
    const w = mountMenu("");
    expect(w.get('[data-testid="lang"]').attributes("aria-expanded")).toBe("false");
    await w.get('[data-testid="lang"]').trigger("click");
    expect(w.get('[data-testid="lang"]').attributes("aria-expanded")).toBe("true");
  });
});
```

- [ ] **Step 2: Run to verify they fail**

Run: `npx vitest run tests/select-menu.test.ts`
Expected: FAIL — `SelectMenu.vue` does not exist.

- [ ] **Step 3: Create the component**

Create `src/components/SelectMenu.vue`:

```vue
<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref } from "vue";

interface Option {
  value: string | number;
  label: string;
}

const props = withDefaults(
  defineProps<{
    modelValue: string | number;
    options: readonly Option[];
    id?: string;
    ariaLabel?: string;
    dataTestid?: string;
  }>(),
  { id: undefined, ariaLabel: undefined, dataTestid: undefined },
);
const emit = defineEmits<{ (e: "update:modelValue", value: string | number): void }>();

const open = ref(false);
const activeIndex = ref(-1);
const triggerRef = ref<HTMLButtonElement | null>(null);
const popupRef = ref<HTMLUListElement | null>(null);
const popupStyle = ref<Record<string, string>>({});

const selectedLabel = computed(
  () => props.options.find((o) => o.value === props.modelValue)?.label ?? "",
);

function positionPopup() {
  const rect = triggerRef.value?.getBoundingClientRect();
  if (!rect) return;
  const spaceBelow = window.innerHeight - rect.bottom;
  const spaceAbove = rect.top;
  const openUp = spaceBelow < 200 && spaceAbove > spaceBelow;
  const maxH = Math.max(120, Math.min(220, (openUp ? spaceAbove : spaceBelow) - 8));
  const style: Record<string, string> = {
    position: "fixed",
    left: `${rect.left}px`,
    minWidth: `${rect.width}px`,
    maxHeight: `${maxH}px`,
    zIndex: "50",
  };
  if (openUp) style.bottom = `${window.innerHeight - rect.top + 4}px`;
  else style.top = `${rect.bottom + 4}px`;
  popupStyle.value = style;
}

async function openMenu() {
  open.value = true;
  activeIndex.value = Math.max(
    0,
    props.options.findIndex((o) => o.value === props.modelValue),
  );
  positionPopup();
  await nextTick();
  positionPopup();
  popupRef.value?.focus();
  window.addEventListener("scroll", closeMenu, true);
  window.addEventListener("resize", closeMenu);
  document.addEventListener("pointerdown", onDocPointerDown, true);
}

function closeMenu() {
  if (!open.value) return;
  open.value = false;
  window.removeEventListener("scroll", closeMenu, true);
  window.removeEventListener("resize", closeMenu);
  document.removeEventListener("pointerdown", onDocPointerDown, true);
  triggerRef.value?.focus();
}

function onDocPointerDown(e: PointerEvent) {
  const t = e.target as Node;
  if (triggerRef.value?.contains(t) || popupRef.value?.contains(t)) return;
  closeMenu();
}

function toggle() {
  if (open.value) closeMenu();
  else void openMenu();
}

function select(value: string | number) {
  emit("update:modelValue", value);
  closeMenu();
}

function onTriggerKeydown(e: KeyboardEvent) {
  if (["ArrowDown", "ArrowUp", "Enter", " "].includes(e.key) && !open.value) {
    e.preventDefault();
    void openMenu();
  }
}

function onPopupKeydown(e: KeyboardEvent) {
  if (e.key === "Escape") {
    e.preventDefault();
    closeMenu();
  } else if (e.key === "ArrowDown") {
    e.preventDefault();
    activeIndex.value = Math.min(props.options.length - 1, activeIndex.value + 1);
  } else if (e.key === "ArrowUp") {
    e.preventDefault();
    activeIndex.value = Math.max(0, activeIndex.value - 1);
  } else if (e.key === "Enter") {
    e.preventDefault();
    const o = props.options[activeIndex.value];
    if (o) select(o.value);
  }
}

onBeforeUnmount(() => {
  window.removeEventListener("scroll", closeMenu, true);
  window.removeEventListener("resize", closeMenu);
  document.removeEventListener("pointerdown", onDocPointerDown, true);
});
</script>

<template>
  <button
    :id="id"
    ref="triggerRef"
    type="button"
    :data-testid="dataTestid"
    :aria-label="ariaLabel"
    :aria-expanded="open"
    aria-haspopup="listbox"
    class="flex items-center justify-between gap-2 rounded-lg border border-white/10 bg-white/5 px-2 py-1 text-sm text-slate-100 focus:border-violet-400 focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400"
    @click="toggle"
    @keydown="onTriggerKeydown"
  >
    <span>{{ selectedLabel }}</span>
    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true">
      <path d="M6 9l6 6 6-6" />
    </svg>
  </button>
  <Teleport to="body">
    <ul
      v-if="open"
      ref="popupRef"
      role="listbox"
      tabindex="-1"
      :style="popupStyle"
      class="panel-scroll overflow-y-auto rounded-lg border border-white/10 bg-slate-900/95 py-1 text-sm text-slate-100 shadow-xl focus:outline-none"
      @keydown="onPopupKeydown"
    >
      <li
        v-for="(o, i) in options"
        :key="String(o.value)"
        role="option"
        :aria-selected="o.value === modelValue"
        :data-testid="dataTestid ? `${dataTestid}-option-${o.value}` : undefined"
        class="cursor-pointer px-3 py-1"
        :class="[
          o.value === modelValue ? 'bg-violet-500/25 text-white' : '',
          i === activeIndex ? 'bg-white/10' : '',
        ]"
        @click="select(o.value)"
        @pointermove="activeIndex = i"
      >
        {{ o.label }}
      </li>
    </ul>
  </Teleport>
</template>
```

- [ ] **Step 4: Run the component tests to verify they pass**

Run: `npx vitest run tests/select-menu.test.ts`
Expected: PASS.

- [ ] **Step 5: Replace the five selects in CaptureSettings**

In `src/components/CaptureSettings.vue`, import the component and build `{value,label}` option lists, then swap each `<select>` for `<SelectMenu>`.

Add the import and the option computeds in `<script setup>`:

```ts
import SelectMenu from "./SelectMenu.vue";
```

```ts
const modelOptions = MODELS.map((m) => ({ value: m, label: capitalize(m) }));
const languageOptions = LANGUAGES.map((l) => ({ value: l.code, label: l.name }));
const bitrateOptions = BITRATES.map((b) => ({ value: b, label: `${b} kbps` }));
const inputMenuOptions = computed(() => [
  { value: "", label: "System default" },
  ...inputOptions.value.map((o) => ({ value: o.value, label: o.label })),
]);
const outputMenuOptions = computed(() => [
  { value: "", label: "System default" },
  ...outputOptions.value.map((o) => ({ value: o.value, label: o.label })),
]);
```

Replace the **Model** `<select>…</select>` with:

```html
        <SelectMenu
          id="capture-transcription-model"
          v-model="transcriptionModel"
          :options="modelOptions"
          data-testid="transcription-model-select"
        />
```

Replace the **Language** `<select>…</select>` with:

```html
        <SelectMenu
          id="capture-transcription-language"
          v-model="transcriptionLanguage"
          :options="languageOptions"
          data-testid="transcription-language-select"
        />
```

Replace the **Bitrate** `<select>…</select>` with:

```html
        <SelectMenu
          id="capture-bitrate"
          v-model="bitrateKbps"
          :options="bitrateOptions"
          data-testid="bitrate-select"
        />
```

Replace the **Microphone** `<select>…</select>` (the full-width one) with:

```html
      <SelectMenu
        id="capture-input-device"
        v-model="inputDevice"
        :options="inputMenuOptions"
        aria-label="Microphone"
        data-testid="input-device-select"
        class="w-full"
      />
```

Replace the **Desktop audio** `<select>…</select>` with:

```html
      <SelectMenu
        id="capture-output-device"
        v-model="outputDevice"
        :options="outputMenuOptions"
        aria-label="Desktop audio device"
        data-testid="output-device-select"
        class="w-full"
      />
```

(Bitrate keeps a numeric model: `bitrateOptions` values are numbers, and `SelectMenu` emits the option's value as-is, so `bitrateKbps` stays a number — the old `.number` modifier is no longer needed.)

- [ ] **Step 6: Update the CaptureSettings tests to drive the custom dropdowns**

In `tests/capture-settings.test.ts`, the dropdown interactions move from native `.setValue()` / `element.value` to the `SelectMenu` (click trigger → click option). Update the three affected tests:

The **"shows the model/language/timestamps controls, loaded correctly"** test — read the trigger label text instead of `element.value`:

```ts
    expect(wrapper.get('[data-testid="transcription-model-select"]').text()).toContain("Medium");
    expect(wrapper.get('[data-testid="transcription-language-select"]').text()).toContain("Spanish");
    const timestamps = wrapper.get<HTMLInputElement>(
      '[data-testid="transcript-timestamps-toggle"]',
    );
    expect(timestamps.element.checked).toBe(false);
```

The **"saves transcription settings…"** test — open each menu and click the option (options are Teleported to `document.body`):

```ts
    await wrapper.get('[data-testid="transcribe-toggle"]').setValue(true);
    await wrapper.get('[data-testid="transcription-model-select"]').trigger("click");
    (document.body.querySelector('[data-testid="transcription-model-select-option-medium"]') as HTMLElement).click();
    await wrapper.get('[data-testid="transcription-language-select"]').trigger("click");
    (document.body.querySelector('[data-testid="transcription-language-select-option-es"]') as HTMLElement).click();
    await wrapper.get("form").trigger("submit");
    await flushPromises();
```

(The saved-payload `expect(...).toEqual(...)` assertion is unchanged — `transcriptionModel: "medium"`, `transcriptionLanguage: "es"`.)

Any existing bitrate/device test using `.setValue()` on those selects is likewise updated to `trigger("click")` + clicking the `…-option-<value>` element. Add `attachTo: document.body` to the mount helper and unmount in `afterEach` so Teleported popups don't leak between tests:

```ts
  afterEach(() => {
    wrapper?.unmount();
    document.body.innerHTML = "";
  });
```

(If the file's mount helper returns the wrapper, keep a module-level reference to unmount; otherwise unmount the returned wrapper in each test.)

- [ ] **Step 7: Run the settings tests + full suite + build**

Run: `npx vitest run tests/capture-settings.test.ts tests/select-menu.test.ts` → PASS.
Run: `npm test` and `npm run build` → green.

- [ ] **Step 8: Commit**

```bash
git add src/components/SelectMenu.vue src/components/CaptureSettings.vue tests/select-menu.test.ts tests/capture-settings.test.ts
git commit -m "feat(ui): replace native selects with a themed SelectMenu dropdown"
```

---

## Self-Review

**Spec coverage:**
- §1 color-scheme → Task 1. ✅
- §2 open/dismiss row → Task 2 (openTranscript clears on success; dismissTranscribed; ✕ button). ✅
- §3 Saved-clears-on-edit → Task 3 (watch). ✅
- §4 grouped sub-settings → Task 4 (border-l block). ✅
- §5 custom SelectMenu → Task 7 (component + replace + tests). ✅
- §6 vault-row indicator → Task 5 (backend vaultId + store + VaultList + ActionPanel). ✅
- §7 buddy indicator → Task 6 (CompanionCharacter + App). ✅
- Invariants (config/IPC unchanged; only backend touch is the event field; recording precedence) → honored across Tasks 5–7. ✅

**Placeholder scan:** none — every step has concrete code/commands.

**Type consistency:** `transcribingVaultId: string | null` defined (Task 5 store) and consumed as `VaultList` prop + `ActionPanel` binding (Task 5). `dismissTranscribed()` defined (Task 2 store) and wired to `data-testid="dismiss-transcript"` (Task 2 component). `SelectMenu` `update:modelValue` + `${dataTestid}-option-${value}` testids defined (Task 7 component) and used in both `select-menu.test.ts` and the updated `capture-settings.test.ts`. `transcribing` prop defined (Task 6 component) and bound in App. ✅
