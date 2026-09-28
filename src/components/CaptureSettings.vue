<script setup lang="ts">
import DocumentsConfigTab from "./DocumentsConfigTab.vue";
import RecordingConfigTab from "./RecordingConfigTab.vue";
import ScreenCaptureConfigTab from "./ScreenCaptureConfigTab.vue";
import TabGroup from "./TabGroup.vue";
import TasksConfigTab from "./TasksConfigTab.vue";

// Vault settings shell: four auto-saving tabs, each self-contained (own load +
// autosave). No Save button — edits persist on blur/debounce (folders) or
// immediately (toggles/selects); the panel header shows the transient status.
//
// Screen sits next to Recording because they are the two capture providers;
// it is second so the pair reads together rather than being split by Tasks.
defineProps<{ vaultId: string }>();

const TABS = [
  { id: "recording", label: "Recording" },
  { id: "screen", label: "Screen" },
  { id: "tasks", label: "Tasks" },
  { id: "documents", label: "Documents" },
];
</script>

<template>
  <TabGroup :tabs="TABS">
    <template #recording>
      <RecordingConfigTab :vault-id="vaultId" />
    </template>
    <template #screen>
      <ScreenCaptureConfigTab :vault-id="vaultId" />
    </template>
    <template #tasks>
      <TasksConfigTab :vault-id="vaultId" />
    </template>
    <template #documents>
      <DocumentsConfigTab :vault-id="vaultId" />
    </template>
  </TabGroup>
</template>
