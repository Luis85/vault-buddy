<script setup lang="ts">
import { computed } from "vue";
import { storeToRefs } from "pinia";
import CompanionCharacter from "./components/CompanionCharacter.vue";
import ActionPanel from "./components/ActionPanel.vue";
import { useCompanionWindow } from "./composables/useCompanionWindow";
import { useVaultsStore } from "./stores/vaults";

const store = useVaultsStore();
const { panelOpen, busyVaultId } = storeToRefs(store);
const working = computed(() => busyVaultId.value !== null);

const { side, valign } = useCompanionWindow(panelOpen);
</script>

<template>
  <main
    class="flex h-screen w-screen gap-2 p-2"
    :class="[
      side === 'left' ? 'flex-row-reverse' : 'flex-row',
      valign === 'up' ? 'items-end' : 'items-start',
    ]"
  >
    <CompanionCharacter :working="working" @toggle="store.togglePanel()" />
    <ActionPanel v-if="panelOpen" />
  </main>
</template>
