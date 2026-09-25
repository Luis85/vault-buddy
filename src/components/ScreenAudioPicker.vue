<script setup lang="ts">
import { invoke } from "@tauri-apps/api/core";
import { computed, onMounted, ref } from "vue";

import { logWarning } from "../logging";
import type { AudioDevices } from "../types";
import Chip from "./ui/Chip.vue";
import SectionHeader from "./ui/SectionHeader.vue";

// Multi-select over the machine's capture endpoints, mirroring the audio
// domain's device lists. Deliberately NO live level bars yet (spec 7.2 asks
// for them): the screen domain emits no per-device level event — Task 8's
// surface carries `screen:frames` only — and a meter fed by nothing would be
// a permanently-dead indicator, which is worse than none.
const props = defineProps<{ inputs: string[]; outputs: string[] }>();
const emit = defineEmits<{
  (e: "update:inputs", value: string[]): void;
  (e: "update:outputs", value: string[]): void;
}>();

const devices = ref<AudioDevices>({ inputs: [], outputs: [] });

/** Spec 6.5: zero sources is a legal, documented outcome — a silent UI demo
 * is a real use case — so this is a note, never a block on Start. */
const silent = computed(() => props.inputs.length + props.outputs.length === 0);

/**
 * Recompute the selection in ENUMERATION order rather than click order, so
 * the names Rust receives are stable regardless of how the user ticked them.
 * The STRINGS are what is load-bearing: `selection_from` builds the cpal
 * device selection from them unchanged and each endpoint is opened BY NAME,
 * so any normalisation here is the difference between recording and silently
 * recording nothing. Order is merely stable, not load-bearing. Recomputing
 * from the list also means an untick really REMOVES a device — an
 * append-only selection would record a microphone the user turned off.
 */
function pick(list: { name: string }[], selected: string[], name: string, on: boolean) {
  return list
    .map((d) => d.name)
    .filter((n) => (n === name ? on : selected.includes(n)));
}

function onInput(name: string, on: boolean) {
  emit("update:inputs", pick(devices.value.inputs, props.inputs, name, on));
}
function onOutput(name: string, on: boolean) {
  emit("update:outputs", pick(devices.value.outputs, props.outputs, name, on));
}

const checked = (event: Event) => (event.target as HTMLInputElement).checked;

onMounted(async () => {
  try {
    devices.value = await invoke<AudioDevices>("list_audio_devices");
  } catch (e) {
    // A device-enumeration failure must never block the capture: a silent
    // capture is legal, so degrade to "no devices" (which renders the note)
    // rather than failing the picker.
    logWarning(`list_audio_devices failed: ${String(e)}`);
  }
});
</script>

<template>
  <div class="flex flex-col gap-2">
    <div>
      <SectionHeader>Microphones</SectionHeader>
      <p
        v-if="devices.inputs.length === 0"
        class="px-2 text-xs text-fg-subtle"
      >
        No microphones found.
      </p>
      <label
        v-for="(d, i) in devices.inputs"
        :key="d.name"
        class="flex cursor-pointer items-center gap-2 px-2 py-0.5 text-xs text-fg-secondary"
      >
        <input
          type="checkbox"
          :data-testid="`audio-input-${i}`"
          :checked="props.inputs.includes(d.name)"
          class="accent-violet-500"
          @change="onInput(d.name, checked($event))"
        >
        <span class="min-w-0 truncate">{{ d.name }}</span>
        <Chip v-if="d.isDefault">Default</Chip>
      </label>
    </div>
    <div>
      <SectionHeader>System audio</SectionHeader>
      <p
        v-if="devices.outputs.length === 0"
        class="px-2 text-xs text-fg-subtle"
      >
        No playback devices found.
      </p>
      <label
        v-for="(d, i) in devices.outputs"
        :key="d.name"
        class="flex cursor-pointer items-center gap-2 px-2 py-0.5 text-xs text-fg-secondary"
      >
        <input
          type="checkbox"
          :data-testid="`audio-output-${i}`"
          :checked="props.outputs.includes(d.name)"
          class="accent-violet-500"
          @change="onOutput(d.name, checked($event))"
        >
        <span class="min-w-0 truncate">{{ d.name }}</span>
        <Chip v-if="d.isDefault">Default</Chip>
      </label>
    </div>
    <p
      v-if="silent"
      data-testid="screen-no-audio"
      class="px-2 text-micro text-fg-subtle"
    >
      No audio will be recorded.
    </p>
  </div>
</template>
