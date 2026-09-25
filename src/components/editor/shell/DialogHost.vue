<script setup lang="ts">
/**
 * The tutorial editor's focus-safe dialog host (Task 19; F-48/F-49;
 * ARCHITECTURE-AND-STACK.md: "`ContextMenu` / `DialogHost` / `LearningCenter`
 * / `GuideOverlay`"; ONBOARDING.md: "A modal dialog suspends the coach;
 * closing it resumes the same lesson and restores appropriate focus … No
 * hidden focus trap may make the editor unreachable.").
 *
 * A controlled component (`ContextMenu.vue`'s own contract, reused here):
 * the caller owns `open` and this component owns focus while it is —
 * trapping Tab inside its content, closing on Escape/backdrop only when
 * `closable`, and restoring focus to whatever was focused the instant it
 * opened. It never closes ITSELF; it only asks (`emit("close")`), the same
 * "the caller flips the prop" discipline `ContextMenu` uses, so a caller
 * that ignores the request (e.g. a Render dialog mid an unbounded
 * operation) simply keeps the dialog open rather than fighting a second
 * source of truth.
 *
 * It registers on the shared `dialogs.ts` stack (`pushDialog`/`popDialog`)
 * so a SECOND `DialogHost` opened from inside this one's own content (a
 * nested confirm) becomes the one Tab/Escape answers to — this instance's
 * trap goes inert, via `isTopDialog`, until the inner one closes. Nothing
 * here hides this instance's content while suspended; a caller that wants
 * visual dimming for a background dialog composes that itself.
 *
 * `suspend`/`resume` fire exactly once per open/close pair, and the same
 * two moments suspend and resume the guide's coach (Task 56;
 * ONBOARDING.md: "a modal dialog suspends the coach; closing it resumes
 * the same lesson"): `editorOnboarding.suspend()`/`resume()` count, so a
 * confirm stacked on a dialog keeps the coach suspended until the last one
 * closes. Focus goes back to the opener exactly as before — the coach does
 * not take it on resume.
 */
import { nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";

import { isTopDialog, nextDialogId, popDialog, pushDialog } from "../../../editor/dialogs";
import { useEditorOnboardingStore } from "../../../stores/editorOnboarding";

const props = withDefaults(
  defineProps<{
    open: boolean;
    /** An accessible name for the dialog region (`aria-label`). */
    label: string;
    /** Whether Escape/backdrop may close this dialog. A dialog mid an
     * irreversible operation (a render/export in flight) passes `false` and
     * offers its own explicit way out instead. */
    closable?: boolean;
  }>(),
  { closable: true },
);
const emit = defineEmits<{
  (e: "close"): void;
  (e: "suspend"): void;
  (e: "resume"): void;
}>();

const id = nextDialogId();
const guide = useEditorOnboardingStore();
const content = ref<HTMLElement | null>(null);
/** Whatever had focus the instant this dialog opened — captured
 * automatically, `ContextMenu.vue`'s own `invoker` precedent, rather than
 * taken as a prop that could drift from what was actually focused. */
let opener: HTMLElement | null = null;

const FOCUSABLE_SELECTOR =
  'a[href], button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';
function focusablesIn(root: HTMLElement): HTMLElement[] {
  return Array.from(root.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR));
}

async function activate(): Promise<void> {
  pushDialog({ id, closable: props.closable });
  opener = document.activeElement as HTMLElement | null;
  guide.suspend();
  emit("suspend");
  // Vue hasn't painted the `v-if="open"` content yet on the same tick this
  // runs (the watch below fires synchronously on the prop change) — wait
  // for it, the `ContextMenu.vue` open-watcher's own `nextTick` before
  // reading `content`.
  await nextTick();
  const first = content.value ? focusablesIn(content.value)[0] : null;
  (first ?? content.value)?.focus();
}

function deactivate(): void {
  popDialog(id);
  guide.resume();
  emit("resume");
  opener?.focus();
  opener = null;
}

watch(
  () => props.open,
  (isOpen) => {
    if (isOpen) void activate();
    else deactivate();
  },
);
onMounted(() => {
  // The watch above only fires on a CHANGE, so a dialog that starts open
  // (rare, but not forbidden by the contract) needs its own activation.
  if (props.open) void activate();
});
onBeforeUnmount(() => {
  // A parent that removes this component outright, without first flipping
  // `open` to false, must not leave a dead entry on the shared stack or an
  // un-resumed guide.
  if (props.open) deactivate();
});

function requestClose(): void {
  if (!props.closable || !isTopDialog(id)) return;
  emit("close");
}

/** The Tab half of the focus trap, split out of `onKeydown` so neither
 * function's own branching (this repo's fallow complexity ratchet) grows
 * past the fold — Escape-vs-Tab dispatch stays in `onKeydown`, cycling the
 * two ends of the focusable list lives here. */
function trapTab(event: KeyboardEvent, root: HTMLElement): void {
  const focusables = focusablesIn(root);
  if (focusables.length === 0) {
    // Nothing to cycle through — keep focus pinned inside the dialog rather
    // than letting Tab escape to whatever sits behind it.
    event.preventDefault();
    root.focus();
    return;
  }
  const first = focusables[0];
  const last = focusables[focusables.length - 1];
  if (event.shiftKey && document.activeElement === first) {
    event.preventDefault();
    last.focus();
  } else if (!event.shiftKey && document.activeElement === last) {
    event.preventDefault();
    first.focus();
  }
}

function onKeydown(event: KeyboardEvent): void {
  if (!isTopDialog(id)) return;
  if (event.key === "Escape") {
    event.preventDefault();
    event.stopPropagation();
    requestClose();
    return;
  }
  if (event.key === "Tab" && content.value) trapTab(event, content.value);
}

function onBackdrop(): void {
  requestClose();
}
</script>

<template>
  <div
    v-if="open"
    data-testid="dialog-host"
    class="fixed inset-0 z-50 flex items-center justify-center bg-black/50"
    @mousedown.self="onBackdrop"
  >
    <div
      ref="content"
      role="dialog"
      aria-modal="true"
      :aria-label="label"
      data-testid="dialog-host-content"
      tabindex="-1"
      class="max-h-[90vh] max-w-[90vw] overflow-y-auto rounded-control border border-line bg-panel p-4 text-fg focus:outline-none"
      @keydown="onKeydown"
    >
      <slot />
    </div>
  </div>
</template>
