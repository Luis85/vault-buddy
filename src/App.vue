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

useCompanionWindow(panelOpen);
</script>

<template>
  <main class="flex h-screen w-screen items-start gap-2 p-2">
    <CompanionCharacter :working="working" @toggle="store.togglePanel()" />
    <ActionPanel v-if="panelOpen" />
  </main>
</template>
