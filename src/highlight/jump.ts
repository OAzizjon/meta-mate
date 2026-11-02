// Mate's surprise entrance as pure data (src/highlight/jump-view.ts plays it): the `"mate-jump"`
// payload from Rust, the cues of the under-3-second timeline and the screen shake. No DOM or
// Tauri imports, so Node tests run this module directly.

/** Name of the Tauri event that plays the entrance. */
export const MATE_JUMP_EVENT = "mate-jump";
export const JUMP_LOCALES = ["en", "ru"] as const;
export type JumpLocale = (typeof JUMP_LOCALES)[number];

export interface JumpMsg {
  readonly type: "jump";
  readonly locale: JumpLocale;
}

export type JumpCue = "enter" | "flash" | "shake" | "balloon" | "line" | "wink" | "exit" | "done";

export interface JumpStep {
  /** Ms after the start. */
  readonly at: number;
  readonly cue: JumpCue;
}

export interface ShakeOffset {
  readonly x: number;
  readonly y: number;
}

/** The leap from below the bottom edge, squash and stretch included (ms). */
export const JUMP_LEAP_MS = 380;
/** The bulb's flash (ms). */
export const JUMP_FLASH_MS = 320;
/** The screen shake (ms) and its strongest offset (CSS px). */
export const JUMP_SHAKE_MS = 300;
export const JUMP_SHAKE_PX = 7;
/** A balloon pops in over this long (ms). */
export const JUMP_POP_MS = 240;
/** He shrinks and drops out of the screen over this long (ms). */
export const JUMP_EXIT_MS = 550;
/** Fades: everything with reduced motion, the balloons and the rays otherwise (ms). */
export const JUMP_FADE_MS = 250;
/** The whole entrance ends by then (ms). */
export const JUMP_MAX_MS = 3000;

const WINK_AT = 1600;
const EXIT_AT = 2000;

// Leap: the flash and the shake as he lands. Reduced motion: no flash, no shake, only fades.
const TIMELINE: readonly JumpStep[] = [
  { at: 0, cue: "enter" },
  { at: 200, cue: "flash" },
  { at: 230, cue: "shake" },
  { at: 300, cue: "balloon" },
  { at: 750, cue: "line" },
  { at: WINK_AT, cue: "wink" },
  { at: EXIT_AT, cue: "exit" },
  { at: EXIT_AT + JUMP_EXIT_MS + 50, cue: "done" },
];
const REDUCED_TIMELINE: readonly JumpStep[] = [
  { at: 0, cue: "enter" },
  { at: JUMP_FADE_MS, cue: "balloon" },
  { at: 750, cue: "line" },
  { at: WINK_AT, cue: "wink" },
  { at: EXIT_AT, cue: "exit" },
  { at: EXIT_AT + JUMP_FADE_MS + 50, cue: "done" },
];

// A decaying jolt, as shares of the strongest offset.
const SHAKE_PATTERN: readonly (readonly [number, number])[] = [
  [0, 0],
  [-1, 0.45],
  [0.9, -0.6],
  [-0.7, 0.4],
  [0.5, -0.3],
  [-0.25, 0.15],
  [0, 0],
];

/**
 * Validates a `"mate-jump"` payload (`{"type":"jump","locale":"en"|"ru"}`, nothing else) and
 * returns a fresh copy, or `null`. Never throws.
 */
export function parseJumpMsg(raw: unknown): JumpMsg | null {
  try {
    if (typeof raw !== "object" || raw === null || Array.isArray(raw)) return null;
    const msg = raw as Readonly<Record<string, unknown>>;
    const own = Object.keys(msg);
    if (own.length !== 2 || !own.includes("type") || !own.includes("locale")) return null;
    const { type, locale } = msg;
    return type === "jump" && typeof locale === "string" &&
      (JUMP_LOCALES as readonly string[]).includes(locale)
      ? { type, locale: locale as JumpLocale } : null;
  } catch {
    // A throwing getter or proxy is malformed input too.
    return null;
  }
}

/** The entrance's cues in time order; with `reduced` motion, fades only. */
export function jumpTimeline(reduced: boolean): readonly JumpStep[] {
  return reduced ? REDUCED_TIMELINE : TIMELINE;
}

/** The shake's offsets (CSS px), from rest back to rest, the strongest `px`. */
export function shakeOffsets(px: number = JUMP_SHAKE_PX): readonly ShakeOffset[] {
  const strength = Number.isFinite(px) && px > 0 ? px : JUMP_SHAKE_PX;
  return SHAKE_PATTERN.map(([x, y]) => ({ x: x * strength, y: y * strength }));
}
