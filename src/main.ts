import { createApp } from "vue";
import { createPinia } from "pinia";
import App from "./App.vue";
import { initLogging, logError } from "./logging";
import "./style.css";

initLogging();
const app = createApp(App);
// Vue swallows component errors before window.onerror can see them —
// route them into the persistent log with the component context Vue gives us.
app.config.errorHandler = (err, _instance, info) => {
  logError(`vue error (${info}): ${String(err)}`);
};
app.use(createPinia()).mount("#app");
