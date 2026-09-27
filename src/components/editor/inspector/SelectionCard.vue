<script setup lang="ts">
/**
 * The selection's title card (visual-parity Task 13; concept spec §5
 * `.selection-title`): a 34px accent glyph tile, the name at 12px/550 and a
 * 10px muted line under it — a clip's "{track} · {d.d}s", a track's role,
 * a multi-selection's hint. Long names ellipsize (review focus 3).
 */
import type { EditorIconName } from "../icons/conceptIcons";
import EditorIcon from "../icons/EditorIcon.vue";

defineProps<{ icon: EditorIconName; name: string; detail: string }>();
</script>

<template>
  <div
    data-testid="inspector-card"
    class="mb-3 flex min-w-0 items-center gap-2.5"
  >
    <span
      data-testid="inspector-card-glyph"
      class="grid h-[34px] w-[34px] shrink-0 place-items-center rounded-[7px] bg-accent-bg text-accent-ink"
    >
      <EditorIcon :name="icon" />
    </span>
    <span class="min-w-0 flex-1">
      <b
        data-testid="inspector-card-name"
        class="block truncate text-[12px] font-[550] text-fg"
      >{{ name }}</b>
      <small
        data-testid="inspector-card-detail"
        class="block truncate text-[10px] text-fg-muted"
      >{{ detail }}</small>
    </span>
  </div>
</template>
