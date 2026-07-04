<script setup lang="ts">
import { CHARACTERS } from "../characters";
import { useSettingsStore } from "../stores/settings";
import BuddyAvatar from "./BuddyAvatar.vue";
import UpdateSettings from "./UpdateSettings.vue";
import DiagnosticsSettings from "./DiagnosticsSettings.vue";

const settings = useSettingsStore();
</script>

<template>
  <div class="flex flex-col gap-3">
    <section>
      <h2
        class="mb-1.5 text-xs font-semibold uppercase tracking-wide text-slate-400"
      >
        Buddy character
      </h2>
      <div
        class="grid grid-cols-3 gap-2"
        role="radiogroup"
        aria-label="Buddy character"
      >
        <button
          v-for="c in CHARACTERS"
          :key="c.id"
          type="button"
          role="radio"
          class="character-option flex cursor-pointer flex-col items-center rounded-xl border p-1.5 transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400"
          :class="
            settings.character === c.id
              ? 'border-violet-400 bg-violet-500/20'
              : 'border-white/10 bg-white/5 hover:bg-white/10'
          "
          :aria-checked="settings.character === c.id"
          :aria-label="`Choose ${c.name}`"
          @click="settings.setCharacter(c.id)"
        >
          <BuddyAvatar
            :character-id="c.id"
            :animated="settings.animationsEnabled"
            :facing="settings.facing"
          />
          <span class="text-xs text-slate-200">{{ c.name }}</span>
        </button>
      </div>
    </section>
    <section class="flex items-center justify-between">
      <span class="text-sm text-slate-200">View direction</span>
      <div class="flex gap-1" role="radiogroup" aria-label="View direction">
        <button
          v-for="dir in ['left', 'right'] as const"
          :key="dir"
          type="button"
          role="radio"
          class="facing-option cursor-pointer rounded-lg border px-2 py-0.5 text-xs capitalize transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400"
          :class="
            settings.facing === dir
              ? 'border-violet-400 bg-violet-500/20 text-slate-100'
              : 'border-white/10 bg-white/5 text-slate-300 hover:bg-white/10'
          "
          :aria-checked="settings.facing === dir"
          @click="settings.setFacing(dir)"
        >
          {{ dir }}
        </button>
      </div>
    </section>
    <section class="flex items-center justify-between">
      <label for="animations-toggle" class="text-sm text-slate-200">
        Animations
      </label>
      <input
        id="animations-toggle"
        type="checkbox"
        class="h-4 w-4 accent-violet-500"
        :checked="settings.animationsEnabled"
        @change="settings.toggleAnimations()"
      />
    </section>
    <section class="flex items-center justify-between">
      <label for="dragging-toggle" class="text-sm text-slate-200">
        Dragging
        <span class="block text-xs text-slate-500">
          Off pins the buddy in place
        </span>
      </label>
      <input
        id="dragging-toggle"
        type="checkbox"
        class="h-4 w-4 accent-violet-500"
        :checked="settings.draggingEnabled"
        @change="settings.toggleDragging()"
      />
    </section>
    <UpdateSettings />
    <DiagnosticsSettings />
  </div>
</template>
