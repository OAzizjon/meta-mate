// Settings controller actions and motion policy, independent of the DOM and Tauri.
import type { Settings, SettingsStore } from "./model.ts";

/** Either the saved preference or the system preference can reduce animation. */
export function settingsReduceMotion(setting: boolean, systemMatches?: boolean): boolean {
  return setting || Boolean(systemMatches);
}

/** Keep settings updates and loads in sync with the view through injected callbacks. */
export function createSettingsActions(
  store: SettingsStore,
  syncControls: (settings: Settings) => void,
  isActive: () => boolean,
  onSaveFailed: (failed: boolean) => void = () => {},
) {
  return {
    applyPatch(patch: Partial<Settings>): Promise<Settings> {
      return store.update(patch).then(
        (updated) => {
          onSaveFailed(false);
          syncControls(updated);
          return updated;
        },
        () => {
          onSaveFailed(true);
          const updated = store.get();
          syncControls(updated);
          return updated;
        },
      );
    },
    load(): Promise<Settings> {
      return store.load().then((loaded) => {
        if (isActive()) {
          syncControls(loaded);
        }
        return loaded;
      });
    },
  };
}
