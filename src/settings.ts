// Settings window controller (WP24): renders accessible controls for every `Settings` field
// using localized labels (`src/i18n`, invariant 7), loads initial state with
// `invoke("load_settings")`, and saves every change with `invoke("save_settings", { settings })`.
// Outside Tauri, uses defaults and keeps changes in memory.
// The sections are built in src/settings/: controls.ts, choices-view.ts, voice-speed-view.ts
// and account-view.ts; this file puts them in order and keeps them in sync.

import { getLocale, setLocale, t } from "./i18n/index.ts";
import type { LocaleCode } from "./i18n/index.ts";
import { bulbBadge } from "./bulb-badge.ts";
import { applyTheme } from "./settings/theme.ts";
import { buildAccountSection } from "./settings/account-view.ts";
import { createSettingsActions, settingsReduceMotion } from "./settings/actions.ts";
import {
  buildLanguageGroup,
  buildThemeGroup,
  buildModelGroup,
  buildPttGroup,
  buildPttKeyGroup,
  buildSizeGroup,
  buildSoundGroup,
  buildToggles,
} from "./settings/choices-view.ts";
import { syncCheckbox, syncRadioSelection } from "./settings/controls.ts";
import type { RadioGroupRefs } from "./settings/controls.ts";
import {
  DEV_STATUS_DURATION_MS,
  MATE_JUMP_CMD,
  OPEN_UNDER_THE_HOOD_CMD,
  buildDevSection,
  devButtonsHidden,
} from "./settings/dev-view.ts";
import type { DevCommand, DevSectionRefs } from "./settings/dev-view.ts";
import {
  UPDATE_INSTALL_CMD,
  UPDATE_STATUS_CMD,
  buildUpdateSection,
  isValidSemver,
  parseUpdateStatus,
  resolveUpdateNote,
} from "./settings/update-view.ts";
import type {
  UpdateNotes,
  UpdateNotice,
  UpdateSectionRefs,
  UpdateStatus,
} from "./settings/update-view.ts";
import { buildVoiceSpeedGroup } from "./settings/voice-speed-view.ts";
import {
  AUTH_CHANGED_EVENT,
  AUTH_PROVIDERS,
  createAccountStore,
  isAuthProvider,
  isAuthStatus,
} from "./settings/account.ts";
import type {
  AccountInvoke,
  AccountState,
  AccountStatus,
  AccountStore,
  AccountSubscribe,
  AuthProvider,
  AuthStatus,
} from "./settings/account.ts";
import {
  DEFAULT_SETTINGS,
  MAX_VOICE_SPEED,
  MIN_VOICE_SPEED,
  MODEL_CHOICES,
  PTT_MODES,
  PTT_KEYS,
  ROBOT_SIZES,
  SETTINGS_CHANGED_EVENT,
  SETTINGS_KEYS,
  SOUND_MODES,
  VOICE_SPEED_STEP,
  createSettingsStore,
  defaultSettings,
  formatVoiceSpeed,
  isModelChoice,
  isPttMode,
  isPttKey,
  isRobotSize,
  isSoundMode,
  isValidVoiceSpeed,
  parseSettings,
  snapVoiceSpeed,
  subscribeSettingsChanged,
  updateSettings,
} from "./settings/model.ts";
import type {
  ModelChoice,
  PttKey,
  PttMode,
  RobotSize,
  Settings,
  SettingsInvoke,
  SettingsStore,
  SettingsSubscribe,
  SoundMode,
} from "./settings/model.ts";

export {
  AUTH_CHANGED_EVENT,
  AUTH_PROVIDERS,
  createAccountStore,
  isAuthProvider,
  isAuthStatus,
  DEFAULT_SETTINGS,
  MAX_VOICE_SPEED,
  MIN_VOICE_SPEED,
  MODEL_CHOICES,
  PTT_MODES,
  PTT_KEYS,
  ROBOT_SIZES,
  SETTINGS_CHANGED_EVENT,
  SETTINGS_KEYS,
  SOUND_MODES,
  VOICE_SPEED_STEP,
  createSettingsStore,
  defaultSettings,
  formatVoiceSpeed,
  isModelChoice,
  isPttMode,
  isPttKey,
  isRobotSize,
  isSoundMode,
  isValidVoiceSpeed,
  parseSettings,
  snapVoiceSpeed,
  subscribeSettingsChanged,
  updateSettings,
  DEV_STATUS_DURATION_MS,
  MATE_JUMP_CMD,
  OPEN_UNDER_THE_HOOD_CMD,
  buildDevSection,
  devButtonsHidden,
  UPDATE_INSTALL_CMD,
  UPDATE_STATUS_CMD,
  buildUpdateSection,
  isValidSemver,
  parseUpdateStatus,
  resolveUpdateNote,
};
export type {
  AccountInvoke,
  AccountState,
  AccountStatus,
  AccountStore,
  AccountSubscribe,
  AuthProvider,
  AuthStatus,
  DevCommand,
  DevSectionRefs,
  ModelChoice,
  PttKey,
  PttMode,
  RobotSize,
  Settings,
  SettingsInvoke,
  SettingsStore,
  SettingsSubscribe,
  SoundMode,
  UpdateNotes,
  UpdateNotice,
  UpdateSectionRefs,
  UpdateStatus,
};

const REDUCED_MOTION_QUERY = "(prefers-reduced-motion: reduce)";

function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

const tauriInvoke: SettingsInvoke = async (cmd, args) => {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke(cmd, args);
};

const tauriSubscribe: SettingsSubscribe = async (onChanged) => {
  const { listen } = await import("@tauri-apps/api/event");
  return listen<unknown>(SETTINGS_CHANGED_EVENT, (event) => {
    onChanged(event.payload);
  });
};

const tauriAccountInvoke: AccountInvoke = async (cmd, args) => {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke(cmd, args);
};

const tauriAccountSubscribe: AccountSubscribe = async (onChanged) => {
  const { listen } = await import("@tauri-apps/api/event");
  return listen<unknown>(AUTH_CHANGED_EVENT, (event) => {
    onChanged(event.payload);
  });
};

const defaultStore = createSettingsStore(isTauri() ? tauriInvoke : null);
const defaultAccountStore = createAccountStore(
  isTauri() ? tauriAccountInvoke : null,
  isTauri() ? tauriAccountSubscribe : null,
);

/** Loads settings via `invoke("load_settings")` (or returns in-memory settings outside Tauri). */
export async function loadSettings(): Promise<Settings> {
  return defaultStore.load();
}

/** Validates and saves settings via `invoke("save_settings", { settings })` (or in memory outside Tauri). */
export async function saveSettings(settings: Settings): Promise<Settings> {
  return defaultStore.save(settings);
}

export interface MountSettingsOptions {
  readonly store?: SettingsStore;
  readonly subscribe?: SettingsSubscribe | null;
  readonly accountStore?: AccountStore;
  readonly invoke?: SettingsInvoke | null;
}

export interface SettingsController {
  getSettings(): Settings;
  setSettings(patch: Partial<Settings>): Promise<Settings>;
  reload(): Promise<Settings>;
  setLocale(locale: LocaleCode): void;
  destroy(): void;
  readonly accountStore: AccountStore;
}

declare global {
  interface Window {
    __settings?: SettingsController;
  }
}

export function mountSettings(
  container: HTMLElement,
  options: MountSettingsOptions = {},
): SettingsController {
  const store =
    options.store ?? createSettingsStore(isTauri() ? tauriInvoke : null);
  const subscribe =
    options.subscribe === undefined
      ? isTauri()
        ? tauriSubscribe
        : null
      : options.subscribe;
  const accountStore = options.accountStore ?? defaultAccountStore;
  const invoke =
    options.invoke === undefined
      ? isTauri()
        ? tauriInvoke
        : null
      : options.invoke;
  let destroyed = false;
  let saveFailed = false;

  const header = document.createElement("header");
  header.className = "settings-header";

  const titleHeading = document.createElement("h1");
  titleHeading.className = "settings-title";

  const savedNote = document.createElement("p");
  savedNote.className = "settings-saved-note";
  const headerText = document.createElement("div");
  headerText.className = "settings-header-text";
  headerText.append(titleHeading, savedNote);
  header.append(bulbBadge(), headerText);
  const saveAlert = document.createElement("p");
  saveAlert.className = "settings-account-status is-error";
  saveAlert.setAttribute("role", "alert");

  const form = document.createElement("form");
  form.className = "settings-form";
  form.addEventListener("submit", (event) => {
    event.preventDefault();
  });

  const applyPatch = (patch: Partial<Settings>): Promise<Settings> => {
    return settingsActions.applyPatch(patch);
  };

  const accountGroup = buildAccountSection(accountStore);
  const updateGroup = buildUpdateSection(invoke, t, getLocale);
  const devGroup = buildDevSection(
    (developerMode) => {
      void applyPatch({ developerMode });
    },
    invoke,
    t,
  );

  const languageGroup = buildLanguageGroup((language) => {
    void applyPatch({ language });
  });

  const themeGroup = buildThemeGroup((theme) => {
    void applyPatch({ theme });
  });

  const modelGroup = buildModelGroup((model) => {
    void applyPatch({ model });
  });

  const pttGroup = buildPttGroup((ptt) => {
    void applyPatch({ ptt });
  });
  const pttKeyGroup = buildPttKeyGroup((pttKey) => {
    void applyPatch({ pttKey });
  });

  const soundGroup = buildSoundGroup((sound) => {
    void applyPatch({ sound });
  });

  const {
    group: speedGroup,
    label: speedLabel,
    input: speedInput,
    show: showVoiceSpeed,
  } = buildVoiceSpeedGroup(
    () => store.get().voiceSpeed,
    (voiceSpeed) => {
      void applyPatch({ voiceSpeed });
    },
  );

  const sizeGroup = buildSizeGroup((robotSize) => {
    void applyPatch({ robotSize });
  });

  const {
    group: togglesGroup,
    reduceMotion: reduceMotionBox,
    spotlight: spotlightBox,
    startWithWindows: startWithWindowsBox,
  } = buildToggles(
    (reduceMotion) => {
      void applyPatch({ reduceMotion });
    },
    (startWithWindows) => {
      void applyPatch({ startWithWindows });
    },
    (spotlight) => {
      void applyPatch({ spotlight });
    },
  );

  // The everyday choices first; the account and developer tools come last (WP85).
  form.append(
    languageGroup.fieldset,
    themeGroup.fieldset,
    modelGroup.fieldset,
    pttKeyGroup.fieldset,
    pttGroup.fieldset,
    soundGroup.fieldset,
    speedGroup,
    sizeGroup.fieldset,
    togglesGroup,
    accountGroup.section,
    updateGroup.section,
    devGroup.section,
  );

  container.replaceChildren(header, saveAlert, form);

  const motionQuery =
    typeof window !== "undefined" && typeof window.matchMedia === "function"
      ? window.matchMedia(REDUCED_MOTION_QUERY)
      : null;

  const syncReducedMotionClass = (reduceMotionSetting: boolean): void => {
    const reduced = settingsReduceMotion(reduceMotionSetting, motionQuery?.matches);
    container.classList.toggle("reduce-motion", reduced);
    if (typeof document !== "undefined") {
      document.documentElement.classList.toggle("reduce-motion", reduced);
    }
  };

  const onMotionMediaChange = (): void => {
    syncReducedMotionClass(store.get().reduceMotion);
  };
  motionQuery?.addEventListener("change", onMotionMediaChange);

  const syncText = (): void => {
    if (typeof document !== "undefined") {
      document.documentElement.lang = getLocale();
      document.title = t("tray.settings");
    }
    titleHeading.textContent = t("tray.settings");
    savedNote.textContent = t("settings.saved_note");
    saveAlert.textContent = saveFailed ? t("settings.save_failed") : "";

    const radioGroups: readonly RadioGroupRefs<string>[] = [
      languageGroup,
      themeGroup,
      modelGroup,
      pttGroup,
      pttKeyGroup,
      soundGroup,
      sizeGroup,
    ];
    for (const group of radioGroups) {
      group.legend.textContent = t(group.legendKey);
      for (const item of group.items) {
        item.titleEl.textContent = item.labelKey ? t(item.labelKey) : (item.literalLabel ?? "");
        if (item.hintEl && item.hintKey) {
          item.hintEl.textContent = t(item.hintKey);
        }
      }
    }
    // The size radios also carry their names themselves, not only through the wrapping label.
    for (const item of sizeGroup.items) {
      if (item.labelKey) {
        item.input.setAttribute("aria-label", t(item.labelKey));
      }
    }

    speedLabel.textContent = t("settings.voice_speed");
    showVoiceSpeed(store.get().voiceSpeed);
    spotlightBox.titleEl.textContent = t(spotlightBox.labelKey);
    reduceMotionBox.titleEl.textContent = t(reduceMotionBox.labelKey);
    startWithWindowsBox.titleEl.textContent = t(startWithWindowsBox.labelKey);
    accountGroup.syncText();
    updateGroup.syncText(t, getLocale());
    devGroup.syncText(t);
  };

  const syncControls = (settings: Settings): void => {
    applyTheme(settings, document.documentElement);
    setLocale(settings.language);
    syncText();
    syncRadioSelection(languageGroup, settings.language);
    syncRadioSelection(themeGroup, settings.theme);
    syncRadioSelection(modelGroup, settings.model);
    syncRadioSelection(pttGroup, settings.ptt);
    syncRadioSelection(pttKeyGroup, settings.pttKey);
    syncRadioSelection(soundGroup, settings.sound);
    syncRadioSelection(sizeGroup, settings.robotSize);

    speedInput.value = String(settings.voiceSpeed);
    showVoiceSpeed(settings.voiceSpeed);

    syncCheckbox(spotlightBox, settings.spotlight);
    syncCheckbox(reduceMotionBox, settings.reduceMotion);
    syncCheckbox(startWithWindowsBox, settings.startWithWindows);
    devGroup.syncState(settings.developerMode);

    syncReducedMotionClass(settings.reduceMotion);
  };

  syncText();
  syncControls(store.get());

  const settingsActions = createSettingsActions(store, syncControls, () => !destroyed, (failed) => {
    saveFailed = failed;
    saveAlert.textContent = failed ? t("settings.save_failed") : "";
  });

  const unsubscribeChanged = subscribeSettingsChanged(
    store,
    subscribe,
    (updated) => {
      if (!destroyed) {
        syncControls(updated);
      }
    },
  );

  void settingsActions.load();
  void updateGroup.load();

  const unsubscribeAccount = accountStore.subscribe((accountState) => {
    if (!destroyed) {
      accountGroup.syncState(accountState);
    }
  });

  void accountStore.load().then((accountState) => {
    if (!destroyed) {
      accountGroup.syncState(accountState);
    }
  });

  return {
    getSettings(): Settings {
      return store.get();
    },
    setSettings(patch: Partial<Settings>): Promise<Settings> {
      return applyPatch(patch);
    },
    reload(): Promise<Settings> {
      void accountStore.load();
      void updateGroup.load();
      return settingsActions.load();
    },
    setLocale(locale: LocaleCode): void {
      setLocale(locale);
      syncText();
    },
    destroy(): void {
      destroyed = true;
      unsubscribeChanged();
      unsubscribeAccount();
      accountGroup.destroy();
      updateGroup.destroy();
      devGroup.destroy();
      motionQuery?.removeEventListener("change", onMotionMediaChange);
      container.replaceChildren();
    },
    accountStore,
  };
}

if (typeof document !== "undefined") {
  const root = document.getElementById("settings-app");
  if (root) {
    const controller = mountSettings(root, {
      store: defaultStore,
      accountStore: defaultAccountStore,
    });
    if (import.meta.env.DEV) {
      window.__settings = controller;
    }
  }
}
