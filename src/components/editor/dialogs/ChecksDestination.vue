<script setup lang="ts">
/**
 * The `noDestination` finding's answer, inside the Checks dialog (Task
 * 54): pick the vault this tutorial publishes into, a folder in it and the
 * dated-folder choice, then Save — ONE `setDestination` edit through
 * `editorProject.execute` (an ordinary acknowledged, undoable edit). The
 * project had no other control for its destination; the Publish dialog
 * picks a vault per publish and never writes it back.
 *
 * A refused edit is the store's `lastError`, and this picker prints it
 * beside its own Save — so while it is on screen it claims `lastError`
 * (`useInlineLastError`, ruling T7-1) and the shell's feedback watcher
 * does not toast the same refusal a second time.
 */
import { computed, onMounted, ref } from "vue";

import { useInlineLastError } from "../../../composables/useInlineLastError";
import type { VaultChoice } from "../../../editorTypes";
import { toEditorError, useEditorProjectStore } from "../../../stores/editorProject";
import DestinationFields from "./DestinationFields.vue";
import DialogButton from "./DialogButton.vue";

const emit = defineEmits<{ (e: "done"): void; (e: "cancel"): void }>();

const editorProject = useEditorProjectStore();
const destination = editorProject.project?.destination;
const inline = useInlineLastError(() => true);

const vaults = ref<VaultChoice[]>([]);
const vaultId = ref(destination?.vault ?? "");
const folder = ref(destination?.folder ?? "");
const dated = ref(destination?.dated ?? false);
const saving = ref(false);
const listError = ref<string | null>(null);

const error = computed(() => listError.value ?? inline.error.value?.message ?? null);
const reason = computed(() => {
  if (!vaultId.value) return "Choose a vault first.";
  return saving.value ? "Setting the destination…" : null;
});

onMounted(async () => {
  try {
    vaults.value = await editorProject.port.listVaults();
  } catch (e) {
    listError.value = `The vault list could not be read. ${toEditorError(e).message}`.trim();
  }
});

async function save(): Promise<void> {
  if (reason.value) return;
  saving.value = true;
  listError.value = null;
  inline.clear();
  const ok = await editorProject.execute({
    kind: "setDestination",
    vaultId: vaultId.value,
    folder: folder.value.trim(),
    dated: dated.value,
  });
  saving.value = false;
  if (ok) emit("done");
}
</script>

<template>
  <section
    data-testid="checks-destination"
    class="flex flex-col gap-3 text-xs"
  >
    <h3 class="font-semibold text-fg">
      Where this tutorial goes
    </h3>
    <DestinationFields
      v-model:vault-id="vaultId"
      v-model:folder="folder"
      v-model:dated="dated"
      :vaults="vaults"
      :disabled="saving"
      testid="checks-destination"
    />
    <p
      v-if="error"
      data-testid="checks-destination-error"
      role="alert"
      class="text-danger-fg"
    >
      {{ error }}
    </p>
    <div class="flex items-center justify-end gap-2">
      <span
        v-if="!vaultId"
        data-testid="checks-destination-reason"
        class="text-[10px] text-fg-muted"
      >Choose a vault first.</span>
      <DialogButton
        data-testid="checks-destination-cancel"
        @click="emit('cancel')"
      >
        Back to checks
      </DialogButton>
      <DialogButton
        variant="primary"
        data-testid="checks-destination-save"
        :reason="reason"
        @click="save"
      >
        Set destination
      </DialogButton>
    </div>
  </section>
</template>
