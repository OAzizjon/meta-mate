// Settings window controller (WP24): renders accessible controls for every `Settings` field
// using localized labels (`src/i18n`, invariant 7), loads initial state with
// `invoke("load_settings")`, and saves every change with `invoke("save_settings", { settings })`.
// Outside Tauri, uses defaults and keeps changes in memory.

import { getLocale, setLocale, t } from "./i18n/index.ts";
import type { LocaleCode, LocaleKey } from "./i18n/index.ts";
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
  isRobotSize,
  isSoundMode,
  isValidVoiceSpeed,
  parseSettings,
  snapVoiceSpeed,
  subscribeSettingsChanged,
  updateSettings,
};
export type {
  AccountInvoke,
  AccountState,
  AccountStatus,
  AccountStore,
  AccountSubscribe,
  AuthProvider,
  AuthStatus,
  ModelChoice,
  PttMode,
  RobotSize,
  Settings,
  SettingsInvoke,
  SettingsStore,
  SettingsSubscribe,
  SoundMode,
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

interface RadioSpec<V extends string> {
  readonly id: string;
  readonly value: V;
  readonly labelKey?: LocaleKey;
  readonly literalLabel?: string;
  readonly hintKey?: LocaleKey;
}

interface RadioGroupRefs<V extends string> {
  readonly fieldset: HTMLFieldSetElement;
  readonly legend: HTMLLegendElement;
  readonly legendKey: LocaleKey;
  readonly items: ReadonlyArray<{
    readonly value: V;
    readonly wrapper: HTMLLabelElement;
    readonly input: HTMLInputElement;
    readonly titleEl: HTMLSpanElement;
    readonly labelKey?: LocaleKey;
    readonly literalLabel?: string;
    readonly hintEl?: HTMLSpanElement;
    readonly hintKey?: LocaleKey;
  }>;
}

function buildRadioGroup<V extends string>(
  name: string,
  legendKey: LocaleKey,
  specs: readonly RadioSpec<V>[],
  optionsClassName: string,
  optionClassName: string,
  onSelect: (value: V) => void,
): RadioGroupRefs<V> {
  const fieldset = document.createElement("fieldset");
  fieldset.className = "settings-group";

  const legend = document.createElement("legend");
  legend.className = "settings-legend";
  fieldset.append(legend);

  const list = document.createElement("div");
  list.className = optionsClassName;
  fieldset.append(list);

  const items = specs.map((spec) => {
    const wrapper = document.createElement("label");
    wrapper.className = optionClassName;
    wrapper.htmlFor = spec.id;

    const input = document.createElement("input");
    input.type = "radio";
    input.id = spec.id;
    input.name = name;
    input.value = spec.value;

    const textWrap = document.createElement("span");
    textWrap.className = "option-text";

    const titleEl = document.createElement("span");
    titleEl.className = "option-title";
    textWrap.append(titleEl);

    let hintEl: HTMLSpanElement | undefined;
    if (spec.hintKey) {
      hintEl = document.createElement("span");
      hintEl.className = "option-hint";
      hintEl.id = `${spec.id}-hint`;
      input.setAttribute("aria-describedby", hintEl.id);
      textWrap.append(hintEl);
    }

    input.addEventListener("change", () => {
      if (input.checked) {
        onSelect(spec.value);
      }
    });

    wrapper.append(input, textWrap);
    list.append(wrapper);

    return {
      value: spec.value,
      wrapper,
      input,
      titleEl,
      labelKey: spec.labelKey,
      literalLabel: spec.literalLabel,
      hintEl,
      hintKey: spec.hintKey,
    };
  });

  return { fieldset, legend, legendKey, items };
}

interface CheckboxRefs {
  readonly wrapper: HTMLLabelElement;
  readonly input: HTMLInputElement;
  readonly titleEl: HTMLSpanElement;
  readonly labelKey: LocaleKey;
}

function buildCheckbox(
  id: string,
  labelKey: LocaleKey,
  onToggle: (checked: boolean) => void,
): CheckboxRefs {
  const wrapper = document.createElement("label");
  wrapper.className = "settings-checkbox-option";
  wrapper.htmlFor = id;

  const input = document.createElement("input");
  input.type = "checkbox";
  input.id = id;

  const textWrap = document.createElement("span");
  textWrap.className = "option-text";

  const titleEl = document.createElement("span");
  titleEl.className = "option-title";
  textWrap.append(titleEl);

  input.addEventListener("change", () => {
    onToggle(input.checked);
  });

  wrapper.append(input, textWrap);
  return { wrapper, input, titleEl, labelKey };
}

interface AccountSectionRefs {
  readonly section: HTMLElement;
  readonly syncText: () => void;
  readonly syncState: (state: AccountState) => void;
  readonly destroy: () => void;
}

function buildAccountSection(accountStore: AccountStore): AccountSectionRefs {
  const section = document.createElement("section");
  section.className = "settings-group settings-account";
  section.setAttribute("aria-labelledby", "settings-account-title");

  const headerDiv = document.createElement("div");
  headerDiv.className = "settings-account-header";

  const titleEl = document.createElement("h2");
  titleEl.id = "settings-account-title";
  titleEl.className = "settings-account-title";

  const whyEl = document.createElement("p");
  whyEl.className = "settings-account-why";

  headerDiv.append(titleEl, whyEl);

  // Signed out view
  const signedOutView = document.createElement("div");
  signedOutView.className = "settings-account-signed-out";

  const buttonsRow = document.createElement("div");
  buttonsRow.className = "settings-account-buttons";

  const googleBtn = document.createElement("button");
  googleBtn.type = "button";
  googleBtn.id = "settings-auth-google";
  googleBtn.className = "settings-account-btn settings-account-btn-google";

  const githubBtn = document.createElement("button");
  githubBtn.type = "button";
  githubBtn.id = "settings-auth-github";
  githubBtn.className = "settings-account-btn settings-account-btn-github";

  buttonsRow.append(googleBtn, githubBtn);
  signedOutView.append(buttonsRow);

  googleBtn.addEventListener("click", () => {
    void accountStore.signIn("google");
  });

  githubBtn.addEventListener("click", () => {
    void accountStore.signIn("github");
  });

  // Signed in view
  const signedInView = document.createElement("div");
  signedInView.className = "settings-account-signed-in";
  signedInView.style.display = "none";

  const userEl = document.createElement("span");
  userEl.className = "settings-account-user";

  const signOutBtn = document.createElement("button");
  signOutBtn.type = "button";
  signOutBtn.id = "settings-auth-signout";
  signOutBtn.className = "settings-account-sign-out";

  signOutBtn.addEventListener("click", () => {
    void accountStore.signOut();
  });

  signedInView.append(userEl, signOutBtn);

  // Accessible status line
  const statusEl = document.createElement("div");
  statusEl.className = "settings-account-status";
  statusEl.setAttribute("role", "status");
  statusEl.setAttribute("aria-live", "polite");

  section.append(headerDiv, signedOutView, signedInView, statusEl);

  const getProviderName = (provider?: AuthProvider): string => {
    if (provider === "github") {
      return t("auth.provider_github");
    }
    return t("auth.provider_google");
  };

  const syncState = (state: AccountState): void => {
    if (state.status === "signed_in") {
      signedOutView.style.display = "none";
      signedInView.style.display = "flex";
      userEl.textContent = t("auth.signed_in", {
        name: state.name,
        provider: getProviderName(state.provider),
      });
      statusEl.textContent = "";
      statusEl.classList.remove("is-error");
      signOutBtn.disabled = false;
    } else if (state.status === "signing_in") {
      signedOutView.style.display = "block";
      signedInView.style.display = "none";
      googleBtn.disabled = true;
      githubBtn.disabled = true;

      const isGoogle = state.provider === "google";
      googleBtn.classList.toggle("is-waiting", isGoogle);
      githubBtn.classList.toggle("is-waiting", !isGoogle);

      googleBtn.textContent = isGoogle ? t("auth.waiting") : t("auth.google");
      githubBtn.textContent = !isGoogle ? t("auth.waiting") : t("auth.github");

      statusEl.textContent = t("auth.waiting");
      statusEl.classList.remove("is-error");
    } else {
      // signed_out
      signedOutView.style.display = "block";
      signedInView.style.display = "none";

      const canSign = accountStore.canSignIn();
      googleBtn.disabled = !canSign;
      githubBtn.disabled = !canSign;
      googleBtn.classList.remove("is-waiting");
      githubBtn.classList.remove("is-waiting");
      googleBtn.textContent = t("auth.google");
      githubBtn.textContent = t("auth.github");

      if (state.error) {
        statusEl.textContent = t(state.error as LocaleKey);
        statusEl.classList.add("is-error");
      } else {
        statusEl.textContent = "";
        statusEl.classList.remove("is-error");
      }
    }
  };

  const syncText = (): void => {
    titleEl.textContent = t("auth.title");
    whyEl.textContent = t("auth.why");
    signOutBtn.textContent = t("auth.sign_out");
    syncState(accountStore.getState());
  };

  syncText();

  return {
    section,
    syncText,
    syncState,
    destroy(): void {
      section.replaceChildren();
    },
  };
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
  let destroyed = false;

  const header = document.createElement("header");
  header.className = "settings-header";

  const titleHeading = document.createElement("h1");
  titleHeading.className = "settings-title";

  const languageNote = document.createElement("p");
  languageNote.className = "settings-language-note";
  header.append(titleHeading, languageNote);

  const form = document.createElement("form");
  form.className = "settings-form";
  form.addEventListener("submit", (event) => {
    event.preventDefault();
  });

  const applyPatch = (patch: Partial<Settings>): Promise<Settings> => {
    return store.update(patch).then((updated) => {
      syncControls(updated);
      return updated;
    });
  };

  const accountGroup = buildAccountSection(accountStore);

  const modelGroup = buildRadioGroup<ModelChoice>(
    "model",
    "settings.model_title",
    [
      {
        id: "settings-model-quick",
        value: "quick",
        labelKey: "settings.model_quick",
        hintKey: "settings.model_quick_hint",
      },
      {
        id: "settings-model-thorough",
        value: "thorough",
        labelKey: "settings.model_thorough",
        hintKey: "settings.model_thorough_hint",
      },
    ],
    "settings-options",
    "settings-radio-option",
    (model) => {
      void applyPatch({ model });
    },
  );

  const pttGroup = buildRadioGroup<PttMode>(
    "ptt",
    "settings.push_to_talk_key",
    [
      {
        id: "settings-ptt-hold",
        value: "hold",
        labelKey: "settings.ptt_hold",
      },
      {
        id: "settings-ptt-toggle",
        value: "toggle",
        labelKey: "settings.ptt_toggle",
      },
    ],
    "settings-options",
    "settings-radio-option",
    (ptt) => {
      void applyPatch({ ptt });
    },
  );

  const soundGroup = buildRadioGroup<SoundMode>(
    "sound",
    "settings.sound_mode",
    [
      {
        id: "settings-sound-calm",
        value: "calm",
        labelKey: "tray.sounds_calm",
      },
      {
        id: "settings-sound-fun",
        value: "fun",
        labelKey: "tray.sounds_fun",
      },
    ],
    "settings-options",
    "settings-radio-option",
    (sound) => {
      void applyPatch({ sound });
    },
  );

  // Voice speed range (0.8..1.3)
  const speedGroup = document.createElement("div");
  speedGroup.className = "settings-group settings-range-group";

  const speedHeader = document.createElement("div");
  speedHeader.className = "settings-range-header";

  const speedLabel = document.createElement("label");
  speedLabel.className = "settings-label";
  speedLabel.htmlFor = "settings-voice-speed";

  const speedOutput = document.createElement("output");
  speedOutput.id = "settings-voice-speed-value";
  speedOutput.className = "settings-range-output";
  speedOutput.setAttribute("for", "settings-voice-speed");

  speedHeader.append(speedLabel, speedOutput);

  const speedInput = document.createElement("input");
  speedInput.type = "range";
  speedInput.id = "settings-voice-speed";
  speedInput.className = "settings-range-input";
  speedInput.min = String(MIN_VOICE_SPEED);
  speedInput.max = String(MAX_VOICE_SPEED);
  speedInput.step = String(VOICE_SPEED_STEP);

  const onSpeedUpdate = (): void => {
    const voiceSpeed = snapVoiceSpeed(Number(speedInput.value));
    speedInput.value = String(voiceSpeed);
    speedOutput.textContent = formatVoiceSpeed(voiceSpeed);
    if (voiceSpeed !== store.get().voiceSpeed) {
      void applyPatch({ voiceSpeed });
    }
  };
  speedInput.addEventListener("input", onSpeedUpdate);
  speedInput.addEventListener("change", onSpeedUpdate);

  speedGroup.append(speedHeader, speedInput);

  // Robot size ("S" | "M" | "L" | "XL")
  const sizeGroup = buildRadioGroup<RobotSize>(
    "robotSize",
    "settings.robot_size",
    ROBOT_SIZES.map((size) => ({
      id: `settings-robot-size-${size}`,
      value: size,
      literalLabel: size,
    })),
    "settings-size-grid",
    "settings-size-option",
    (robotSize) => {
      void applyPatch({ robotSize });
    },
  );

  // Boolean toggles: reduceMotion and startWithWindows
  const togglesGroup = document.createElement("div");
  togglesGroup.className = "settings-group settings-toggles";

  const reduceMotionBox = buildCheckbox(
    "settings-reduce-motion",
    "settings.reduce_motion",
    (reduceMotion) => {
      void applyPatch({ reduceMotion });
    },
  );

  const startWithWindowsBox = buildCheckbox(
    "settings-start-with-windows",
    "settings.start_with_windows",
    (startWithWindows) => {
      void applyPatch({ startWithWindows });
    },
  );

  togglesGroup.append(reduceMotionBox.wrapper, startWithWindowsBox.wrapper);

  form.append(
    accountGroup.section,
    modelGroup.fieldset,
    pttGroup.fieldset,
    soundGroup.fieldset,
    speedGroup,
    sizeGroup.fieldset,
    togglesGroup,
  );

  container.replaceChildren(header, form);

  const motionQuery =
    typeof window !== "undefined" && typeof window.matchMedia === "function"
      ? window.matchMedia(REDUCED_MOTION_QUERY)
      : null;

  const syncReducedMotionClass = (reduceMotionSetting: boolean): void => {
    const reduced = reduceMotionSetting || Boolean(motionQuery?.matches);
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
    languageNote.textContent = t("settings.language_automatic");

    const radioGroups: readonly RadioGroupRefs<string>[] = [
      modelGroup,
      pttGroup,
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

    speedLabel.textContent = t("settings.voice_speed");
    reduceMotionBox.titleEl.textContent = t(reduceMotionBox.labelKey);
    startWithWindowsBox.titleEl.textContent = t(startWithWindowsBox.labelKey);
    accountGroup.syncText();
  };

  const syncRadioSelection = <V extends string>(
    group: RadioGroupRefs<V>,
    selected: V,
  ): void => {
    for (const item of group.items) {
      const checked = item.value === selected;
      item.input.checked = checked;
      item.wrapper.classList.toggle("is-selected", checked);
    }
  };

  const syncControls = (settings: Settings): void => {
    syncRadioSelection(modelGroup, settings.model);
    syncRadioSelection(pttGroup, settings.ptt);
    syncRadioSelection(soundGroup, settings.sound);
    syncRadioSelection(sizeGroup, settings.robotSize);

    speedInput.value = String(settings.voiceSpeed);
    speedOutput.textContent = formatVoiceSpeed(settings.voiceSpeed);

    reduceMotionBox.input.checked = settings.reduceMotion;
    reduceMotionBox.wrapper.classList.toggle("is-selected", settings.reduceMotion);

    startWithWindowsBox.input.checked = settings.startWithWindows;
    startWithWindowsBox.wrapper.classList.toggle("is-selected", settings.startWithWindows);

    syncReducedMotionClass(settings.reduceMotion);
  };

  syncText();
  syncControls(store.get());

  const unsubscribeChanged = subscribeSettingsChanged(
    store,
    subscribe,
    (updated) => {
      if (!destroyed) {
        syncControls(updated);
      }
    },
  );

  void store.load().then((loaded) => {
    if (!destroyed) {
      syncControls(loaded);
    }
  });

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
      return store.load().then((loaded) => {
        if (!destroyed) {
          syncControls(loaded);
        }
        return loaded;
      });
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
