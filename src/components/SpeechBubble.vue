<script setup lang="ts">
const props = defineProps<{
  text: string;
  // The tail points back at the buddy: `side` puts it on the buddy-facing
  // face; `valign` sets its vertical position — `middle` (level with the
  // buddy) is the common case, `top`/`bottom` only when a screen edge pushes
  // the bubble above or below the buddy's center.
  side: "left" | "right";
  valign: "top" | "middle" | "bottom";
  // When set, this bubble carries a click action (e.g. an update
  // announcement): it renders an interactive affordance and emits `activate`.
  // A custom event name (not `click`) so a plain native click on the card
  // can never be mistaken for the action.
  clickable?: boolean;
}>();
const emit = defineEmits<{ (e: "activate"): void }>();

// Only an actionable bubble reacts — a plain greeting/ack stays inert.
function activate() {
  if (props.clickable) emit("activate");
}
</script>

<template>
  <div
    data-testid="speech-bubble"
    class="bubble"
    :class="[`side-${side}`, `valign-${valign}`, { clickable }]"
    role="status"
    aria-live="polite"
    :tabindex="clickable ? 0 : undefined"
    :title="clickable ? 'Open' : undefined"
    @click="activate"
    @keydown.enter.prevent="activate"
    @keydown.space.prevent="activate"
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
}
/* Vertical tail position, aimed at the buddy. */
.bubble.valign-top::after {
  top: 20px;
}
.bubble.valign-middle::after {
  top: 50%;
  margin-top: -5px; /* centre the 10px diamond on the card's vertical middle */
}
.bubble.valign-bottom::after {
  bottom: 20px;
}
.bubble.side-right::after {
  left: -4px;
}
.bubble.side-left::after {
  right: -4px;
}

/* An actionable bubble reads as interactive. The bubble auto-dismisses in a
   few seconds, so a hover-only hint could be missed — carry a PERSISTENT
   violet ring (the app accent, layered onto the existing shadow) at rest,
   with a pointer cursor and a hover lift on top. */
.bubble.clickable {
  cursor: pointer;
  box-shadow:
    0 4px 14px rgba(0, 0, 0, 0.22),
    0 0 0 1.5px rgba(139, 92, 246, 0.55);
  transition:
    transform 120ms ease,
    box-shadow 120ms ease;
}
.bubble.clickable:hover {
  transform: translateY(-1px);
  box-shadow:
    0 8px 20px rgba(0, 0, 0, 0.3),
    0 0 0 1.5px rgba(139, 92, 246, 0.9);
}
.bubble.clickable:focus-visible {
  outline: none;
  box-shadow:
    0 8px 20px rgba(0, 0, 0, 0.3),
    0 0 0 2px rgba(139, 92, 246, 1);
}
</style>
