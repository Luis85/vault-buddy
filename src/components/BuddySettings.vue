<script setup lang="ts">
import { invoke } from "@tauri-apps/api/core";
import { computed, onMounted, ref } from "vue";

import { CHARACTERS } from "../characters";
import { logWarning } from "../logging";
import { type MessageDuration,useSettingsStore } from "../stores/settings";
import BuddyAvatar from "./BuddyAvatar.vue";
import DiagnosticsSettings from "./DiagnosticsSettings.vue";
import DocumentImportSettings from "./DocumentImportSettings.vue";
import McpSettings from "./McpSettings.vue";
import SelectMenu from "./SelectMenu.vue";
import TabGroup from "./TabGroup.vue";
import UpdateSettings from "./UpdateSettings.vue";

const settings = useSettingsStore();

const DURATION_OPTIONS = [
  { value: "short", label: "Short" },
  { value: "normal", label: "Normal" },
  { value: "long", label: "Long" },
] as const;

const messageDuration = computed({
  get: () => settings.messageDuration,
  set: (v: string | number) => settings.setMessageDuration(v as MessageDuration),
});

// Card under the pointer/keyboard focus — its avatar plays the run loop as a
// try-before-you-pick preview. Gated on animationsEnabled so animations-off
// also silences previews (BuddyAvatar's .still would freeze them anyway; the
// gate keeps the semantics honest).
const previewId = ref<string | null>(null);

// OS-owned state (the registry on Windows): read fresh on mount, never
// stored in localStorage/the settings store. null = unknown (read pending
// or failed) — the toggle stays disabled so it can't write blind.
const autostart = ref<boolean | null>(null);
const autostartBusy = ref(false);
const autostartError = ref<string | null>(null);

onMounted(async () => {
  try {
    autostart.value = await invoke<boolean>("get_autostart");
  } catch (e) {
    autostartError.value = String(e);
    logWarning(`get_autostart failed: ${String(e)}`);
  }
});

async function toggleAutostart(event: Event) {
  const enabled = (event.target as HTMLInputElement).checked;
  const previous = autostart.value;
  // Optimistic with revert-on-failure (the Tasks-toggle pattern); busy
  // disables the checkbox so two writes can't race.
  autostart.value = enabled;
  autostartBusy.value = true;
  autostartError.value = null;
  try {
    await invoke("set_autostart", { enabled });
  } catch (e) {
    autostart.value = previous;
    autostartError.value = String(e);
    logWarning(`set_autostart failed: ${String(e)}`);
  } finally {
    autostartBusy.value = false;
  }
}
</script>

<template>
  <TabGroup
    :tabs="[
      { id: 'buddy', label: 'Buddy' },
      { id: 'system', label: 'System' },
      { id: 'integrations', label: 'Integrations' },
    ]"
  >
    <template #buddy>
      <div class="flex flex-col gap-3">
        <section>
          <h2
            class="mb-1.5 text-xs font-semibold uppercase tracking-wide text-slate-400"
          >
            Buddy character
          </h2>
          <div
            class="grid grid-cols-3 gap-2"
            role="radiogroup"
            aria-label="Buddy character"
          >
            <button
              v-for="c in CHARACTERS"
              :key="c.id"
              type="button"
              role="radio"
              class="character-option relative flex cursor-pointer flex-col items-center rounded-xl border p-1.5 transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400"
              :class="
                settings.character === c.id
                  ? 'border-violet-400 bg-violet-500/20'
                  : 'border-white/10 bg-white/5 hover:bg-white/10'
              "
              :aria-checked="settings.character === c.id"
              :aria-label="`Choose ${c.name}`"
              @click="settings.setCharacter(c.id)"
              @pointerenter="previewId = c.id"
              @pointerleave="previewId = null"
              @focusin="previewId = c.id"
              @focusout="previewId = null"
            >
              <span
                v-if="settings.character === c.id"
                data-testid="selected-badge"
                class="absolute right-1 top-1 flex h-3.5 w-3.5 items-center justify-center rounded-full bg-violet-500 text-[9px] font-bold text-white"
                aria-hidden="true"
              >✓</span>
              <BuddyAvatar
                :character-id="c.id"
                :animated="settings.animationsEnabled"
                :working="previewId === c.id && settings.animationsEnabled"
              />
              <span class="text-xs text-slate-200">{{ c.name }}</span>
            </button>
          </div>
        </section>
        <section>
          <h2
            class="mb-1.5 text-xs font-semibold uppercase tracking-wide text-slate-400"
          >
            Behavior
          </h2>
          <div class="flex flex-col gap-3 rounded-xl border border-white/10 bg-white/5 p-2">
            <div class="flex items-center justify-between">
              <label
                for="animations-toggle"
                class="text-sm text-slate-200"
              >
                Animations
              </label>
              <input
                id="animations-toggle"
                type="checkbox"
                class="h-4 w-4 accent-violet-500"
                :checked="settings.animationsEnabled"
                @change="settings.toggleAnimations()"
              >
            </div>
            <div class="flex items-center justify-between">
              <label
                for="dragging-toggle"
                class="text-sm text-slate-200"
              >
                Dragging
                <span class="block text-xs text-slate-500">
                  Off pins the buddy in place
                </span>
              </label>
              <input
                id="dragging-toggle"
                type="checkbox"
                class="h-4 w-4 accent-violet-500"
                :checked="settings.draggingEnabled"
                @change="settings.toggleDragging()"
              >
            </div>
            <div class="flex items-center justify-between">
              <label
                for="messages-toggle"
                class="text-sm text-slate-200"
              >
                Buddy messages
                <span class="block text-xs text-slate-500">
                  The buddy comments on what you do
                </span>
              </label>
              <input
                id="messages-toggle"
                type="checkbox"
                class="h-4 w-4 accent-violet-500"
                :checked="settings.buddyMessagesEnabled"
                @change="settings.toggleBuddyMessages()"
              >
            </div>
            <div class="flex items-center justify-between gap-2">
              <label
                for="message-duration"
                class="text-sm text-slate-200"
              >
                Message duration
                <span class="block text-xs text-slate-500">
                  How long the buddy's bubbles stay up
                </span>
              </label>
              <SelectMenu
                id="message-duration"
                v-model="messageDuration"
                :options="DURATION_OPTIONS"
                data-testid="message-duration-select"
              />
            </div>
          </div>
        </section>
      </div>
    </template>
    <template #system>
      <div class="flex flex-col gap-3">
        <section>
          <h2
            class="mb-1.5 text-xs font-semibold uppercase tracking-wide text-slate-400"
          >
            System
          </h2>
          <div class="rounded-xl border border-white/10 bg-white/5 p-2">
            <div class="flex items-center justify-between">
              <label
                for="autostart-toggle"
                class="text-sm text-slate-200"
              >
                Start with Windows
                <span class="block text-xs text-slate-500">
                  Launch the buddy when you log in
                </span>
              </label>
              <input
                id="autostart-toggle"
                data-testid="autostart-toggle"
                type="checkbox"
                class="h-4 w-4 accent-violet-500"
                :checked="autostart === true"
                :disabled="autostart === null || autostartBusy"
                @change="toggleAutostart"
              >
            </div>
            <p
              v-if="autostartError"
              data-testid="autostart-error"
              class="mt-1.5 text-xs text-red-300"
            >
              {{ autostartError }}
            </p>
          </div>
        </section>
        <UpdateSettings />
        <DiagnosticsSettings />
      </div>
    </template>
    <template #integrations>
      <div class="flex flex-col gap-3">
        <McpSettings />
        <DocumentImportSettings />
      </div>
    </template>
  </TabGroup>
</template>
