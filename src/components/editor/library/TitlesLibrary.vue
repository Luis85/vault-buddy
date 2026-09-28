<script setup lang="ts">
/**
 * The titles/chapter-card library (Task 33; F-37; visual-parity Task 10,
 * concept spec §3.3): "GIVE IT STRUCTURE", the **Insert intro at the
 * beginning** button (`insertIntro`: every clip moves later together and
 * the card lands at 0 on a new top track — one command, one undo step),
 * four 16:9 template cards (Intro / Chapter / Closing / Plain background),
 * and **Import a still image** — the media import, which accepts PNG, JPEG
 * and WebP (`useMediaImport`; the Media tab shows its progress, so the
 * library switches there).
 *
 * **Track choice is `placeOnFreeTrack`** (visual-parity Task 7, audit
 * finding 1a) — the rule the media library's "+" shares: the first
 * unlocked video track, top-down, free for the card's whole span, else a
 * new video track above the topmost one, then the card on it. A new track
 * at the TOP is `addCard`'s own `trackId: null` (a new index-0 video track,
 * the timeline's "Add title here" precedent): ONE command, so one undo step
 * and no half-done state if it is refused. Only a new track below an audio
 * track at the top takes the two-step `addTrack` + `addCard`.
 *
 * **Card text.** The Title/Subtitle fields below the cards are what the
 * next insert writes (this editor has no other place to type a card's
 * text); an empty title falls back to the preset's own label ("Intro",
 * "Chapter", …) rather than sending an empty string.
 *
 * Every insert is disabled, and says why, while no project is open.
 */
import { computed, ref } from "vue";

import { useMediaImport } from "../../../composables/useMediaImport";
import type { EditorCommand } from "../../../editor/editorCommandTypes";
import { insertOnFreeTrack, placeOnFreeTrack } from "../../../editor/placeOnFreeTrack";
import type { CardPreset } from "../../../editorTypes";
import { useEditorProjectStore } from "../../../stores/editorProject";
import { useEditorWorkspaceStore } from "../../../stores/editorWorkspace";
import EditorIcon from "../icons/EditorIcon.vue";
import LibraryHeading from "./LibraryHeading.vue";

const project = useEditorProjectStore();
const workspace = useEditorWorkspaceStore();
const mediaImport = useMediaImport();

const title = ref("");
const subtitle = ref("");

/** A sensible starting length for a generated card -- editable afterward
 * via a trim, never enforced as a fixed duration here. */
const DEFAULT_CARD_DURATION_MS = 3_000;

interface Template {
  id: CardPreset;
  /** What the card's own name falls back to on the timeline. */
  fallback: string;
  /** The canvas headline and the label under it (§3.3). */
  headline: string;
  label: string;
  canvas: string;
  headlineClass: string;
  eyebrowClass: string;
}

const TEMPLATES: Template[] = [
  {
    id: "intro",
    fallback: "Intro",
    headline: "A clear beginning",
    label: "Intro card",
    canvas: "bg-card-intro",
    headlineClass: "text-[15px] text-card-ink",
    eyebrowClass: "text-card-eyebrow",
  },
  {
    id: "chapter",
    fallback: "Chapter",
    headline: "One step at a time",
    label: "Chapter card",
    canvas: "bg-card-chapter",
    headlineClass: "text-[15px] text-card-ink",
    eyebrowClass: "text-card-eyebrow",
  },
  {
    id: "outro",
    fallback: "Outro",
    headline: "What happens next?",
    label: "Closing card",
    canvas: "bg-card-outro",
    headlineClass: "text-[15px] text-card-ink",
    eyebrowClass: "text-card-outro-eyebrow",
  },
  {
    id: "blank",
    fallback: "Blank",
    headline: "Room for your idea",
    label: "Plain background",
    canvas: "bg-card-blank",
    headlineClass: "text-[13px] text-card-blank-ink",
    eyebrowClass: "text-card-eyebrow",
  },
];

const NO_PROJECT = "Open a project first.";
const insertReason = computed(() => (project.project ? null : NO_PROJECT));

function cardText(fallback: string): { title: string; subtitle: string } {
  return { title: title.value.trim() || fallback, subtitle: subtitle.value.trim() };
}

function insertCard(preset: CardPreset, fallback: string): void {
  const p = project.project;
  if (!p) return;
  const startMs = workspace.playheadMs;
  const span = { atMs: startMs, lengthMs: DEFAULT_CARD_DURATION_MS };
  const card = (trackId: string | null): EditorCommand => ({
    kind: "addCard",
    preset,
    trackId,
    startMs,
    durationMs: DEFAULT_CARD_DURATION_MS,
    ...cardText(fallback),
  });
  const placement = placeOnFreeTrack(p, "video", span.atMs, span.lengthMs);
  if ("newTrackIndex" in placement && placement.newTrackIndex === 0) {
    void project.execute(card(null));
    return;
  }
  void insertOnFreeTrack((command) => project.execute(command), () => project.project, "video", span, card);
}

function insertIntro(): void {
  if (!project.project) return;
  void project.execute({ kind: "insertIntro", durationMs: DEFAULT_CARD_DURATION_MS, ...cardText("Intro") });
}

function importImage(): void {
  if (mediaImport.start()) workspace.setLibraryTab("media");
}
</script>

<template>
  <div
    data-testid="titles-library"
    class="h-full overflow-y-auto text-fg"
  >
    <LibraryHeading
      label="GIVE IT STRUCTURE"
      pill="Local"
      testid="titles"
    />
    <p class="text-[11px] text-fg-muted">
      A clear beginning, useful chapters, and a next step.
    </p>

    <button
      type="button"
      data-testid="titles-insert-intro"
      :disabled="insertReason !== null"
      :title="insertReason ?? 'Insert a three-second intro before everything, moving every track later together'"
      class="mt-4 flex w-full items-start gap-2.5 rounded-[9px] border border-accent bg-accent-bg p-3 text-left hover:bg-accent-bg focus:outline-none focus-visible:ring-2 focus-visible:ring-focus disabled:cursor-not-allowed disabled:opacity-40"
      @click="insertIntro"
    >
      <EditorIcon
        name="layers"
        :size="19"
        class="shrink-0 text-accent"
      />
      <span class="min-w-0">
        <b class="block text-[11px] font-semibold leading-normal">Insert intro at the beginning</b>
        <small class="block text-[9px] leading-relaxed text-fg-secondary">Move every existing track together</small>
      </span>
    </button>

    <ul
      class="my-4 grid gap-3"
      aria-label="Insert a title card"
    >
      <li
        v-for="t in TEMPLATES"
        :key="t.id"
      >
        <button
          type="button"
          :data-testid="`titles-add-${t.id}`"
          :disabled="insertReason !== null"
          :title="insertReason ?? `Add the ${t.label.toLowerCase()} at the playhead`"
          class="block w-full overflow-hidden rounded-[9px] border border-line bg-raised p-0 text-left hover:border-accent focus:outline-none focus-visible:ring-2 focus-visible:ring-focus disabled:cursor-not-allowed disabled:opacity-40"
          @click="insertCard(t.id, t.fallback)"
        >
          <span
            :data-testid="`titles-canvas-${t.id}`"
            class="flex aspect-video w-full flex-col justify-center gap-2.5 p-[18px]"
            :class="t.canvas"
          >
            <b
              class="w-[85%] font-semibold leading-tight"
              :class="t.headlineClass"
            >{{ t.headline }}</b>
            <small
              class="text-[6px] tracking-[1.3px]"
              :class="t.eyebrowClass"
            >VAULT BUDDY · YOUR TUTORIAL</small>
          </span>
          <span class="flex items-center justify-between px-[11px] py-[9px] text-[11px] text-fg">
            <span :data-testid="`titles-label-${t.id}`">{{ t.label }}</span>
            <EditorIcon
              name="plus"
              :size="14"
            />
          </span>
        </button>
      </li>
    </ul>

    <p class="text-[11px] leading-relaxed text-fg-muted">
      Cards add a three-second clip at the playhead on a free video track.
      The intro button inserts three seconds before all existing content.
      Both can be undone.
    </p>

    <div class="mt-3 flex flex-col gap-2">
      <label class="flex flex-col gap-1 text-[10px] text-fg-secondary">
        Card title
        <input
          v-model="title"
          data-testid="titles-title"
          type="text"
          aria-label="Title card text"
          placeholder="The card's own name"
          class="h-8 min-h-0 px-2 py-1 text-[11px] text-fg"
        >
      </label>
      <label class="flex flex-col gap-1 text-[10px] text-fg-secondary">
        Card subtitle
        <input
          v-model="subtitle"
          data-testid="titles-subtitle"
          type="text"
          aria-label="Title card subtitle"
          class="h-8 min-h-0 px-2 py-1 text-[11px] text-fg"
        >
      </label>
    </div>

    <button
      type="button"
      data-testid="titles-import-image"
      :disabled="mediaImport.refusal.value !== null"
      :title="mediaImport.refusal.value ?? 'Import a PNG, JPEG or WebP image'"
      class="mt-4 inline-flex items-center gap-[7px] rounded-[7px] border border-line bg-panel px-2.5 py-[7px] text-[11px] text-fg hover:bg-hover focus:outline-none focus-visible:ring-2 focus-visible:ring-focus disabled:cursor-not-allowed disabled:opacity-40"
      @click="importImage"
    >
      <EditorIcon
        name="image"
        :size="16"
      />
      Import a still image
    </button>
    <p class="mt-2 text-[11px] leading-relaxed text-fg-muted">
      PNG, JPEG or WebP. A still image starts as a five-second clip.
    </p>
  </div>
</template>
