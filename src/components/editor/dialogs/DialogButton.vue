<script setup lang="ts">
/**
 * A dialog footer action (visual-parity Task 21; concept spec §9 shared
 * chrome, §1.5's button base): `bordered` is the concept's `.btn` (Keep
 * editing, Back to edit, Export diagnostics), `primary` its `.primary`
 * (Save copy, Render video, Continue to render: white 600 on
 * `--color-primary`). 34px tall, 12px, an optional icon before the label.
 * Hover and the disabled look come from the editor's base button rules
 * (`style.css`, `.vb-editor`); the primary fill keeps its own hover, only
 * while enabled.
 *
 * Listeners, `data-testid` and `aria-*` fall through to the `<button>`.
 * `reason` is why it cannot act right now: set, the button is disabled and
 * says so in its `title` (design D14) — one prop, so a caller cannot
 * disable a button and forget to say why.
 */
import type { EditorIconName } from "../icons/conceptIcons";
import EditorIcon from "../icons/EditorIcon.vue";

withDefaults(
  defineProps<{
    variant?: "bordered" | "primary";
    icon?: EditorIconName | null;
    /** Why the button cannot act; `null` = it can. */
    reason?: string | null;
  }>(),
  { variant: "bordered", icon: null, reason: null },
);

const VARIANTS: Record<string, string> = {
  bordered: "border-line bg-panel text-fg",
  primary: "border-transparent bg-primary font-semibold text-white enabled:hover:bg-primary-hover",
};
</script>

<template>
  <button
    type="button"
    :disabled="reason !== null"
    :title="reason ?? undefined"
    class="inline-flex min-h-[34px] shrink-0 items-center gap-[7px] rounded-[7px] border px-[13px] py-[7px] text-xs leading-none whitespace-nowrap"
    :class="VARIANTS[variant]"
  >
    <EditorIcon
      v-if="icon"
      :name="icon"
      :size="16"
    />
    <slot />
  </button>
</template>
