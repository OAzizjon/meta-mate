// Shared by the page language subscription, Settings, and Mate's settings binding.
import { parseSettings } from "./model.ts";
import type { Settings, SettingsInvoke, SettingsSubscribe } from "./model.ts";

export interface ThemeTarget {
  readonly dataset: { theme?: string };
}

export function applyTheme(raw: unknown, root: ThemeTarget): Settings {
  const settings = parseSettings(raw);
  root.dataset.theme = settings.theme;
  return settings;
}

/** Subscribe before loading; a live update takes precedence over a stale initial read. */
export async function followWindowSettings(
  root: ThemeTarget,
  invoke: SettingsInvoke,
  subscribe: SettingsSubscribe,
  onChange: (settings: Settings) => void,
): Promise<void> {
  const apply = (raw: unknown): void => {
    onChange(applyTheme(raw, root));
  };
  let changed = false;
  try {
    await subscribe((raw) => {
      changed = true;
      apply(raw);
    });
  } catch {
    // A failed event listener must not leave the initial palette hidden.
  }
  try {
    const initial = await invoke("load_settings");
    if (!changed) apply(initial);
  } catch {
    if (!changed) apply(undefined);
  }
}
