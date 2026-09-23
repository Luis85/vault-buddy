<script setup lang="ts">
/**
 * The Save project menu's toggle and popup (Task 39; F-40): Save, Save a
 * portable copy…, Save a lightweight copy…, Open a project file…. Purely
 * presentational — it only says which item was chosen; `EditorHeader`
 * decides what each one does. Closes on a choice, on Escape (focus back on
 * the toggle) and on a pointer press anywhere outside it.
 */
import { onBeforeUnmount, onMounted, ref } from "vue";

type SaveMenuItem = "save" | "portable" | "lightweight" | "open";

defineProps<{ disabled: boolean }>();
const emit = defineEmits<{ (e: "choose", item: SaveMenuItem): void }>();

const ITEMS: readonly { id: SaveMenuItem; label: string }[] = [
  { id: "save", label: "Save" },
  { id: "portable", label: "Save a portable copy…" },
  { id: "lightweight", label: "Save a lightweight copy…" },
  { id: "open", label: "Open a project file…" },
];

const open = ref(false);
const root = ref<HTMLElement | null>(null);
const toggle = ref<HTMLButtonElement | null>(null);

function choose(item: SaveMenuItem): void {
  open.value = false;
  emit("choose", item);
}

function onEscape(): void {
  open.value = false;
  toggle.value?.focus();
}

function onPointerDown(event: PointerEvent): void {
  if (open.value && !root.value?.contains(event.target as Node)) open.value = false;
}

onMounted(() => document.addEventListener("pointerdown", onPointerDown));
onBeforeUnmount(() => document.removeEventListener("pointerdown", onPointerDown));
</script>

<template>
  <div
    ref="root"
    class="relative"
    @keydown.esc.stop="onEscape"
  >
    <button
      ref="toggle"
      type="button"
      data-testid="editor-header-save-menu-toggle"
      aria-haspopup="menu"
      :aria-expanded="open"
      aria-label="More save options"
      :disabled="disabled"
      class="cursor-pointer rounded-control border border-white/10 bg-white/5 px-1.5 py-1 text-xs text-fg hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-focus disabled:cursor-default disabled:opacity-50"
      @click="open = !open"
    >
      ▾
    </button>
    <div
      v-if="open"
      role="menu"
      aria-label="Save project"
      class="absolute right-0 top-full z-20 mt-1 flex min-w-52 flex-col gap-0.5 rounded-control border border-white/10 bg-slate-800 p-1 shadow-lg"
    >
      <button
        v-for="item in ITEMS"
        :key="item.id"
        type="button"
        role="menuitem"
        :data-testid="`editor-header-menu-${item.id}`"
        class="cursor-pointer rounded px-2 py-1 text-left text-xs text-fg-secondary hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-focus"
        @click="choose(item.id)"
      >
        {{ item.label }}
      </button>
    </div>
  </div>
</template>
