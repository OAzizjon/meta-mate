// The voice bar's logic (src/highlight/voicebar-view.ts draws it): the `"voice-bar"` payloads
// from Rust, the bars' quick rise and slow fall, their heights from a short level history, and
// the newest words heard that fit on two lines. No DOM or Tauri imports, so Node tests run this
// module directly. The words are the person's own: nothing here logs them.

/** Name of the Tauri event that drives the voice bar. */
export const VOICE_BAR_EVENT = "voice-bar";
export const VOICE_BAR_STATES = ["listening", "thinking", "idle"] as const;
export type VoiceBarState = (typeof VOICE_BAR_STATES)[number];

export type VoiceBarMsg =
  | { readonly type: "state"; readonly state: VoiceBarState }
  | { readonly type: "level"; readonly level: number }
  | { readonly type: "heard"; readonly text: string };

/** Bars in the waveform. */
export const BAR_COUNT = 28;
/** The shortest bar as a share of the tallest: never a dead flat line. */
export const BAR_MIN = 0.08;
/** Levels kept: the newest in the middle, one older per bar toward both ends. */
export const HISTORY_SIZE = Math.ceil(BAR_COUNT / 2);
/** One level enters the history every this many ms. */
export const HISTORY_STEP_MS = 50;
/** Time constants (ms): the bars rise quickly and fall slowly. */
export const ATTACK_MS = 35;
export const RELEASE_MS = 220;
/** Thinking: the travelling shimmer, then the fade, gone after this many ms. */
export const THINKING_MS = 1500;
/** Fade-out (ms). */
export const FADE_MS = 250;
/** Rust sends at most this many characters of what was heard. */
export const MAX_HEARD_CHARS = 200;
/** Lines of words shown above the bars. */
export const HEARD_LINES = 2;
/** The fewest characters a tail may hold. */
export const MIN_TAIL_CHARS = 12;

const ELLIPSIS = "…";
// Gain-normalized RMS: silence is about 0.005, speech about 0.05 to 0.6.
const NOISE_FLOOR = 0.01;
const SPEECH_PEAK = 0.6;
const MAX_BARS = 64;
const SHIMMER = 0.07;
const SHIMMER_PERIOD_MS = 1400;
const THINKING_HUMP = 0.22;
const TRAVEL_MS = 1100;
// An average glyph is about half an em; wrapping at word ends wastes some of each line.
const GLYPH_EM = 0.55;
const WRAP_SHARE = 0.85;

type JsonObject = Readonly<Record<string, unknown>>;

/**
 * Validates a `"voice-bar"` payload and returns a fresh copy, or `null` when anything is off
 * (unknown type or state, missing or extra key, a level outside 0..1, more than 200 characters).
 * Never throws.
 */
export function parseVoiceBarMsg(raw: unknown): VoiceBarMsg | null {
  try {
    return parseMsg(raw);
  } catch {
    // A throwing getter or proxy is malformed input too.
    return null;
  }
}

function parseMsg(raw: unknown): VoiceBarMsg | null {
  if (typeof raw !== "object" || raw === null || Array.isArray(raw)) return null;
  const msg = raw as JsonObject;
  const { type } = msg;
  if (type === "state") {
    const { state } = msg;
    return exact(msg, ["type", "state"]) && typeof state === "string" &&
      (VOICE_BAR_STATES as readonly string[]).includes(state)
      ? { type, state: state as VoiceBarState } : null;
  }
  if (type === "level") {
    const { level } = msg;
    return exact(msg, ["type", "level"]) && typeof level === "number" &&
      Number.isFinite(level) && level >= 0 && level <= 1
      ? { type, level: level === 0 ? 0 : level } : null;
  }
  if (type === "heard") {
    const { text } = msg;
    // Rust counts chars: a character outside the BMP counts once.
    return exact(msg, ["type", "text"]) && typeof text === "string" &&
      text.length <= MAX_HEARD_CHARS * 2 && Array.from(text).length <= MAX_HEARD_CHARS
      ? { type, text } : null;
  }
  return null;
}

function exact(msg: JsonObject, keys: readonly string[]): boolean {
  const own = Object.keys(msg);
  return own.length === keys.length && own.every((key) => keys.includes(key));
}

function clamp01(value: number): number {
  return Number.isFinite(value) ? Math.min(1, Math.max(0, value)) : 0;
}

/** The microphone level as a bar height 0..1: silence 0, loud speech 1, quiet speech visible. */
export function loudness(level: number): number {
  return Math.sqrt(clamp01((level - NOISE_FLOOR) / (SPEECH_PEAK - NOISE_FLOOR)));
}

/**
 * `current` one step of `dtMs` toward `target` (both 0..1): rising with the `attackMs` time
 * constant, falling with `releaseMs`. Never overshoots.
 */
export function smoothLevel(
  current: number,
  target: number,
  dtMs: number,
  attackMs: number = ATTACK_MS,
  releaseMs: number = RELEASE_MS,
): number {
  const from = clamp01(current);
  const to = clamp01(target);
  if (!(dtMs > 0)) return from;
  const tau = to > from ? attackMs : releaseMs;
  if (!(tau > 0)) return to;
  return to + (from - to) * Math.exp(-dtMs / tau);
}

/** The history (oldest first) with `value` added, keeping only the newest `size`; a fresh array. */
export function pushLevel(
  history: readonly number[],
  value: number,
  size: number = HISTORY_SIZE,
): number[] {
  const next = [...history, clamp01(value)];
  const keep = Number.isInteger(size) && size > 0 ? size : HISTORY_SIZE;
  return next.length > keep ? next.slice(next.length - keep) : next;
}

function barCount(bars: number): number {
  return Number.isInteger(bars) && bars > 0 ? Math.min(bars, MAX_BARS) : BAR_COUNT;
}

/**
 * Heights (BAR_MIN..1) of `bars` bars for a level history (oldest first): the newest level in
 * the middle, older ones toward both ends, the middle a little taller. With `shimmer` a faint
 * wave travels through them at `timeMs`, so silence never looks dead; reduced motion leaves it
 * out.
 */
export function barHeights(
  history: readonly number[],
  bars: number,
  timeMs: number,
  shimmer: boolean,
): number[] {
  const n = barCount(bars);
  const mid = (n - 1) / 2;
  const phase = Number.isFinite(timeMs) ? (2 * Math.PI * timeMs) / SHIMMER_PERIOD_MS : 0;
  const heights: number[] = [];
  for (let i = 0; i < n; i++) {
    const distance = Math.abs(i - mid);
    const level = clamp01(history[history.length - 1 - Math.floor(distance)] ?? 0);
    const envelope = mid > 0 ? 1 - 0.45 * (distance / mid) ** 2 : 1;
    // A fixed texture per bar, so the two halves are not mirror images.
    const texture = 0.85 + 0.15 * Math.sin(i * 2.3 + 1);
    let height = level * envelope * texture;
    if (shimmer) height += SHIMMER * (0.5 + 0.5 * Math.sin(phase - i * 0.5));
    heights.push(BAR_MIN + (1 - BAR_MIN) * clamp01(height));
  }
  return heights;
}

/** Thinking: low bars with one calm hump travelling left to right; flat without `shimmer`. */
export function thinkingHeights(bars: number, elapsedMs: number, shimmer: boolean): number[] {
  const n = barCount(bars);
  if (!shimmer) return new Array<number>(n).fill(BAR_MIN);
  const width = Math.max(1.5, n / 8);
  const t = Number.isFinite(elapsedMs) && elapsedMs > 0 ? elapsedMs : 0;
  const center = ((t % TRAVEL_MS) / TRAVEL_MS) * (n - 1 + 4 * width) - 2 * width;
  return Array.from({ length: n }, (_, i) =>
    BAR_MIN + THINKING_HUMP * Math.exp(-(((i - center) / width) ** 2)));
}

/** Every bar one `smoothLevel` step toward its target; a fresh array as long as `targets`. */
export function stepBars(
  current: readonly number[],
  targets: readonly number[],
  dtMs: number,
): number[] {
  return targets.map((target, i) => smoothLevel(current[i] ?? BAR_MIN, target, dtMs));
}

/** About how many characters fit on `lines` lines `widthPx` wide at `fontPx`. */
export function tailBudget(widthPx: number, fontPx: number, lines: number = HEARD_LINES): number {
  if (!(widthPx > 0) || !(fontPx > 0) || !(lines > 0)) return MIN_TAIL_CHARS;
  const perLine = Math.floor(widthPx / (fontPx * GLYPH_EM));
  const budget = Math.floor(perLine * lines * WRAP_SHARE);
  return Math.min(MAX_HEARD_CHARS, Math.max(MIN_TAIL_CHARS, budget));
}

/**
 * The newest words of `text` in at most `maxChars` characters, whitespace collapsed. When older
 * words are cut, the tail starts at a whole word (unless one word fills it) after an ellipsis.
 */
export function visibleTail(text: string, maxChars: number): string {
  const words = text.replace(/\s+/gu, " ").trim();
  const chars = Array.from(words);
  const max = Number.isFinite(maxChars) ? Math.max(2, Math.floor(maxChars)) : MAX_HEARD_CHARS;
  if (chars.length <= max) return words;
  const start = chars.length - (max - 1);
  let tail = chars.slice(start);
  if (chars[start - 1] !== " ") {
    const space = tail.indexOf(" ");
    if (space >= 0) tail = tail.slice(space + 1);
  }
  return ELLIPSIS + tail.join("");
}
