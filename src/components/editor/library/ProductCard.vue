<script setup lang="ts">
/**
 * One Rendered Product in the library (Task 47; F-41; SCREENS 09): its
 * name, the revision it was rendered from, its range, when it was made and
 * whether its file is still on disk — plus Watch (the real file,
 * `ProductPlayer`) and Restore this edit with its confirmation step.
 * Presentational: `ProductLibrary` owns which card is watching/confirming
 * and performs the restore.
 *
 * A missing file keeps the card and its lineage; only Watch goes, and it
 * says why (R20: a disabled control carries a reason).
 */
import { computed } from "vue";

import { rangeTimeLabel } from "../../../editor/renderRanges";
import type { ProductDto } from "../../../editorTypes";
import AppButton from "../../ui/AppButton.vue";
import ProductPlayer from "../preview/ProductPlayer.vue";

const props = defineProps<{
  product: ProductDto;
  watching: boolean;
  confirming: boolean;
  /** A restore is in flight somewhere in the library. */
  busy: boolean;
}>();
const emit = defineEmits<{
  (e: "toggle-watch"): void;
  (e: "ask-restore"): void;
  (e: "confirm-restore"): void;
  (e: "cancel-restore"): void;
}>();

const MISSING = "The rendered file is no longer on disk.";

const id = computed(() => props.product.id);
const rangeLabel = computed(() => {
  const r = props.product.renderRange;
  return r ? `${rangeTimeLabel(r.startMs)}–${rangeTimeLabel(r.endMs)}` : "Whole project";
});
const createdLabel = computed(() => {
  const at = new Date(props.product.createdAt);
  return Number.isNaN(at.getTime()) ? props.product.createdAt : at.toLocaleString();
});
const watchTitle = computed(() => (props.product.available ? undefined : MISSING));
const media = computed(() => ({ productId: props.product.id }));
const showPlayer = computed(() => props.watching && props.product.available);
</script>

<template>
  <article
    :data-testid="`product-card-${id}`"
    class="flex flex-col gap-1 rounded-control border border-line bg-raised p-2 text-xs"
  >
    <div class="flex items-baseline justify-between gap-2">
      <span class="truncate text-sm font-medium text-fg">{{ product.name }}</span>
      <span class="shrink-0 rounded bg-accent/20 px-1 text-micro text-accent-fg">r{{ product.revision }}</span>
    </div>
    <span class="text-fg-muted">{{ rangeLabel }}</span>
    <time
      :datetime="product.createdAt"
      :data-testid="`product-created-${id}`"
      class="text-fg-subtle"
    >{{ createdLabel }}</time>
    <span
      v-if="!product.available"
      :data-testid="`product-unavailable-${id}`"
      class="text-danger-fg"
    >Unavailable — {{ MISSING }}</span>

    <div class="flex flex-wrap gap-1">
      <AppButton
        variant="secondary"
        size="sm"
        :data-testid="`product-watch-${id}`"
        :disabled="!product.available"
        :title="watchTitle"
        @click="emit('toggle-watch')"
      >
        {{ watching ? "Hide" : "Watch" }}
      </AppButton>
      <AppButton
        variant="ghost"
        size="sm"
        :data-testid="`product-restore-${id}`"
        :disabled="busy"
        @click="emit('ask-restore')"
      >
        Restore this edit
      </AppButton>
    </div>

    <div
      v-if="confirming"
      class="flex flex-col gap-1 rounded-control border border-line p-2"
    >
      <p
        :data-testid="`product-restore-question-${id}`"
        class="text-fg-secondary"
      >
        Replace the current edit with the one this video was rendered from? The
        video stays as it is, and Undo brings your current edit back.
      </p>
      <div class="flex gap-1">
        <AppButton
          variant="primary"
          size="sm"
          :data-testid="`product-restore-confirm-${id}`"
          @click="emit('confirm-restore')"
        >
          Restore
        </AppButton>
        <AppButton
          variant="ghost"
          size="sm"
          :data-testid="`product-restore-cancel-${id}`"
          @click="emit('cancel-restore')"
        >
          Keep current edit
        </AppButton>
      </div>
    </div>

    <ProductPlayer
      v-if="showPlayer"
      :media="media"
      :label="`Rendered video: ${product.name}`"
    />
  </article>
</template>
