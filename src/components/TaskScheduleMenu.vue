<script setup lang="ts">
import { nextTick, onBeforeUnmount, onMounted, ref, useId, watch } from "vue";

import { comingSaturday, localDatePlus, localToday } from "../utils/taskFields";
import AppIcon from "./AppIcon.vue";
import IconButton from "./ui/IconButton.vue";

// A per-row schedule popover: Today / Tomorrow / This weekend / a native date
// pick / Clear. Presentational — it emits the chosen do-date (or null to
// clear); the container runs the write. Escape/focus/outside-click follow
// TaskSectionMenu's pattern (GAP-27: swallow own Escape so it doesn't bubble
// to PanelRoot's panel-close handler).
const props = defineProps<{ title: string; scheduled: string | null; busy: boolean }>();
const emit = defineEmits<{ (e: "schedule", value: string | null): void }>();

const open = ref(false);
const root = ref<HTMLElement | null>(null);
const popover = ref<HTMLElement | null>(null);
const pick = ref("");
// A stable unique id linking the trigger (aria-controls) to the role="dialog"
// popover, so assistive tech reports the disclosure relationship (Codex, PR #75).
const popoverId = useId();

// Whether the popover renders ABOVE the trigger instead of below it (Codex
// P2): the row list lives inside the panel's `.panel-scroll` overflow
// container, so a downward-only absolute popover on a row near the bottom of
// that container is clipped and its lower items (the date picker, Clear)
// become unreachable. We clip against the NEAREST `.panel-scroll` ancestor's
// bottom — the actual overflow edge — not `window.innerHeight`, which
// over-estimates the space below in a compact panel or with the composer
// options expanded (Codex, PR #75); `window.innerHeight` is only the fallback
// when no such ancestor is found. POPOVER_HEIGHT is a fixed estimate (5 items
// ≈ 200px) rather than a measurement of the not-yet-open popover, which
// doesn't exist in the DOM until `open` flips true.
const POPOVER_HEIGHT = 200;
const flipUp = ref(false);

// Pure so the decision itself stays trivially unit-testable without a real
// layout engine (happy-dom's getBoundingClientRect is zeroed by default).
function shouldFlipUp(triggerBottom: number, popoverHeight: number, viewportBottom: number): boolean {
  return triggerBottom + popoverHeight > viewportBottom;
}

function toggle() {
  const opening = !open.value;
  if (opening) {
    pick.value = props.scheduled ?? "";
    // Measured against the trigger BEFORE flipping `open` — the popover div
    // doesn't exist in the DOM yet (v-if), so the button is the only
    // descendant of `root` to measure regardless of ordering, but reading it
    // first keeps the intent unambiguous.
    const rect = root.value?.querySelector("button")?.getBoundingClientRect();
    const scrollEl = root.value?.closest(".panel-scroll");
    const clipBottom = scrollEl ? scrollEl.getBoundingClientRect().bottom : window.innerHeight;
    flipUp.value = rect ? shouldFlipUp(rect.bottom, POPOVER_HEIGHT, clipBottom) : false;
  }
  open.value = opening;
}
function close() { open.value = false; }
// Keyboard-driven closes (choosing an option, Escape) return focus to the
// trigger — removing the focused popup child/container via v-if otherwise drops
// focus to <body>, forcing a keyboard user to re-traverse the panel (Codex, PR
// #75). Outside-click closes deliberately do NOT refocus (the user aimed elsewhere).
function closeAndRefocus() {
  close();
  void nextTick(() => (root.value?.querySelector("button") as HTMLElement | null)?.focus());
}
function choose(value: string | null) {
  if (props.busy) return;
  emit("schedule", value);
  closeAndRefocus();
}
function onPick() { if (pick.value) choose(pick.value); }

watch(open, (o) => { if (o) void nextTick(() => popover.value?.focus()); });
function onRootKeydown(e: KeyboardEvent) {
  if (e.key !== "Escape" || e.isComposing || !open.value) return;
  e.preventDefault();
  e.stopPropagation();
  closeAndRefocus();
}
function onWindowPointerDown(e: PointerEvent) {
  if (!open.value) return;
  if (root.value && !root.value.contains(e.target as Node)) close();
}
onMounted(() => window.addEventListener("pointerdown", onWindowPointerDown));
onBeforeUnmount(() => window.removeEventListener("pointerdown", onWindowPointerDown));

const itemClass =
  "cursor-pointer rounded px-1.5 py-0.5 text-left text-micro text-fg-secondary transition-colors hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-focus disabled:cursor-default disabled:opacity-40";
</script>

<template>
  <div
    ref="root"
    class="relative inline-flex"
    @keydown="onRootKeydown"
  >
    <!-- Reuse the shared IconButton (size sm) — it sits beside TaskRow's own
         IconButtons and owns the hover/focus/disabled treatment (GAP-66). It
         forwards the native click (TaskRow binds @click on it), so @click.stop
         applies. `label` is IconButton's aria-label prop. -->
    <IconButton
      size="sm"
      :data-testid="`task-schedule-${title}`"
      :disabled="busy"
      :label="`Schedule ${title}`"
      title="Schedule"
      aria-haspopup="dialog"
      :aria-controls="open ? popoverId : undefined"
      :aria-expanded="open"
      @click.stop="toggle"
    >
      <AppIcon :size="14">
        <rect
          x="3"
          y="4"
          width="18"
          height="18"
          rx="2"
        />
        <path d="M16 2v4M8 2v4M3 10h18" />
      </AppIcon>
    </IconButton>
    <div
      v-if="open"
      :id="popoverId"
      ref="popover"
      role="dialog"
      aria-label="Schedule"
      tabindex="-1"
      :data-testid="`task-schedule-popover-${title}`"
      class="absolute right-0 z-10 flex min-w-40 flex-col gap-0.5 rounded-control border border-white/10 bg-slate-800 p-1 shadow-lg focus:outline-none"
      :class="flipUp ? 'bottom-full mb-1' : 'top-full mt-1'"
      @click.stop
    >
      <button
        type="button"
        data-testid="task-schedule-today"
        :class="itemClass"
        @click="choose(localToday())"
      >
        Today
      </button>
      <button
        type="button"
        data-testid="task-schedule-tomorrow"
        :class="itemClass"
        @click="choose(localDatePlus(1))"
      >
        Tomorrow
      </button>
      <button
        type="button"
        data-testid="task-schedule-weekend"
        :class="itemClass"
        @click="choose(comingSaturday())"
      >
        This weekend
      </button>
      <input
        v-model="pick"
        type="date"
        data-testid="task-schedule-pick"
        aria-label="Pick a do date"
        class="rounded border border-white/10 bg-white/5 px-1.5 py-0.5 text-micro text-fg focus:border-focus focus:outline-none"
        @change="onPick"
      >
      <button
        v-if="scheduled"
        type="button"
        data-testid="task-schedule-clear"
        :class="itemClass"
        @click="choose(null)"
      >
        Clear
      </button>
    </div>
  </div>
</template>
