import "./style.css";

import { getCurrentWindow } from "@tauri-apps/api/window";
import { createPinia } from "pinia";
import { createApp } from "vue";

import { initLogging, logVueError } from "./logging";
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
// logVueError also restores the console trace a custom handler disables.
app.config.errorHandler = (err, _instance, info) => logVueError(err, info);
app.use(createPinia()).mount("#app");
