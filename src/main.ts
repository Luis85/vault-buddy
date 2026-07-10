import "./style.css";

import { getCurrentWindow } from "@tauri-apps/api/window";
import { createPinia } from "pinia";
import { createApp } from "vue";

import { initLogging, logError } from "./logging";
import { rootFor } from "./roots";

initLogging();

let label = "main";
try {
  label = getCurrentWindow().label;
} catch {
  // not under Tauri (dev/tests) — default to the buddy root
}

const app = createApp(rootFor(label));
// Vue swallows component errors before window.onerror can see them —
// route them into the persistent log with the component context Vue gives us.
app.config.errorHandler = (err, _instance, info) => {
  // Vue's default console logging is replaced once a custom errorHandler is
  // set — without this, a plain browser dev session (no logError sink) sees
  // the error vanish instead of the usual console trace.
  console.error(err);
  logError(`vue error (${info}): ${String(err)}`);
};
app.use(createPinia()).mount("#app");
