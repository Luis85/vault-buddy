<script setup lang="ts">
/**
 * One `MenuPanel` item (visual-parity Task 5; concept spec §8): a 31px row
 * with a 17px glyph slot (a check replaces the icon while `checked`), the
 * label, and a right-aligned shortcut or a submenu chevron. Click, focus
 * and pointer listeners fall through to the button; `MenuPanelList` owns
 * what they do.
 */
import { computed } from "vue";

import EditorIcon from "../icons/EditorIcon.vue";
import type { MenuAction } from "./menuModel";
import { isDisabled } from "./menuModel";

const props = defineProps<{ item: MenuAction; testid: string; expanded: boolean }>();

/** `aria-disabled`, not `disabled`: a disabled item stays focusable so a
 * keyboard user reaches it and hears its reason in the hint line. */
const disabled = computed(() => isDisabled(props.item));
/** A checkbox item reads its state aloud; a plain item is a menuitem. */
const role = computed(() => (props.item.checked === undefined ? "menuitem" : "menuitemcheckbox"));
/** The concept draws a check in the glyph slot instead of the icon while
 * an item is checked, and a plain box when an item names no icon. */
const glyph = computed(() => (props.item.checked ? "check" : (props.item.icon ?? "box")));
/** A destructive item is danger-coloured, label and glyph alike. */
const tone = computed(() => (props.item.danger ? "danger text-danger-fg" : "text-fg"));
const glyphTone = computed(() => (props.item.danger ? "text-danger-fg" : "text-fg-secondary"));
</script>

<template>
  <button
    type="button"
    :role="role"
    tabindex="-1"
    :data-testid="testid"
    :aria-checked="item.checked"
    :aria-disabled="disabled"
    :aria-haspopup="item.submenu && 'menu'"
    :aria-expanded="item.submenu && expanded"
    :title="item.disabledReason ?? undefined"
    class="flex h-[31px] min-h-[31px] w-full shrink-0 items-center gap-[9px] rounded-[5px] px-[9px] py-1.5 text-left text-[11px] leading-[1.4] hover:bg-accent-bg hover:text-accent-ink focus:bg-accent-bg focus:text-accent-ink focus-visible:shadow-[inset_0_0_0_1px_var(--color-ring)] focus-visible:outline-none"
    :class="[tone, { 'cursor-default opacity-50': disabled }]"
  >
    <span
      class="flex w-[17px] shrink-0 justify-center"
      :class="glyphTone"
    >
      <EditorIcon
        :name="glyph"
        :size="14"
      />
    </span>
    <span class="min-w-0 flex-1 truncate">{{ item.label }}</span>
    <EditorIcon
      v-if="item.submenu"
      name="chevronRight"
      :size="14"
      class="ml-auto shrink-0 text-fg-muted"
    />
    <kbd
      v-else-if="item.kbd"
      aria-hidden="true"
      class="vb-mono ml-auto shrink-0 text-[9px] text-fg-muted"
    >{{ item.kbd }}</kbd>
  </button>
</template>
