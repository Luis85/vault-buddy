// The single place the app funnels diagnostics through. Every export is a
// no-op unless we're running inside Tauri, so the Vitest/happy-dom suite
// (no Tauri runtime) neither throws nor needs the log plugin. Under Tauri the
// calls reach `@tauri-apps/plugin-log`, whose Rust side writes them into the
// same rotating file the panic hook writes `crash.log` beside.
import {
  error as pluginError,
  warn as pluginWarn,
  info as pluginInfo,
} from "@tauri-apps/plugin-log";

function underTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

// Fire-and-forget: a logging failure must never break the UI or surface as an
// unhandled rejection (which our own handler would then re-log in a loop).
function emit(fn: (message: string) => Promise<void>, message: string): void {
  if (!underTauri()) return;
  try {
    void fn(message).catch(() => {});
  } catch {
    // plugin unavailable — logging must stay invisible to the app
  }
}

/** Info-level lifecycle marker, e.g. "drag start @ 1920,12". */
export function logBreadcrumb(message: string): void {
  emit(pluginInfo, message);
}

/** Warn-level marker for a failure the app otherwise swallows. */
export function logWarning(message: string): void {
  emit(pluginWarn, message);
}

/**
 * Route uncaught frontend errors into the persistent log so a webview fault
 * during a drag leaves a trail alongside the Rust crash record. Idempotent
 * enough for one startup call.
 */
export function initLogging(): void {
  if (!underTauri()) return;
  window.addEventListener("error", (event) => {
    emit(
      pluginError,
      `window error: ${event.message} @ ${event.filename}:${event.lineno}:${event.colno}`,
    );
  });
  window.addEventListener("unhandledrejection", (event) => {
    emit(pluginError, `unhandled rejection: ${String(event.reason)}`);
  });
}
