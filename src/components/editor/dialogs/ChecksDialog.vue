<script setup lang="ts">
/**
 * Before you share (Task 54; F-45, F-33, F-38; SCREENS 07: "Checks shows
 * actionable findings rather than an invented quality score"). Rust's
 * findings (`editor_get_checks`, through `editorChecks`), summarized as
 * "N blockers · M review warnings" and grouped by severity — Fix, Review,
 * Note — each with the one button that reveals its object
 * (`checkReveal.revealFinding`: select it, open its tab or panel, scroll the
 * timeline) and closes this dialog so the object can be seen. The
 * destination is answered here instead (`ChecksDestination`), since there
 * is nothing else to reveal for it.
 *
 * States it plainly: these inspect the edit, not the tutorial's meaning;
 * nothing transcribes speech, reviews content or detects private details.
 * A read that failed says so — it never reads as "nothing found".
 *
 * **Continue to render** asks the header's Render button to open its
 * dialog (`requestReveal("render")`); it is disabled, with the reason
 * beside it, while anything blocks — the same rule the Render dialog holds.
 */
import { computed, ref, watch } from "vue";

import { revealFinding } from "../../../editor/checkReveal";
import { requestReveal } from "../../../editor/revealBus";
import type { CheckFinding } from "../../../editorTypes";
import { useEditorChecksStore } from "../../../stores/editorChecks";
import AppButton from "../../ui/AppButton.vue";
import DialogHost from "../shell/DialogHost.vue";
import ChecksDestination from "./ChecksDestination.vue";
import ChecksFindingList from "./ChecksFindingList.vue";

const props = defineProps<{ open: boolean }>();
const emit = defineEmits<{ (e: "close"): void }>();

const checks = useEditorChecksStore();
/** The vault picker is showing instead of the list. */
const choosing = ref(false);

watch(
  () => props.open,
  (open) => {
    if (!open) return;
    choosing.value = false;
    void checks.refresh();
  },
  { immediate: true },
);

const renderReason = computed(() => checks.blockedReason);

function act(finding: CheckFinding): void {
  if (finding.action === "setDestination") {
    choosing.value = true;
    return;
  }
  if (revealFinding(finding)) emit("close");
}

function destinationSet(): void {
  choosing.value = false;
  void checks.refresh();
}

function continueToRender(): void {
  if (renderReason.value) return;
  emit("close");
  requestReveal("render");
}
</script>

<template>
  <DialogHost
    :open="open"
    label="Before you share"
    @close="emit('close')"
  >
    <div
      data-testid="checks-dialog"
      class="flex w-[34rem] max-w-full flex-col gap-3"
    >
      <header class="flex items-start justify-between gap-2">
        <div>
          <h2 class="text-sm font-semibold text-fg">
            Before you share
          </h2>
          <p class="text-xs text-fg-muted">
            Actionable checks, not a quality score.
          </p>
        </div>
        <AppButton
          variant="ghost"
          size="sm"
          data-testid="checks-close"
          @click="emit('close')"
        >
          Close
        </AppButton>
      </header>

      <ChecksDestination
        v-if="choosing"
        @done="destinationSet"
        @cancel="choosing = false"
      />
      <template v-else>
        <ChecksFindingList @act="act" />
        <p class="text-micro text-fg-subtle">
          No automatic speech transcription, content review or privacy detection
          is performed. This is not an accessibility certification.
        </p>
        <div class="flex items-center justify-end gap-2">
          <span
            v-if="renderReason"
            data-testid="checks-render-reason"
            class="text-micro text-fg-subtle"
          >{{ renderReason }}</span>
          <AppButton
            variant="secondary"
            size="sm"
            data-testid="checks-back"
            @click="emit('close')"
          >
            Back to edit
          </AppButton>
          <AppButton
            variant="primary"
            size="sm"
            data-testid="checks-render"
            :disabled="Boolean(renderReason)"
            @click="continueToRender"
          >
            Continue to render
          </AppButton>
        </div>
      </template>
    </div>
  </DialogHost>
</template>
