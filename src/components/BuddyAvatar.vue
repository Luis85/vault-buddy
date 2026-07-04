<script setup lang="ts">
import { computed, onUnmounted, ref, watch } from "vue";
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

// A constant idle loop reads as fidgeting no matter the speed. Instead the
// sprite stands still and, at random moments, either plays ONE quick idle
// cycle or glances the other way (a scaleX(-1) mirror — the cheapest
// possible "looking around"). The jitter keeps multiple avatars
// (companion + settings previews) from acting in sync.
const IDLE_MIN_DELAY_MS = 3000;
const IDLE_DELAY_JITTER_MS = 4000;
const IDLE_FLIP_CHANCE = 0.5;

const idlePlaying = ref(false);
const facing = ref<1 | -1>(1);
let idleTimer: ReturnType<typeof setTimeout> | undefined;

const idleEligible = computed(
  () => character.value.sprite !== null && !props.working && props.animated,
);

function scheduleIdleBurst() {
  clearTimeout(idleTimer);
  idleTimer = setTimeout(
    fireIdleBurst,
    IDLE_MIN_DELAY_MS + Math.random() * IDLE_DELAY_JITTER_MS,
  );
}

function fireIdleBurst() {
  if (Math.random() < IDLE_FLIP_CHANCE) {
    // instant turn — pixel-art characters snap around, no tween needed
    facing.value = facing.value === 1 ? -1 : 1;
    scheduleIdleBurst();
  } else {
    idlePlaying.value = true; // animationend re-arms
  }
}

function onSheetAnimationEnd() {
  // the one-shot idle cycle finished — back to standing still, re-arm
  idlePlaying.value = false;
  if (idleEligible.value) scheduleIdleBurst();
}

watch(
  idleEligible,
  (eligible) => {
    idlePlaying.value = false;
    clearTimeout(idleTimer);
    if (eligible) scheduleIdleBurst();
  },
  { immediate: true },
);

// animations off means "canonical pose" — face forward-right again so the
// frozen buddy isn't stuck looking off to the side
watch(
  () => props.animated,
  (on) => {
    if (!on) facing.value = 1;
  },
);

onUnmounted(() => clearTimeout(idleTimer));
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
      :class="{ running: working, playing: idlePlaying, flipped: facing === -1 }"
      :style="{ backgroundImage: `url(${sheet})` }"
      @animationend="onSheetAnimationEnd"
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
}
/* idle burst — one quick cycle, then animationend hands control back to
   the random scheduler in the script */
.sheet.playing {
  animation: frames 0.5s steps(4);
}
/* working — a continuous run loop (infinite animations never fire
   animationend, so the scheduler stays out of the way) */
.sheet.running {
  animation: frames 0.45s steps(4) infinite;
}
/* glancing the other way — mirrors idle bursts and the run loop alike */
.sheet.flipped {
  transform: scaleX(-1);
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
