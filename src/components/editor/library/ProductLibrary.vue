<script setup lang="ts">
/**
 * The project's Rendered Products (Task 47; F-41, F-42; SCREENS 09), the
 * library's Products tab (`LibraryPanel`; the guide's `library.products`
 * target). One `ProductCard` per ledger entry: name, the revision it was
 * rendered from, its range, when it was made, and whether its file is
 * still on disk.
 *
 * **Watch** plays the ACTUAL encoded file (`ProductPlayer`, fed by
 * `editor_media_url({ productId })`) — never the editable preview, which is
 * only an approximation of a render. **Restore this edit** asks first, then
 * makes the product's frozen edit the live one (`editor_restore_product`:
 * a new revision, one undo step); the product itself never changes.
 *
 * A product whose file is gone keeps its card and its lineage — the
 * revision and edit it came from are in the ledger, not in the file — so it
 * can still be restored; only Watch is unavailable, and says why. A Review
 * render is never listed: it is not a product (F18).
 */
import { onMounted, ref, watch } from "vue";

import { useEditorProductsStore } from "../../../stores/editorProducts";
import { useEditorProjectStore } from "../../../stores/editorProject";
import ProductCard from "./ProductCard.vue";

const editorProject = useEditorProjectStore();
const products = useEditorProductsStore();

onMounted(() => void products.refresh());
watch(
  () => editorProject.sessionId,
  () => void products.refresh(),
);

const watchingId = ref<string | null>(null);
const confirmingId = ref<string | null>(null);
const restoringId = ref<string | null>(null);

function toggleWatch(id: string): void {
  watchingId.value = watchingId.value === id ? null : id;
}

async function confirmRestore(id: string): Promise<void> {
  confirmingId.value = null;
  restoringId.value = id;
  try {
    await products.restore(id);
  } finally {
    restoringId.value = null;
  }
}
</script>

<template>
  <div
    data-testid="product-library"
    class="flex h-full flex-col gap-2 overflow-y-auto"
  >
    <p
      v-if="products.error"
      role="alert"
      data-testid="product-library-error"
      class="text-xs text-danger-fg"
    >
      {{ products.error.message }}
    </p>
    <p
      v-if="products.current.length === 0"
      data-testid="product-library-empty"
      class="text-xs text-fg-subtle"
    >
      No rendered videos yet. Render video in the header makes one; it appears here.
    </p>
    <ProductCard
      v-for="p in products.current"
      :key="p.id"
      :product="p"
      :watching="watchingId === p.id"
      :confirming="confirmingId === p.id"
      :busy="restoringId !== null"
      @toggle-watch="toggleWatch(p.id)"
      @ask-restore="confirmingId = p.id"
      @confirm-restore="confirmRestore(p.id)"
      @cancel-restore="confirmingId = null"
    />
  </div>
</template>
