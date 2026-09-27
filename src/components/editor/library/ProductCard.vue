<script setup lang="ts">
/**
 * One Rendered Product in the library (Task 47; F-41; SCREENS 09): its
 * name, the revision it was rendered from, its range, when it was made and
 * whether its file is still on disk — plus Watch (the real file,
 * `ProductPlayer`), Publish to vault… (Task 48; `ProductLibrary` opens the
 * Publish dialog) and Restore this edit with its confirmation step.
 * Presentational: `ProductLibrary` owns which card is watching/confirming
 * and performs the restore.
 *
 * A missing file keeps the card and its lineage; only Watch and Publish
 * go, and each says why (R20: a disabled control carries a reason).
 *
 * Visual-parity Task 10 (concept spec §3.6 `.product-card`): a 9px-radius
 * card — a pill saying whether it "Matches this edit" (rendered by this
 * session from the edit on screen) or an "Earlier edit", the mono revision, the name,
 * the length and range, when it was made, then Watch and Publish as small
 * bordered buttons and Restore as the concept's undo icon.
 */
import { computed } from "vue";

import { rangeTimeLabel } from "../../../editor/renderRanges";
import type { ProductDto } from "../../../editorTypes";
import { formatDuration } from "../../../utils/formatDuration";
import AppButton from "../../ui/AppButton.vue";
import EditorIcon from "../icons/EditorIcon.vue";
import ProductPlayer from "../preview/ProductPlayer.vue";

const props = defineProps<{
  product: ProductDto;
  watching: boolean;
  confirming: boolean;
  /** A restore is in flight somewhere in the library. */
  busy: boolean;
  /** Rendered by this session from the edit on screen
   * (`editorProducts.matchesEdit`). */
  current: boolean;
}>();
const emit = defineEmits<{
  (e: "toggle-watch"): void;
  (e: "publish"): void;
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
const metaLabel = computed(() => `${formatDuration(props.product.durationMs)} · ${rangeLabel.value}`);
const createdLabel = computed(() => {
  const at = new Date(props.product.createdAt);
  return Number.isNaN(at.getTime()) ? props.product.createdAt : at.toLocaleString();
});
const watchTitle = computed(() => (props.product.available ? undefined : MISSING));
const media = computed(() => ({ productId: props.product.id }));
const showPlayer = computed(() => props.watching && props.product.available);
const SMALL_BUTTON =
  "flex min-h-[30px] items-center gap-1.5 rounded-[7px] border border-line bg-panel px-2 py-1 text-[11px] text-fg hover:bg-hover focus:outline-none focus-visible:ring-2 focus-visible:ring-focus disabled:cursor-not-allowed disabled:opacity-40";
</script>

<template>
  <article
    :data-testid="`product-card-${id}`"
    class="my-2.5 flex flex-col rounded-[9px] border border-line p-3 text-fg"
  >
    <div class="flex items-center justify-between gap-2">
      <span
        :data-testid="`product-match-${id}`"
        class="rounded px-1.5 py-0.5 text-[9px] tracking-[0.3px]"
        :class="current ? 'bg-audio-bg text-audio' : 'bg-gold-bg text-gold'"
      >{{ current ? "Matches this edit" : "Earlier edit" }}</span>
      <span class="font-mono text-[10px] text-fg-muted">r{{ product.revision }}</span>
    </div>
    <b class="mt-2.5 block break-words text-[12px] font-medium leading-normal">{{ product.name }}</b>
    <p class="mt-[5px] text-[10px] leading-normal text-fg-muted">
      {{ metaLabel }}
    </p>
    <time
      :datetime="product.createdAt"
      :data-testid="`product-created-${id}`"
      class="text-[10px] text-fg-muted"
    >{{ createdLabel }}</time>
    <span
      v-if="!product.available"
      :data-testid="`product-unavailable-${id}`"
      class="mt-1 text-[10px] text-danger-fg"
    >Unavailable — {{ MISSING }}</span>

    <div class="mt-2.5 flex flex-wrap items-center gap-1.5">
      <button
        type="button"
        :data-testid="`product-watch-${id}`"
        :disabled="!product.available"
        :title="watchTitle"
        :class="SMALL_BUTTON"
        @click="emit('toggle-watch')"
      >
        <EditorIcon
          name="play"
          :size="14"
        />
        {{ watching ? "Hide" : "Watch" }}
      </button>
      <button
        type="button"
        :data-testid="`product-publish-${id}`"
        :disabled="!product.available"
        :title="watchTitle"
        :class="SMALL_BUTTON"
        @click="emit('publish')"
      >
        <EditorIcon
          name="vault"
          :size="14"
        />
        Publish to vault…
      </button>
      <button
        type="button"
        :data-testid="`product-restore-${id}`"
        :disabled="busy"
        :aria-label="`Restore the edit ${product.name} was rendered from`"
        title="Restore the edit this video was rendered from"
        class="ml-auto flex h-8 w-8 items-center justify-center p-1.5 text-fg-muted hover:bg-hover focus:outline-none focus-visible:ring-2 focus-visible:ring-focus disabled:cursor-not-allowed disabled:opacity-40"
        @click="emit('ask-restore')"
      >
        <EditorIcon
          name="undo"
          :size="16"
        />
      </button>
    </div>

    <div
      v-if="confirming"
      class="mt-2.5 flex flex-col gap-1.5 rounded-[7px] border border-line p-2 text-[11px]"
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
