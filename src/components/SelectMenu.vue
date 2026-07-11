<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref } from "vue";

interface Option {
  value: string | number;
  label: string;
}

const props = withDefaults(
  defineProps<{
    modelValue: string | number;
    options: readonly Option[];
    id?: string;
    ariaLabel?: string;
    dataTestid?: string;
    // The component has two root nodes (button + Teleport), so a fallthrough
    // `class` can't reach the trigger — full-width is an explicit prop.
    wide?: boolean;
  }>(),
  { id: undefined, ariaLabel: undefined, dataTestid: undefined, wide: false },
);
const emit = defineEmits<{ (e: "update:modelValue", value: string | number): void }>();

const open = ref(false);
const activeIndex = ref(-1);
const triggerRef = ref<HTMLButtonElement | null>(null);
const popupRef = ref<HTMLUListElement | null>(null);
const popupStyle = ref<Record<string, string>>({});

const selectedLabel = computed(
  () => props.options.find((o) => o.value === props.modelValue)?.label ?? "",
);

function positionPopup() {
  const rect = triggerRef.value?.getBoundingClientRect();
  if (!rect) return;
  const spaceBelow = window.innerHeight - rect.bottom;
  const spaceAbove = rect.top;
  const openUp = spaceBelow < 200 && spaceAbove > spaceBelow;
  const maxH = Math.max(120, Math.min(220, (openUp ? spaceAbove : spaceBelow) - 8));
  const style: Record<string, string> = {
    position: "fixed",
    left: `${rect.left}px`,
    minWidth: `${rect.width}px`,
    maxHeight: `${maxH}px`,
    zIndex: "50",
  };
  if (openUp) style.bottom = `${window.innerHeight - rect.top + 4}px`;
  else style.top = `${rect.bottom + 4}px`;
  popupStyle.value = style;
}

// Option ids for aria-activedescendant — the listbox has focus, so AT needs
// an id trail to the highlighted option (GAP-33).
const listboxId = computed(() => props.id ?? props.dataTestid ?? "select-menu");
const optionId = (i: number) => `${listboxId.value}-opt-${i}`;

function setActive(i: number) {
  activeIndex.value = i;
  // A 13-item list scrolls at 220px — keep the highlight on-screen.
  void nextTick(() => {
    document.getElementById(optionId(i))?.scrollIntoView?.({ block: "nearest" });
  });
}

// Scroll must never DISMISS the menu (the old close-on-scroll made a popup
// taller than its 220px max unreachable — wheeling its own option list closed
// it). A scroll inside the popup is navigation: ignore it. A scroll outside
// moves the trigger under the position:fixed popup: re-anchor instead.
// Capture-phase listener because scroll doesn't bubble.
function onWindowScroll(e: Event) {
  if (popupRef.value?.contains(e.target as Node)) return;
  positionPopup();
}

async function openMenu() {
  open.value = true;
  activeIndex.value = Math.max(
    0,
    props.options.findIndex((o) => o.value === props.modelValue),
  );
  positionPopup();
  await nextTick();
  positionPopup();
  popupRef.value?.focus();
  window.addEventListener("scroll", onWindowScroll, true);
  window.addEventListener("resize", closeMenu);
  document.addEventListener("pointerdown", onDocPointerDown, true);
}

function closeMenu() {
  if (!open.value) return;
  open.value = false;
  window.removeEventListener("scroll", onWindowScroll, true);
  window.removeEventListener("resize", closeMenu);
  document.removeEventListener("pointerdown", onDocPointerDown, true);
  triggerRef.value?.focus();
}

function onDocPointerDown(e: PointerEvent) {
  const t = e.target as Node;
  if (triggerRef.value?.contains(t) || popupRef.value?.contains(t)) return;
  closeMenu();
}

function toggle() {
  if (open.value) closeMenu();
  else void openMenu();
}

function select(value: string | number) {
  emit("update:modelValue", value);
  closeMenu();
}

function onTriggerKeydown(e: KeyboardEvent) {
  if (["ArrowDown", "ArrowUp", "Enter", " "].includes(e.key) && !open.value) {
    e.preventDefault();
    void openMenu();
  }
}

function onPopupKeydown(e: KeyboardEvent) {
  if (e.key === "Escape") {
    e.preventDefault();
    // GAP-27: without stopPropagation the Escape bubbles to window, where
    // PanelRoot closes the whole panel — dismissing a dropdown must only
    // dismiss the dropdown.
    e.stopPropagation();
    closeMenu();
  } else if (e.key === "ArrowDown") {
    e.preventDefault();
    setActive(Math.min(props.options.length - 1, activeIndex.value + 1));
  } else if (e.key === "ArrowUp") {
    e.preventDefault();
    setActive(Math.max(0, activeIndex.value - 1));
  } else if (e.key === "Home") {
    e.preventDefault();
    setActive(0);
  } else if (e.key === "End") {
    e.preventDefault();
    setActive(props.options.length - 1);
  } else if (e.key === "Enter") {
    e.preventDefault();
    const o = props.options[activeIndex.value];
    if (o) select(o.value);
  }
}

onBeforeUnmount(() => {
  window.removeEventListener("scroll", onWindowScroll, true);
  window.removeEventListener("resize", closeMenu);
  document.removeEventListener("pointerdown", onDocPointerDown, true);
});
</script>

<template>
  <button
    :id="id"
    ref="triggerRef"
    type="button"
    :data-testid="dataTestid"
    :aria-label="ariaLabel"
    :aria-expanded="open"
    aria-haspopup="listbox"
    :class="{ 'w-full': wide }"
    class="flex items-center justify-between gap-2 rounded-lg border border-white/10 bg-white/5 px-2 py-1 text-sm text-slate-100 focus:border-violet-400 focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400"
    @click="toggle"
    @keydown="onTriggerKeydown"
  >
    <span>{{ selectedLabel }}</span>
    <svg
      width="12"
      height="12"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      stroke-width="2"
      aria-hidden="true"
    >
      <path d="M6 9l6 6 6-6" />
    </svg>
  </button>
  <Teleport to="body">
    <ul
      v-if="open"
      ref="popupRef"
      role="listbox"
      tabindex="-1"
      :style="popupStyle"
      :aria-activedescendant="activeIndex >= 0 ? optionId(activeIndex) : undefined"
      class="panel-scroll overflow-y-auto rounded-lg border border-white/10 bg-slate-900/95 py-1 text-sm text-slate-100 shadow-xl focus:outline-none"
      @keydown="onPopupKeydown"
    >
      <li
        v-for="(o, i) in options"
        :id="optionId(i)"
        :key="String(o.value)"
        role="option"
        :aria-selected="o.value === modelValue"
        :data-testid="dataTestid ? `${dataTestid}-option-${o.value}` : undefined"
        class="cursor-pointer px-3 py-1"
        :class="[
          o.value === modelValue ? 'bg-violet-500/25 text-white' : '',
          i === activeIndex ? 'bg-white/10' : '',
        ]"
        @click="select(o.value)"
        @pointermove="activeIndex = i"
      >
        {{ o.label }}
      </li>
    </ul>
  </Teleport>
</template>
