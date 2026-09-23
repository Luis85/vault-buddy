<script setup lang="ts">
/**
 * Save a project file (Task 39; F-40; SCREENS 08 "Save editable project";
 * A17). Explains the two formats — portable (a `.vbproject.zip` carrying
 * the available originals, including those a retained render snapshot
 * still uses) and lightweight (a `.vbproject.json` whose originals are
 * reconnected later) — then hands the choice to Rust, which opens its own
 * save dialog (`useProjectExport`).
 *
 * The heading and the actions stay reachable while the body scrolls
 * (sticky inside `DialogHost`'s own scroll box); the radios are ordinary
 * full-size inputs inside full-width labels. The status line reports
 * pending, success, failure and cancel, and says "Saved to <file name>"
 * ONLY from a matching receipt. Escape and the backdrop close the dialog
 * except while a save is pending, whose reply the dialog still owes the
 * user. Every Rust message renders as text (mustache), never as markup.
 */
import { computed, ref, watch } from "vue";

import { useProjectExport } from "../../../composables/useProjectPackage";
import type { PackageFormat } from "../../../editorTypes";
import AppButton from "../../ui/AppButton.vue";
import DialogHost from "../shell/DialogHost.vue";

const props = defineProps<{ open: boolean; initialFormat: PackageFormat }>();
const emit = defineEmits<{ (e: "close"): void }>();

const exporter = useProjectExport();
const { state } = exporter;
const format = ref<PackageFormat>(props.initialFormat);

watch(
  () => props.open,
  (open) => {
    if (!open) return;
    format.value = props.initialFormat;
    exporter.reset();
  },
);

const pending = computed(() => state.value.phase === "pending");
const confirmLabel = computed(() =>
  format.value === "portable" ? "Save portable copy…" : "Save lightweight copy…",
);

/** The one status line — derived from the export's own state, never a
 * timer, so "Saved to" can only follow a receipt. */
const status = computed<{ text: string; alert: boolean }>(() => {
  const s = state.value;
  switch (s.phase) {
    case "pending":
      return { text: "Preparing the project file…", alert: false };
    case "success":
      return { text: `Saved to ${s.fileName}`, alert: false };
    case "failure":
      return { text: `The project file was not saved. ${s.message}`, alert: true };
    case "cancelled":
      return { text: "Nothing was saved — the file dialog was closed.", alert: false };
    default:
      return { text: "", alert: false };
  }
});

function save(): void {
  void exporter.run(format.value);
}

function close(): void {
  if (!pending.value) emit("close");
}
</script>

<template>
  <DialogHost
    :open="open"
    label="Save a project file"
    :closable="!pending"
    @close="close"
  >
    <div
      data-testid="save-project-dialog"
      class="flex w-[30rem] max-w-full flex-col gap-3"
    >
      <header class="sticky -top-4 z-10 -mx-4 -mt-4 border-b border-line bg-panel px-4 pb-3 pt-4">
        <h2 class="text-sm font-semibold text-fg">
          Save a project file
        </h2>
        <p class="text-xs text-fg-muted">
          Keep an editable copy outside the editor. Nothing is rendered or flattened, and
          your original media is never changed.
        </p>
      </header>

      <fieldset class="flex flex-col gap-2">
        <legend class="sr-only">
          Project file format
        </legend>
        <label
          class="flex cursor-pointer gap-3 rounded-control border border-line p-3 hover:bg-white/5"
          :class="format === 'portable' ? 'border-focus bg-white/5' : ''"
        >
          <input
            v-model="format"
            type="radio"
            name="save-project-format"
            value="portable"
            data-testid="save-project-format-portable"
            class="mt-0.5 h-4 w-4 accent-violet-500"
          >
          <span class="flex flex-col gap-1">
            <span class="text-sm font-medium text-fg">Portable project (.vbproject.zip)</span>
            <span class="text-xs text-fg-secondary">
              Your edit plus the available original media in one ZIP, up to 200 MiB. Open
              it with “Open a project file” to continue on any computer.
            </span>
          </span>
        </label>
        <label
          class="flex cursor-pointer gap-3 rounded-control border border-line p-3 hover:bg-white/5"
          :class="format === 'lightweight' ? 'border-focus bg-white/5' : ''"
        >
          <input
            v-model="format"
            type="radio"
            name="save-project-format"
            value="lightweight"
            data-testid="save-project-format-lightweight"
            class="mt-0.5 h-4 w-4 accent-violet-500"
          >
          <span class="flex flex-col gap-1">
            <span class="text-sm font-medium text-fg">Lightweight project file (.vbproject.json)</span>
            <span class="text-xs text-fg-secondary">
              Your edit and workspace only — a small file with no media inside.
            </span>
          </span>
        </label>
      </fieldset>

      <p
        v-if="format === 'portable'"
        data-testid="save-project-originals-warning"
        class="rounded-control border border-amber-400/30 bg-amber-400/10 p-2 text-xs text-amber-100"
      >
        A portable file includes your original recordings and imported media — also the
        parts you trimmed or covered. Share a rendered video instead when the people you
        send it to must not receive the originals. Media that is no longer available is
        left out and listed for reconnection when the file is opened.
      </p>
      <p
        v-else
        data-testid="save-project-reconnect-note"
        class="text-xs text-fg-secondary"
      >
        Opening this file on another computer lists every original as missing: keep your
        media, and reconnect it there.
      </p>
      <p class="text-xs text-fg-muted">
        Rendered videos and the editable snapshot behind each one are kept with the
        project, together with any original a snapshot still uses.
      </p>

      <p
        data-testid="save-project-status"
        :role="status.alert ? 'alert' : 'status'"
        class="min-h-4 break-words text-xs"
        :class="status.alert ? 'text-danger-fg' : 'text-fg-secondary'"
      >
        {{ status.text }}
      </p>

      <footer class="sticky -bottom-4 -mx-4 -mb-4 flex justify-end gap-2 border-t border-line bg-panel px-4 pb-4 pt-3">
        <AppButton
          variant="ghost"
          data-testid="save-project-cancel"
          :disabled="pending"
          @click="close"
        >
          Keep editing
        </AppButton>
        <AppButton
          data-testid="save-project-confirm"
          :disabled="pending"
          @click="save"
        >
          {{ confirmLabel }}
        </AppButton>
      </footer>
    </div>
  </DialogHost>
</template>
