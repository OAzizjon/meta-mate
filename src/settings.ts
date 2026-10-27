// Settings window controller (WP24): renders accessible controls for every `Settings` field
// using localized labels (`src/i18n`, invariant 7), loads initial state with
// `invoke("load_settings")`, and saves every change with `invoke("save_settings", { settings })`.
// Outside Tauri, uses defaults and keeps changes in memory.

import { getLocale, setLocale, t } from "./i18n/index.ts";
import type { LocaleCode, LocaleKey } from "./i18n/index.ts";
import {
  DEFAULT_SETTINGS,
  MAX_VOICE_SPEED,
  MIN_VOICE_SPEED,
  MODEL_CHOICES,
  PTT_MODES,
  ROBOT_SIZES,
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
  updateSettings,
} from "./settings/model.ts";
import type {
  ModelChoice,
  PttMode,
  RobotSize,
  Settings,
  SettingsInvoke,
  SettingsStore,
  SoundMode,
} from "./settings/model.ts";

export {
  DEFAULT_SETTINGS,
  MAX_VOICE_SPEED,
  MIN_VOICE_SPEED,
  MODEL_CHOICES,
  PTT_MODES,
  ROBOT_SIZES,
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
  updateSettings,
};
export type {
  ModelChoice,
  PttMode,
  RobotSize,
  Settings,
  SettingsInvoke,
  SettingsStore,
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

const defaultStore = createSettingsStore(isTauri() ? tauriInvoke : null);

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
}

export interface SettingsController {
  getSettings(): Settings;
  setSettings(patch: Partial<Settings>): Promise<Settings>;
  reload(): Promise<Settings>;
  setLocale(locale: LocaleCode): void;
  destroy(): void;
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

export function mountSettings(
  container: HTMLElement,
  options: MountSettingsOptions = {},
): SettingsController {
  const store =
    options.store ?? createSettingsStore(isTauri() ? tauriInvoke : null);

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
    const parsed = Number(speedInput.value);
    const voiceSpeed = isValidVoiceSpeed(parsed)
      ? Math.round(parsed * 100) / 100
      : DEFAULT_SETTINGS.voiceSpeed;
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

  void store.load().then((loaded) => {
    syncControls(loaded);
  });

  return {
    getSettings(): Settings {
      return store.get();
    },
    setSettings(patch: Partial<Settings>): Promise<Settings> {
      return applyPatch(patch);
    },
    reload(): Promise<Settings> {
      return store.load().then((loaded) => {
        syncControls(loaded);
        return loaded;
      });
    },
    setLocale(locale: LocaleCode): void {
      setLocale(locale);
      syncText();
    },
    destroy(): void {
      motionQuery?.removeEventListener("change", onMotionMediaChange);
      container.replaceChildren();
    },
  };
}

if (typeof document !== "undefined") {
  const root = document.getElementById("settings-app");
  if (root) {
    const controller = mountSettings(root, { store: defaultStore });
    if (import.meta.env.DEV) {
      window.__settings = controller;
    }
  }
}
