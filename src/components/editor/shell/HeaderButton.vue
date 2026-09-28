<script setup lang="ts">
/**
 * One header action (visual-parity Task 8; concept spec §2 `.header-actions`
 * and §1.5's button base): 34px tall, 11px, an icon then the label, gap 7.
 * `ghost` is the concept's transparent button (Help, Checks), `bordered` its
 * `.btn` (Save project), `primary` its `.primary` (Render video: white
 * 600 on `--color-primary`). Hover and the disabled look come from the
 * editor's base button rules (`style.css`, `.vb-editor`); the primary fill
 * keeps its own hover, only while enabled (the base rule's
 * `:hover:not(:disabled)` posture).
 *
 * Listeners, `data-testid`, `title` and `aria-*` fall through to the
 * `<button>`, and a guide target bound with `:ref` reaches it as `$el`.
 */
import type { EditorIconName } from "../icons/conceptIcons";
import EditorIcon from "../icons/EditorIcon.vue";

withDefaults(
  defineProps<{
    icon: EditorIconName;
    variant?: "ghost" | "bordered" | "primary";
    disabled?: boolean;
    /** The icon's own colour (Help's book is `text-accent`). */
    iconClass?: string;
  }>(),
  { variant: "ghost", disabled: false, iconClass: "" },
);

const VARIANTS: Record<string, string> = {
  ghost: "border-transparent bg-transparent text-fg",
  bordered: "border-line bg-panel text-fg",
  primary: "border-transparent bg-primary font-semibold text-white enabled:hover:bg-primary-hover",
};
</script>

<template>
  <button
    type="button"
    :disabled="disabled"
    class="inline-flex min-h-[34px] shrink-0 items-center gap-[7px] rounded-[7px] border px-2.5 py-[7px] text-[11px] leading-none whitespace-nowrap"
    :class="VARIANTS[variant]"
  >
    <EditorIcon
      :name="icon"
      :class="iconClass"
    />
    <slot />
  </button>
</template>
