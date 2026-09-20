<script setup lang="ts">
import { invoke } from "@tauri-apps/api/core";
import { computed, onMounted, ref } from "vue";

import { logWarning } from "../logging";
import { useScreenCaptureStore } from "../stores/screenCapture";
import { useVaultsStore } from "../stores/vaults";
import type { CaptureSourceInfo, RegionSelection } from "../types";
import ScreenAudioPicker from "./ScreenAudioPicker.vue";
import ScreenRegionPicker from "./ScreenRegionPicker.vue";
import TabGroup from "./TabGroup.vue";
import AppButton from "./ui/AppButton.vue";
import Banner from "./ui/Banner.vue";
import EmptyState from "./ui/EmptyState.vue";

// Screen, Window and Region (spec 7.2). The Region tab arrived in phase 3
// together with the overlay it opens; phase 2 deliberately shipped without
// it rather than rendering a disabled tab with nothing behind it
// (docs/Gaps.md GAP-111 item 2).
const TABS = [
  { id: "screen", label: "Screen" },
  { id: "window", label: "Window" },
  { id: "region", label: "Region" },
] as const;

/** The two tabs that render a plain row list from `list_capture_sources`.
 * Region builds its own row from a selection, so it gets its own slot. */
const LIST_TABS = TABS.filter((t) => t.id !== "region");

const props = defineProps<{ vaultId: string }>();
const store = useVaultsStore();
const screenCapture = useScreenCaptureStore();

const sources = ref<CaptureSourceInfo[]>([]);
const selectedId = ref<string | null>(null);
const inputs = ref<string[]>([]);
const outputs = ref<string[]>([]);
const error = ref<string | null>(null);
const starting = ref(false);

const region = ref<RegionSelection | null>(null);
/** Which monitor "Select region…" opens the overlay on. A region lives on
 * exactly one monitor — see the overlay's own reasoning — so the Region tab
 * lists the monitors as targets rather than guessing the primary. */
const regionTargetId = ref<string | null>(null);
/** The monitor the held region was actually cut from, snapshotted when it was
 * selected. NOT the same thing as `regionTargetId`, which is only "which row
 * is highlighted right now" and moves on any click: once the two diverge, a
 * target-keyed label names the wrong monitor (the Rust/TS parity failure
 * `utils/regionLabel.ts` exists to prevent), a target-keyed drop rule clears a
 * valid region when some OTHER monitor goes, and leaves an armed region whose
 * own monitor went — which Start then hands to `source::resolve` as
 * `SourceGone`. Snapshotted rather than parsed back out of `region.sourceId`:
 * that id's `region:<monitor>,x,y,w,h` shape is Rust's to define. */
const regionMonitorId = ref<string | null>(null);
const selectingRegion = ref(false);

const rowsFor = (kind: string) =>
  sources.value.filter((s) => s.kind === kind);
const hasSource = (id: string | null) => sources.value.some((s) => s.id === id);
const screens = computed(() => rowsFor("screen"));
// Also gated on a selection in flight: the overlay covers the target monitor
// while this panel sits on another and stays clickable, so an ungated Start
// would begin recording under the overlay AND navigate away, unmounting the
// picker the pending drag has to resolve into.
const canStart = computed(
  () => selectedId.value !== null && !starting.value && !selectingRegion.value,
);

async function onSelectRegion() {
  if (regionTargetId.value === null || selectingRegion.value) return;
  selectingRegion.value = true;
  error.value = null;
  try {
    const picked = await invoke<RegionSelection | null>("select_capture_region", {
      sourceId: regionTargetId.value,
    });
    // `null` is a cancel (Escape, or a click that was not a drag), not a
    // failure: leave whatever was selected before exactly as it was.
    if (picked) {
      region.value = picked;
      // Snapshot the monitor WITH the region: everything that asks "which
      // monitor is this region on" must read this, never the mutable target.
      regionMonitorId.value = regionTargetId.value;
      selectedId.value = picked.sourceId;
    }
  } catch (e) {
    logWarning(`select_capture_region failed: ${String(e)}`);
    error.value = String(e);
  } finally {
    selectingRegion.value = false;
  }
}

/**
 * Drop every pointer the freshly-enumerated list no longer backs. Runs in
 * dependency order — region, then target, then the armed selection — so a
 * dropped region also disarms Start on the same pass.
 */
function dropVanished() {
  // A region id is NEVER in `sources` — it is built from a selection, not
  // enumerated — so the "drop what the list no longer offers" rule has to ask
  // about the region's own MONITOR instead. Without this the region is cleared
  // on every refresh and Start silently disarms itself. It asks about
  // `regionMonitorId`, never the target pointer — see that ref.
  if (region.value && !hasSource(regionMonitorId.value)) {
    region.value = null;
    regionMonitorId.value = null;
  }
  // The target pointer follows the same rule whether or not a region is held:
  // left dangling it keeps "Select region…" enabled on an id Rust can only
  // refuse.
  if (regionTargetId.value !== null && !hasSource(regionTargetId.value)) {
    regionTargetId.value = null;
  }
  const known = hasSource(selectedId.value) || selectedId.value === region.value?.sourceId;
  if (selectedId.value && !known) {
    selectedId.value = null;
  }
}

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
    dropVanished();
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
        v-for="t in LIST_TABS"
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
      <template #region>
        <ScreenRegionPicker
          :screens="screens"
          :target-id="regionTargetId"
          :monitor-id="regionMonitorId"
          :region="region"
          :starting="starting"
          :selected-id="selectedId"
          :selecting="selectingRegion"
          @update:target-id="regionTargetId = $event"
          @update:selected-id="selectedId = $event"
          @select="onSelectRegion"
        />
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
