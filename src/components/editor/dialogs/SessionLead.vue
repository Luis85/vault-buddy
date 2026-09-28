<script setup lang="ts">
/**
 * The session dialogs' lead row (visual-parity Task 22; concept spec §9.9,
 * `session-safety.css: .session-lead`, `.session-icon`): an accent icon
 * tile (padding 13, radius 10, a 23 px icon), then an h3 15px and its
 * paragraph (12px, secondary ink) — the default slot. Shared by the
 * recovery offer, the close guard and Discard project, so the three read as
 * one family. Carries no `<b>`: the recovery offer's body must hold none
 * (its text renders Rust's words, which may quote markup).
 */
import type { EditorIconName } from "../icons/conceptIcons";
import EditorIcon from "../icons/EditorIcon.vue";

defineProps<{ icon: EditorIconName; title: string }>();
</script>

<template>
  <div class="flex items-center gap-3.5">
    <span class="shrink-0 rounded-[10px] bg-accent-bg p-[13px] text-accent">
      <EditorIcon
        :name="icon"
        :size="23"
      />
    </span>
    <div class="min-w-0">
      <h3 class="text-[15px] font-semibold leading-[1.5] text-fg">
        {{ title }}
      </h3>
      <p class="mt-1 text-xs leading-[1.65] text-fg-secondary">
        <slot />
      </p>
    </div>
  </div>
</template>
