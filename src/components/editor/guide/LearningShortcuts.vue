<script setup lang="ts">
/**
 * The learning center's Shortcuts (Task 57; F-47): the table `answers.ts`
 * builds from `shortcuts.ts` — one row per action, every key bound to it —
 * then the two keys `shortcuts.ts` answers with a predicate instead of a
 * table entry. A single source: change a binding there, and this changes.
 */
import { OTHER_KEYS, SHORTCUT_TABLE } from "../../../editor/guide/answers";

const ROWS = [
  ...SHORTCUT_TABLE.map((r) => ({ id: r.actionId, action: r.actionId as string | undefined, label: r.label, keys: r.keys })),
  ...OTHER_KEYS.map((r) => ({ id: r.label, action: undefined, label: r.label, keys: r.keys })),
];
</script>

<template>
  <section class="flex max-h-[50vh] flex-col gap-2 overflow-y-auto pr-1 text-xs">
    <p class="text-fg-muted">
      Shortcuts work while focus is in the editor and not in a text field.
    </p>
    <table class="w-full border-collapse">
      <tbody>
        <tr
          v-for="row in ROWS"
          :key="row.id"
          :data-testid="row.action ? 'learning-shortcut' : 'learning-shortcut-other'"
          :data-action="row.action"
          class="border-b border-line"
        >
          <td class="py-1 text-fg-secondary">
            {{ row.label }}
          </td>
          <td class="py-1 text-right">
            <kbd
              v-for="key in row.keys"
              :key="key"
              class="ml-1 rounded border border-line bg-raised px-1 font-mono text-fg"
            >{{ key }}</kbd>
          </td>
        </tr>
      </tbody>
    </table>
  </section>
</template>
