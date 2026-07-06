# Error feedback, propagation & open-findings cleanup — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make transcription/capture failures visible in-app through one view-independent notification surface, route the reasons the backend already emits, fix three backend swallows that mislead the user, and resolve the 11 open findings from the increment-4 review.

**Architecture:** A new tiny `notifications` Pinia store + a `NotificationHost` mounted once in `ActionPanel` becomes the single, view-independent surface for transient messages. The capture store routes every currently-swallowed error path to it. Three backend honesty fixes (a `SkippedForeign` outcome, an early-stop reason on `capture:saved`, a surfaced note-write failure) stop reporting problems as success — those touch the Windows-only shell, so they are held and pushed as one compile-bracket.

**Tech Stack:** Vue 3 + Pinia + Tailwind 4 (frontend, Vitest + happy-dom); Rust — `vault_buddy_core`, `vault_buddy_transcribe`, `vault_buddy_capture` (Linux-testable), the Tauri shell (`src-tauri/src/*.rs`, Windows-only compile, CI `windows-app` gate).

Spec: `docs/superpowers/specs/2026-07-06-error-feedback-and-open-findings-design.md`.

## Global Constraints

- **Never-clobber is untouched.** `SkippedForeign` reporting only changes *what we say*, not *what we write*. Guarded `replace_if_ours` vs sanctioned `force_write_sidecar` usage is unchanged; `is_regenerable` is unchanged.
- **No swallowed error (diagnostics invariant).** A caught error still logs (`log::*` / `logWarning`) AND now reaches the user (a notification, an inline reason, or the buddy). `logWarning` is the breadcrumb; the notification is the user half.
- **Compile-bracket.** Tasks 10–12 change `transcribe_recording`'s return type and the `capture:saved` payload, breaking the Windows-only shell until Task 12 updates it. **HOLD Tasks 10, 11, 12; push together** so the `windows-app` job never sees a broken intermediate. Frontend Tasks 1–9 each keep `npm run build` + `npm test` green — **push each**.
- **Frontend never breaks against the not-yet-shipped backend.** Task 5 adds listeners/types for `capture:transcribeSkipped` and the new `capture:saved` fields; they are inert until Task 12 emits them, and the payload shapes here are the contract both sides use verbatim.
- **New event/payload contracts (use verbatim on both sides):**
  - `capture:transcribeSkipped` → `{ mp3: string, message: string }`.
  - `capture:saved` → `{ mp3: string, note: string | null, endedEarly: boolean, warning: string | null }` (adds `endedEarly` + `warning` to today's `{ mp3, note }`).
- **Notifications store API (use verbatim across Tasks 1–9):** `notify(kind, message, opts?) → number`, `error(message) → number`, `warning(message) → number`, `success(message) → number`, `info(message) → number`, `dismiss(id)`, `clear()`; state `items: Notification[]`; `Notification = { id: number, kind: "error" | "warning" | "success" | "info", message: string }`; `MAX_ITEMS = 5`; default ttl: error/warning `null` (sticky), success/info `4000`.
- **Main-thread / lock discipline unchanged.** Tasks 10–12 only make the two new shell commands *more* consistent with it (`lock_ignoring_poison`, drop-lock-before-`is_recording`); no lock is taken across inference or held across an fsync.
- **Diagnostics:** every spawned thread stays named; the panic hook / crash handler ordering is untouched.
- **Commit scopes:** `feat(ui)`, `fix(ui)`, `feat(core)`, `feat(transcribe)`, `feat(capture)`, `feat(shell)`, `fix(shell)`.
- **Verification commands** — Frontend (repo root): `npx vitest run tests/<file>`, `npm test`, `npm run build`. Rust (from `src-tauri/`): `cargo test -p vault_buddy_core`, `cargo test -p vault_buddy_transcribe`, `cargo test -p vault_buddy_capture`, `cargo fmt --check`, `cargo clippy -p vault_buddy_core -p vault_buddy_transcribe -p vault_buddy_capture --all-targets -- -D warnings`.

---

### Task 1: `notifications` store

**Files:**
- Create: `src/stores/notifications.ts`
- Test: `tests/notifications-store.test.ts`

**Interfaces:**
- Produces: `useNotificationsStore()` with the API in Global Constraints. `notify` dedupes against the newest un-dismissed item (same `kind` + `message` → returns that id, no push); auto-dismiss via `setTimeout(dismiss, ttlMs)` when `ttlMs != null`; caps `items` at `MAX_ITEMS` (drop oldest). Ids are a monotonic counter.

- [ ] **Step 1: Write the failing tests**

```ts
// tests/notifications-store.test.ts
import { beforeEach, afterEach, describe, expect, it, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import { useNotificationsStore } from "../src/stores/notifications";

describe("notifications store", () => {
  beforeEach(() => { setActivePinia(createPinia()); vi.useFakeTimers(); });
  afterEach(() => { vi.useRealTimers(); });

  it("error/warning are sticky; success auto-dismisses", () => {
    const n = useNotificationsStore();
    n.error("boom");
    n.success("done");
    expect(n.items.map((i) => i.message)).toEqual(["boom", "done"]);
    vi.advanceTimersByTime(4000);
    expect(n.items.map((i) => i.message)).toEqual(["boom"]); // success gone, error stays
  });

  it("dedupes the newest identical message", () => {
    const n = useNotificationsStore();
    const a = n.error("same");
    const b = n.error("same");
    expect(a).toBe(b);
    expect(n.items).toHaveLength(1);
  });

  it("caps at MAX_ITEMS, dropping oldest", () => {
    const n = useNotificationsStore();
    for (let i = 0; i < 7; i++) n.error(`e${i}`);
    expect(n.items).toHaveLength(5);
    expect(n.items[0].message).toBe("e2");
    expect(n.items[4].message).toBe("e6");
  });

  it("dismiss removes by id; clear empties", () => {
    const n = useNotificationsStore();
    const id = n.warning("w");
    n.info("i");
    n.dismiss(id);
    expect(n.items.map((i) => i.message)).toEqual(["i"]);
    n.clear();
    expect(n.items).toEqual([]);
  });
});
```

- [ ] **Step 2: Run to verify failure** — `npx vitest run tests/notifications-store.test.ts` → FAIL (module missing).

- [ ] **Step 3: Implement `src/stores/notifications.ts`**

```ts
import { defineStore } from "pinia";

export type NotifyKind = "error" | "warning" | "success" | "info";
export interface Notification { id: number; kind: NotifyKind; message: string; }

/** Newest failure must not be pushed off by a burst; small cap keeps the panel readable. */
const MAX_ITEMS = 5;
const DEFAULT_TTL: Record<NotifyKind, number | null> = {
  error: null, warning: null, success: 4000, info: 4000,
};

let seq = 0; // monotonic id source — no Date.now needed

export const useNotificationsStore = defineStore("notifications", {
  state: () => ({ items: [] as Notification[] }),
  actions: {
    notify(kind: NotifyKind, message: string, opts?: { ttlMs?: number | null }): number {
      // Dedupe a retried command spamming the same line: if the newest item is
      // an identical kind+message, reuse it rather than stacking duplicates.
      const last = this.items[this.items.length - 1];
      if (last && last.kind === kind && last.message === message) return last.id;
      const id = ++seq;
      this.items.push({ id, kind, message });
      if (this.items.length > MAX_ITEMS) this.items.splice(0, this.items.length - MAX_ITEMS);
      const ttlMs = opts?.ttlMs !== undefined ? opts.ttlMs : DEFAULT_TTL[kind];
      if (ttlMs != null) setTimeout(() => this.dismiss(id), ttlMs);
      return id;
    },
    error(message: string) { return this.notify("error", message); },
    warning(message: string) { return this.notify("warning", message); },
    success(message: string) { return this.notify("success", message); },
    info(message: string) { return this.notify("info", message); },
    dismiss(id: number) { this.items = this.items.filter((i) => i.id !== id); },
    clear() { this.items = []; },
  },
});
```

- [ ] **Step 4: Run to verify pass** — `npx vitest run tests/notifications-store.test.ts` → PASS (4).
- [ ] **Step 5: Commit** — `git add src/stores/notifications.ts tests/notifications-store.test.ts && git commit -m "feat(ui): notifications store (view-independent transient messages)"`
- [ ] **Step 6: Push** — `git push -u origin claude/vault-buddy-local-stt-rktgl2`

---

### Task 2: Route capture-store errors to the notification surface

**Files:**
- Modify: `src/stores/capture.ts` (event handlers in `init()`; actions `cancelTranscription`, `retranscribe`, `openTranscript`, `start`, `stop`, `pause`, `resume`)
- Test: `tests/capture-store.test.ts` (add cases)

**Interfaces:**
- Consumes: `useNotificationsStore()` (Task 1).
- Rule: every place the store learns of a failure raises a notification AND keeps its existing behavior. `capture.error`/`capture.warning` fields are retained (buddy + RecordingBar read them). The notifications store dedupe makes the start-failure double (event `capture:failed` + the `start()` rejection) collapse when the messages match.

- [ ] **Step 1: Write failing tests** in `tests/capture-store.test.ts` (mirror the existing harness — `setActivePinia`, `mockIPC`, event emission via `@tauri-apps/api/mocks` / the store's `listen` handlers as the file already does):

```ts
// A capture:transcribeFailed event raises an error notification carrying the reason.
it("surfaces a transcription failure reason as a notification", async () => {
  const capture = useCaptureStore(); const notes = useNotificationsStore();
  await capture.init();
  // emit capture:transcribeFailed { mp3, message } via the existing test's emit helper
  await emit("capture:transcribeFailed", { mp3: "/v/a.mp3", message: "whisper inference: bad model" });
  expect(notes.items.some((i) => i.kind === "error" && i.message.includes("whisper inference: bad model"))).toBe(true);
  expect(capture.transcriptions["/v/a.mp3"].error).toBe("whisper inference: bad model"); // inline reason still set
});

// retranscribe rejection is no longer an unhandled rejection — it notifies.
it("notifies (not throws) when retranscribe is rejected", async () => {
  mockIPC((cmd) => { if (cmd === "retranscribe") throw new Error("Recording not found"); });
  const capture = useCaptureStore(); const notes = useNotificationsStore();
  await capture.retranscribe("/v/gone.mp3"); // must NOT reject
  expect(notes.items.some((i) => i.kind === "error" && i.message.includes("Recording not found"))).toBe(true);
});

// cancel rejection notifies.
it("notifies when cancel is rejected", async () => {
  mockIPC((cmd) => { if (cmd === "cancel_transcription") throw new Error("No such transcription in the queue"); });
  const capture = useCaptureStore(); const notes = useNotificationsStore();
  await capture.cancelTranscription("/v/x.mp3");
  expect(notes.items.some((i) => i.message.includes("No such transcription"))).toBe(true);
});

// openTranscript rejection notifies (was routed to the list-only warning banner).
it("notifies when open transcript is rejected", async () => {
  mockIPC((cmd) => { if (cmd === "open_transcript") throw new Error("launch failed"); });
  const capture = useCaptureStore(); const notes = useNotificationsStore();
  await capture.openTranscript("/v/x.mp3");
  expect(notes.items.some((i) => i.message.includes("launch failed"))).toBe(true);
});
```

- [ ] **Step 2: Run to verify failure** — `npx vitest run tests/capture-store.test.ts` → the new cases FAIL.

- [ ] **Step 3: Implement** in `src/stores/capture.ts`:
  - Import: `import { useNotificationsStore } from "./notifications";`
  - `capture:failed` handler: after `this.error = event.payload.message;` add `useNotificationsStore().error(event.payload.message);`
  - `capture:warning` handler: keep `this.warning = ...`; add `if (this.status !== "recording") useNotificationsStore().warning(event.payload.message);` (live warnings stay in RecordingBar).
  - `capture:transcribeFailed` handler: after the `upsert(... failed ...)`, add `useNotificationsStore().error(\`Transcription failed: ${message}\`);`
  - `cancelTranscription` catch: add `useNotificationsStore().error(\`Couldn't cancel transcription: ${String(e)}\`);` (keep `logWarning`).
  - `retranscribe`: wrap the `invoke` in `try { await invoke("retranscribe", { path: mp3 }); } catch (e) { useNotificationsStore().error(\`Couldn't re-transcribe: ${String(e)}\`); logWarning(\`retranscribe rejected: ${String(e)}\`); }` (adds the missing catch).
  - `openTranscript` catch: replace `this.warning = String(e);` with `useNotificationsStore().error(\`Couldn't open transcript: ${String(e)}\`);` (keep `logWarning`).
  - `start`/`stop`/`pause`/`resume` catches: after each `this.error = String(e);` add `useNotificationsStore().error(String(e));`

- [ ] **Step 4: Run to verify pass** — `npx vitest run tests/capture-store.test.ts` → PASS. Then `npm test` (full suite green — existing capture-store tests unaffected since fields are retained).
- [ ] **Step 5: Commit** — `git commit -am "feat(ui): route capture/transcription errors to the notification surface"`
- [ ] **Step 6: Push**

---

### Task 3: `NotificationHost` + mount in `ActionPanel`, remove list-only banners

**Files:**
- Create: `src/components/NotificationHost.vue`
- Modify: `src/components/ActionPanel.vue` (mount the host; remove the `capture.error` banner at ~:172-177 and the `capture.warning` banner at ~:179-184)
- Test: `tests/notification-host.test.ts`; update `tests/action-panel.test.ts`

**Interfaces:**
- Consumes: `useNotificationsStore()` (Task 1); capture errors already flow into it (Task 2).

- [ ] **Step 1: Write failing tests**

```ts
// tests/notification-host.test.ts
import { beforeEach, describe, expect, it } from "vitest";
import { mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import NotificationHost from "../src/components/NotificationHost.vue";
import { useNotificationsStore } from "../src/stores/notifications";

describe("NotificationHost", () => {
  beforeEach(() => setActivePinia(createPinia()));
  it("renders items with kind styling and role, and dismisses", async () => {
    const n = useNotificationsStore(); n.error("boom");
    const w = mount(NotificationHost);
    const item = w.get('[data-testid="notification"]');
    expect(item.text()).toContain("boom");
    expect(item.attributes("role")).toBe("alert");
    await w.get('[data-testid="notification-dismiss"]').trigger("click");
    expect(n.items).toHaveLength(0);
    expect(w.find('[data-testid="notification"]').exists()).toBe(false);
  });
});
```

Also update `tests/action-panel.test.ts`: any assertion that a `capture.error`/`capture.warning` banner renders in the list view is replaced by asserting the message reaches the notification host (set `useNotificationsStore().error(...)` and assert `[data-testid="notification"]` appears regardless of `store.view`).

- [ ] **Step 2: Run to verify failure** — `npx vitest run tests/notification-host.test.ts` → FAIL (missing component).

- [ ] **Step 3: Implement `src/components/NotificationHost.vue`** — render `notifications.items` as a bottom-pinned dismissible stack; `role="alert"` + `aria-live="assertive"` for `error`, else `role="status"` + `aria-live="polite"`; kind colors (error `bg-red-500/20 text-red-200`, warning `bg-amber-500/15 text-amber-200`, success `bg-emerald-500/15 text-emerald-200`, info `bg-white/10 text-slate-200`); each row `data-testid="notification"` with a `data-testid="notification-dismiss"` ✕ calling `notifications.dismiss(item.id)`. Container `data-testid="notification-host"`, `class="pointer-events-none absolute inset-x-3 bottom-3 z-10 flex flex-col gap-1"` with each toast `pointer-events-auto`.

```vue
<script setup lang="ts">
import { useNotificationsStore } from "../stores/notifications";
const notifications = useNotificationsStore();
const cls: Record<string, string> = {
  error: "bg-red-500/20 text-red-200",
  warning: "bg-amber-500/15 text-amber-200",
  success: "bg-emerald-500/15 text-emerald-200",
  info: "bg-white/10 text-slate-200",
};
</script>
<template>
  <div v-if="notifications.items.length" data-testid="notification-host"
       class="pointer-events-none absolute inset-x-3 bottom-3 z-10 flex flex-col gap-1">
    <div v-for="item in notifications.items" :key="item.id" data-testid="notification"
         :role="item.kind === 'error' ? 'alert' : 'status'"
         :aria-live="item.kind === 'error' ? 'assertive' : 'polite'"
         :class="['pointer-events-auto flex items-start justify-between gap-2 rounded-lg px-2 py-1 text-xs', cls[item.kind]]">
      <span class="min-w-0 break-words">{{ item.message }}</span>
      <button type="button" data-testid="notification-dismiss" aria-label="Dismiss"
              class="shrink-0 cursor-pointer rounded p-0.5 hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-white/40"
              @click="notifications.dismiss(item.id)">
        <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" aria-hidden="true"><path d="M18 6 6 18M6 6l12 12"/></svg>
      </button>
    </div>
  </div>
</template>
```

- [ ] **Step 4: Mount in `ActionPanel.vue`** — import `NotificationHost`; add `<NotificationHost />` as the last child of the root `relative` div (line ~65). **Delete** the `capture.error` `<p>` (`v-if="view === 'list' && capture.error"`, ~:172-177) and the `capture.warning` `<p>` (`v-if="view === 'list' && capture.status === 'idle' && capture.warning"`, ~:179-184). Leave `store.error` (vault-list error) and the `RecordingBar :warning` prop untouched.

- [ ] **Step 5: Run to verify pass** — `npx vitest run tests/notification-host.test.ts tests/action-panel.test.ts` → PASS. Then `npm test` + `npm run build`.
- [ ] **Step 6: Commit** — `git commit -am "feat(ui): NotificationHost overlays every panel view (replaces list-only banners)"`
- [ ] **Step 7: Push**

---

### Task 4: Buddy speaks the failure reason

**Files:**
- Modify: `src/roots/useBuddyAnnouncements.ts`, `src/roots/buddyMessages.ts` (paths per the audit; confirm by opening)
- Test: the existing buddy-announcements test file (mirror it)

**Interfaces:**
- Consumes: `capture.error` (retained, carries the message) and a failed job's `job.error`.

- [ ] **Step 1: Write failing test** — asserting that when `capture.error` is set to a reason, the announced/spoken message contains a (possibly truncated) form of that reason, not the generic "Hmm, that didn't work 😕". Mirror the existing announcement test's harness.
- [ ] **Step 2: Run to verify failure.**
- [ ] **Step 3: Implement** — `buddyMessages.ts`: change `failureMessage()` to accept an optional `reason?: string` and return e.g. `reason ? \`Hmm — ${truncate(reason, 60)} 😕\` : "Hmm, that didn't work 😕"`. `useBuddyAnnouncements.ts`: pass the current `capture.error` (or the failed job's `error`) into `failureMessage(...)`. Add a small `truncate(s, n)` helper (ellipsis past `n`).
- [ ] **Step 4: Run to verify pass** — the buddy test + `npm test`.
- [ ] **Step 5: Commit** — `git commit -am "feat(ui): buddy speaks the failure reason, not a generic line"`
- [ ] **Step 6: Push**

---

### Task 5: Store handlers + types for the new backend signals (inert until Task 12)

**Files:**
- Modify: `src/types.ts` (add `CaptureTranscribeSkipped`; extend `CaptureSaved`), `src/stores/capture.ts` (a `capture:transcribeSkipped` listener; read the new `capture:saved` fields)
- Test: `tests/capture-store.test.ts`

**Interfaces:**
- Consumes: the event contracts in Global Constraints; `useNotificationsStore()`.
- Produces: on `capture:transcribeSkipped{mp3,message}` → `upsert(mp3, { phase: "done", progress: 1 })` (a complete transcript exists) AND `notifications.warning(message)`. On `capture:saved` with `endedEarly === true` or `warning` present → `notifications.warning(...)` (in addition to today's save handling).

- [ ] **Step 1: Write failing tests**

```ts
it("transcribeSkipped keeps the transcript complete and warns", async () => {
  const capture = useCaptureStore(); const notes = useNotificationsStore();
  await capture.init();
  await emit("capture:transcribeSkipped", { mp3: "/v/a.mp3", message: "kept your existing transcript — not overwritten" });
  expect(capture.transcriptions["/v/a.mp3"].phase).toBe("done");
  expect(notes.items.some((i) => i.kind === "warning" && i.message.includes("kept your existing transcript"))).toBe(true);
});

it("an early-stopped save raises a warning with the reason", async () => {
  const capture = useCaptureStore(); const notes = useNotificationsStore();
  await capture.init();
  await emit("capture:saved", { mp3: "/v/a.mp3", note: null, endedEarly: true, warning: "recording ended early: disk full" });
  expect(notes.items.some((i) => i.kind === "warning" && i.message.includes("disk full"))).toBe(true);
});
```

- [ ] **Step 2: Run to verify failure.**
- [ ] **Step 3: Implement**:
  - `src/types.ts`: `export interface CaptureTranscribeSkipped { mp3: string; message: string; }`; extend `CaptureSaved` to `{ mp3: string; note: string | null; endedEarly?: boolean; warning?: string | null; }` (optional keeps old emitters valid).
  - `capture.ts` `init()`: add `await listen<CaptureTranscribeSkipped>("capture:transcribeSkipped", (e) => { this.upsert(e.payload.mp3, { phase: "done", progress: 1 }); useNotificationsStore().warning(e.payload.message); });`
  - `capture:saved` handler: after the existing body, add `if (event.payload.endedEarly || event.payload.warning) useNotificationsStore().warning(\`Recording ended early: ${event.payload.warning ?? "saved what we had"}\`);`
- [ ] **Step 4: Run to verify pass** — new cases + `npm test`.
- [ ] **Step 5: Commit** — `git commit -am "feat(ui): handle transcribeSkipped + early-stop save reasons"`
- [ ] **Step 6: Push**

---

### Task 6: `Transcriptions.vue` — vault name, indeterminate download, store-held stuck timer

**Files:**
- Modify: `src/components/Transcriptions.vue`; `src/stores/capture.ts` (add a store-held stuck timestamp so it survives the view remount)
- Test: `tests/transcriptions.test.ts`

- [ ] **Step 1: Write failing tests** — (a) an active/queued row for a known vault renders the vault **name** (seed `useVaultsStore().vaults` with `{ id, name }`), not the raw hex id; (b) a `downloading` job with `progress === null` shows "Downloading model…"/spinner and **no** "0%"; (c) the stuck hint, once shown, is not reset by a component remount (the timestamp lives in the store).
- [ ] **Step 2: Run to verify failure.**
- [ ] **Step 3: Implement**:
  - **B1:** import `useVaultsStore`; `vaultName(id) = vaults.find(v => v.id === id)?.name ?? id`; render `vaultName(job.vaultId)`.
  - **B2:** in the phase label / progress area, when `phase === "downloading" && job.progress == null`, render "Downloading model…" with the indeterminate spinner and **omit** the `%` text (guard the `Math.round(progress*100)+"%"` behind `progress != null`).
  - **B4:** move the stuck tracking into the store: add `stuckSinceMs: Record<string, number>` (keyed by active mp3) updated on progress change, or a single `activeStuckSinceMs` reset only on a real progress delta; `Transcriptions.vue` reads it instead of a component-local `ref`. Keep the 2-minute threshold.
- [ ] **Step 4: Run to verify pass** — `npx vitest run tests/transcriptions.test.ts` + `npm test`.
- [ ] **Step 5: Commit** — `git commit -am "feat(ui): Transcriptions shows vault names, honest download state, persistent stuck hint"`
- [ ] **Step 6: Push**

---

### Task 7: Inline failure reason in Recordings + Summary

**Files:**
- Modify: `src/components/Recordings.vue` (failed row `title`), `src/components/TranscriptionSummary.vue` (failed chip `title`)
- Test: `tests/recordings.test.ts`, `tests/transcription-summary.test.ts`

- [ ] **Step 1: Write failing tests** — (a) a Recordings row whose live job carries `job.error` exposes that reason via `title` on the status indicator; (b) the summary's "N failed" chip's `title` contains the newest failed job's `error`.
- [ ] **Step 2: Run to verify failure.**
- [ ] **Step 3: Implement** — Recordings: when `capture.transcriptions[r.mp3]?.error` exists, use it as the status `:title` (fallback to the current label). Summary: compute `newestFailedReason` from `capture.finishedTranscriptions.find(j => j.phase === "failed")?.error` and bind it as the chip `:title`. No layout change.
- [ ] **Step 4: Run to verify pass** — both files + `npm test`.
- [ ] **Step 5: Commit** — `git commit -am "feat(ui): show the transcription failure reason in the recordings list and summary"`
- [ ] **Step 6: Push**

---

### Task 8: RecordMode save error + `waitingForRecording` re-sync

**Files:**
- Modify: `src/components/RecordMode.vue` (route the swallowed `set_capture_config` save failure), `src/stores/capture.ts` (re-sync `waitingForRecording`)
- Test: `tests/record-mode.test.ts`, `tests/capture-store.test.ts`

- [ ] **Step 1: Write failing tests** — (a) when `set_capture_config` rejects from RecordMode, a notification is raised (today it's `logWarning` only); (b) `waitingForRecording` reflects the latest state after a relevant capture event, not just the init seed.
- [ ] **Step 2: Run to verify failure.**
- [ ] **Step 3: Implement**:
  - RecordMode `persist()` catch: add `useNotificationsStore().error(\`Couldn't save transcription settings: ${String(e)}\`);` (keep `logWarning`).
  - **B3:** re-sync `waitingForRecording` — clear it to `false` on `capture:transcribing`/`capture:transcribeProgress` (a job is now running, not waiting) and on `capture:saved`; if a cleaner backend signal is unavailable, derive it (`waitingForRecording && !activeTranscription`). Keep it defensive; document the chosen trigger in a comment.
- [ ] **Step 4: Run to verify pass** — both files + `npm test` + `npm run build`.
- [ ] **Step 5: Commit** — `git commit -am "feat(ui): surface RecordMode save failures; keep waitingForRecording fresh"`
- [ ] **Step 6: Push**

---

### Task 9: Frontend cleanups — shared formatter, scoped ids, seed/bound tests, summary a11y tests

**Files:**
- Create: `src/utils/formatDuration.ts`
- Modify: `src/components/RecordingBar.vue`, `src/components/Transcriptions.vue` (use the shared formatter), `src/components/TranscriptionSettings.vue` (scoped ids)
- Test: `tests/format-duration.test.ts`, `tests/capture-store.test.ts` (E4), `tests/transcription-summary.test.ts` (E5)

- [ ] **Step 1: Write failing tests** —
  - `formatDuration(ms)` → `mm:ss` (e.g. `0→"0:00"`, `65000→"1:05"`, `3661000→"61:01"` — match RecordingBar's current output exactly; open it first to copy the format).
  - **E4:** `activeSeedProgress` with a `downloading` active job that has `received`/`total` seeds the byte ratio; assert `transcriptions` never exceeds the getter cap under a long run (fire many finished jobs, assert `finishedTranscriptions` ≤ `MAX_FINISHED`; if a hard prune is added, assert the map itself is bounded).
  - **E5:** `TranscriptionSummary` renders `role="button"`, is keyboard-activatable (`keydown.enter`/`space` opens the Transcriptions view), and the failed/active color classes are present.
- [ ] **Step 2: Run to verify failure.**
- [ ] **Step 3: Implement**:
  - **C2:** `src/utils/formatDuration.ts` `export function formatDuration(ms: number): string` — extract RecordingBar's exact mm:ss logic; replace the inline formatters in `RecordingBar.vue` and `Transcriptions.vue` with it.
  - **C3:** `TranscriptionSettings.vue` — prefix the DOM ids (`capture-transcribe-toggle`, etc.) with an optional `idPrefix` prop (default keeps today's ids) so two instances can't collide; update `:for`/`:id` bindings.
  - **E4/E5:** implementation only if a test exposes a real gap (e.g. add a hard map-prune in `upsert`/a finished-eviction if the map is genuinely unbounded); otherwise the tests just lock current behavior.
- [ ] **Step 4: Run to verify pass** — the three test files + `npm test` + `npm run build`.
- [ ] **Step 5: Commit** — `git commit -am "refactor(ui): shared duration formatter, scoped settings ids, seed/bound + a11y tests"`
- [ ] **Step 6: Push**

---

### Task 10: `TranscribeOutcome` — `SkippedForeign` is no longer success (transcribe + core tests)

**Files:**
- Modify: `src-tauri/transcribe/src/lib.rs` (`transcribe_recording` return type; update its tests)
- Test: `src-tauri/transcribe/src/lib.rs` `#[cfg(test)]`; `src-tauri/core/src/transcript.rs` `#[cfg(test)]` (E1)

**Interfaces:**
- Produces: `pub enum TranscribeOutcome { Written(PathBuf), SkippedForeign(PathBuf) }`; `transcribe_recording(...) -> Result<TranscribeOutcome, TranscribeError>`. The `force` branch returns `Written`; the `replace_if_ours` branch returns `Written`/`SkippedForeign` per `ReplaceOutcome`.

> **PUSH-HELD (compile-bracket):** this changes `transcribe_recording`'s return type, breaking the shell caller (`process_transcription` in `capture_commands.rs`). Commit + verify locally, but **do not push** until Task 12. See Global Constraints.

- [ ] **Step 1: Write failing tests** (transcribe/src/lib.rs) —
  - update `transcribe_writes_the_sidecar`: `assert!(matches!(transcribe_recording(...).unwrap(), TranscribeOutcome::Written(p) if p == transcript_path(&mp3)));`
  - add `skips_a_complete_sidecar_and_reports_it`: pre-write a `complete` sidecar, run non-force, assert `matches!(..., TranscribeOutcome::SkippedForeign(_))` and the original body ("OLD") remains.
  - update `force_regenerates_a_complete_transcript` and the other `.unwrap()`-path assertions for the new return type.
  - **E2:** add a `FakeOk`-based precancel isolation test: pre-cancel the token, run with `FakeOk` (which ignores the token), assert `Err(TranscribeError::Cancelled)` — proving the *after-decode* bail fired (FakeOk would otherwise return Ok).
  - **E3:** assert `on_progress` is actually invoked: pass a boxed closure incrementing a shared `Arc<AtomicUsize>`, run `FakeOk`, assert the counter > 0.
  - **E1** (core/src/transcript.rs): a dedicated `render_cancelled` injection test — a name needing YAML-quoting renders a safely-quoted `cancelled` sidecar.
- [ ] **Step 2: Run to verify failure** — `cd src-tauri && cargo test -p vault_buddy_transcribe -p vault_buddy_core` → the updated/new tests FAIL to compile/pass.
- [ ] **Step 3: Implement** — add the `TranscribeOutcome` enum; change the return type; `force` → `Ok(TranscribeOutcome::Written(path))`; the `replace_if_ours` match arms → `Written => Ok(Written(path))`, `SkippedForeign => { log::warn!(...); Ok(SkippedForeign(path)) }`.
- [ ] **Step 4: Run to verify pass** — `cd src-tauri && cargo test -p vault_buddy_transcribe -p vault_buddy_core && cargo fmt --check && cargo clippy -p vault_buddy_transcribe -p vault_buddy_core --all-targets -- -D warnings`.
- [ ] **Step 5: Commit (DO NOT PUSH)** — `git commit -am "feat(transcribe): TranscribeOutcome distinguishes a written vs skipped-foreign sidecar"`

---

### Task 11: Surface a failed companion note (capture crate)

**Files:**
- Modify: `src-tauri/capture/src/session.rs` (the note-write failure path ~:415-421; set `Outcome.warning`)
- Test: `src-tauri/capture/src/session.rs` `#[cfg(test)]` (ALSA is present — builds/tests locally)

**Interfaces:**
- Produces: on a companion-note write failure after a successful audio save, `Outcome.warning` carries `"note not written: <reason>"` (today it is `log::warn!`-only with `note = None`). `Outcome`'s existing `warning`/`ended_early` fields are unchanged in shape.

> **PUSH-HELD (compile-bracket):** part of the Task 10–12 bracket — commit, do not push until Task 12.

- [ ] **Step 1: Write a failing test** — exercise the note-write branch with a note directory made unwritable (or inject a write error via the existing seam) and assert the returned `Outcome.warning` is `Some(_)` containing the reason, while the audio path still finalizes (`mp3` present). Mirror `session.rs`'s existing test setup; if no seam exists, add the smallest one (e.g. factor the note write behind a testable helper) — do not broaden scope beyond that.
- [ ] **Step 2: Run to verify failure** — `cd src-tauri && cargo test -p vault_buddy_capture` → FAIL.
- [ ] **Step 3: Implement** — in the note-write failure arm, set/append `outcome.warning` with `format!("note not written: {e}")` (keep the `log::warn!` and `note = None`).
- [ ] **Step 4: Run to verify pass** — `cd src-tauri && cargo test -p vault_buddy_capture && cargo fmt --check && cargo clippy -p vault_buddy_capture --all-targets -- -D warnings`.
- [ ] **Step 5: Commit (DO NOT PUSH)** — `git commit -am "feat(capture): a failed companion note surfaces as an Outcome warning"`

---

### Task 12: Shell — wire the honesty fixes + findings, close the bracket

**Files:**
- Modify: `src-tauri/src/capture_commands.rs` — `process_transcription` (handle `TranscribeOutcome`), `emit_saved` (new payload), the two `TranscriptionState` commands (A1/A2), `set_phase` helper (C1), `open_transcript` doc (D1). (Windows-only compile; `cargo fmt --check` is the only local gate.)

**Interfaces:**
- Consumes: `TranscribeOutcome` (Task 10), `Outcome.warning` (Task 11).
- Produces the event contracts in Global Constraints (`capture:transcribeSkipped`, extended `capture:saved`).

> **Closes the bracket.** After this task compiles (fmt-check clean) and the frontend is green, **push Tasks 10 + 11 + 12 together**: `git push origin claude/vault-buddy-local-stt-rktgl2`. Watch the `windows-app` job — it is the compile gate for all three.

- [ ] **Step 1: `process_transcription` — outcome handling.** Where it currently `Ok(path) =>` emits `capture:transcribed`, match `TranscribeOutcome`: `Written(_)` → emit `capture:transcribed` (unchanged); `SkippedForeign(_)` → emit `capture:transcribeSkipped { mp3, message: "kept your existing transcript — not overwritten" }` (a distinct honest signal, not success).
- [ ] **Step 2: `emit_saved` — carry the reason.** Add `endedEarly: outcome.ended_early` and `warning: outcome.warning` to the emitted `capture:saved` payload (exact keys per Global Constraints).
- [ ] **Step 3: A1 — poison-safe locks.** In `cancel_transcription` and `transcription_queue_status`, replace `.lock().unwrap()` on `TranscriptionState` with the file's `lock_ignoring_poison` helper (grep it in the same file) so a poisoned mutex never aborts across the WebView2 FFI.
- [ ] **Step 4: A2 — drop-before-`is_recording`.** In `transcription_queue_status`, snapshot the fields it needs and drop the `TranscriptionState` guard **before** calling `is_recording()` (which locks `CaptureState`), matching `run_recovery`'s discipline.
- [ ] **Step 5: C1 — `set_phase` helper.** Extract the ~3× repeated "lock, set the active job's phase, unlock, emit" block into one `set_phase(...)` helper and call it at the three sites.
- [ ] **Step 6: D1 — stale comment.** In `open_transcript`'s doc/comment, remove the reference to the deleted `TranscriptionStatus` component.
- [ ] **Step 7: Local check** — `cd src-tauri && cargo fmt --check`. (Full shell compile is the CI `windows-app` job; it cannot build on Linux — no webkit2gtk.)
- [ ] **Step 8: Commit** — `git commit -am "feat(shell): emit skipped/early-stop reasons; poison-safe locks; set_phase helper"`
- [ ] **Step 9: Push the bracket (10+11+12)** — `git push origin claude/vault-buddy-local-stt-rktgl2`; confirm `windows-app` goes green.

---

## Self-Review

**Spec coverage:** notification store (T1) + host (T3) + routing (T2) + new-signal handling (T5) = the surface; buddy reason (T4); inline reasons (T7); Transcriptions polish incl. vault-name (T6); RecordMode error + `waitingForRecording` (T8); SkippedForeign honesty (T10+T12); early-stop reason (T11+T12 emit_saved); note-write (T11); findings A1/A2/C1/D1 (T12), B1/B2/B4 (T6), B3 (T8), C2/C3 (T9), E1/E2/E3 (T10), E4/E5 (T9). All spec sections mapped.

**Placeholder scan:** none — every step names the file, the exact edit, and the command. Pattern-following steps (T4/T6/T7/T8/T11) point at the exact file + the mirror to copy and give the test assertions; novel code (T1, T3) is shown in full.

**Type consistency:** the notifications API, `Notification`, the two event payloads, and `TranscribeOutcome` are pinned once in Global Constraints and referenced verbatim by every consuming task.
