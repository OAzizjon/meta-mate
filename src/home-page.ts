// The Home window's entry (home.html): the locale lookup, Mate, then the page itself.
import { getAppName, getLocale, t } from "./i18n/index.ts";
import { followSettingsLocale } from "./i18n/settings.ts";
import { mountHome, setHomeI18n } from "./home.ts";
import type { HomeController, HomeInvoke } from "./home.ts";
import { mountRobot } from "./robot/index.ts";
import { SETTINGS_CHANGED_EVENT } from "./settings/model.ts";
import type { SettingsSubscribe } from "./settings/model.ts";

setHomeI18n(t, getLocale);

const tauriInvoke: HomeInvoke = async (cmd, args) => {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke(cmd, args);
};

const tauriSubscribe: SettingsSubscribe = async (onChanged) => {
  const { listen } = await import("@tauri-apps/api/event");
  return listen<unknown>(SETTINGS_CHANGED_EVENT, (event) => {
    onChanged(event.payload);
  });
};

if (typeof document !== "undefined") {
  const root = document.getElementById("home-app");
  if (root) {
    const isTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
    // Rust titles the window too (APP_NAME); this keeps the page's own title the same.
    document.title = getAppName();
    const mount = (): HomeController => mountHome(root, {
      invoke: isTauri ? tauriInvoke : null,
      subscribe: isTauri ? tauriSubscribe : null,
      mountMate: (container) => {
        // Large, idle and calm: his open eyes follow the mouse.
        const robot = mountRobot(container);
        robot.setMood("calm");
        robot.setState("idle");
        robot.setSize("L");
        return robot;
      },
    });
    let controller = mount();
    void followSettingsLocale(() => {
      controller.destroy();
      controller = mount();
    }).catch((error: unknown) => console.error("home: language:", error));

    if (import.meta.env.DEV) {
      (window as unknown as { __home?: HomeController }).__home = controller;
    }
  }
}
