<script setup lang="ts">
/**
 * The Render dialog after Start (Task 47; SCREENS 09): the job's progress
 * (`RenderProgress` — 100 % only from the `complete` terminal), Cancel
 * while it runs, the one status line once it ends, and on completion
 * **Watch rendered file** (the real file, `ProductPlayer`), **Publish to
 * vault…** (Task 48: the dialog opens `PublishDialog` for the product) and
 * **Render another**. Presentational: the dialog owns the job.
 */
import { computed, ref } from "vue";

import type { RenderProgressView } from "../../../editor/renderProgress";
import AppButton from "../../ui/AppButton.vue";
import ProductPlayer from "../preview/ProductPlayer.vue";
import RenderProgress from "../preview/RenderProgress.vue";

const props = defineProps<{
  job: RenderProgressView | null;
  running: boolean;
  /** The one line once the render ended (or never started); `null` while
   * it runs. `alert` marks a failure — a cancel is not one. */
  status: { text: string; alert: boolean } | null;
  /** Set only from a `complete` terminal. */
  productId: string | null;
  name: string;
}>();
const emit = defineEmits<{ (e: "cancel"): void; (e: "another"): void; (e: "publish"): void }>();

const watching = ref(false);
const media = computed(() => (props.productId ? { productId: props.productId } : null));
const statusRole = computed(() => (props.status?.alert ? "alert" : "status"));
const statusClass = computed(() => (props.status?.alert ? "text-danger-fg" : "text-fg-secondary"));
const againLabel = computed(() => (media.value ? "Render another" : "Try again"));

function another(): void {
  watching.value = false;
  emit("another");
}
</script>

<template>
  <RenderProgress
    v-if="job"
    :job="job"
  />
  <p
    v-if="status"
    data-testid="render-dialog-status"
    :role="statusRole"
    class="text-xs"
    :class="statusClass"
  >
    {{ status.text }}
  </p>
  <ProductPlayer
    v-if="watching && media"
    :media="media"
    :label="`Rendered video: ${name}`"
  />
  <div class="flex flex-wrap items-center justify-end gap-2">
    <AppButton
      v-if="running"
      variant="secondary"
      size="sm"
      data-testid="render-dialog-cancel"
      @click="emit('cancel')"
    >
      Cancel render
    </AppButton>
    <template v-if="media">
      <AppButton
        variant="secondary"
        size="sm"
        data-testid="render-dialog-watch"
        @click="watching = true"
      >
        Watch rendered file
      </AppButton>
      <AppButton
        variant="secondary"
        size="sm"
        data-testid="render-dialog-publish"
        @click="emit('publish')"
      >
        Publish to vault…
      </AppButton>
    </template>
    <AppButton
      v-if="status"
      variant="primary"
      size="sm"
      data-testid="render-dialog-another"
      @click="another"
    >
      {{ againLabel }}
    </AppButton>
  </div>
</template>
