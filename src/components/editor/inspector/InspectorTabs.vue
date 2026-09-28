<script setup lang="ts">
/**
 * A clip's categories (visual-parity Task 13; concept spec §5
 * `.expanded-tabs`): a 2×3 grid on the app background — Clip, Layout,
 * Fades / Audio, Speed, Color, or Clip, Fades, Audio, Speed for a sound
 * clip — whose active button carries `.active`. A `role="tablist"` with a
 * roving tabindex (`useRovingTablist`); the Layout and Fades tabs are the
 * guide's fallback route to their sections (Task 55), which bind the same
 * keys themselves and win while open.
 */
import { useGuideTabTargets } from "../../../composables/useGuideTarget";
import { useRovingTablist } from "../../../composables/useRovingTablist";
import { TAB_LABELS } from "../../../editor/inspectorState";
import type { InspectorTab } from "../../../editor/menuContext";

const props = defineProps<{ tabs: readonly InspectorTab[]; active: InspectorTab }>();
const emit = defineEmits<(e: "select", tab: InspectorTab) => void>();

const { setTabRef, onKeydown } = useRovingTablist(
  () => props.tabs.length,
  () => props.tabs.indexOf(props.active),
  (i) => emit("select", props.tabs[i]),
);
const bindTabTarget = useGuideTabTargets<InspectorTab>({
  layout: ["inspector.layout"],
  fades: ["inspector.fades"],
});

/** Each tab binds by its own id, so a tab that leaves (a sound clip has no
 * Layout) unbinds its guide target rather than another tab's; a removed
 * tab's null never clears the roving slot a remaining tab now holds. */
function bind(tab: InspectorTab, i: number, el: Element | null): void {
  bindTabTarget(tab, el);
  if (el) setTabRef(i, el);
}
</script>

<template>
  <div
    role="tablist"
    aria-label="Inspector categories"
    data-testid="inspector-tablist"
    class="mb-[15px] grid grid-cols-3 gap-[3px] rounded-lg border border-line bg-app p-1"
    @keydown="onKeydown"
  >
    <button
      v-for="(tab, i) in tabs"
      :id="`inspector-tab-${tab}`"
      :key="tab"
      :ref="(el) => bind(tab, i, el as Element | null)"
      type="button"
      role="tab"
      :data-testid="`inspector-tab-${tab}`"
      :aria-selected="tab === active"
      :aria-controls="tab === active ? `inspector-tabpanel-${tab}` : undefined"
      :tabindex="tab === active ? 0 : -1"
      class="min-h-[30px] cursor-pointer rounded-[5px] border text-[11px] transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-focus"
      :class="tab === active ? 'active border-accent/28 bg-accent-bg text-accent-ink' : 'border-transparent text-fg-secondary hover:bg-hover'"
      @click="emit('select', tab)"
    >
      {{ TAB_LABELS[tab] }}
    </button>
  </div>
</template>
