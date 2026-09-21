<script setup lang="ts">
import { invoke } from "@tauri-apps/api/core";
import { computed, onMounted, ref } from "vue";

import { logWarning } from "../logging";
import { useFfmpegStore } from "../stores/ffmpeg";
import { useScreenCaptureStore } from "../stores/screenCapture";
import { useVaultsStore } from "../stores/vaults";
import type { CaptureSourceInfo, RegionSelection, StagedCaptureSummary } from "../types";
import ScreenAudioPicker from "./ScreenAudioPicker.vue";
import ScreenRegionPicker from "./ScreenRegionPicker.vue";
import ScreenWindowPicker from "./ScreenWindowPicker.vue";
import StagedCaptureList from "./StagedCaptureList.vue";
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
const props = defineProps<{ vaultId: string }>();
const store = useVaultsStore();
const screenCapture = useScreenCaptureStore();
const ffmpeg = useFfmpegStore();

/**
 * The export's toolchain, checked BEFORE the recording rather than at the
 * payoff (docs/Gaps.md GAP-144). It WARNS and deliberately does not gate
 * `canStart`: ffmpeg is needed only by the final Save into a vault, so
 * recording and editing are fully available without it. Disabling Start — the
 * shape a blocked document Import takes, because a Pandoc-less import cannot
 * proceed at all — would take away working functionality, which is a worse
 * bug than the late discovery it would be replacing.
 *
 * Held back while the probe is in flight, so the picker can't flash "ffmpeg
 * is missing" at every open. A FAILED probe leaves the status null and the
 * notice shows: it blocks nothing, so warning on an unknown answer costs a
 * line of text, while staying silent costs the user a forty-minute recording
 * they cannot save.
 */
const ffmpegMissing = computed(
  () => !ffmpeg.checking && !ffmpeg.status?.installed,
);

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

/** Captures that were recorded but never saved or discarded (spec §10).
 * Shown FIRST, above the tabs: this is how a user finds work they abandoned,
 * and a tab would hide it behind a click. */
const staged = ref<StagedCaptureSummary[]>([]);
/** The one staged row whose discard is in flight. A base rather than a bool
 * so the other rows stay usable. */
const stagedBusy = ref<string | null>(null);
/** Tells the list to drop its armed confirm when nothing else will. The list
 * disarms whenever `staged` is reassigned, which covers a SUCCESSFUL discard
 * (it re-reads); a REFUSED one deliberately re-reads nothing, so without this
 * the armed row survives the refusal and the next single click deletes the
 * recording unconfirmed. */
const stagedDisarm = ref(0);

/**
 * Re-read the staged list.
 *
 * A VIEW, so it degrades exactly as its Rust half does — `list_staged_captures`
 * answers an unreadable staging directory with an empty Vec rather than an
 * error. Only a real list replaces the one on screen: a transient failure
 * must not blank a list the user is reading (the `vaults` store's own rule),
 * and it is never surfaced as a banner, because "nothing is staged" is the
 * overwhelmingly common truth and a scan failure is not something the user
 * can act on from here.
 */
async function loadStaged() {
  try {
    const rows = await invoke<StagedCaptureSummary[]>("list_staged_captures");
    if (Array.isArray(rows)) staged.value = rows;
  } catch (e) {
    logWarning(`list_staged_captures failed: ${String(e)}`);
  }
}

/** Resume editing. `open_capture_editor` is the SAME command the panel
 * capture bar's Edit button invokes — one way into the editor, not two. */
async function onResumeStaged(base: string) {
  if (stagedBusy.value !== null) return;
  error.value = null;
  try {
    await invoke("open_capture_editor", { base });
  } catch (e) {
    logWarning(`open_capture_editor failed: ${String(e)}`);
    error.value = String(e);
  }
}

/**
 * Discard, already confirm-gated by the list (spec §10).
 *
 * `discard_staged_capture` genuinely refuses — a base being exported right
 * now, or one that is not ours — and its message is user-facing, so a
 * refusal surfaces in the same banner as every other failure here and the
 * row STAYS. Only a successful discard re-reads the list, and it re-reads
 * rather than splicing: the discard also clears any abandoned export temp,
 * and only the backend knows what actually survived.
 *
 * A refusal therefore changes nothing the list can see — which is exactly
 * why it has to disarm the row explicitly: the surviving row is still armed,
 * and a second click on a control that just refused would be read as a
 * retry, not as a confirmed delete.
 */
async function onDiscardStaged(base: string) {
  if (stagedBusy.value !== null) return;
  stagedBusy.value = base;
  error.value = null;
  try {
    await invoke("discard_staged_capture", { base });
    await loadStaged();
  } catch (e) {
    logWarning(`discard_staged_capture failed: ${String(e)}`);
    error.value = String(e);
    stagedDisarm.value += 1;
  } finally {
    stagedBusy.value = null;
  }
}

onMounted(() => {
  void loadSources();
  void loadStaged();
  // Cached across opens by the store — a found ffmpeg is not re-probed, a
  // missing one is, so an install made in answer to the notice is picked up.
  void ffmpeg.ensureDetected();
});

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
    <StagedCaptureList
      :captures="staged"
      :busy-base="stagedBusy"
      :disarm-nonce="stagedDisarm"
      @resume="onResumeStaged"
      @discard="onDiscardStaged"
    />
    <TabGroup :tabs="[...TABS]">
      <template #screen>
        <EmptyState
          v-if="screens.length === 0"
          title="No capture sources available."
          hint="Open a window or connect a display, then reopen this screen."
        />
        <ul
          v-else
          class="flex flex-col gap-1"
        >
          <li
            v-for="s in screens"
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
      <template #window>
        <ScreenWindowPicker
          :windows="rowsFor('window')"
          :selected-id="selectedId"
          @update:selected-id="selectedId = $event"
        />
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
    <!-- Sits directly above Start because it qualifies exactly that button:
         pressing it works, and the Save that follows will not. NOT above the
         staged list, which spec 10 requires to come first. -->
    <Banner
      v-if="ffmpegMissing"
      data-testid="ffmpeg-preflight"
      tone="warning"
    >
      <span class="block">
        You can record and edit this capture now, but saving it into a vault
        needs ffmpeg, which isn't installed.
      </span>
      <button
        type="button"
        data-testid="ffmpeg-preflight-settings"
        class="mt-1 cursor-pointer underline underline-offset-2 hover:text-fg focus:outline-none focus-visible:ring-2 focus-visible:ring-focus"
        @click="store.openSettings('integrations')"
      >
        Set up ffmpeg
      </button>
    </Banner>
    <AppButton
      data-testid="screen-start"
      :disabled="!canStart"
      @click="onStart"
    >
      {{ starting ? "Starting…" : "Start capture" }}
    </AppButton>
  </div>
</template>
