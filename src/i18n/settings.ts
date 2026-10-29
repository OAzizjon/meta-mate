// Page entries share the saved language and follow changes from other windows.
import { getLocale, setLocale } from "./index.ts";
import { parseSettings, SETTINGS_CHANGED_EVENT } from "../settings/model.ts";

export async function followSettingsLocale(render: () => void): Promise<void> {
  if (typeof window === "undefined" || !("__TAURI_INTERNALS__" in window)) return;
  const { listen } = await import("@tauri-apps/api/event");
  const { invoke } = await import("@tauri-apps/api/core");
  const apply = (raw: unknown): void => {
    const previous = getLocale();
    const locale = setLocale(parseSettings(raw).language);
    document.documentElement.lang = locale;
    if (locale !== previous) render();
  };
  let changed = false;
  await listen<unknown>(SETTINGS_CHANGED_EVENT, (event) => {
    changed = true;
    apply(event.payload);
  });
  const initial = await invoke("load_settings");
  if (!changed) apply(initial);
}
