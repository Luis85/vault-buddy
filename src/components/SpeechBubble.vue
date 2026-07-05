<script setup lang="ts">
defineProps<{
  text: string;
  // mirror the layout so the tail sits on the buddy's side; when the window
  // is edge-shifted the bubble unfolds away from the edge and the tail still
  // points back toward the buddy
  side: "left" | "right";
  valign: "up" | "down";
}>();
</script>

<template>
  <div
    data-testid="speech-bubble"
    class="bubble"
    :class="[`side-${side}`, `valign-${valign}`]"
    role="status"
    aria-live="polite"
  >
    {{ text }}
  </div>
</template>

<style scoped>
.bubble {
  position: relative;
  max-width: 168px;
  border-radius: 12px;
  background: #ffffff;
  color: #1f2333;
  padding: 8px 10px;
  font-size: 12px;
  line-height: 1.35;
  box-shadow: 0 4px 14px rgba(0, 0, 0, 0.22);
  /* the bubble sits beside the buddy; keep a small gap for the tail */
  margin: 0 8px;
}

/* Tail: a small diamond nudged to the edge nearest the buddy. side-right
   means the buddy is to the LEFT of the bubble, so the tail sits on the
   left face, and vice versa. */
.bubble::after {
  content: "";
  position: absolute;
  width: 10px;
  height: 10px;
  background: inherit;
  transform: rotate(45deg);
  top: 24px;
}
.bubble.valign-up::after {
  top: auto;
  bottom: 24px;
}
.bubble.side-right::after {
  left: -4px;
}
.bubble.side-left::after {
  right: -4px;
}
</style>
