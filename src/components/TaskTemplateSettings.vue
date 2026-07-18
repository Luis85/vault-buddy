<script setup lang="ts">
// Presentational "Task template" card for the Tasks settings tab: two
// free-text templates (extra frontmatter + body) that compose onto every NEW
// task document add_task creates. No store/invoke/autosave of its own — state,
// autosave scheduling, and the on-mount load all stay in TasksConfigTab.vue;
// this component only renders the card and emits the raw user input back up
// (the TaskIdSettings.vue precedent).
defineProps<{
  extraFrontmatter: string;
  bodyTemplate: string;
}>();

const emit = defineEmits<{
  "update:extraFrontmatter": [value: string];
  "update:bodyTemplate": [value: string];
  blur: [];
}>();

// Shown under both textareas below — render_task substitutes the SAME vars
// set (title/date/due/priority) into both the extra frontmatter and the body
// template, so one shared hint is accurate for either field (unlike the
// document-import template, whose frontmatter/body draw from disjoint sets).
// The literal `{{...}}` placeholder syntax must live in a script string,
// never typed directly into template text: Vue's mustache tokenizer finds the
// FIRST `}}` textually (no brace-depth awareness), so writing it inline in
// the template would terminate the interpolation early and corrupt the
// markup (RecordingSettings.vue / DocumentsConfigTab.vue precedent).
const TEMPLATE_PLACEHOLDER_HINT =
  "Placeholders: {{title}}, {{date}}, {{due}}, {{priority}}. Identity fields (type, status, title, created) are always added.";
</script>

<template>
  <section>
    <h2 class="mb-1.5 text-xs font-semibold uppercase tracking-wide text-slate-400">
      Task template
    </h2>
    <div class="flex flex-col gap-3 rounded-xl border border-white/10 bg-white/5 p-2">
      <div class="flex flex-col gap-1">
        <label
          for="task-extra-frontmatter"
          class="text-sm text-slate-200"
        >
          Extra frontmatter
        </label>
        <textarea
          id="task-extra-frontmatter"
          data-testid="task-extra-frontmatter"
          :value="extraFrontmatter"
          rows="3"
          placeholder="project: Alpha"
          class="w-full resize-y rounded-lg border border-white/10 bg-white/5 px-2 py-1 font-mono text-xs text-slate-100 placeholder:text-slate-500 focus:border-violet-400 focus:outline-none"
          @input="emit('update:extraFrontmatter', ($event.target as HTMLTextAreaElement).value)"
          @blur="emit('blur')"
        />
        <p class="text-xs text-slate-500">
          {{ TEMPLATE_PLACEHOLDER_HINT }}
        </p>
      </div>
      <div class="flex flex-col gap-1">
        <label
          for="task-body-template"
          class="text-sm text-slate-200"
        >
          Body template
        </label>
        <textarea
          id="task-body-template"
          data-testid="task-body-template"
          :value="bodyTemplate"
          rows="3"
          placeholder="- [ ] Follow up"
          class="w-full resize-y rounded-lg border border-white/10 bg-white/5 px-2 py-1 font-mono text-xs text-slate-100 placeholder:text-slate-500 focus:border-violet-400 focus:outline-none"
          @input="emit('update:bodyTemplate', ($event.target as HTMLTextAreaElement).value)"
          @blur="emit('blur')"
        />
        <p class="text-xs text-slate-500">
          {{ TEMPLATE_PLACEHOLDER_HINT }}
        </p>
      </div>
    </div>
  </section>
</template>
