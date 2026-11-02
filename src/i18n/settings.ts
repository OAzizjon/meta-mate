// Page entries share the saved language and background and follow changes from other windows.
import { getLocale, setLocale } from "./index.ts";
import { SETTINGS_CHANGED_EVENT } from "../settings/model.ts";
import { applyTheme, followWindowSettings } from "../settings/theme.ts";

export async function followSettingsLocale(render: () => void): Promise<void> {
  if (typeof document === "undefined") return;
  applyTheme(undefined, document.documentElement);
  if (typeof window === "undefined" || !("__TAURI_INTERNALS__" in window)) return;
  const { listen } = await import("@tauri-apps/api/event");
  const { invoke } = await import("@tauri-apps/api/core");
  await followWindowSettings(document.documentElement, invoke, async (onChanged) => {
    return listen<unknown>(SETTINGS_CHANGED_EVENT, (event) => onChanged(event.payload));
  }, (settings) => {
    // The in-app "less motion" choice, like Windows' own (theme.css `.reduce-motion`).
    document.documentElement.classList.toggle("reduce-motion", settings.reduceMotion);
    const previous = getLocale();
    const locale = setLocale(settings.language);
    document.documentElement.lang = locale;
    if (locale !== previous) render();
  });
}
