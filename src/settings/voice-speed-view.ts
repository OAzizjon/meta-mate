// The Settings window's voice speed slider (src/settings.ts): the speed beside its label, and
// in words for screen readers. A new speed is snapped to a step before it is passed on.

import { t } from "../i18n/index.ts";
import type { LocaleKey } from "../i18n/index.ts";
import {
  MAX_VOICE_SPEED,
  MIN_VOICE_SPEED,
  VOICE_SPEED_STEP,
  formatVoiceSpeed,
  snapVoiceSpeed,
  voiceSpeedPace,
} from "./model.ts";
import type { VoiceSpeedPace } from "./model.ts";

/** The voice speed in words for screen readers, e.g. "1.1×, faster than usual". */
const SPEED_PACE_LABELS: Readonly<Record<VoiceSpeedPace, LocaleKey>> = {
  slower: "settings.voice_speed_slower",
  usual: "settings.voice_speed_usual",
  faster: "settings.voice_speed_faster",
};

export interface VoiceSpeedRefs {
  readonly group: HTMLDivElement;
  readonly label: HTMLLabelElement;
  readonly input: HTMLInputElement;
  /** Shows `voiceSpeed` beside the label and in the slider's `aria-valuetext`. */
  readonly show: (voiceSpeed: number) => void;
}

/** `current` is the saved speed; `onChange` gets a different one the person picked. */
export function buildVoiceSpeedGroup(
  current: () => number,
  onChange: (voiceSpeed: number) => void,
): VoiceSpeedRefs {
  // Voice speed range (0.8..1.3)
  const speedGroup = document.createElement("div");
  speedGroup.className = "settings-group settings-range-group";

  const speedHeader = document.createElement("div");
  speedHeader.className = "settings-range-header";

  const speedLabel = document.createElement("label");
  speedLabel.id = "settings-voice-speed-label";
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
  speedInput.setAttribute("aria-labelledby", speedLabel.id);

  // The value as shown, and in words for screen readers (a bare "1.1" says little).
  const showVoiceSpeed = (voiceSpeed: number): void => {
    const speed = formatVoiceSpeed(voiceSpeed);
    speedOutput.textContent = speed;
    speedInput.setAttribute(
      "aria-valuetext",
      t(SPEED_PACE_LABELS[voiceSpeedPace(voiceSpeed)], { speed }),
    );
  };

  const onSpeedUpdate = (): void => {
    const voiceSpeed = snapVoiceSpeed(Number(speedInput.value));
    speedInput.value = String(voiceSpeed);
    showVoiceSpeed(voiceSpeed);
    if (voiceSpeed !== current()) {
      onChange(voiceSpeed);
    }
  };
  speedInput.addEventListener("input", onSpeedUpdate);
  speedInput.addEventListener("change", onSpeedUpdate);

  speedGroup.append(speedHeader, speedInput);

  return { group: speedGroup, label: speedLabel, input: speedInput, show: showVoiceSpeed };
}
