<script setup lang="ts">
/**
 * The `noDestination` finding's answer, inside the Checks dialog (Task
 * 54): pick the vault this tutorial publishes into, a folder in it and the
 * dated-folder choice, then Save — ONE `setDestination` edit through
 * `editorProject.execute` (an ordinary acknowledged, undoable edit). The
 * project had no other control for its destination; the Publish dialog
 * picks a vault per publish and never writes it back.
 */
import { onMounted, ref } from "vue";

import type { VaultChoice } from "../../../editorTypes";
import { toEditorError, useEditorProjectStore } from "../../../stores/editorProject";
import AppButton from "../../ui/AppButton.vue";
import DestinationFields from "./DestinationFields.vue";

const emit = defineEmits<{ (e: "done"): void; (e: "cancel"): void }>();

const editorProject = useEditorProjectStore();
const destination = editorProject.project?.destination;

const vaults = ref<VaultChoice[]>([]);
const vaultId = ref(destination?.vault ?? "");
const folder = ref(destination?.folder ?? "");
const dated = ref(destination?.dated ?? false);
const saving = ref(false);
const error = ref<string | null>(null);

onMounted(async () => {
  try {
    vaults.value = await editorProject.port.listVaults();
  } catch (e) {
    error.value = `The vault list could not be read. ${toEditorError(e).message}`.trim();
  }
});

async function save(): Promise<void> {
  if (!vaultId.value || saving.value) return;
  saving.value = true;
  const ok = await editorProject.execute({
    kind: "setDestination",
    vaultId: vaultId.value,
    folder: folder.value.trim(),
    dated: dated.value,
  });
  saving.value = false;
  if (ok) emit("done");
  else error.value = editorProject.lastError?.message ?? "The destination could not be set.";
}

</script>

<template>
  <section
    data-testid="checks-destination"
    class="flex flex-col gap-2 text-xs"
  >
    <h3 class="font-semibold text-fg-secondary">
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
      role="alert"
      class="text-danger-fg"
    >
      {{ error }}
    </p>
    <div class="flex items-center justify-end gap-2">
      <span
        v-if="!vaultId"
        data-testid="checks-destination-reason"
        class="text-micro text-fg-subtle"
      >Choose a vault first.</span>
      <AppButton
        variant="ghost"
        size="sm"
        data-testid="checks-destination-cancel"
        @click="emit('cancel')"
      >
        Back to checks
      </AppButton>
      <AppButton
        variant="primary"
        size="sm"
        data-testid="checks-destination-save"
        :disabled="!vaultId || saving"
        @click="save"
      >
        Set destination
      </AppButton>
    </div>
  </section>
</template>
