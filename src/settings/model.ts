// Pure settings model, validation, and in-memory store (WP24).
// No DOM, i18n JSON, or Tauri imports so Node (`node --test`) runs this module directly.

export const MODEL_CHOICES = ["quick", "thorough"] as const;
export type ModelChoice = (typeof MODEL_CHOICES)[number];

export const PTT_MODES = ["hold", "toggle"] as const;
export type PttMode = (typeof PTT_MODES)[number];

export const SOUND_MODES = ["calm", "fun"] as const;
export type SoundMode = (typeof SOUND_MODES)[number];

export const ROBOT_SIZES = ["S", "M", "L", "XL"] as const;
export type RobotSize = (typeof ROBOT_SIZES)[number];

export const MIN_VOICE_SPEED = 0.8;
export const MAX_VOICE_SPEED = 1.3;
export const VOICE_SPEED_STEP = 0.05;
export const DEFAULT_VOICE_SPEED = 1;

export interface Settings {
  model: "quick" | "thorough";
  ptt: "hold" | "toggle";
  sound: "calm" | "fun";
  voiceSpeed: number /* 0.8..1.3 */;
  robotSize: "S" | "M" | "L" | "XL";
  reduceMotion: boolean;
  startWithWindows: boolean;
}

export const SETTINGS_KEYS = [
  "model",
  "ptt",
  "sound",
  "voiceSpeed",
  "robotSize",
  "reduceMotion",
  "startWithWindows",
] as const satisfies readonly (keyof Settings)[];

export const DEFAULT_SETTINGS: Readonly<Settings> = Object.freeze({
  model: "quick",
  ptt: "hold",
  sound: "calm",
  voiceSpeed: DEFAULT_VOICE_SPEED,
  robotSize: "M",
  reduceMotion: false,
  startWithWindows: false,
});

/** Returns a fresh mutable copy of `DEFAULT_SETTINGS`. */
export function defaultSettings(): Settings {
  return { ...DEFAULT_SETTINGS };
}

export function isModelChoice(value: unknown): value is ModelChoice {
  return typeof value === "string" && (MODEL_CHOICES as readonly string[]).includes(value);
}

export function isPttMode(value: unknown): value is PttMode {
  return typeof value === "string" && (PTT_MODES as readonly string[]).includes(value);
}

export function isSoundMode(value: unknown): value is SoundMode {
  return typeof value === "string" && (SOUND_MODES as readonly string[]).includes(value);
}

export function isRobotSize(value: unknown): value is RobotSize {
  return typeof value === "string" && (ROBOT_SIZES as readonly string[]).includes(value);
}

export function isValidVoiceSpeed(value: unknown): value is number {
  return (
    typeof value === "number" &&
    Number.isFinite(value) &&
    value >= MIN_VOICE_SPEED &&
    value <= MAX_VOICE_SPEED
  );
}

function isPlainObject(value: unknown): value is Readonly<Record<string, unknown>> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function readOwnField(obj: Readonly<Record<string, unknown>>, key: string): unknown {
  try {
    return Object.hasOwn(obj, key) ? obj[key] : undefined;
  } catch {
    return undefined;
  }
}

/**
 * Strictly validates an untrusted settings payload:
 * - Drops unknown fields (returns only the 7 `Settings` keys).
 * - Replaces missing, wrong-type, or out-of-range fields with their defaults.
 * - Never throws (even on throwing getters or proxies).
 */
export function parseSettings(raw: unknown): Settings {
  try {
    if (!isPlainObject(raw)) {
      return defaultSettings();
    }

    const model = readOwnField(raw, "model");
    const ptt = readOwnField(raw, "ptt");
    const sound = readOwnField(raw, "sound");
    const voiceSpeed = readOwnField(raw, "voiceSpeed");
    const robotSize = readOwnField(raw, "robotSize");
    const reduceMotion = readOwnField(raw, "reduceMotion");
    const startWithWindows = readOwnField(raw, "startWithWindows");

    return {
      model: isModelChoice(model) ? model : DEFAULT_SETTINGS.model,
      ptt: isPttMode(ptt) ? ptt : DEFAULT_SETTINGS.ptt,
      sound: isSoundMode(sound) ? sound : DEFAULT_SETTINGS.sound,
      voiceSpeed: isValidVoiceSpeed(voiceSpeed) ? voiceSpeed : DEFAULT_SETTINGS.voiceSpeed,
      robotSize: isRobotSize(robotSize) ? robotSize : DEFAULT_SETTINGS.robotSize,
      reduceMotion:
        typeof reduceMotion === "boolean" ? reduceMotion : DEFAULT_SETTINGS.reduceMotion,
      startWithWindows:
        typeof startWithWindows === "boolean"
          ? startWithWindows
          : DEFAULT_SETTINGS.startWithWindows,
    };
  } catch {
    return defaultSettings();
  }
}

/**
 * Merges `patch` onto `current` and validates the result through `parseSettings`.
 */
export function updateSettings(current: Settings, patch: Partial<Settings>): Settings {
  return parseSettings({ ...parseSettings(current), ...patch });
}

/**
 * Formats `voiceSpeed` as digits and `×` only (`"1.0×"`, `"0.85×"`), with no words (invariant 7).
 */
export function formatVoiceSpeed(speed: number): string {
  const valid = isValidVoiceSpeed(speed) ? speed : DEFAULT_SETTINGS.voiceSpeed;
  const rounded = Math.round(valid * 100) / 100;
  const text = Number.isInteger(rounded * 10) ? rounded.toFixed(1) : rounded.toFixed(2);
  return `${text}\u00d7`;
}

/** Minimal IPC invoke signature matching `@tauri-apps/api/core`'s `invoke`. */
export type SettingsInvoke = (
  cmd: string,
  args?: Readonly<Record<string, unknown>>,
) => Promise<unknown>;

export interface SettingsStore {
  /** Current in-memory snapshot (always a fresh copy). */
  get(): Settings;
  /** Loads via `invoke("load_settings")` when available, falling back to the in-memory state. */
  load(): Promise<Settings>;
  /** Updates in-memory state and saves via `invoke("save_settings", { settings })`. */
  save(next: unknown): Promise<Settings>;
  /** Applies a partial change and saves immediately. */
  update(patch: Partial<Settings>): Promise<Settings>;
}

/**
 * Creates a settings store backed by `invoke("load_settings")` and
 * `invoke("save_settings", { settings })`, falling back to in-memory defaults outside Tauri.
 */
export function createSettingsStore(invokeFn: SettingsInvoke | null = null): SettingsStore {
  let state: Settings = defaultSettings();
  let modifiedBeforeLoad = false;

  return {
    get(): Settings {
      return { ...state };
    },
    async load(): Promise<Settings> {
      if (!invokeFn) {
        return { ...state };
      }
      try {
        const raw = await invokeFn("load_settings");
        if (!modifiedBeforeLoad) {
          state = parseSettings(raw);
        }
      } catch {
        // Outside Tauri or before the command is registered: keep in-memory state.
      }
      return { ...state };
    },
    async save(next: unknown): Promise<Settings> {
      modifiedBeforeLoad = true;
      state = parseSettings(next);
      const snapshot: Settings = { ...state };
      if (invokeFn) {
        try {
          await invokeFn("save_settings", { settings: snapshot });
        } catch {
          // Keep the validated change in memory even if IPC fails.
        }
      }
      return { ...state };
    },
    async update(patch: Partial<Settings>): Promise<Settings> {
      return this.save({ ...state, ...patch });
    },
  };
}
