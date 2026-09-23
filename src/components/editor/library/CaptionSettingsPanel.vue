<script setup lang="ts">
/**
 * Caption presentation (Task 36; F-35): whether captions show, whether
 * the render burns them in, where they sit, how big, and whether they get
 * a readable background. Presentational -- emits one partial
 * `setCaptionSettings` payload per change; `CaptionsLibrary` sends it.
 *
 * A project with no `captions` yet shows the reference editor's defaults
 * (on, burned in, 30 px, bottom, background) -- exactly what Rust creates
 * the first time a caption or a setting needs somewhere to live
 * (`commands::captions::default_settings`). A font size outside 18-56 is
 * never emitted (Rust refuses it too); the field shows the stored size
 * again instead.
 */
import { computed } from "vue";

import type { CaptionPosition, CaptionSettings } from "../../../editorTypes";

export interface CaptionSettingsPatch {
  enabled?: boolean;
  burnIn?: boolean;
  fontSize?: number;
  position?: CaptionPosition;
  background?: boolean;
}

const props = defineProps<{
  settings: CaptionSettings | null;
  disabledReason: string | null;
}>();

const emit = defineEmits<{ (e: "change", patch: CaptionSettingsPatch): void }>();

const FONT_MIN = 18;
const FONT_MAX = 56;

const DEFAULTS = { enabled: true, burnIn: true, fontSize: 30, position: "bottom" as CaptionPosition, background: true };

const shown = computed(() => {
  const s = props.settings;
  if (!s) return DEFAULTS;
  return { enabled: s.enabled, burnIn: s.burn_in, fontSize: s.font_size, position: s.position, background: s.background };
});

function checked(event: Event): boolean {
  return (event.target as HTMLInputElement).checked;
}

function onPosition(event: Event): void {
  emit("change", { position: (event.target as HTMLSelectElement).value as CaptionPosition });
}

function onFontSize(event: Event): void {
  const input = event.target as HTMLInputElement;
  const size = Number(input.value);
  if (!Number.isFinite(size) || size < FONT_MIN || size > FONT_MAX || size === shown.value.fontSize) {
    input.value = String(shown.value.fontSize);
    return;
  }
  emit("change", { fontSize: size });
}
</script>

<template>
  <fieldset
    data-testid="caption-settings"
    :disabled="disabledReason !== null"
    :title="disabledReason ?? undefined"
    class="flex flex-col gap-1 rounded border border-line px-1.5 py-1"
  >
    <legend class="px-0.5 text-fg-subtle">
      Caption appearance
    </legend>
    <label class="flex items-center gap-1">
      <input
        data-testid="caption-enabled"
        type="checkbox"
        :checked="shown.enabled"
        class="accent-violet-500"
        @change="emit('change', { enabled: checked($event) })"
      >
      Show captions (preview and checks)
    </label>
    <label class="flex items-center gap-1">
      <input
        data-testid="caption-burn-in"
        type="checkbox"
        :checked="shown.burnIn"
        class="accent-violet-500"
        @change="emit('change', { burnIn: checked($event) })"
      >
      Burn into the rendered video
    </label>
    <label class="flex items-center gap-1">
      <input
        data-testid="caption-background"
        type="checkbox"
        :checked="shown.background"
        class="accent-violet-500"
        @change="emit('change', { background: checked($event) })"
      >
      Readable background
    </label>
    <div class="flex gap-2">
      <label class="flex items-center gap-1">
        Position
        <select
          data-testid="caption-position"
          :value="shown.position"
          class="rounded border border-line bg-stage px-0.5 text-fg focus:outline-none focus-visible:ring-1 focus-visible:ring-focus"
          @change="onPosition"
        >
          <option value="bottom">Bottom</option>
          <option value="top">Top</option>
        </select>
      </label>
      <label class="flex items-center gap-1">
        Size
        <input
          data-testid="caption-font-size"
          type="number"
          :min="FONT_MIN"
          :max="FONT_MAX"
          :value="shown.fontSize"
          class="w-12 rounded border border-line bg-stage px-0.5 text-fg focus:outline-none focus-visible:ring-1 focus-visible:ring-focus"
          @change="onFontSize"
        >
      </label>
    </div>
  </fieldset>
</template>
