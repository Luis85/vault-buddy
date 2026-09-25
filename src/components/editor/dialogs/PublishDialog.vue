<script setup lang="ts">
/**
 * Publish a rendered video into a vault (Task 48; F-43; ADR R13 — the
 * TENTH sanctioned vault write): pick the vault, a folder in it, whether to
 * date the folder and whether to write a companion note, then Publish —
 * Rust copies the immutable product in (never overwriting anything) and
 * writes the note naming the file that actually landed
 * (`editor_publish_product`).
 *
 * **The vault defaults to the CAPTURE's** (`project.destination.vault`,
 * F-01) — never whatever the panel last had selected, which is another
 * window's state about another task. A capture whose vault Obsidian no
 * longer lists gets no default at all: the user picks one, and the
 * disabled Publish says so (R20).
 *
 * **The vault's Screen settings are the defaults** (Task 59 fix round 1,
 * GAP-211): *Date folders* → the dated toggle, *Write a companion note* →
 * the note toggle, read from `get_screen_capture_config` for the vault
 * picked — and re-read whenever the pick changes. The user can still
 * override either for this publish. Settings that cannot be read fall back
 * to the project's own date choice and a note, never to a refusal.
 *
 * **Never closes onto a publish in flight.** From the click until the
 * receipt (or the refusal) lands, Close is disabled and Escape/backdrop
 * are refused (`DialogHost`'s `closable`) — the copy is registered in
 * Rust's job registry, but this dialog is the only place its outcome is
 * shown. The receipt names the landed file(s) and offers Open (the note
 * when there is one, else the video — `open_screen_capture`); a note that
 * could not be written is a warning beside a video that WAS published.
 */
import { computed, ref, watch } from "vue";

import type { PublishReceipt, VaultChoice } from "../../../editorTypes";
import { logWarning } from "../../../logging";
import { toEditorError, useEditorProjectStore } from "../../../stores/editorProject";
import AppButton from "../../ui/AppButton.vue";
import DialogHost from "../shell/DialogHost.vue";
import PublishForm from "./PublishForm.vue";

const props = defineProps<{ open: boolean; productId: string | null; productName: string }>();
const emit = defineEmits<{ (e: "close"): void }>();

const editorProject = useEditorProjectStore();

const vaults = ref<VaultChoice[]>([]);
const vaultId = ref("");
const folder = ref("");
const dated = ref(false);
const createNote = ref(true);
const publishing = ref(false);
const error = ref<string | null>(null);
const receipt = ref<PublishReceipt | null>(null);

/** The vault's Screen settings as this publish's defaults. A ticket keeps
 * a slow reply for a vault no longer picked from overwriting a newer one. */
let defaultsTicket = 0;
async function loadDefaults(id: string): Promise<void> {
  const ticket = ++defaultsTicket;
  if (!id) return;
  try {
    const defaults = await editorProject.port.publishDefaults(id);
    if (ticket !== defaultsTicket) return;
    dated.value = defaults.dated;
    createNote.value = defaults.createNote;
  } catch (e) {
    logWarning(`editor publish: the vault's Screen settings could not be read: ${toEditorError(e).message}`);
  }
}

watch(vaultId, (id) => void loadDefaults(id));

async function loadVaults(capture: string): Promise<void> {
  try {
    vaults.value = await editorProject.port.listVaults();
  } catch (e) {
    vaults.value = [];
    error.value = `The vault list could not be read. ${toEditorError(e).message}`.trim();
  }
  vaultId.value = vaults.value.some((v) => v.id === capture) ? capture : "";
}

function reset(): void {
  const destination = editorProject.project?.destination;
  folder.value = destination?.folder ?? "";
  dated.value = destination?.dated ?? false;
  createNote.value = true;
  error.value = null;
  receipt.value = null;
  vaultId.value = "";
  void loadVaults(destination?.vault ?? "");
}

watch(
  () => props.open,
  (open) => {
    if (open) reset();
  },
  { immediate: true },
);

const reason = computed<string | null>(() => {
  if (!editorProject.sessionId) return "No project is open.";
  if (!props.productId) return "Render a video first.";
  if (publishing.value) return "Publishing…";
  if (!vaultId.value) return "Choose a vault to publish into.";
  return null;
});

async function publish(): Promise<void> {
  const sessionId = editorProject.sessionId;
  const productId = props.productId;
  if (reason.value !== null || !sessionId || !productId) return;
  publishing.value = true;
  error.value = null;
  try {
    receipt.value = await editorProject.port.publishProduct(sessionId, productId, {
      vaultId: vaultId.value,
      folder: folder.value.trim(),
      dated: dated.value,
      createNote: createNote.value,
    });
  } catch (e) {
    error.value = toEditorError(e).message;
  } finally {
    publishing.value = false;
  }
}

/** A path's last component — the landed NAME, which is all the dialog
 * shows (the full path only travels to `open_screen_capture`). */
function fileName(path: string): string {
  return path.split(/[\\/]/).pop() ?? path;
}

async function openInObsidian(): Promise<void> {
  const landed = receipt.value;
  if (!landed) return;
  try {
    await editorProject.port.openScreenCapture(landed.vaultId, landed.notePath ?? landed.videoPath);
  } catch (e) {
    const message = toEditorError(e).message;
    logWarning(`editor publish: could not open the published file: ${message}`);
    error.value = `Obsidian could not open it. ${message}`;
  }
}

function close(): void {
  if (!publishing.value) emit("close");
}
</script>

<template>
  <DialogHost
    :open="open"
    label="Publish to vault"
    :closable="!publishing"
    @close="close"
  >
    <div
      data-testid="publish-dialog"
      class="flex w-[28rem] max-w-full flex-col gap-3"
    >
      <header class="flex items-start justify-between gap-2">
        <div>
          <h2 class="text-sm font-semibold text-fg">
            Publish to vault
          </h2>
          <p class="text-xs text-fg-muted">
            A copy of “{{ productName }}” goes into your vault. The rendered video
            and your project stay as they are.
          </p>
        </div>
        <AppButton
          variant="ghost"
          size="sm"
          data-testid="publish-close"
          :disabled="publishing"
          @click="close"
        >
          Close
        </AppButton>
      </header>

      <div
        v-if="receipt"
        data-testid="publish-result"
        role="status"
        class="flex flex-col gap-1 text-xs text-fg-secondary"
      >
        <p>Published into {{ receipt.vaultName }}:</p>
        <p
          data-testid="publish-video-name"
          class="font-medium text-fg"
        >
          {{ fileName(receipt.videoPath) }}
        </p>
        <p
          v-if="receipt.notePath"
          data-testid="publish-note-name"
          class="font-medium text-fg"
        >
          {{ fileName(receipt.notePath) }}
        </p>
        <p
          v-if="receipt.warning"
          data-testid="publish-warning"
          class="text-danger-fg"
        >
          {{ receipt.warning }}
        </p>
        <div class="flex justify-end">
          <AppButton
            variant="primary"
            size="sm"
            data-testid="publish-open"
            @click="openInObsidian"
          >
            Open in Obsidian
          </AppButton>
        </div>
      </div>
      <template v-else>
        <PublishForm
          v-model:vault-id="vaultId"
          v-model:folder="folder"
          v-model:dated="dated"
          v-model:create-note="createNote"
          :vaults="vaults"
          :disabled="publishing"
        />
        <div class="flex items-center justify-end gap-2">
          <span
            v-if="reason"
            data-testid="publish-start-reason"
            class="text-micro text-fg-subtle"
          >{{ reason }}</span>
          <AppButton
            variant="primary"
            size="sm"
            data-testid="publish-start"
            :disabled="reason !== null"
            @click="publish"
          >
            Publish
          </AppButton>
        </div>
      </template>
      <p
        v-if="error"
        role="alert"
        data-testid="publish-error"
        class="text-xs text-danger-fg"
      >
        {{ error }}
      </p>
    </div>
  </DialogHost>
</template>
