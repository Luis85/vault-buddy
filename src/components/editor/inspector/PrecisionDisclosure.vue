<script setup lang="ts">
/**
 * A precision disclosure (visual-parity Task 14; concept spec §5 "Precision
 * disclosures"): a `<details>` section, collapsed by default, whose 11px
 * summary ends in a `+` / `−` marker. Whether it is open is remembered per
 * selection in `editorWorkspace` (`storeKey`), view state that is never an
 * edit and never saved.
 */
import { computed } from "vue";

import { useEditorWorkspaceStore } from "../../../stores/editorWorkspace";

const props = defineProps<{ title: string; storeKey: string; testid: string }>();

const workspace = useEditorWorkspaceStore();
const open = computed(() => workspace.disclosureOpen(props.storeKey));

function onToggle(event: Event): void {
  workspace.setDisclosureOpen(props.storeKey, (event.target as HTMLDetailsElement).open);
}
</script>

<template>
  <details
    :data-testid="testid"
    :open="open"
    class="border-t border-line"
    @toggle="onToggle"
  >
    <summary
      class="flex min-h-10 cursor-pointer list-none items-center justify-between gap-2 py-[13px] text-[11px] font-[550] text-fg focus:outline-none focus-visible:ring-2 focus-visible:ring-focus [&::-webkit-details-marker]:hidden"
    >
      {{ title }}
      <span
        aria-hidden="true"
        class="text-base font-normal text-fg-muted"
      >{{ open ? "−" : "+" }}</span>
    </summary>
    <div class="flex flex-col gap-2.5 pb-[15px]">
      <slot />
    </div>
  </details>
</template>
