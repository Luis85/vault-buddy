<script setup lang="ts">
/**
 * Discard project (Task 59), from the header's project menu.
 *
 * Until Task 59 the retired phase-4 editor's Discard was the only thing
 * that ever asked Rust to discard a tutorial project — and a staged capture
 * a project has PINNED (every capture opened in the editor is) cannot be
 * discarded until its project is (R6, `staged_commands::discard_conflict`).
 * So this confirm is what keeps "throw this recording away" reachable: the
 * project goes (`editor_close_session` with `discardProject`: its edits,
 * renders and review files, owned files only, no-follow), the pin is
 * cleared, and the recording returns to the Record Screen list as an
 * ordinary staged capture the user can discard there. ADR invariant 6: the
 * recording itself is never deleted by this.
 *
 * Lives in `EditorRoot`, OUTSIDE the shell: a successful discard closes the
 * session, the shell unmounts, and a dialog inside it would unmount with
 * it before it could say a refusal. Success hides the window: there is
 * nothing left in it to show.
 *
 * **A refusal keeps the editor usable** (Task 59 fix round 1). The store
 * drops its session BEFORE it asks Rust, while a refused discard leaves the
 * Rust session live (`close_locked` returns before `drop_session`). So a
 * refusal hands the project id back to `EditorRoot`'s `reattach`, which
 * reopens it — Rust reuses the live session, or adopts the project if the
 * unpin landed before the removal failed — and the shell comes back behind
 * this dialog. The refusal is said in the role wording Rust gives it (the
 * store has already taken the `<path:#hash8>` handle out,
 * `src/editor/errorCopy.ts`), and Discard stays enabled for a retry.
 *
 * Visual-parity Task 22 (concept spec §9.9–9.10): the session dialogs'
 * lead row (`SessionLead`) says first that the recording stays, the footer
 * is `DialogButton`s — Discard project in the danger ink — and why it waits
 * is on screen (`FooterReason`, D14). The discard and its reattach run as
 * this dialog's own request (`useInlineLastError().track`, ruling T7-1):
 * their refusals are said here, never also toasted.
 */
import { computed, ref, watch } from "vue";

import { useInlineLastError } from "../../../composables/useInlineLastError";
import { useEditorProjectStore } from "../../../stores/editorProject";
import DialogHost from "../shell/DialogHost.vue";
import DialogButton from "./DialogButton.vue";
import FooterReason from "./FooterReason.vue";
import SessionLead from "./SessionLead.vue";

const props = defineProps<{
  open: boolean;
  /** Reopen `projectId` after a refused discard; `true` when the editor has
   * a session again. */
  reattach: (projectId: string) => Promise<boolean>;
}>();
const emit = defineEmits<{ (e: "close"): void; (e: "discarded"): void }>();

const project = useEditorProjectStore();
const inline = useInlineLastError();
const busy = ref(false);
const error = ref<string | null>(null);

watch(
  () => props.open,
  (open) => {
    if (open) error.value = null;
  },
);

function dismiss(): void {
  if (busy.value) return;
  error.value = null;
  emit("close");
}

/** Discard, or say why not — `null` once the project is gone. */
async function discard(projectId: string | null): Promise<string | null> {
  await project.close("discardProject");
  if (!project.lastError) return null;
  const refusal = project.lastError.message;
  const back = projectId !== null && (await props.reattach(projectId));
  return back
    ? `The project could not be discarded. ${refusal}`
    : `The project could not be discarded, and it could not be reopened here. ${refusal} Open it again from the panel.`;
}

async function confirm(): Promise<void> {
  const projectId = project.snapshot?.projectId ?? null;
  busy.value = true;
  error.value = null;
  try {
    error.value = await inline.track(() => discard(projectId));
    if (error.value === null) emit("discarded");
  } finally {
    busy.value = false;
  }
}

/** Why Cancel (and the ✕) waits, or `null`. */
const waitReason = computed(() => (busy.value ? "Discarding the project…" : null));
/** Why Discard project cannot act, or `null`. */
const confirmReason = computed(() => waitReason.value ?? (project.sessionId ? null : "No project is open."));
</script>

<template>
  <DialogHost
    :open="props.open"
    label="Discard project"
    :closable="!busy"
    :close-reason="waitReason"
    @close="dismiss"
  >
    <template #title>
      Discard this project?
    </template>
    <template #subtitle>
      Its edits and rendered videos go. Your recording does not.
    </template>

    <div
      data-testid="discard-project-dialog"
      class="flex flex-col gap-4"
    >
      <SessionLead
        data-testid="discard-project-lead"
        icon="trash"
        title="The recording stays"
      >
        Its edits, rendered videos and review files are deleted from this computer.
        Videos you published into a vault stay there, and the recording stays in
        your staged captures, where you can discard it too.
      </SessionLead>
      <p
        v-if="error"
        role="alert"
        class="text-xs text-danger-fg"
      >
        {{ error }}
      </p>
    </div>

    <template #footer>
      <FooterReason
        data-testid="discard-project-reason"
        :text="confirmReason"
      />
      <DialogButton
        data-testid="discard-project-cancel"
        :reason="waitReason"
        @click="dismiss"
      >
        Cancel
      </DialogButton>
      <DialogButton
        variant="danger"
        data-testid="discard-project-confirm"
        :reason="confirmReason"
        @click="confirm"
      >
        Discard project
      </DialogButton>
    </template>
  </DialogHost>
</template>
