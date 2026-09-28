<script setup lang="ts">
/**
 * The learning center's Shortcuts (Task 57; F-47): the table `answers.ts`
 * builds from `shortcuts.ts` — one row per action, every key bound to it —
 * then the two keys `shortcuts.ts` answers with a predicate instead of a
 * table entry. A single source: change a binding there, and this changes.
 * Visual-parity Task 23 (concept §9.3): ruled rows, the action on the left
 * and its keys on the right. The concept's "All editor shortcuts" button
 * is omitted (D14): this table already is every shortcut.
 */
import { OTHER_KEYS, SHORTCUT_TABLE } from "../../../editor/guide/answers";

const ROWS = [
  ...SHORTCUT_TABLE.map((r) => ({ id: r.actionId, action: r.actionId as string | undefined, label: r.label, keys: r.keys })),
  ...OTHER_KEYS.map((r) => ({ id: r.label, action: undefined, label: r.label, keys: r.keys })),
];
</script>

<template>
  <section>
    <div class="mb-[17px]">
      <h3 class="mb-[5px] text-[15px] font-semibold tracking-[-0.2px] text-fg">
        Keep your hands on the keyboard.
      </h3>
      <p class="text-xs text-fg-muted">
        The same controls are available without dragging or right-clicking. Shortcuts work while focus is in the
        editor, not in a text field.
      </p>
    </div>
    <div
      v-for="row in ROWS"
      :key="row.id"
      :data-testid="row.action ? 'learning-shortcut' : 'learning-shortcut-other'"
      :data-action="row.action"
      class="flex items-center justify-between gap-5 border-b border-line py-[11px] text-xs"
    >
      <span class="text-fg-secondary">{{ row.label }}</span>
      <span class="flex shrink-0 gap-1">
        <kbd
          v-for="key in row.keys"
          :key="key"
          class="vb-mono rounded-[4px] border border-line bg-raised px-[7px] py-1 text-[11px] whitespace-nowrap text-fg"
        >{{ key }}</kbd>
      </span>
    </div>
  </section>
</template>
