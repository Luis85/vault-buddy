<script setup lang="ts">
import { computed } from "vue";
import { getCharacter } from "../characters";

const props = withDefaults(
  defineProps<{
    characterId?: string;
    working?: boolean;
    animated?: boolean;
  }>(),
  { characterId: "classic", working: false, animated: true },
);

const character = computed(() => getCharacter(props.characterId));
const sheet = computed(() => {
  const sprite = character.value.sprite;
  if (!sprite) return null;
  return props.working ? sprite.run : sprite.idle;
});
</script>

<template>
  <!-- classic: the original hand-drawn SVG blob, kept for nostalgia -->
  <svg
    v-if="!sheet"
    class="avatar classic"
    :class="{ working, still: !animated }"
    width="64"
    height="64"
    viewBox="0 0 96 96"
    aria-hidden="true"
  >
    <ellipse cx="48" cy="52" rx="34" ry="32" fill="#7c5cff" />
    <circle class="eye" cx="38" cy="46" r="5" fill="#fff" />
    <circle class="eye" cx="58" cy="46" r="5" fill="#fff" />
    <path
      d="M40 62 Q48 70 56 62"
      stroke="#fff"
      stroke-width="3"
      fill="none"
      stroke-linecap="round"
    />
  </svg>
  <!--
    sprite: a 4-frame 16×28 strip played via steps(). The outer div keeps
    the classic buddy's 64×64 footprint so the window/layout geometry is
    identical for every character; the inner div is the film strip.
  -->
  <div
    v-else
    class="avatar sprite"
    :class="{ working, still: !animated }"
    aria-hidden="true"
  >
    <div
      class="sheet"
      :class="{ running: working }"
      :style="{ backgroundImage: `url(${sheet})` }"
    />
  </div>
</template>

<style scoped>
/* ---- classic (SVG) ---- */
/* idle */
.classic {
  animation: bob 3s ease-in-out infinite;
}
/* greeting */
.classic:hover:not(.working) {
  animation: wiggle 0.6s ease-in-out infinite;
}
/* working */
.classic.working {
  animation: pulse 0.9s ease-in-out infinite;
}
.classic .eye {
  animation: blink 4s infinite;
  transform-origin: center;
  transform-box: fill-box;
}

/* ---- sprite (pixel art) ---- */
.sprite {
  display: flex;
  width: 64px;
  height: 64px;
  align-items: flex-end;
  justify-content: center;
}
/* greeting — the strip keeps playing while the whole character sways */
.sprite:hover:not(.working) {
  animation: wiggle 0.6s ease-in-out infinite;
}
.sheet {
  width: 32px;
  height: 56px;
  /* 64×28 strip at 2× — 4 frames of 32×56 */
  background-size: 128px 56px;
  image-rendering: pixelated;
  /* idle is a calm breathing loop (~3 fps) — anything faster reads as
     jittery for a character that just stands around on the desktop */
  animation: frames 1.3s steps(4) infinite;
}
/* working — the run cycle, a touch faster than idle */
.sheet.running {
  animation-duration: 0.45s;
}
@keyframes frames {
  from {
    background-position-x: 0;
  }
  to {
    background-position-x: -128px;
  }
}

/* user turned the animation off (right-click menu or settings) — freezes
   idle, hover and working states alike, sprites on their first frame */
.avatar.still,
.avatar.still .eye,
.avatar.still .sheet {
  animation: none !important;
}

@keyframes bob {
  0%,
  100% {
    transform: translateY(0);
  }
  50% {
    transform: translateY(-4px);
  }
}
@keyframes wiggle {
  0%,
  100% {
    transform: rotate(-4deg);
  }
  50% {
    transform: rotate(4deg);
  }
}
@keyframes pulse {
  0%,
  100% {
    transform: scale(1);
  }
  50% {
    transform: scale(0.94);
  }
}
@keyframes blink {
  0%,
  92%,
  100% {
    transform: scaleY(1);
  }
  96% {
    transform: scaleY(0.1);
  }
}
</style>
