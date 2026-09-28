<script setup lang="ts">
/**
 * The learning center's **Progress and preferences** (Task 57; F-47;
 * ONBOARDING.md § State and persistence; SCREENS 11: "Progress-file
 * fallback contains no media").
 *
 * - **Dim the editor around the highlighted control** and **Motion** — the
 *   two presentation preferences, kept by Start over.
 * - **Save progress file…** hands the CURRENT progress to Rust, which
 *   validates it like any save and opens its own save dialog: the portable
 *   backup, and the only way to keep progress that is "Session only".
 * - **Load progress file…** (Task 57's "Restore progress file…", renamed to
 *   the concept's wording by visual-parity Task 23; the store verb is still
 *   `restoreFrom`, the testid `learning-restore-file`) — Rust opens its own
 *   open dialog and returns the file's progress only if it passes the same
 *   validation. It is then
 *   installed PAUSED (`restoreFrom`): restoring never starts the guide,
 *   opens a device or touches the project. A refused or dismissed restore
 *   changes nothing.
 *
 * The status line is set only from a reply — "Saved to" names the file Rust
 * reported writing. Every message renders as text.
 *
 * Visual-parity Task 23 (concept §9.3): the learning center's footer, a
 * "Progress & preferences" disclosure whose summary says where progress is
 * kept (design D10: "Progress remembered on this PC", or "Session only"
 * when it cannot be stored).
 */
import { computed, ref } from "vue";

import { useEditorOnboardingStore } from "../../../stores/editorOnboarding";
import { useEditorProjectStore } from "../../../stores/editorProject";
import EditorIcon from "../icons/EditorIcon.vue";

const onboarding = useEditorOnboardingStore();
const project = useEditorProjectStore();

const busy = ref(false);
const BUSY_REASON = "Waiting for the file dialog to close.";
const status = ref<{ text: string; alert: boolean }>({ text: "", alert: false });

const storageNote = computed(() =>
  onboarding.sessionOnly
    ? "Progress cannot be stored on this PC right now and lasts until the editor closes. Save a progress file to keep it."
    : "Progress is stored for you on this PC, apart from your projects.",
);
const storageBadge = computed(() => (onboarding.sessionOnly ? "Session only" : "Progress remembered on this PC"));

function messageOf(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

function onDimming(event: Event): void {
  onboarding.setPreferences({ dimming: (event.target as HTMLInputElement).checked });
}
function onMotion(event: Event): void {
  const motion = (event.target as HTMLSelectElement).value as "system" | "reduced" | "full";
  onboarding.setPreferences({ motion });
}

async function saveFile(): Promise<void> {
  busy.value = true;
  try {
    const name = await project.port.exportGuideProgress(onboarding.snapshot());
    status.value = name === null
      ? { text: "Nothing was saved — the file dialog was closed.", alert: false }
      : { text: `Saved to ${name}. It holds lesson progress only — no project or media.`, alert: false };
  } catch (e) {
    status.value = { text: `The progress file was not saved. ${messageOf(e)}`, alert: true };
  } finally {
    busy.value = false;
  }
}

async function restoreFile(): Promise<void> {
  busy.value = true;
  try {
    const restored = await project.port.importGuideProgress();
    if (restored === null) {
      status.value = { text: "Nothing was restored — the file dialog was closed.", alert: false };
      return;
    }
    onboarding.restoreFrom(restored);
    status.value = { text: "Guide progress restored. Resume the walkthrough, or pick a lesson.", alert: false };
  } catch (e) {
    status.value = { text: `Nothing was restored. ${messageOf(e)}`, alert: true };
  } finally {
    busy.value = false;
  }
}
</script>

<template>
  <footer class="border-t border-line bg-app px-[34px] pt-[17px] pb-5 text-[11px] text-fg-secondary">
    <details data-testid="learning-preferences">
      <summary class="flex min-h-6 cursor-pointer items-center justify-between gap-3 text-xs text-fg">
        Progress &amp; preferences
        <span
          data-testid="learning-storage"
          class="text-[10px] text-fg-muted"
        >{{ storageBadge }}</span>
      </summary>
      <p class="my-3 leading-[1.7] text-fg-muted">
        {{ storageNote }}
      </p>
      <label class="my-[9px] flex items-center gap-2">
        <input
          type="checkbox"
          data-testid="learning-pref-dimming"
          :checked="onboarding.progress.preferences.dimming"
          @change="onDimming"
        >
        Dim the surrounding editor during a step
      </label>
      <label class="mb-3.5 flex items-center gap-2">
        Motion
        <select
          data-testid="learning-pref-motion"
          class="text-[11px]"
          :value="onboarding.progress.preferences.motion"
          @change="onMotion"
        >
          <option value="system">Follow Windows</option>
          <option value="reduced">Reduced</option>
          <option value="full">Full</option>
        </select>
      </label>
      <div class="flex flex-wrap gap-[9px]">
        <button
          type="button"
          data-testid="learning-save-file"
          :disabled="busy"
          :title="busy ? BUSY_REASON : undefined"
          class="flex items-center gap-1.5 rounded-[7px] border border-line bg-panel px-2.5 py-1.5 text-[11px] text-fg"
          @click="saveFile"
        >
          <EditorIcon
            name="save"
            :size="15"
          />
          Save progress file…
        </button>
        <button
          type="button"
          data-testid="learning-restore-file"
          :disabled="busy"
          :title="busy ? BUSY_REASON : undefined"
          class="flex items-center gap-1.5 rounded-[7px] border border-line bg-panel px-2.5 py-1.5 text-[11px] text-fg"
          @click="restoreFile"
        >
          <EditorIcon
            name="folder"
            :size="15"
          />
          Load progress file…
        </button>
      </div>
      <small class="mt-3 block text-[10px] text-fg-muted">
        Progress files contain lesson identifiers and these preferences only, never your project or media.
      </small>
      <p
        data-testid="learning-file-status"
        :role="status.alert ? 'alert' : 'status'"
        class="mt-2 min-h-4 break-words"
        :class="status.alert ? 'text-danger-fg' : 'text-fg-secondary'"
      >
        {{ status.text }}
      </p>
    </details>
  </footer>
</template>
