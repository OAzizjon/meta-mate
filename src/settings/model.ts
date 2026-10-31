// Pure settings model, validation, and in-memory store (WP24).
// No DOM, i18n JSON, or Tauri imports so Node (`node --test`) runs this module directly.

export const MODEL_CHOICES = ["quick", "thorough"] as const;
export type ModelChoice = (typeof MODEL_CHOICES)[number];

export const PTT_MODES = ["hold", "toggle"] as const;
export type PttMode = (typeof PTT_MODES)[number];

export const PTT_KEYS = ["rightCtrl", "rightAlt", "capsLock", "insert", "f9", "f10"] as const;
export type PttKey = (typeof PTT_KEYS)[number];

export function isPttKey(value: unknown): value is PttKey {
  return typeof value === "string" && (PTT_KEYS as readonly string[]).includes(value);
}

export const SOUND_MODES = ["calm", "fun"] as const;
export type SoundMode = (typeof SOUND_MODES)[number];

export const ROBOT_SIZES = ["S", "M", "L", "XL"] as const;
export type RobotSize = (typeof ROBOT_SIZES)[number];

export const MIN_VOICE_SPEED = 0.8;
export const MAX_VOICE_SPEED = 1.3;
export const VOICE_SPEED_STEP = 0.05;
export const DEFAULT_VOICE_SPEED = 1;
export const SETTINGS_CHANGED_EVENT = "settings-changed" as const;

export type Language = "auto" | "en" | "ru";

export interface Settings {
  language: Language;
  model: "quick" | "thorough";
  ptt: "hold" | "toggle";
  pttKey: PttKey;
  sound: "calm" | "fun";
  voiceSpeed: number /* 0.8..1.3 */;
  robotSize: "S" | "M" | "L" | "XL";
  reduceMotion: boolean;
  spotlight: boolean;
  startWithWindows: boolean;
  showHomeAtStart: boolean;
}

export const SETTINGS_KEYS = [
  "language",
  "model",
  "ptt",
  "pttKey",
  "sound",
  "voiceSpeed",
  "robotSize",
  "reduceMotion",
  "spotlight",
  "startWithWindows",
  "showHomeAtStart",
] as const satisfies readonly (keyof Settings)[];

export const DEFAULT_SETTINGS: Readonly<Settings> = Object.freeze({
  language: "auto",
  model: "quick",
  ptt: "hold",
  pttKey: "rightCtrl",
  sound: "calm",
  voiceSpeed: DEFAULT_VOICE_SPEED,
  robotSize: "M",
  reduceMotion: false,
  spotlight: true,
  startWithWindows: false,
  showHomeAtStart: true,
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

/**
 * Validates `value` against `MIN_VOICE_SPEED..MAX_VOICE_SPEED` and snaps it to the nearest
 * `VOICE_SPEED_STEP` (0.05), rounded to 2 decimal places; out-of-range or non-finite values
 * fall back to `DEFAULT_SETTINGS.voiceSpeed`.
 */
export function snapVoiceSpeed(value: unknown): number {
  if (!isValidVoiceSpeed(value)) {
    return DEFAULT_SETTINGS.voiceSpeed;
  }
  const stepped = Math.round(value / VOICE_SPEED_STEP) * VOICE_SPEED_STEP;
  return Math.round(stepped * 100) / 100;
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
 * - Drops unknown fields (returns only `Settings` keys).
 * - Replaces missing, wrong-type, or out-of-range fields with their defaults.
 * - Snaps `voiceSpeed` to `VOICE_SPEED_STEP` (0.05), rounded to 2 decimals.
 * - Never throws (even on throwing getters or proxies).
 */
export function parseSettings(raw: unknown): Settings {
  try {
    if (!isPlainObject(raw)) {
      return defaultSettings();
    }

    const language = readOwnField(raw, "language");
    const model = readOwnField(raw, "model");
    const ptt = readOwnField(raw, "ptt");
    const pttKey = readOwnField(raw, "pttKey");
    const sound = readOwnField(raw, "sound");
    const voiceSpeed = readOwnField(raw, "voiceSpeed");
    const robotSize = readOwnField(raw, "robotSize");
    const spotlight = readOwnField(raw, "spotlight");
    const reduceMotion = readOwnField(raw, "reduceMotion");
    const startWithWindows = readOwnField(raw, "startWithWindows");
    const showHomeAtStart = readOwnField(raw, "showHomeAtStart");

    return {
      language: language === "en" || language === "ru" ? language : "auto",
      model: isModelChoice(model) ? model : DEFAULT_SETTINGS.model,
      ptt: isPttMode(ptt) ? ptt : DEFAULT_SETTINGS.ptt,
      pttKey: isPttKey(pttKey) ? pttKey : DEFAULT_SETTINGS.pttKey,
      sound: isSoundMode(sound) ? sound : DEFAULT_SETTINGS.sound,
      voiceSpeed: snapVoiceSpeed(voiceSpeed),
      robotSize: isRobotSize(robotSize) ? robotSize : DEFAULT_SETTINGS.robotSize,
      reduceMotion:
        typeof reduceMotion === "boolean" ? reduceMotion : DEFAULT_SETTINGS.reduceMotion,
      spotlight: typeof spotlight === "boolean" ? spotlight : DEFAULT_SETTINGS.spotlight,
      startWithWindows:
        typeof startWithWindows === "boolean"
          ? startWithWindows
          : DEFAULT_SETTINGS.startWithWindows,
      showHomeAtStart:
        typeof showHomeAtStart === "boolean" ? showHomeAtStart : DEFAULT_SETTINGS.showHomeAtStart,
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
  const rounded = snapVoiceSpeed(speed);
  const text = Number.isInteger(rounded * 10) ? rounded.toFixed(1) : rounded.toFixed(2);
  return `${text}\u00d7`;
}

export type VoiceSpeedPace = "slower" | "usual" | "faster";

/** Whether `voiceSpeed` (snapped like `formatVoiceSpeed`) is below, at or above the usual speed. */
export function voiceSpeedPace(speed: number): VoiceSpeedPace {
  const snapped = snapVoiceSpeed(speed);
  if (snapped < DEFAULT_VOICE_SPEED) return "slower";
  if (snapped > DEFAULT_VOICE_SPEED) return "faster";
  return "usual";
}

/** Minimal IPC invoke signature matching `@tauri-apps/api/core`'s `invoke`. */
export type SettingsInvoke = (
  cmd: string,
  args?: Readonly<Record<string, unknown>>,
) => Promise<unknown>;

/** Subscribes to external `settings-changed` payloads and resolves to an unlisten callback. */
export type SettingsSubscribe = (
  onChanged: (raw: unknown) => void,
) => Promise<() => void>;

export interface SettingsStore {
  /** Current in-memory snapshot (always a fresh copy). */
  get(): Settings;
  /** Loads via `invoke("load_settings")` when available, falling back to the in-memory state. */
  load(): Promise<Settings>;
  /** Updates in-memory state and saves via `invoke("save_settings", { settings })`. */
  save(next: unknown): Promise<Settings>;
  /** Applies a partial change and saves immediately. */
  update(patch: Partial<Settings>): Promise<Settings>;
  /** Replaces in-memory state from an external `settings-changed` event without invoking `save_settings`. */
  replace(next: unknown): Settings;
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
        await invokeFn("save_settings", { settings: snapshot });
      }
      return { ...state };
    },
    async update(patch: Partial<Settings>): Promise<Settings> {
      return this.save({ ...state, ...patch });
    },
    replace(next: unknown): Settings {
      modifiedBeforeLoad = true;
      state = parseSettings(next);
      return { ...state };
    },
  };
}

/**
 * Listens for external `settings-changed` events via `subscribeFn`, updates `store` in memory
 * without echoing `save_settings`, and invokes `onRefresh` with the validated settings.
 * Returns an unsubscribe function that stops forwarding events.
 */
export function subscribeSettingsChanged(
  store: SettingsStore,
  subscribeFn: SettingsSubscribe | null,
  onRefresh?: (settings: Settings) => void,
): () => void {
  if (!subscribeFn) {
    return () => {};
  }
  let active = true;
  let unlisten: (() => void) | null = null;

  void subscribeFn((raw) => {
    if (!active) return;
    const updated = store.replace(raw);
    onRefresh?.(updated);
  })
    .then((stop) => {
      if (!active) {
        stop();
      } else {
        unlisten = stop;
      }
    })
    .catch(() => {
      // Outside Tauri or when event listening is unavailable: keep local state.
    });

  return () => {
    active = false;
    unlisten?.();
    unlisten = null;
  };
}
