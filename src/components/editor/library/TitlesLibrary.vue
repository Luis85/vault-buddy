<script setup lang="ts">
/**
 * The titles/chapter-card library (Task 33; F-37; DATA-MODEL.md § Entities):
 * a title/subtitle pair plus four one-click preset inserts (Intro/Chapter/
 * Outro/Blank) -- `addCard` through `editorProject.execute`, the
 * `MediaLibrary.vue` "+" precedent (`execute()` itself no-ops with no
 * session open, so this component needs no separate guard for that case;
 * Rust stays the authority for everything else -- an overlap or a locked
 * track surfaces as the store's error).
 *
 * **Track choice mirrors `MediaLibrary`'s "+"**: the first unlocked VIDEO
 * track (`trackCompat.firstAcceptingTrack`, the ONE copy of that rule) if
 * one exists, else `trackId: null` -- `addCard`'s own contract for "create
 * a new top video track" (`cards.rs`'s module doc), so a fresh project's
 * very first card still lands somewhere.
 *
 * **An empty title falls back to the preset's own label** ("Intro",
 * "Chapter", …) rather than sending an empty string -- a blank card is
 * still meaningfully named on the timeline until the user edits it via
 * `updateCard` (a later inspector surface, not this component's job).
 */
import { ref } from "vue";

import { firstAcceptingTrack } from "../../../editor/trackCompat";
import type { CardPreset } from "../../../editorTypes";
import { useEditorProjectStore } from "../../../stores/editorProject";
import { useEditorWorkspaceStore } from "../../../stores/editorWorkspace";

const project = useEditorProjectStore();
const workspace = useEditorWorkspaceStore();

const title = ref("");
const subtitle = ref("");

/** A sensible starting length for a generated card -- editable afterward
 * via `updateCard`/a trim, never enforced as a fixed duration here. */
const DEFAULT_CARD_DURATION_MS = 3_000;

const PRESETS: { id: CardPreset; label: string }[] = [
  { id: "intro", label: "Intro" },
  { id: "chapter", label: "Chapter" },
  { id: "outro", label: "Outro" },
  { id: "blank", label: "Blank" },
];

function insertCard(preset: CardPreset, label: string): void {
  const target = firstAcceptingTrack(project.project, "video");
  void project.execute({
    kind: "addCard",
    preset,
    trackId: target ? target.id : null,
    startMs: workspace.playheadMs,
    durationMs: DEFAULT_CARD_DURATION_MS,
    title: title.value.trim() || label,
    subtitle: subtitle.value.trim(),
  });
}
</script>

<template>
  <div
    data-testid="titles-library"
    class="flex h-full flex-col gap-2 text-micro text-fg-secondary"
  >
    <label class="flex flex-col gap-0.5">
      Title
      <input
        v-model="title"
        data-testid="titles-title"
        type="text"
        aria-label="Title card text"
        class="rounded border border-line bg-stage px-1 py-0.5 text-fg focus:outline-none focus-visible:ring-1 focus-visible:ring-focus"
      >
    </label>
    <label class="flex flex-col gap-0.5">
      Subtitle
      <input
        v-model="subtitle"
        data-testid="titles-subtitle"
        type="text"
        aria-label="Title card subtitle"
        class="rounded border border-line bg-stage px-1 py-0.5 text-fg focus:outline-none focus-visible:ring-1 focus-visible:ring-focus"
      >
    </label>
    <ul
      class="flex flex-col gap-1"
      aria-label="Insert a title card"
    >
      <li
        v-for="preset in PRESETS"
        :key="preset.id"
      >
        <button
          type="button"
          :data-testid="`titles-add-${preset.id}`"
          class="w-full rounded border border-line px-2 py-0.5 text-left text-fg hover:bg-white/10 focus:outline-none focus-visible:ring-1 focus-visible:ring-focus"
          @click="insertCard(preset.id, preset.label)"
        >
          {{ preset.label }}
        </button>
      </li>
    </ul>
  </div>
</template>
