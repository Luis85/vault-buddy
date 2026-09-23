<script setup lang="ts">
/**
 * Every tutorial project on disk (Task 37 Part B, F4), shown beside the
 * resume-or-discard staged-capture list on the Record Screen picker's
 * `StagedCaptureList.vue` — a SEPARATE component rather than a second
 * section inside that file, because adding one there pushed its single
 * `<template>` function's cognitive complexity past this repo's quality
 * ratchet (fallow's `health` complexity threshold); splitting keeps both
 * templates simple, the `ScreenRegionPicker`/`ScreenAudioPicker` precedent
 * for extracting a block out of `ScreenSourcePicker`.
 *
 * PRESENTATIONAL — no `invoke`, no store, no event listener.
 * `ScreenSourcePicker` owns `list_tutorial_projects` / `open_project_editor`
 * behind it, the same way it owns the staged list's three commands.
 *
 * **No Discard button here (F4)**: discarding a project is a session-scoped,
 * revision-aware operation (`editor_close_session(discardProject)`) that
 * only the editor's own close/recovery UI (Task 37 Part A) performs — a
 * button here would have nothing safe to call.
 */
import type { ProjectSummaryDto } from "../editorTypes";
import { relativeAgeLabel } from "../utils/relativeAge";
import AppButton from "./ui/AppButton.vue";
import Chip from "./ui/Chip.vue";
import SectionHeader from "./ui/SectionHeader.vue";

defineProps<{ projects: ProjectSummaryDto[] }>();
const emit = defineEmits<{ resumeProject: [projectFileId: string] }>();
</script>

<template>
  <section
    v-if="projects.length > 0"
    data-testid="tutorial-projects-list"
    class="flex flex-col gap-1"
  >
    <SectionHeader>Tutorial projects</SectionHeader>
    <ul class="flex flex-col gap-1">
      <li
        v-for="p in projects"
        :key="p.projectFileId"
        :data-testid="`project-row-${p.projectFileId}`"
        class="rounded-control border border-white/10 bg-white/5 px-3 py-2"
      >
        <div class="flex items-center gap-2">
          <span class="min-w-0 flex-1 truncate text-sm font-medium text-fg">
            {{ p.title }}
          </span>
          <Chip
            v-if="p.hasRecovery"
            variant="accent"
          >
            Has unsaved changes
          </Chip>
        </div>
        <div class="mt-0.5 flex items-center gap-2">
          <span class="min-w-0 flex-1 truncate text-micro text-fg-subtle">
            {{ relativeAgeLabel(p.updatedAt, Date.now()) }}
          </span>
          <AppButton
            :data-testid="`project-resume-${p.projectFileId}`"
            size="sm"
            variant="secondary"
            @click="emit('resumeProject', p.projectFileId)"
          >
            Resume
          </AppButton>
        </div>
      </li>
    </ul>
  </section>
</template>
