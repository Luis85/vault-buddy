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
  <!--
    The buddy lives in a fixed cell with the exact collapsed-window size
    (140x170). When the window grows by the size delta and the layout
    mirrors, the cell lands precisely where the collapsed window was, so
    the character never visibly moves — the placement offset in
    useCompanionWindow assumes exactly this geometry.
  -->
  <main
    class="flex h-screen w-screen"
    :class="[
      side === 'left' ? 'flex-row-reverse' : 'flex-row',
      valign === 'up' ? 'items-end' : 'items-start',
    ]"
  >
    <div
      data-testid="buddy-cell"
      class="flex h-[170px] w-[140px] shrink-0 items-start justify-start p-2"
    >
      <CompanionCharacter :working="working" @toggle="store.togglePanel()" />
    </div>
    <div v-if="panelOpen" class="min-w-0 flex-1 self-stretch p-2">
      <ActionPanel />
    </div>
  </main>
</template>
