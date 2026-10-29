// Rust → Mate's WebView: the one ordered Tauri channel of `src-tauri/src/answer_stream.rs`
// (`AnswerMsg`, serde internal tagging on `type`, camelCase fields). It carries the robot's
// state, the mic level while listening, the answer's timeline items, its speech PCM, barge-in
// cancels and locale lines.
// Tauri is loaded only when subscribing, so Node tests can import the parser. Spoken and shown
// text never goes to the log.

import type { LocaleKey, LocaleCode } from "../i18n/index.ts";
import { parseTimelineItem } from "./timeline.ts";
import type { TimelineItem } from "./timeline.ts";

/** The robot states Rust sets directly (the timeline sets the others). */
export const ANSWER_STATES = ["listening", "looking", "thinking", "idle"] as const;
export type AnswerState = (typeof ANSWER_STATES)[number];

export const ANSWER_MODELS = [
  "muse-spark-1.3", "muse-wonder-1.0", "sam-3.1", "muse-voice-transcribe-1.0",
] as const;
export type AnswerModel = (typeof ANSWER_MODELS)[number];

/** The locale lines Rust may show in the bubble (it may send their audio as sentence 0). */
export const ANSWER_LINE_KEYS = [
  "robot.network_error",
  "robot.cant_find",
  "robot.listening",
  "robot.click_stop",
  // The look and pointing (src-tauri/src/screen.rs LookFailure::line, pointing.rs).
  "robot.cant_look",
  "robot.protected_window",
  "robot.other_monitor",
  "robot.window_closed",
  // The teacher (src-tauri/src/teacher.rs line_key) and its idle limit.
  "robot.click_app_first",
  "robot.wrong_place",
  "robot.right_click",
  "robot.right_click_2",
  "robot.right_click_3",
  "robot.right_click_4",
  "robot.done",
  "robot.too_many_turns",
  "robot.wont_press",
  // "Do it for me" (teacher.rs line_key).
  "robot.do_it_for_me",
  "robot.pressing",
  "robot.press_cancelled",
  "robot.press_failed",
  "robot.sleep",
] as const satisfies readonly LocaleKey[];
export type AnswerLineKey = (typeof ANSWER_LINE_KEYS)[number];

/** `AnswerMsg`, with the timeline item already validated. */
export type AnswerMsg =
  | { readonly type: "model"; readonly generation: number; readonly model: AnswerModel | null }
  | { readonly type: "state"; readonly generation: number; readonly state: AnswerState }
  | { readonly type: "level"; readonly generation: number; readonly level: number }
  | { readonly type: "heard"; readonly generation: number; readonly text: string }
  | { readonly type: "item"; readonly generation: number; readonly item: TimelineItem }
  | {
      readonly type: "pcm";
      readonly generation: number;
      /** 0 is a cached phrase outside the timeline. */
      readonly sentence: number;
      readonly sampleRate: number;
      /** An empty chunk with `eos` ends a sentence that has no audio. */
      readonly eos: boolean;
      /** PCM16LE mono; `decodePcm` turns it into samples. */
      readonly samplesB64: string;
    }
  | { readonly type: "cancel"; readonly generation: number }
  | { readonly type: "line"; readonly generation: number; readonly key: AnswerLineKey };

type AnswerType = AnswerMsg["type"];
type JsonObject = Readonly<Record<string, unknown>>;

/** The exact keys of each message on the wire. */
const KEYS: Readonly<Record<AnswerType, readonly string[]>> = {
  model: ["type", "generation", "model"],
  state: ["type", "generation", "state"],
  level: ["type", "generation", "level"],
  heard: ["type", "generation", "text"],
  item: ["type", "generation", "item"],
  pcm: ["type", "generation", "sentence", "sampleRate", "eos", "samplesB64"],
  cancel: ["type", "generation"],
  line: ["type", "generation", "key"],
};

const MIN_SAMPLE_RATE = 8000;
const MAX_SAMPLE_RATE = 48_000;

/**
 * Checks one channel message against the wire format and returns a fresh copy, or null when
 * anything is off (unknown type or state, missing or extra key, wrong type, out-of-range
 * number, PCM that is not base64 of whole 16-bit samples). Never throws.
 */
export function parseAnswerMsg(raw: unknown): AnswerMsg | null {
  try {
    return parseMsg(raw);
  } catch {
    // A throwing getter or proxy is malformed input too.
    return null;
  }
}

function parseMsg(raw: unknown): AnswerMsg | null {
  const type = typeOf(raw);
  if (!type) return null;
  const msg = exact(raw, KEYS[type]);
  if (!msg) return null;
  const { generation } = msg;
  if (!isId(generation)) return null;
  switch (type) {
    case "model": {
      const { model } = msg;
      return model === null || (typeof model === "string" && (ANSWER_MODELS as readonly string[]).includes(model))
        ? { type, generation, model: model as AnswerModel | null }
        : null;
    }
    case "state": {
      const { state } = msg;
      return isAnswerState(state) ? { type, generation, state } : null;
    }
    case "level": {
      const { level } = msg;
      return isLevel(level) ? { type, generation, level: level === 0 ? 0 : level } : null;
    }
    case "heard": {
      const { text } = msg;
      // Match Rust's chars() limit, including characters outside the BMP.
      return typeof text === "string" && text.length <= 400 && Array.from(text).length <= 200
        ? { type, generation, text }
        : null;
    }
    case "item": {
      const item = parseTimelineItem(msg.item);
      return item ? { type, generation, item } : null;
    }
    case "pcm": {
      const { sentence, sampleRate, eos, samplesB64 } = msg;
      return isId(sentence) &&
        isSampleRate(sampleRate) &&
        typeof eos === "boolean" &&
        typeof samplesB64 === "string" &&
        isPcm16(samplesB64)
        ? { type, generation, sentence, sampleRate, eos, samplesB64 }
        : null;
    }
    case "cancel":
      return { type, generation };
    case "line": {
      const { key } = msg;
      return isAnswerLineKey(key) ? { type, generation, key } : null;
    }
    default:
      return null;
  }
}

/**
 * The PCM16LE samples of a `pcm` message. Throws a TypeError on anything `parseAnswerMsg`
 * would reject (not canonical base64, or an odd byte count).
 */
export function decodePcm(b64: string): Int16Array {
  if (!isPcm16(b64)) throw new TypeError("not base64 PCM16");
  const length = base64ByteLength(b64);
  const chars = b64.length - padding(b64);
  // Past the last character (a short final quad) the bits are zero.
  const at = (index: number): number => (index < chars ? sextet(b64, index) : 0);
  const bytes = new Uint8Array(length);
  for (let i = 0, out = 0; i < chars; i += 4) {
    const quad = (at(i) << 18) | (at(i + 1) << 12) | (at(i + 2) << 6) | at(i + 3);
    // Uint8Array keeps the low 8 bits.
    bytes[out++] = quad >> 16;
    if (out < length) bytes[out++] = quad >> 8;
    if (out < length) bytes[out++] = quad;
  }
  const samples = new Int16Array(length / 2);
  for (let i = 0; i < samples.length; i++) {
    // Int16Array stores modulo 2^16, so 0x8000..0xffff become negative.
    samples[i] = bytes[2 * i] | (bytes[2 * i + 1] << 8);
  }
  return samples;
}

/** Connects one subscription and hands every raw message to `onRaw` (tests pass a fake). */
export type AnswerConnect = (locale: LocaleCode, onRaw: (raw: unknown) => void) => Promise<void>;

/**
 * Subscribes Mate's window to the answer stream; `locale` is the UI language, the voice of
 * Rust's cached phrases. Malformed messages are dropped. The returned function makes this
 * subscription ignore further messages (a new subscription replaces the channel in Rust).
 */
export async function subscribeAnswerStream(
  locale: LocaleCode,
  onMessage: (msg: AnswerMsg) => void,
  connect: AnswerConnect = connectTauri,
): Promise<() => void> {
  let active = true;
  try {
    await connect(locale, (raw) => {
      if (!active) return;
      const msg = parseAnswerMsg(raw);
      if (!msg) {
        // Names a known type at most: the payload stays out of the log.
        console.warn(`[answer-stream] dropped a malformed ${typeOf(raw) ?? "unknown"} message`);
        void reportPlayback("dropped", 0, typeOf(raw) ?? "unknown");
        return;
      }
      try {
        onMessage(msg);
      } catch (error) {
        // Tauri's channel stops delivering after a throwing handler: never let one escape.
        console.error(`[answer-stream] ${msg.type} handler failed`, error);
        void reportPlayback("failed", 0, msg.type);
      }
    });
  } catch (error) {
    active = false;
    throw error;
  }
  return () => {
    active = false;
  };
}

/** What the window tells Rust about playback: the audio engine's state, an answer played out. */
export type PlaybackReport =
  | "audio_running"
  | "audio_suspended"
  | "done"
  | "pcm"
  | "dropped"
  | "failed"
  | "trace";

/** The target in Mate's window, CSS pixels, as `point_at` answers (null: not shown). */
export interface MateRect {
  readonly left: number;
  readonly top: number;
  readonly right: number;
  readonly bottom: number;
}

/**
 * Asks Rust to show `point` of answer `generation`: the highlight shines on it and Mate
 * flies beside it. Null when it can't be shown, or outside Tauri.
 */
export async function pointAt(
  generation: number,
  point: unknown,
  step: { readonly number: number; readonly total: number } | null,
): Promise<MateRect | null> {
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    const rect = await invoke<MateRect | null>("point_at", { generation, point, step });
    const finite = (v: unknown): v is number => typeof v === "number" && Number.isFinite(v);
    if (rect && finite(rect.left) && finite(rect.top) && finite(rect.right) && finite(rect.bottom)) {
      return { left: rect.left, top: rect.top, right: rect.right, bottom: rect.bottom };
    }
    return null;
  } catch {
    return null;
  }
}

/** Best effort: a report that cannot be delivered is dropped. */
export async function reportPlayback(
  event: PlaybackReport,
  generation: number,
  detail = "",
): Promise<void> {
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    await invoke("report_playback", { event, generation, detail });
  } catch {
    // Outside Tauri, or the command is gone: nothing to tell.
  }
}

async function connectTauri(_locale: LocaleCode, onRaw: (raw: unknown) => void): Promise<void> {
  const { Channel, invoke } = await import("@tauri-apps/api/core");
  const channel = new Channel<unknown>();
  channel.onmessage = onRaw;
  await invoke("subscribe_answer_stream", {
    channel,
    // Keep Windows' language separately so switching back to automatic works immediately.
    locale: typeof navigator !== "undefined" && navigator.language.toLowerCase().startsWith("ru") ? "ru" : "en",
  });
}

/** The message's `type` when it is a known one. */
function typeOf(raw: unknown): AnswerType | null {
  try {
    if (typeof raw !== "object" || raw === null || Array.isArray(raw)) return null;
    const { type } = raw as JsonObject;
    return typeof type === "string" && Object.hasOwn(KEYS, type) ? (type as AnswerType) : null;
  } catch {
    return null;
  }
}

/** A plain JSON object whose own keys are exactly `keys` (in any order). */
function exact(json: unknown, keys: readonly string[]): JsonObject | null {
  if (typeof json !== "object" || json === null || Array.isArray(json)) return null;
  const own = Object.keys(json);
  return own.length === keys.length && own.every((key) => keys.includes(key))
    ? (json as JsonObject)
    : null;
}

/** u64 as a JSON number: only the exactly representable range. */
function isId(value: unknown): value is number {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
}

function isSampleRate(value: unknown): value is number {
  return (
    typeof value === "number" &&
    Number.isInteger(value) &&
    value >= MIN_SAMPLE_RATE &&
    value <= MAX_SAMPLE_RATE
  );
}

function isAnswerState(value: unknown): value is AnswerState {
  return typeof value === "string" && (ANSWER_STATES as readonly string[]).includes(value);
}

function isLevel(value: unknown): value is number {
  return typeof value === "number" && Number.isFinite(value) && value >= 0 && value <= 1;
}

function isAnswerLineKey(value: unknown): value is AnswerLineKey {
  return typeof value === "string" && (ANSWER_LINE_KEYS as readonly string[]).includes(value);
}

const ALPHABET = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
const EQUALS = 0x3d;

/** Sextet value per ASCII code, -1 outside the standard alphabet. */
const SEXTETS = new Int8Array(128).fill(-1);
for (let i = 0; i < ALPHABET.length; i++) SEXTETS[ALPHABET.charCodeAt(i)] = i;

/** The value of the character at `index`, -1 when it is outside the alphabet (`=` too). */
function sextet(b64: string, index: number): number {
  const code = b64.charCodeAt(index);
  return code < SEXTETS.length ? SEXTETS[code] : -1;
}

/** Trailing `=` characters (at most two count). */
function padding(b64: string): number {
  let pad = 0;
  while (pad < 2 && pad < b64.length && b64.charCodeAt(b64.length - 1 - pad) === EQUALS) pad++;
  return pad;
}

/**
 * The bytes `b64` decodes to, or -1 unless it is canonical standard base64 (RFC 4648 alphabet,
 * no whitespace, `=` padding optional but complete when present, zero unused bits).
 */
function base64ByteLength(b64: string): number {
  const pad = padding(b64);
  const chars = b64.length - pad;
  if ((pad > 0 && b64.length % 4 !== 0) || chars % 4 === 1) return -1;
  for (let i = 0; i < chars; i++) {
    if (sextet(b64, i) < 0) return -1;
  }
  const rest = chars % 4;
  if (rest > 0 && (sextet(b64, chars - 1) & (rest === 2 ? 0x0f : 0x03)) !== 0) return -1;
  return Math.floor((chars * 3) / 4);
}

/** Canonical base64 of whole 16-bit samples (an empty string is zero samples). */
function isPcm16(b64: string): boolean {
  const length = base64ByteLength(b64);
  return length >= 0 && length % 2 === 0;
}
