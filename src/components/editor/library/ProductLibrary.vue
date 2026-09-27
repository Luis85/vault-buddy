<script setup lang="ts">
/**
 * The project's Rendered Products (Task 47; F-41, F-42; SCREENS 09), in
 * the library's Project section (`ProjectSection`, visual-parity Task 10,
 * design D9 — Products left the tabs). A "RENDERED PRODUCTS" heading with
 * the count, then one `ProductCard` per ledger entry, newest first: name,
 * the revision it was rendered from (and whether that is the revision on
 * screen), its range, when it was made, and whether its file is still on
 * disk — or the concept's dashed "No renders yet" (§3.6).
 *
 * **Watch** plays the ACTUAL encoded file (`ProductPlayer`, fed by
 * `editor_media_url({ productId })`) — never the editable preview, which is
 * only an approximation of a render. **Restore this edit** asks first, then
 * makes the product's frozen edit the live one (`editor_restore_product`:
 * a new revision, one undo step); the product itself never changes.
 * **Publish to vault…** (Task 48) opens `PublishDialog` for that product —
 * any available product, not only the one a render just finished.
 *
 * A product whose file is gone keeps its card and its lineage — the
 * revision and edit it came from are in the ledger, not in the file — so it
 * can still be restored; only Watch is unavailable, and says why. A Review
 * render is never listed: it is not a product (F18).
 */
import { computed, onMounted, ref, watch } from "vue";

import { useEditorProductsStore } from "../../../stores/editorProducts";
import { useEditorProjectStore } from "../../../stores/editorProject";
import PublishDialog from "../dialogs/PublishDialog.vue";
import EditorIcon from "../icons/EditorIcon.vue";
import LibraryHeading from "./LibraryHeading.vue";
import ProductCard from "./ProductCard.vue";

const editorProject = useEditorProjectStore();
const products = useEditorProductsStore();
const newestFirst = computed(() => products.current.slice().reverse());
const onScreenRevision = computed(() => editorProject.snapshot?.revision ?? null);

onMounted(() => void products.refresh());
watch(
  () => editorProject.sessionId,
  () => void products.refresh(),
);

const watchingId = ref<string | null>(null);
const confirmingId = ref<string | null>(null);
const restoringId = ref<string | null>(null);
/** The product the Publish dialog is open for. */
const publishing = ref<{ id: string; name: string } | null>(null);

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
    class="flex flex-col"
  >
    <LibraryHeading
      label="RENDERED PRODUCTS"
      :pill="String(products.current.length)"
      testid="products"
    />
    <p
      v-if="products.error"
      role="alert"
      data-testid="product-library-error"
      class="text-[11px] text-danger-fg"
    >
      {{ products.error.message }}
    </p>
    <div
      v-if="products.current.length === 0"
      data-testid="product-library-empty"
      class="my-3.5 flex flex-col items-center gap-2.5 rounded-[9px] border border-dashed border-line px-3 py-[22px] text-center"
    >
      <EditorIcon
        name="video"
        :size="26"
        class="text-fg-muted"
      />
      <b class="text-[12px] font-semibold text-fg">No renders yet</b>
      <span class="text-[11px] leading-[1.6] text-fg-muted">Save now. Render when ready. Your project stays editable either way.</span>
    </div>
    <ProductCard
      v-for="p in newestFirst"
      :key="p.id"
      :product="p"
      :current="p.revision === onScreenRevision"
      :watching="watchingId === p.id"
      :confirming="confirmingId === p.id"
      :busy="restoringId !== null"
      @toggle-watch="toggleWatch(p.id)"
      @publish="publishing = { id: p.id, name: p.name }"
      @ask-restore="confirmingId = p.id"
      @confirm-restore="confirmRestore(p.id)"
      @cancel-restore="confirmingId = null"
    />
    <PublishDialog
      :open="publishing !== null"
      :product-id="publishing?.id ?? null"
      :product-name="publishing?.name ?? ''"
      @close="publishing = null"
    />
  </div>
</template>
