<script setup lang="ts">
// A per-vault "folder inside the vault" text setting: a section heading, a
// labeled text input (v-model), and an optional field-level error line.
// Shared by CaptureSettings' Tasks and Documents folders — identical markup
// that would otherwise be duplicated (and push the parent template over the
// complexity threshold). The input emits `edit` on first keystroke so the
// parent's loaded-or-edited save gate works unchanged.
defineProps<{
  heading: string;
  label: string;
  placeholder: string;
  inputId: string;
  inputTestid: string;
  errorTestid: string;
  modelValue: string;
  error: string | null;
  // Disabled while a sibling save must not be raced (e.g. the Tasks tab locks
  // the folder input during an in-flight list save). Optional; defaults off.
  disabled?: boolean;
}>();

const emit = defineEmits<{
  "update:modelValue": [value: string];
  edit: [];
}>();

function onInput(event: Event) {
  emit("update:modelValue", (event.target as HTMLInputElement).value);
  emit("edit");
}
</script>

<template>
  <section>
    <h2 class="mb-1.5 text-xs font-semibold uppercase tracking-wide text-slate-400">
      {{ heading }}
    </h2>
    <!-- Bordered card so this folder setting reads as one of the Buddy-settings
         style section groups the rest of the vault-settings form now uses. -->
    <div class="rounded-xl border border-white/10 bg-white/5 p-2">
      <label
        :for="inputId"
        class="mb-1 block text-sm text-slate-200"
      >
        {{ label }}
        <span class="block text-xs text-slate-500">Inside the vault</span>
      </label>
      <input
        :id="inputId"
        :data-testid="inputTestid"
        :value="modelValue"
        :placeholder="placeholder"
        :aria-label="label"
        :disabled="disabled"
        type="text"
        class="w-full rounded-lg border border-white/10 bg-white/5 px-2 py-1 text-sm text-slate-100 placeholder:text-slate-500 focus:border-violet-400 focus:outline-none disabled:cursor-default disabled:opacity-50"
        @input="onInput"
      >
      <p
        v-if="error"
        :data-testid="errorTestid"
        class="mt-1 text-xs text-red-300"
      >
        {{ error }}
      </p>
    </div>
  </section>
</template>
