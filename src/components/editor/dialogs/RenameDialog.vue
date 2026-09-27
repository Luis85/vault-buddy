<script setup lang="ts">
/**
 * Rename tutorial (visual-parity Task 8; concept spec §9.10 `editDialog`):
 * one Title field, Cancel / Apply. Opened by the header's document title
 * and by the Project menu's "Rename tutorial…". Apply sends one `rename`
 * command; a refused rename leaves the dialog open with the draft, and the
 * refusal is said by the shell's feedback toast (visual-parity Task 7).
 * An unchanged title closes without sending anything; an empty one cannot
 * be applied, and Apply says why.
 */
import { computed, ref, watch } from "vue";

import { useEditorProjectStore } from "../../../stores/editorProject";
import AppButton from "../../ui/AppButton.vue";
import DialogHost from "../shell/DialogHost.vue";

const props = defineProps<{ open: boolean }>();
const emit = defineEmits<(e: "close") => void>();

const editorProject = useEditorProjectStore();
const draft = ref("");
const busy = ref(false);

watch(
  () => props.open,
  (open) => {
    if (open) draft.value = editorProject.snapshot?.title ?? "";
  },
  { immediate: true },
);

const next = computed(() => draft.value.trim());
const reason = computed(() => (next.value ? null : "A title can't be empty."));

async function apply(): Promise<void> {
  if (reason.value || busy.value) return;
  if (next.value === editorProject.snapshot?.title) {
    emit("close");
    return;
  }
  busy.value = true;
  const ok = await editorProject.execute({ kind: "rename", title: next.value });
  busy.value = false;
  if (ok) emit("close");
}

function cancel(): void {
  if (!busy.value) emit("close");
}
</script>

<template>
  <DialogHost
    :open="open"
    label="Rename tutorial"
    :closable="!busy"
    :close-reason="busy ? 'Renaming…' : null"
    close-testid="rename-dialog-close"
    @close="cancel"
  >
    <template #title>
      Rename tutorial
    </template>

    <label
      data-testid="rename-dialog"
      class="flex flex-col gap-[5px]"
    >
      <span class="text-[10px] text-fg-muted">Title</span>
      <input
        v-model="draft"
        data-testid="rename-dialog-input"
        type="text"
        maxlength="160"
        @keydown.enter.prevent="apply"
      >
    </label>

    <template #footer>
      <AppButton
        variant="ghost"
        data-testid="rename-dialog-cancel"
        :disabled="busy"
        @click="cancel"
      >
        Cancel
      </AppButton>
      <AppButton
        data-testid="rename-dialog-apply"
        :disabled="Boolean(reason) || busy"
        :title="reason ?? undefined"
        @click="apply"
      >
        Apply
      </AppButton>
    </template>
  </DialogHost>
</template>
