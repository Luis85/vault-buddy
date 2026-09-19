<script setup lang="ts">
import { invoke } from "@tauri-apps/api/core";
import { computed, onMounted, ref } from "vue";

import { logWarning } from "../logging";
import { useScreenCaptureStore } from "../stores/screenCapture";
import { useVaultsStore } from "../stores/vaults";
import type { CaptureSourceInfo } from "../types";
import ScreenAudioPicker from "./ScreenAudioPicker.vue";
import TabGroup from "./TabGroup.vue";
import AppButton from "./ui/AppButton.vue";
import Banner from "./ui/Banner.vue";
import EmptyState from "./ui/EmptyState.vue";

// Screen and Window only. Region capture is phase 3, and a disabled Region
// tab would be dead UI inviting a click with nothing behind it.
const TABS = [
  { id: "screen", label: "Screen" },
  { id: "window", label: "Window" },
] as const;

const props = defineProps<{ vaultId: string }>();
const store = useVaultsStore();
const screenCapture = useScreenCaptureStore();

const sources = ref<CaptureSourceInfo[]>([]);
const selectedId = ref<string | null>(null);
const inputs = ref<string[]>([]);
const outputs = ref<string[]>([]);
const error = ref<string | null>(null);
const starting = ref(false);

const rowsFor = (kind: string) =>
  sources.value.filter((s) => s.kind === kind);
const canStart = computed(() => selectedId.value !== null && !starting.value);

/**
 * Re-enumerate. A selection the refreshed list no longer offers is dropped:
 * leaving it armed would re-point Start at a row that is no longer on screen,
 * so the next click reproduces the same failure with no visible cause.
 * Deliberately does NOT clear `error` — the refresh after a refused start
 * happens precisely so the user can read that refusal against a true list.
 */
async function loadSources() {
  try {
    sources.value = await invoke<CaptureSourceInfo[]>("list_capture_sources");
    if (selectedId.value && !sources.value.some((s) => s.id === selectedId.value)) {
      selectedId.value = null;
    }
  } catch (e) {
    // An empty list and a failed read mean different things — "nothing to
    // capture" invites opening a window, a failure invites a retry — so this
    // surfaces instead of degrading to the empty state.
    logWarning(`list_capture_sources failed: ${String(e)}`);
    error.value = String(e);
  }
}

onMounted(() => void loadSources());

async function onStart() {
  if (selectedId.value === null || starting.value) return;
  starting.value = true;
  error.value = null;
  try {
    await screenCapture.start(props.vaultId, selectedId.value, inputs.value, outputs.value);
    // The capture bar lives on the list view beside RecordingBar, the same
    // place the audio domain's start lands (RecordMode.start).
    store.showList();
  } catch (e) {
    // Spec 7.2/14: never a started-then-dead capture. Re-read FIRST so the
    // list is true, then write the refusal — the refusal is what the user
    // has to read, so it must win over any error the refresh raised.
    await loadSources();
    error.value = String(e);
  } finally {
    starting.value = false;
  }
}
</script>

<template>
  <div class="flex flex-col gap-3">
    <Banner
      v-if="error"
      data-testid="screen-error"
      tone="danger"
      role="alert"
    >
      {{ error }}
    </Banner>
    <TabGroup :tabs="[...TABS]">
      <template
        v-for="t in TABS"
        #[t.id]
      >
        <EmptyState
          v-if="rowsFor(t.id).length === 0"
          :key="`${t.id}-empty`"
          title="No capture sources available."
          hint="Open a window or connect a display, then reopen this screen."
        />
        <ul
          v-else
          :key="`${t.id}-list`"
          class="flex flex-col gap-1"
        >
          <li
            v-for="s in rowsFor(t.id)"
            :key="s.id"
          >
            <button
              type="button"
              :data-testid="`source-${s.id}`"
              :aria-pressed="selectedId === s.id"
              class="w-full cursor-pointer rounded-control border bg-white/5 px-3 py-2 text-left transition-colors hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-focus"
              :class="selectedId === s.id ? 'border-violet-400' : 'border-white/10'"
              @click="selectedId = s.id"
            >
              <span class="block truncate text-sm font-medium text-fg">{{ s.title }}</span>
              <span class="block truncate text-xs text-fg-muted">{{ s.detail }}</span>
            </button>
          </li>
        </ul>
      </template>
    </TabGroup>
    <div class="border-t border-white/10 pt-3">
      <ScreenAudioPicker
        v-model:inputs="inputs"
        v-model:outputs="outputs"
      />
    </div>
    <AppButton
      data-testid="screen-start"
      :disabled="!canStart"
      @click="onStart"
    >
      {{ starting ? "Starting…" : "Start capture" }}
    </AppButton>
  </div>
</template>
