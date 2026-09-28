<script setup lang="ts">
/**
 * A dialog's Close ×, drawn in the editor's own tokens (visual-parity Task
 * 24, Ruling T21-3): it replaced the panel window's `IconButton` in
 * `DialogHost` and the mixer popover, whose white-opacity hover tint is
 * white over white on the editor's light theme. `reason` is why it cannot
 * close right now: set, the button is disabled and says so in its `title`
 * (design D14), like `DialogButton`'s. Listeners and `data-testid` fall
 * through to the `<button>`.
 */
import EditorIcon from "../icons/EditorIcon.vue";

withDefaults(
  defineProps<{
    /** The accessible name, and the tooltip while it can close. */
    label: string;
    /** Why it cannot close; `null` = it can. */
    reason?: string | null;
  }>(),
  { reason: null },
);
</script>

<template>
  <button
    type="button"
    :aria-label="label"
    :title="reason ?? label"
    :disabled="reason !== null"
    class="flex h-8 w-8 shrink-0 cursor-pointer items-center justify-center rounded-control text-fg-muted transition hover:bg-hover hover:text-fg active:scale-95 focus:outline-none focus-visible:ring-2 focus-visible:ring-focus disabled:cursor-default disabled:opacity-50"
  >
    <EditorIcon name="x" />
  </button>
</template>
