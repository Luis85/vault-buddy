<script setup lang="ts">
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { computed, onMounted, onUnmounted, ref } from "vue";

import { logWarning } from "../logging";
import { copyToClipboard } from "../utils/clipboard";

type McpStatus = { state: string; port: number | null; message: string | null };
type McpConfig = {
  enabled: boolean;
  port: number;
  allowWrites: boolean;
  token: string;
  status: McpStatus;
};

const cfg = ref<McpConfig | null>(null);
const error = ref<string | null>(null);
// Serializes saves: every request builds a full config from cfg.value, so
// two quick toggles would race — the second sent from a pre-response
// snapshot could undo the first (Codex review catch). While a save is in
// flight all controls are disabled, so a stale-snapshot request can never
// be issued.
const saving = ref(false);
let unlisten: (() => void) | null = null;

onMounted(async () => {
  try {
    cfg.value = await invoke<McpConfig>("get_mcp_config");
  } catch (e) {
    // not running under Tauri (unit tests) or IPC failure — leave the card
    // empty. Warn-level like every other degraded-but-continuing component
    // path (CaptureSettings/Tasks); logError is reserved for main.ts's
    // uncaught-vue-error hook.
    logWarning(`mcp settings: get_mcp_config failed: ${String(e)}`);
  }
  try {
    unlisten = await listen<McpStatus>("mcp:status", (event) => {
      if (cfg.value) cfg.value.status = event.payload;
    });
  } catch (e) {
    logWarning(`mcp settings: listen failed: ${String(e)}`);
  }
});
onUnmounted(() => unlisten?.());

async function save(patch: Partial<Pick<McpConfig, "enabled" | "port" | "allowWrites">>) {
  if (!cfg.value || saving.value) return;
  saving.value = true;
  error.value = null;
  const input = {
    enabled: cfg.value.enabled,
    port: cfg.value.port,
    allowWrites: cfg.value.allowWrites,
    ...patch,
  };
  try {
    cfg.value = await invoke<McpConfig>("set_mcp_config", { input });
  } catch (e) {
    // Surfaced in the card's error line AND logged — the "no swallowed
    // error" rule wants a file trace even for user-visible failures
    // (mirrors CaptureSettings.vue's save()).
    error.value = String(e);
    logWarning(`mcp settings: set_mcp_config failed: ${String(e)}`);
  } finally {
    saving.value = false;
  }
}

async function regenerate() {
  if (saving.value) return;
  saving.value = true;
  error.value = null;
  try {
    cfg.value = await invoke<McpConfig>("regenerate_mcp_token");
  } catch (e) {
    error.value = String(e);
    logWarning(`mcp settings: regenerate_mcp_token failed: ${String(e)}`);
  } finally {
    saving.value = false;
  }
}

function copy(text: string) {
  copyToClipboard(text, "mcp settings");
}

const statusLabel = computed(() => {
  const s = cfg.value?.status;
  if (!s) return "";
  if (s.state === "running") return `Running on 127.0.0.1:${s.port}`;
  if (s.state === "error") return s.message ?? "Error";
  return "Stopped";
});

const url = computed(() => `http://127.0.0.1:${cfg.value?.port ?? 22082}/mcp`);
const claudeSnippet = computed(
  () =>
    `claude mcp add --transport http vault-buddy ${url.value} --header "Authorization: Bearer ${cfg.value?.token ?? ""}"`,
);
const cursorSnippet = computed(() =>
  JSON.stringify(
    {
      mcpServers: {
        "vault-buddy": {
          url: url.value,
          headers: { Authorization: `Bearer ${cfg.value?.token ?? ""}` },
        },
      },
    },
    null,
    2,
  ),
);
const claudeDesktopSnippet = computed(() =>
  JSON.stringify(
    {
      mcpServers: {
        "vault-buddy": {
          command: "npx",
          args: [
            "mcp-remote",
            url.value,
            "--header",
            `Authorization: Bearer ${cfg.value?.token ?? ""}`,
          ],
        },
      },
    },
    null,
    2,
  ),
);
</script>

<template>
  <section v-if="cfg">
    <h2 class="mb-1.5 text-xs font-semibold uppercase tracking-wide text-slate-400">
      AI integrations — MCP server
    </h2>
    <div class="flex flex-col gap-2 rounded-xl border border-white/10 bg-white/5 p-2">
      <div class="flex items-center justify-between gap-2">
        <label
          for="mcp-enabled"
          class="text-sm text-slate-200"
        >
          Local MCP server
          <span class="block text-xs text-slate-500">{{ statusLabel }}</span>
        </label>
        <input
          id="mcp-enabled"
          data-testid="mcp-enabled"
          type="checkbox"
          class="h-4 w-4 accent-violet-500 disabled:cursor-default disabled:opacity-50"
          :checked="cfg.enabled"
          :disabled="saving"
          @change="save({ enabled: ($event.target as HTMLInputElement).checked })"
        >
      </div>
      <div class="flex items-center justify-between gap-2">
        <label
          for="mcp-port"
          class="text-sm text-slate-200"
        >Port</label>
        <input
          id="mcp-port"
          data-testid="mcp-port"
          type="number"
          min="1024"
          max="65535"
          class="w-24 rounded-lg border border-white/10 bg-white/5 px-2 py-0.5 text-right text-sm text-slate-200 disabled:cursor-default disabled:opacity-50"
          :value="cfg.port"
          :disabled="saving"
          @change="save({ port: Number(($event.target as HTMLInputElement).value) })"
        >
      </div>
      <div class="flex items-center justify-between gap-2">
        <label
          for="mcp-writes"
          class="text-sm text-slate-200"
        >
          Allow vault writes
          <span class="block text-xs text-slate-500">
            AI clients may add tasks, update task status, and create today's daily note
          </span>
        </label>
        <input
          id="mcp-writes"
          data-testid="mcp-writes"
          type="checkbox"
          class="h-4 w-4 accent-violet-500 disabled:cursor-default disabled:opacity-50"
          :checked="cfg.allowWrites"
          :disabled="saving"
          @change="save({ allowWrites: ($event.target as HTMLInputElement).checked })"
        >
      </div>
      <div
        v-if="cfg.token"
        class="flex items-center justify-between gap-2"
      >
        <span class="text-sm text-slate-200">Token</span>
        <span class="flex items-center gap-1">
          <code class="max-w-40 truncate text-xs text-slate-400">{{ cfg.token }}</code>
          <button
            type="button"
            data-testid="mcp-copy-token"
            class="cursor-pointer rounded-lg border border-white/10 bg-white/5 px-2 py-0.5 text-xs text-slate-300 hover:bg-white/10"
            @click="copy(cfg.token)"
          >
            Copy
          </button>
          <button
            type="button"
            data-testid="mcp-regenerate"
            class="cursor-pointer rounded-lg border border-white/10 bg-white/5 px-2 py-0.5 text-xs text-slate-300 hover:bg-white/10 disabled:cursor-default disabled:opacity-50"
            :disabled="saving"
            @click="regenerate"
          >
            Regenerate
          </button>
        </span>
      </div>
      <details
        v-if="cfg.enabled && cfg.token"
        class="text-xs text-slate-400"
      >
        <summary class="cursor-pointer select-none text-slate-300">
          Client setup
        </summary>
        <div class="mt-1.5 flex flex-col gap-2">
          <div>
            <div class="mb-0.5 flex items-center justify-between">
              <span>Claude Code</span>
              <button
                type="button"
                class="cursor-pointer text-slate-300 hover:text-slate-100"
                @click="copy(claudeSnippet)"
              >
                Copy
              </button>
            </div>
            <pre class="overflow-x-auto rounded-lg bg-black/30 p-1.5">{{ claudeSnippet }}</pre>
          </div>
          <div>
            <div class="mb-0.5 flex items-center justify-between">
              <span>Cursor (.cursor/mcp.json)</span>
              <button
                type="button"
                class="cursor-pointer text-slate-300 hover:text-slate-100"
                @click="copy(cursorSnippet)"
              >
                Copy
              </button>
            </div>
            <pre class="overflow-x-auto rounded-lg bg-black/30 p-1.5">{{ cursorSnippet }}</pre>
          </div>
          <div>
            <div class="mb-0.5 flex items-center justify-between">
              <span>Claude Desktop (via mcp-remote)</span>
              <button
                type="button"
                class="cursor-pointer text-slate-300 hover:text-slate-100"
                @click="copy(claudeDesktopSnippet)"
              >
                Copy
              </button>
            </div>
            <pre class="overflow-x-auto rounded-lg bg-black/30 p-1.5">{{ claudeDesktopSnippet }}</pre>
          </div>
        </div>
      </details>
      <p
        v-if="error"
        class="text-xs text-rose-400"
      >
        {{ error }}
      </p>
    </div>
  </section>
</template>
