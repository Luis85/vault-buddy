<script setup lang="ts">
import { onMounted, onBeforeUnmount, ref, watch } from 'vue';
/** Media handles live in a scoped controller, not deep reactive project state. */
export interface PreviewController {
  attach(canvas: HTMLCanvasElement): void;
  seek(timeMs: number): void;
  resize(width: number, height: number): void;
  detach(): void;
}
const props = defineProps<{ controller: PreviewController; playheadMs: number; description: string }>();
const emit = defineEmits<{ context: [event: MouseEvent]; focusProperties: [] }>();
const surface = ref<HTMLCanvasElement|null>(null);
let observer: ResizeObserver|undefined;
onMounted(() => {
  const canvas = surface.value;
  if (!canvas) return;
  props.controller.attach(canvas);
  observer = new ResizeObserver(([entry]) => {
    if (entry) props.controller.resize(entry.contentRect.width, entry.contentRect.height);
  });
  observer.observe(canvas); props.controller.seek(props.playheadMs);
});
watch(() => props.playheadMs, time => props.controller.seek(time));
onBeforeUnmount(() => { observer?.disconnect(); props.controller.detach(); });
</script>
<template>
  <canvas ref="surface" data-guide-id="preview" tabindex="0" :aria-label="description"
    @contextmenu.prevent="emit('context', $event)" @keydown.f6.prevent="emit('focusProperties')" />
</template>
<style scoped>
canvas { display:block; width:100%; height:100%; touch-action:none; }
canvas:focus-visible { outline:2px solid var(--color-focus, #b6a2f5); outline-offset:2px; }
</style>
