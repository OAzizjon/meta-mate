// The Settings window's choices (src/settings.ts): how Mate thinks, how to talk to him, the
// sound mode and his size as radio groups, and the two on/off toggles. Each builder gets what
// to do with a new value; src/settings.ts saves it and writes the localized text. Only the
// model choice, whose options each have a line of description, keeps the cards; the short
// choices are segments on one row.

import type { LocaleKey } from "../i18n/index.ts";
import { buildCheckbox, buildRadioGroup } from "./controls.ts";
import type { CheckboxRefs, RadioGroupRefs } from "./controls.ts";
import { PTT_KEYS, ROBOT_SIZES } from "./model.ts";
import type { Language, ModelChoice, PttKey, PttMode, RobotSize, SoundMode, Theme } from "./model.ts";

export function buildThemeGroup(onSelect: (theme: Theme) => void): RadioGroupRefs<Theme> {
  return buildRadioGroup<Theme>(
    "theme",
    "settings.background",
    [
      { id: "settings-theme-light", value: "light", labelKey: "settings.theme_light" },
      { id: "settings-theme-dark", value: "dark", labelKey: "settings.theme_dark" },
    ],
    "segments",
    onSelect,
  );
}

export function buildPttKeyGroup(onSelect: (key: PttKey) => void): RadioGroupRefs<PttKey> {
  return buildRadioGroup<PttKey>(
    "pttKey",
    "settings.ptt_key",
    PTT_KEYS.map((key) => ({
      id: `settings-ptt-key-${key}`,
      value: key,
      labelKey: `settings.key_${key}` as LocaleKey,
    })),
    "segments",
    onSelect,
  );
}

/** The robot size choices as people read them (the stored values stay S, M, L, XL). */
const SIZE_LABELS: Readonly<Record<RobotSize, LocaleKey>> = {
  S: "settings.size_s",
  M: "settings.size_m",
  L: "settings.size_l",
  XL: "settings.size_xl",
};

export function buildModelGroup(
  onSelect: (model: ModelChoice) => void,
): RadioGroupRefs<ModelChoice> {
  return buildRadioGroup<ModelChoice>(
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
    "cards",
    onSelect,
  );
}

export function buildPttGroup(onSelect: (ptt: PttMode) => void): RadioGroupRefs<PttMode> {
  return buildRadioGroup<PttMode>(
    "ptt",
    "settings.push_to_talk_key",
    [
      {
        id: "settings-ptt-hold",
        value: "hold",
        labelKey: "settings.ptt_hold",
        hintKey: "settings.ptt_hold_hint",
      },
      {
        id: "settings-ptt-toggle",
        value: "toggle",
        labelKey: "settings.ptt_toggle",
        hintKey: "settings.ptt_toggle_hint",
      },
    ],
    "segments",
    onSelect,
  );
}

export function buildSoundGroup(onSelect: (sound: SoundMode) => void): RadioGroupRefs<SoundMode> {
  return buildRadioGroup<SoundMode>(
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
    "segments",
    onSelect,
  );
}

// Robot size ("S" | "M" | "L" | "XL")
export function buildSizeGroup(
  onSelect: (robotSize: RobotSize) => void,
): RadioGroupRefs<RobotSize> {
  return buildRadioGroup<RobotSize>(
    "robotSize",
    "settings.robot_size",
    ROBOT_SIZES.map((size) => ({
      id: `settings-robot-size-${size}`,
      value: size,
      labelKey: SIZE_LABELS[size],
    })),
    "segments",
    onSelect,
  );
}

export interface TogglesRefs {
  readonly group: HTMLDivElement;
  readonly reduceMotion: CheckboxRefs;
  readonly spotlight: CheckboxRefs;
  readonly startWithWindows: CheckboxRefs;
}

// Boolean toggles: reduceMotion and startWithWindows
export function buildToggles(
  onReduceMotion: (reduceMotion: boolean) => void,
  onStartWithWindows: (startWithWindows: boolean) => void,
  onSpotlight: (spotlight: boolean) => void = () => {},
): TogglesRefs {
  const group = document.createElement("div");
  group.className = "settings-group settings-toggles";

  const reduceMotion = buildCheckbox(
    "settings-reduce-motion",
    "settings.reduce_motion",
    onReduceMotion,
  );

  const startWithWindows = buildCheckbox(
    "settings-start-with-windows",
    "settings.start_with_windows",
    onStartWithWindows,
  );

  const spotlight = buildCheckbox("settings-spotlight", "settings.spotlight", onSpotlight);
  group.append(spotlight.wrapper, reduceMotion.wrapper, startWithWindows.wrapper);
  return { group, reduceMotion, startWithWindows, spotlight };
}

export function buildLanguageGroup(onSelect: (language: Language) => void): RadioGroupRefs<Language> {
  return buildRadioGroup<Language>(
    "language",
    "settings.language_title",
    [
      { id: "settings-language-auto", value: "auto", labelKey: "settings.language_auto" },
      { id: "settings-language-en", value: "en", labelKey: "settings.language_en" },
      { id: "settings-language-ru", value: "ru", labelKey: "settings.language_ru" },
    ],
    "segments",
    onSelect,
  );
}
