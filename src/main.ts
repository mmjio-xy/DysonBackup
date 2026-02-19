import { createApp } from "vue";
import "@fortawesome/fontawesome-free/css/all.min.css";
import App from "./App.vue";

createApp(App).mount("#app");

// release 模式下屏蔽右键菜单和 F12
if (!import.meta.env.DEV) {
  document.addEventListener("contextmenu", (e) => e.preventDefault());
  document.addEventListener("keydown", (e) => {
    if (e.key === "F12") e.preventDefault();
  });
}
