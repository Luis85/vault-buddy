<script setup lang="ts">
import { computed } from "vue";

import { useVaultsStore } from "../stores/vaults";
import type { Vault } from "../types";
import AppIcon from "./AppIcon.vue";
import Avatar from "./ui/Avatar.vue";
import CountBadge from "./ui/CountBadge.vue";
import IconButton from "./ui/IconButton.vue";
import SectionHeader from "./ui/SectionHeader.vue";
import StatusDot from "./ui/StatusDot.vue";

const props = defineProps<{
  vaults: Vault[];
  busyVaultId: string | null;
  busyCommand: "open_vault" | "open_daily_note" | null;
  captureDisabled: boolean;
  recordingVaultId: string | null;
  transcribingVaultId: string | null;
  // Open-task count per vault id; a missing entry (or 0) hides the badge.
  taskCounts: Record<string, number>;
}>();
defineEmits<{
  (e: "open-vault", id: string): void;
  (e: "open-daily-note", id: string): void;
  (e: "capture", id: string): void;
  (e: "capture-settings", id: string): void;
  (e: "open-tasks", id: string): void;
}>();

// Favorites (Task 5) are pure frontend state (localStorage-backed, panel-list
// ordering only), so the row star reads/writes the store directly rather than
// round-tripping through a prop — the same pattern RecordMode/Tasks/
// TaskListSettings already use to read store state alongside ActionPanel's
// props.
const store = useVaultsStore();
const isFav = (id: string) => store.favorites.has(id);

// Pulled out of the template (rather than four inline ternaries on
// isFav(vault.id)) to keep the row markup's branching down — the template's
// own cognitive-complexity score counts each inline ternary, and the row
// already carries several conditional badges (open/recording/transcribing).
const favoriteTitle = (id: string) => (isFav(id) ? "Unfavorite" : "Favorite");
const favoriteGlyph = (id: string) => (isFav(id) ? "★" : "☆");
const favoriteButtonClass = (id: string) =>
  isFav(id) ? "text-amber-300" : "text-slate-400 hover:text-amber-300";

// Obsidian allows two registered vaults whose folders share a name; without a
// disambiguator the rows would be identical while opening different vaults.
const duplicatedNames = computed(() => {
  const seen = new Set<string>();
  const dupes = new Set<string>();
  for (const vault of props.vaults) {
    const key = vault.name.toLowerCase();
    if (seen.has(key)) dupes.add(key);
    seen.add(key);
  }
  return dupes;
});

const isAmbiguous = (vault: Vault) =>
  duplicatedNames.value.has(vault.name.toLowerCase());

const isBusy = (vault: Vault, command: "open_vault" | "open_daily_note") =>
  props.busyVaultId === vault.id && props.busyCommand === command;

// Duplicate-name vaults must also differ in their accessible names, not
// just visually — screen-reader users would otherwise hear two identical
// controls that target different vaults.
const accessibleName = (vault: Vault) =>
  isAmbiguous(vault) ? `${vault.name} (${vault.path})` : vault.name;

const favoriteAriaLabel = (vault: Vault) =>
  `${favoriteTitle(vault.id)} ${accessibleName(vault)}`;

// Favorites are pinned above Open now / Other vaults. A favorite appears once
// (in Favorites), regardless of open state — its row keeps the ordinary
// per-row "open" dot below, so the open signal isn't lost for a
// favorited-and-open vault; that dot is keyed on `vault.open` alone, not on
// which group renders the row, so it carries over with no extra markup.
// Alphabetical order (from discovery) is preserved within each group; with
// nothing favorited/open the remainder stays a flat, header-less list, same
// as before Task 5.
const groups = computed(() => {
  const favs = props.vaults.filter((v) => isFav(v.id));
  const rest = props.vaults.filter((v) => !isFav(v.id));
  const open = rest.filter((v) => v.open);
  const other = rest.filter((v) => !v.open);
  const out: {
    key: string;
    section: string;
    label: string | null;
    vaults: Vault[];
  }[] = [];
  if (favs.length) {
    out.push({ key: "fav", section: "favorites", label: "Favorites", vaults: favs });
  }
  if (open.length) {
    out.push({ key: "open", section: "open", label: "Open now", vaults: open });
  }
  if (other.length) {
    // With favorites or open present, the remainder gets an "Other vaults"
    // header; with nothing pinned/open it stays a flat, header-less list.
    const flat = !favs.length && !open.length;
    out.push({
      key: "rest",
      section: "other",
      label: flat ? null : "Other vaults",
      vaults: other,
    });
  }
  return out;
});
</script>

<template>
  <div
    v-for="group in groups"
    :key="group.key"
    :data-section="group.section"
    class="mt-2 first:mt-0"
  >
    <SectionHeader v-if="group.label">
      {{ group.label }}
    </SectionHeader>
    <ul class="space-y-1">
      <li
        v-for="vault in group.vaults"
        :key="vault.id"
        :title="vault.path"
      >
        <div
          class="flex items-center gap-1 rounded-lg transition-colors hover:bg-white/10"
        >
          <button
            type="button"
            :data-testid="`vault-favorite-${vault.id}`"
            :aria-pressed="isFav(vault.id)"
            :aria-label="favoriteAriaLabel(vault)"
            :title="favoriteTitle(vault.id)"
            :disabled="busyVaultId !== null"
            class="mr-1 shrink-0 cursor-pointer rounded-control p-1.5 transition-colors hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400 disabled:cursor-default disabled:opacity-50"
            :class="favoriteButtonClass(vault.id)"
            @click.stop="store.toggleFavorite(vault.id)"
          >
            <span aria-hidden="true">{{ favoriteGlyph(vault.id) }}</span>
          </button>
          <button
            type="button"
            class="flex min-w-0 flex-1 cursor-pointer items-center gap-2 rounded-lg px-2 py-1.5 text-left focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400 disabled:cursor-default disabled:opacity-50"
            :disabled="busyVaultId !== null"
            :aria-label="`Open vault ${accessibleName(vault)}`"
            @click="$emit('open-vault', vault.id)"
          >
            <Avatar
              :name="vault.name"
              size="md"
            />
            <span class="min-w-0 flex-1">
              <span class="flex items-center gap-1.5">
                <span class="truncate text-sm font-medium text-slate-100">
                  {{ vault.name }}
                </span>
                <StatusDot
                  v-if="vault.open"
                  tone="success"
                  title="Open in Obsidian"
                />
                <StatusDot
                  v-if="vault.id === recordingVaultId"
                  tone="recording"
                  pulse
                  title="Recording…"
                />
                <StatusDot
                  v-if="vault.id === transcribingVaultId"
                  tone="transcribing"
                  pulse
                  title="Transcribing…"
                />
              </span>
              <span
                v-if="isAmbiguous(vault)"
                class="block truncate text-xs text-slate-400"
              >
                {{ vault.path }}
              </span>
            </span>
            <span
              v-if="isBusy(vault, 'open_vault')"
              class="h-4 w-4 shrink-0 animate-spin rounded-full border-2 border-white/30 border-t-white"
              role="status"
              aria-label="Opening vault…"
            />
          </button>
          <IconButton
            :label="`Open today's daily note in ${accessibleName(vault)}`"
            title="Open today's daily note"
            :disabled="busyVaultId !== null"
            @click="$emit('open-daily-note', vault.id)"
          >
            <span
              v-if="isBusy(vault, 'open_daily_note')"
              class="block h-4 w-4 animate-spin rounded-full border-2 border-white/30 border-t-white"
              role="status"
              aria-label="Opening daily note…"
            />
            <AppIcon v-else>
              <rect
                x="3"
                y="5"
                width="18"
                height="16"
                rx="2"
              />
              <path d="M8 3v4M16 3v4M3 11h18" />
            </AppIcon>
          </IconButton>
          <IconButton
            data-testid="open-tasks"
            class="relative"
            :disabled="busyVaultId !== null"
            :label="`Tasks in ${accessibleName(vault)}${(taskCounts?.[vault.id] ?? 0) > 0 ? ` (${taskCounts[vault.id]} open)` : ''}`"
            title="Tasks"
            @click="$emit('open-tasks', vault.id)"
          >
            <CountBadge
              :count="taskCounts?.[vault.id] ?? 0"
              data-testid="task-count"
              class="absolute -right-0.5 -top-0.5"
            />
            <AppIcon>
              <path d="M9 11l3 3 8-8" />
              <path d="M20 12v6a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2h9" />
            </AppIcon>
          </IconButton>
          <IconButton
            :label="`Capture knowledge in ${accessibleName(vault)}`"
            title="Capture knowledge"
            :disabled="busyVaultId !== null || captureDisabled"
            @click="$emit('capture', vault.id)"
          >
            <AppIcon>
              <rect
                x="3"
                y="3"
                width="18"
                height="18"
                rx="4"
              />
              <path d="M12 8v8M8 12h8" />
            </AppIcon>
          </IconButton>
          <IconButton
            :label="`Capture settings for ${accessibleName(vault)}`"
            title="Capture settings"
            :disabled="busyVaultId !== null"
            @click="$emit('capture-settings', vault.id)"
          >
            <AppIcon>
              <circle
                cx="12"
                cy="12"
                r="3"
              />
              <path
                d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09a1.65 1.65 0 0 0-1-1.51 1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09a1.65 1.65 0 0 0 1.51-1 1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33h.09a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51h.09a1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82v.09a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"
              />
            </AppIcon>
          </IconButton>
        </div>
      </li>
    </ul>
  </div>
</template>
