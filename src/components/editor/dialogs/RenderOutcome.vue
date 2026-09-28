<script setup lang="ts">
/**
 * The Render dialog after Start (Task 47; SCREENS 09): the job's progress
 * (`RenderProgress` — 100 % only from the `complete` terminal), Cancel
 * while it runs, the one status line once it ends, and on completion
 * **Watch rendered file** (the real file, `ProductPlayer`), **Publish to
 * vault…** (Task 48: the dialog opens `PublishDialog` for the product) and
 * **Render another** — and (visual-parity Task 10, design D9) **Show in
 * workspace**, the library's Project section where every product lives.
 * Presentational: the dialog owns the job.
 *
 * Visual-parity Task 21: the completion names where a publish goes by the
 * vault's NAME (design D6) — never its id — or says no vault is set yet.
 */
import { computed, ref } from "vue";

import type { RenderProgressView } from "../../../editor/renderProgress";
import ProductPlayer from "../preview/ProductPlayer.vue";
import RenderProgress from "../preview/RenderProgress.vue";
import DialogButton from "./DialogButton.vue";

const props = defineProps<{
  job: RenderProgressView | null;
  running: boolean;
  /** The one line once the render ended (or never started); `null` while
   * it runs. `alert` marks a failure — a cancel is not one. */
  status: { text: string; alert: boolean } | null;
  /** Set only from a `complete` terminal. */
  productId: string | null;
  name: string;
  /** The project's destination vault by name; `null` when none is set. */
  vaultName: string | null;
}>();
const emit = defineEmits<{
  (e: "cancel"): void;
  (e: "another"): void;
  (e: "publish"): void;
  (e: "products"): void;
}>();

const watching = ref(false);
const media = computed(() => (props.productId ? { productId: props.productId } : null));
const statusRole = computed(() => (props.status?.alert ? "alert" : "status"));
const statusClass = computed(() => (props.status?.alert ? "text-danger-fg" : "text-fg-secondary"));
const againLabel = computed(() => (media.value ? "Render another" : "Try again"));
const publishLine = computed(() =>
  props.vaultName
    ? `Publish it into ${props.vaultName} with Publish to vault…, or keep it in this project's workspace.`
    : "Publish it into a vault with Publish to vault…, or keep it in this project's workspace.",
);

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
  <p
    v-if="media"
    data-testid="render-dialog-publish-line"
    class="text-[11px] text-fg-muted"
  >
    {{ publishLine }}
  </p>
  <ProductPlayer
    v-if="watching && media"
    :media="media"
    :label="`Rendered video: ${name}`"
  />
  <div class="flex flex-wrap items-center justify-end gap-2">
    <DialogButton
      v-if="running"
      data-testid="render-dialog-cancel"
      @click="emit('cancel')"
    >
      Cancel render
    </DialogButton>
    <template v-if="media">
      <DialogButton
        data-testid="render-dialog-watch"
        @click="watching = true"
      >
        Watch rendered file
      </DialogButton>
      <DialogButton
        data-testid="render-dialog-publish"
        @click="emit('publish')"
      >
        Publish to vault…
      </DialogButton>
      <DialogButton
        data-testid="render-dialog-products"
        @click="emit('products')"
      >
        Show in workspace
      </DialogButton>
    </template>
    <DialogButton
      v-if="status"
      variant="primary"
      data-testid="render-dialog-another"
      @click="another"
    >
      {{ againLabel }}
    </DialogButton>
  </div>
</template>
