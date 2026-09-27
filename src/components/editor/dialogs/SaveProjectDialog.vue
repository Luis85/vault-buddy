<script setup lang="ts">
/**
 * Save a copy as project file (Task 39; F-40; SCREENS 08; A17; design D8:
 * Save project itself commits with no dialog — this is the Project menu's
 * "Save a copy as project file…"). Explains the two formats — portable (a
 * `.vbproject.zip` carrying the available originals, including those a
 * retained render snapshot still uses) and lightweight (a `.vbproject.json`
 * whose originals are reconnected later) — then hands the choice to Rust,
 * which opens its own save dialog (`useProjectExport`).
 *
 * Visual-parity Task 21 (concept spec §9.5, screen 08): the intro tile,
 * the **Project name** (Rust's save dialog suggests the file name from the
 * tutorial's title, so a changed name RENAMES the tutorial first — one
 * `rename` edit, the concept's own "Rename project" step — and the copy is
 * of the renamed revision), the two format cards (`SaveCopyFormats`), the
 * checklist of what a copy keeps, and Keep editing · **Save copy**. The
 * concept's "Include available rendered videos" checkbox has no native
 * backend — a project file never carries rendered videos — so its slot
 * says where they stay instead of offering a box that could not be
 * honoured (design D14).
 *
 * The status line (`SaveCopyStatus`) reports pending, success, failure and
 * cancel, and says "Saved to <file name>" ONLY from a matching receipt. A
 * refused rename is the store's `lastError`, shown in that status line;
 * this dialog claims it while open (`useInlineLastError`, ruling T7-1) so
 * the shell does not toast it too. Escape and the backdrop close the dialog
 * except while the rename or the save is pending, whose reply the dialog
 * still owes the user. Every Rust message renders as text, never markup.
 */
import { computed, ref, watch } from "vue";

import { useInlineLastError } from "../../../composables/useInlineLastError";
import { useProjectExport } from "../../../composables/useProjectPackage";
import type { PackageFormat } from "../../../editorTypes";
import { useEditorProjectStore } from "../../../stores/editorProject";
import EditorIcon from "../icons/EditorIcon.vue";
import DialogHost from "../shell/DialogHost.vue";
import DialogButton from "./DialogButton.vue";
import SaveCopyFormats from "./SaveCopyFormats.vue";
import SaveCopyStatus from "./SaveCopyStatus.vue";

const props = defineProps<{ open: boolean; initialFormat: PackageFormat }>();
const emit = defineEmits<{ (e: "close"): void }>();

const editorProject = useEditorProjectStore();
const exporter = useProjectExport();
const { state } = exporter;
const inline = useInlineLastError(() => props.open);
const format = ref<PackageFormat>(props.initialFormat);
const name = ref("");
const renaming = ref(false);

/** What every copy keeps (concept §9.5 `.project-save-checklist`). */
const KEPT = ["All tracks & clips", "Fades & teaching layers", "Mixer & chapter markers", "Playhead & workspace layout"];
const PREPARING = "Preparing the project file…";

watch(
  () => props.open,
  (open) => {
    if (!open) return;
    format.value = props.initialFormat;
    name.value = editorProject.snapshot?.title ?? "";
    exporter.reset();
  },
  { immediate: true },
);

const pending = computed(() => renaming.value || state.value.phase === "pending");
/** Keep editing and the ✕ wait for a reply the dialog still owes. */
const waitReason = computed(() => (pending.value ? PREPARING : null));
const reason = computed<string | null>(() => {
  if (pending.value) return PREPARING;
  return name.value.trim() ? null : "Give the project a name.";
});

/** Rename first when the name changed; the copy is of what is on screen. */
async function save(): Promise<void> {
  if (reason.value) return;
  inline.clear();
  exporter.reset();
  const title = name.value.trim();
  if (title !== editorProject.snapshot?.title) {
    renaming.value = true;
    const ok = await editorProject.execute({ kind: "rename", title });
    renaming.value = false;
    if (!ok) return;
  }
  await exporter.run(format.value);
}

function close(): void {
  if (!pending.value) emit("close");
}
</script>

<template>
  <DialogHost
    :open="open"
    label="Save a copy as project file"
    :closable="!pending"
    :close-reason="waitReason"
    @close="close"
  >
    <template #title>
      Save a copy as project file
    </template>
    <template #subtitle>
      Keep the workspace in a file. Continue whenever you are ready.
    </template>

    <div
      data-testid="save-project-dialog"
      class="flex flex-col gap-4"
    >
      <div
        data-testid="save-project-intro"
        class="mb-1.5 flex items-start gap-4"
      >
        <span class="shrink-0 rounded-xl border border-line bg-accent-bg p-3 text-accent">
          <EditorIcon
            name="folder"
            :size="28"
          />
        </span>
        <div>
          <h3 class="mb-[7px] text-[15px] font-semibold text-fg">
            Your workspace. Ready to continue.
          </h3>
          <p class="text-[11px] leading-[1.6] text-fg-secondary">
            Save an editable copy without rendering. Nothing is flattened, and your original media is never changed.
          </p>
        </div>
      </div>

      <label class="flex min-w-0 flex-col gap-[5px] text-[10px] text-fg-secondary">
        Project name
        <input
          v-model="name"
          data-testid="save-project-name"
          type="text"
          maxlength="160"
          :disabled="pending"
          class="text-xs"
        >
      </label>

      <SaveCopyFormats
        v-model="format"
        :disabled="pending"
      />

      <p
        data-testid="save-project-products"
        class="text-[10px] leading-[1.6] text-fg-muted"
      >
        Rendered videos stay in this project's workspace. Each render's record and the edit it was made from are
        always kept in the file.
      </p>

      <ul
        data-testid="save-project-checklist"
        class="my-1.5 grid grid-cols-2 gap-3 text-[11px] text-fg max-[560px]:grid-cols-1"
      >
        <li
          v-for="item in KEPT"
          :key="item"
          class="flex items-center gap-2"
        >
          <EditorIcon
            name="check"
            :size="15"
            class="shrink-0 text-audio"
          />
          {{ item }}
        </li>
      </ul>

      <SaveCopyStatus
        :format="format"
        :state="state"
        :refusal="inline.error.value?.message ?? null"
      />
    </div>

    <template #footer>
      <span
        v-if="reason && !pending"
        data-testid="save-project-reason"
        class="mr-auto text-[10px] text-fg-muted"
      >{{ reason }}</span>
      <DialogButton
        data-testid="save-project-cancel"
        :reason="waitReason"
        @click="close"
      >
        Keep editing
      </DialogButton>
      <DialogButton
        variant="primary"
        icon="save"
        data-testid="save-project-confirm"
        :reason="reason"
        @click="save"
      >
        Save copy
      </DialogButton>
    </template>
  </DialogHost>
</template>
