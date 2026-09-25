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
 * - **Restore progress file…** — Rust opens its own open dialog and returns
 *   the file's progress only if it passes the same validation. It is then
 *   installed PAUSED (`restoreFrom`): restoring never starts the guide,
 *   opens a device or touches the project. A refused or dismissed restore
 *   changes nothing.
 *
 * The status line is set only from a reply — "Saved to" names the file Rust
 * reported writing. Every message renders as text.
 */
import { computed, ref } from "vue";

import { useEditorOnboardingStore } from "../../../stores/editorOnboarding";
import { useEditorProjectStore } from "../../../stores/editorProject";
import AppButton from "../../ui/AppButton.vue";

const onboarding = useEditorOnboardingStore();
const project = useEditorProjectStore();

const busy = ref(false);
const status = ref<{ text: string; alert: boolean }>({ text: "", alert: false });

const storageNote = computed(() =>
  onboarding.sessionOnly
    ? "Progress cannot be stored on this device right now and lasts until the editor closes. Save a progress file to keep it."
    : "Progress is stored for you on this computer, apart from your projects.",
);

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
  <section class="flex flex-col gap-2 border-t border-line pt-3 text-xs">
    <h3 class="font-semibold text-fg-secondary">
      Progress and preferences
    </h3>
    <p class="text-fg-muted">
      {{ storageNote }}
    </p>
    <label class="flex items-center gap-2 text-fg-secondary">
      <input
        type="checkbox"
        data-testid="learning-pref-dimming"
        class="h-4 w-4 accent-violet-500"
        :checked="onboarding.progress.preferences.dimming"
        @change="onDimming"
      >
      Dim the editor around the highlighted control
    </label>
    <label class="flex items-center gap-2 text-fg-secondary">
      Motion
      <select
        data-testid="learning-pref-motion"
        class="rounded-control border border-line bg-raised px-1 py-0.5 text-fg"
        :value="onboarding.progress.preferences.motion"
        @change="onMotion"
      >
        <option value="system">Follow Windows</option>
        <option value="reduced">Reduced</option>
        <option value="full">Full</option>
      </select>
    </label>
    <div class="flex flex-wrap gap-2">
      <AppButton
        size="sm"
        variant="secondary"
        data-testid="learning-save-file"
        :disabled="busy"
        @click="saveFile"
      >
        Save progress file…
      </AppButton>
      <AppButton
        size="sm"
        variant="secondary"
        data-testid="learning-restore-file"
        :disabled="busy"
        @click="restoreFile"
      >
        Restore progress file…
      </AppButton>
    </div>
    <p class="text-fg-subtle">
      A progress file holds lesson names and these preferences only — never your project or media.
    </p>
    <p
      data-testid="learning-file-status"
      :role="status.alert ? 'alert' : 'status'"
      class="min-h-4 break-words"
      :class="status.alert ? 'text-danger-fg' : 'text-fg-secondary'"
    >
      {{ status.text }}
    </p>
  </section>
</template>
