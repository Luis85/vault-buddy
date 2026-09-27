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
 *
 * Visual-parity Task 21 (concept spec §9.4, screen 07): 680 wide, the
 * issue rows and closing help of the concept, and its footer — **Export
 * diagnostics** (Help's own path, `useDiagnosticsExport`) · **Back to
 * edit** · **Continue to render**. The concept's "editable project has not
 * been downloaded" note is browser-only and has no native twin (design
 * D10): a native project is committed by Save project.
 */
import { computed, ref, watch } from "vue";

import { useDiagnosticsExport } from "../../../composables/useDiagnosticsExport";
import { revealFinding } from "../../../editor/checkReveal";
import { requestReveal } from "../../../editor/revealBus";
import type { CheckFinding } from "../../../editorTypes";
import { useEditorChecksStore } from "../../../stores/editorChecks";
import DialogHost from "../shell/DialogHost.vue";
import ChecksDestination from "./ChecksDestination.vue";
import ChecksFindingList from "./ChecksFindingList.vue";
import DialogButton from "./DialogButton.vue";

const props = defineProps<{ open: boolean }>();
const emit = defineEmits<{ (e: "close"): void }>();

const checks = useEditorChecksStore();
const { exportDiagnostics } = useDiagnosticsExport();
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
    :width="680"
    close-testid="checks-close"
    @close="emit('close')"
  >
    <template #title>
      Before you share
    </template>
    <template #subtitle>
      Actionable checks, not a quality score.
    </template>

    <div
      data-testid="checks-dialog"
      class="flex flex-col gap-4"
    >
      <ChecksDestination
        v-if="choosing"
        @done="destinationSet"
        @cancel="choosing = false"
      />
      <template v-else>
        <ChecksFindingList @act="act" />
        <p
          data-testid="checks-help"
          class="text-[10px] text-fg-muted"
        >
          No automatic speech transcription, content review or privacy detection is performed. This is not an accessibility certification.
        </p>
      </template>
    </div>

    <template
      v-if="!choosing"
      #footer
    >
      <span
        v-if="renderReason"
        data-testid="checks-render-reason"
        class="mr-auto text-[10px] text-fg-muted"
      >{{ renderReason }}</span>
      <DialogButton
        data-testid="checks-diagnostics"
        @click="exportDiagnostics"
      >
        Export diagnostics
      </DialogButton>
      <DialogButton
        data-testid="checks-back"
        @click="emit('close')"
      >
        Back to edit
      </DialogButton>
      <DialogButton
        variant="primary"
        data-testid="checks-render"
        :reason="renderReason"
        @click="continueToRender"
      >
        Continue to render
      </DialogButton>
    </template>
  </DialogHost>
</template>
