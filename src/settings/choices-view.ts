// The Settings window's choices (src/settings.ts): how Mate thinks, how to talk to him, the
// sound mode and his size as radio groups, and the two on/off toggles. Each builder gets what
// to do with a new value; src/settings.ts saves it and writes the localized text.

import type { LocaleKey } from "../i18n/index.ts";
import { buildCheckbox, buildRadioGroup } from "./controls.ts";
import type { CheckboxRefs, RadioGroupRefs } from "./controls.ts";
import { ROBOT_SIZES } from "./model.ts";
import type { ModelChoice, PttMode, RobotSize, SoundMode } from "./model.ts";

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
    "settings-options",
    "settings-radio-option",
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
      },
      {
        id: "settings-ptt-toggle",
        value: "toggle",
        labelKey: "settings.ptt_toggle",
      },
    ],
    "settings-options",
    "settings-radio-option",
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
    "settings-options",
    "settings-radio-option",
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
    "settings-size-grid",
    "settings-size-option",
    onSelect,
  );
}

export interface TogglesRefs {
  readonly group: HTMLDivElement;
  readonly reduceMotion: CheckboxRefs;
  readonly startWithWindows: CheckboxRefs;
}

// Boolean toggles: reduceMotion and startWithWindows
export function buildToggles(
  onReduceMotion: (reduceMotion: boolean) => void,
  onStartWithWindows: (startWithWindows: boolean) => void,
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

  group.append(reduceMotion.wrapper, startWithWindows.wrapper);
  return { group, reduceMotion, startWithWindows };
}
