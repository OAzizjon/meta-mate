// Applies validated Settings (src/settings/model.ts) to Mate's WebView: root reduce-motion
// class, robot controller (reduceMotion, calm/fun mode, and window-fitted SVG size S/M/L/XL),
// and the Web Audio engine (sound mode calm/fun, voiceSpeed, reduceMotion).
// Pure and free of DOM/Tauri/JSON imports so `node --test` runs this module directly.

import { DEFAULT_ROBOT_SIZE, ROBOT_SIZE_PX } from "../robot/types.ts";
import {
  ROBOT_SIZES,
  SETTINGS_CHANGED_EVENT,
  createSettingsStore,
  parseSettings,
  subscribeSettingsChanged,
} from "./model.ts";
import type {
  RobotSize,
  Settings,
  SettingsInvoke,
  SettingsStore,
  SettingsSubscribe,
  SoundMode,
} from "./model.ts";

export { SETTINGS_CHANGED_EVENT, parseSettings };
export type { RobotSize, Settings, SettingsInvoke, SettingsStore, SettingsSubscribe, SoundMode };

/** CSS class toggled on the root element when motion is reduced. */
export const REDUCE_MOTION_CLASS = "reduce-motion" as const;

/**
 * Mate's overlay window size in CSS pixels at 100 % scale (`MATE_SIZE` in
 * `src-tauri/src/overlay.rs`). Used when viewport bounds are not yet laid out.
 */
export const MATE_WINDOW_SIZE_PX = 220;

/** Mate's SVG viewBox dimensions (`VIEW_W` x `VIEW_H` in `src/robot/geometry.ts`). */
export const ROBOT_VIEWBOX_W = 260;
export const ROBOT_VIEWBOX_H = 240;

export interface ViewportSize {
  readonly width: number;
  readonly height: number;
}

export interface MateAudioSettings {
  readonly mode: SoundMode;
  readonly voiceSpeed: number;
  readonly reduceMotion: boolean;
}

export interface ClassListTarget {
  readonly classList: {
    toggle(token: string, force?: boolean): unknown;
  };
}

export interface MateRobotTarget {
  setReduceMotion(reduce: boolean): void;
  setFunMode(fun: boolean): void;
  setSize(size: RobotSize): void;
}

export interface MateAudioTarget {
  setSettings(settings: MateAudioSettings): void;
}

export interface MateSettingsTarget {
  readonly root?: ClassListTarget | null;
  readonly roots?: readonly (ClassListTarget | null | undefined)[];
  readonly robot?: MateRobotTarget | null;
  readonly audio?: MateAudioTarget | null;
  readonly viewport?: ViewportSize | (() => ViewportSize | null) | null;
  readonly prefersReducedMotion?: boolean | (() => boolean);
}

export interface AppliedMateSettings {
  readonly settings: Settings;
  readonly effectiveRobotSize: RobotSize;
  readonly rootReduceMotion: boolean;
  readonly funMode: boolean;
  readonly audioSettings: MateAudioSettings;
}

/**
 * Computes the rendered SVG width and height in CSS pixels for a `RobotSize` preset
 * (`S` = 64x59, `M` = 96x89, `L` = 160x148, `XL` = 220x203).
 */
export function robotSizeDimensions(size: RobotSize): ViewportSize {
  const width = ROBOT_SIZE_PX[size];
  const height = Math.round((width * ROBOT_VIEWBOX_H) / ROBOT_VIEWBOX_W);
  return { width, height };
}

/**
 * Returns `true` when the SVG dimensions for `size` fit inside `viewport` (CSS pixels).
 */
export function doesRobotSizeFit(size: RobotSize, viewport: ViewportSize): boolean {
  if (
    !Number.isFinite(viewport.width) ||
    !Number.isFinite(viewport.height) ||
    viewport.width <= 0 ||
    viewport.height <= 0
  ) {
    return false;
  }
  const dims = robotSizeDimensions(size);
  return dims.width <= viewport.width && dims.height <= viewport.height;
}

/**
 * Picks `requested` when it fits inside `viewport`, or steps down through `XL -> L -> M -> S`
 * to the largest preset that fits. When `viewport` is omitted, null, or unlaid (`<= 0`),
 * uses `MATE_WINDOW_SIZE_PX` (220x220 CSS px), where all presets `S..XL` fit.
 */
export function fitRobotSize(
  requested: RobotSize = DEFAULT_ROBOT_SIZE,
  viewport?: ViewportSize | null,
): RobotSize {
  const effectiveViewport: ViewportSize =
    viewport &&
    Number.isFinite(viewport.width) &&
    Number.isFinite(viewport.height) &&
    viewport.width > 0 &&
    viewport.height > 0
      ? viewport
      : { width: MATE_WINDOW_SIZE_PX, height: MATE_WINDOW_SIZE_PX };

  const startIndex = ROBOT_SIZES.indexOf(requested);
  const from = startIndex >= 0 ? startIndex : ROBOT_SIZES.indexOf(DEFAULT_ROBOT_SIZE);
  for (let i = from; i >= 0; i--) {
    const candidate = ROBOT_SIZES[i];
    if (candidate && doesRobotSizeFit(candidate, effectiveViewport)) {
      return candidate;
    }
  }
  return "S";
}

/** Maps validated `Settings` to the audio engine's `AudioSettings` subset. */
export function toAudioSettings(settings: Settings): MateAudioSettings {
  return {
    mode: settings.sound,
    voiceSpeed: settings.voiceSpeed,
    reduceMotion: settings.reduceMotion,
  };
}

/**
 * Validates `raw` with `parseSettings` and applies `reduceMotion`, `sound` (`calm`/`fun`),
 * and window-fitted `robotSize` (`S`/`M`/`L`/`XL`) to the provided targets.
 */
export function applySettingsToMate(
  raw: unknown,
  target: MateSettingsTarget,
): AppliedMateSettings {
  const settings = parseSettings(raw);
  const osReduced =
    typeof target.prefersReducedMotion === "function"
      ? Boolean(target.prefersReducedMotion())
      : Boolean(target.prefersReducedMotion);
  const rootReduceMotion = settings.reduceMotion || osReduced;
  const funMode = settings.sound === "fun";
  const viewport =
    typeof target.viewport === "function" ? target.viewport() : (target.viewport ?? null);
  const effectiveRobotSize = fitRobotSize(settings.robotSize, viewport);
  const audioSettings = toAudioSettings(settings);

  if (target.root) {
    target.root.classList.toggle(REDUCE_MOTION_CLASS, rootReduceMotion);
  }
  if (target.roots) {
    for (const root of target.roots) {
      root?.classList.toggle(REDUCE_MOTION_CLASS, rootReduceMotion);
    }
  }

  if (target.robot) {
    target.robot.setReduceMotion(settings.reduceMotion);
    target.robot.setFunMode(funMode);
    target.robot.setSize(effectiveRobotSize);
  }

  if (target.audio) {
    target.audio.setSettings(audioSettings);
  }

  return {
    settings,
    effectiveRobotSize,
    rootReduceMotion,
    funMode,
    audioSettings,
  };
}

export interface MateSettingsBindingOptions {
  readonly store?: SettingsStore;
  readonly invoke?: SettingsInvoke | null;
  readonly subscribe?: SettingsSubscribe | null;
}

export interface MateSettingsBinding {
  /** Current validated settings snapshot. */
  get(): Settings;
  /** Re-applies the current settings (e.g. after resize or OS motion-preference change). */
  reapply(): AppliedMateSettings;
  /** Parses `raw` with `parseSettings`, updates the store without saving, and applies it. */
  apply(raw: unknown): AppliedMateSettings;
  /** Attaches or replaces the audio target once the audio engine is ready and syncs it. */
  setAudio(audio: MateAudioTarget | null): void;
  /** Resolves once the initial `load_settings` call finishes and has been applied. */
  readonly ready: Promise<Settings>;
  /** Unsubscribes from `settings-changed`. */
  destroy(): void;
}

/**
 * Loads settings once at start via `invoke("load_settings")` (falling back to defaults outside
 * Tauri), listens for `"settings-changed"` events, and applies every update to Mate's window.
 */
export function startMateSettings(
  target: MateSettingsTarget,
  options: MateSettingsBindingOptions = {},
): MateSettingsBinding {
  const store = options.store ?? createSettingsStore(options.invoke ?? null);
  let audioTarget: MateAudioTarget | null = target.audio ?? null;
  let destroyed = false;

  const currentTarget = (): MateSettingsTarget => ({
    ...target,
    audio: audioTarget,
  });

  const applyCurrent = (settings: Settings): AppliedMateSettings =>
    applySettingsToMate(settings, currentTarget());

  // Apply defaults (or initial store state) synchronously before IPC settles.
  applyCurrent(store.get());

  const unsubscribe = subscribeSettingsChanged(
    store,
    options.subscribe ?? null,
    (updated) => {
      if (!destroyed) {
        applyCurrent(updated);
      }
    },
  );

  const ready = store.load().then((loaded) => {
    if (!destroyed) {
      applyCurrent(loaded);
    }
    return loaded;
  });

  return {
    get(): Settings {
      return store.get();
    },
    reapply(): AppliedMateSettings {
      return applyCurrent(store.get());
    },
    apply(raw: unknown): AppliedMateSettings {
      const updated = store.replace(raw);
      return applyCurrent(updated);
    },
    setAudio(audio: MateAudioTarget | null): void {
      audioTarget = audio;
      if (audioTarget && !destroyed) {
        audioTarget.setSettings(toAudioSettings(store.get()));
      }
    },
    ready,
    destroy(): void {
      destroyed = true;
      unsubscribe();
    },
  };
}
