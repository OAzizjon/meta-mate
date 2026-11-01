// Mate's on-screen hints (owner request, 2026-11-01): which hint, when, and what Mate remembers
// about them. src/hints/view.ts draws them. Pure: no DOM, timers or Tauri, so `node --test`
// runs it directly.
//
// - consent: while the look-consent question is in the bubble, a keyboard with Fn and F8 lit
//   and "or click me". Mate's 220 px window cannot hold the picture and the question's text at a
//   readable size together, so they take turns (`consentPhase`): the picture first, which a
//   glance understands, then the text for its reading time.
// - talk: until the person has asked three questions, after 20 s with nothing on the window:
//   the talk key pressing down, "Hold & talk", and "or click me". At most once in 2 minutes.
// - drag: once ever, at the first quiet moment after an answer ends: "Drag me anywhere".
import type { LocaleKey } from "../i18n/index.ts";
import type { StorageLike } from "../onboarding.ts";

export type HintKind = "consent" | "talk" | "drag";
/** The hints the policy schedules; the consent hint follows the bubble instead. */
export type TimedHint = Exclude<HintKind, "consent">;

/** Nothing on the window this long before the talk hint. */
export const TALK_IDLE_MS = 20_000;
/** At most one talk hint in this long. */
export const TALK_COOLDOWN_MS = 120_000;
/** How long the talk hint stays. */
export const TALK_SHOW_MS = 6_000;
/** After this many questions the talk hint never comes back. */
export const TALK_QUESTIONS = 3;
/** How long the drag hint stays (once ever). */
export const DRAG_SHOW_MS = 4_000;
/** The consent picture's turn, before the question's text has its own. */
export const CONSENT_PICTURE_MS = 6_000;

export const HINT_ASKED_KEY = "metamate.hints.asked";
export const HINT_DRAG_KEY = "metamate.hints.drag";

/** Every word the hints show (invariant 7); `hints.talk_*` follows the push-to-talk mode. */
export const HINT_KEYS = [
  "hints.key_fn",
  "hints.key_f8",
  "hints.f8_alone",
  "hints.click_me",
  "hints.talk_hold",
  "hints.talk_press",
  "hints.drag",
] as const satisfies readonly LocaleKey[];

/** What Mate remembers across launches. */
export interface HintMemory {
  /** Questions asked so far, at most `TALK_QUESTIONS`. */
  readonly asked: number;
  readonly dragShown: boolean;
}

/** Storage that is missing or throws remembers nothing: the hints come back next launch. */
export function loadHintMemory(storage: StorageLike | null): HintMemory {
  try {
    const asked = Number(storage?.getItem(HINT_ASKED_KEY) ?? 0);
    return {
      asked: Number.isSafeInteger(asked) && asked > 0 ? Math.min(asked, TALK_QUESTIONS) : 0,
      dragShown: storage?.getItem(HINT_DRAG_KEY) === "1",
    };
  } catch {
    return { asked: 0, dragShown: false };
  }
}

function saveHintMemory(storage: StorageLike | null, memory: HintMemory): void {
  try {
    storage?.setItem(HINT_ASKED_KEY, String(memory.asked));
    if (memory.dragShown) storage?.setItem(HINT_DRAG_KEY, "1");
  } catch {
    // Quota or disabled storage: this session still remembers.
  }
}

/** What is on Mate's window, apart from the hints themselves. */
export interface HintScene {
  /** Mate is awake and idle: no answer, no lesson step. */
  readonly idle: boolean;
  /** A bubble, the beam, onboarding, the menu, a stamp or the consent hint is up, or the page is hidden. */
  readonly busy: boolean;
}

export interface HintPolicy {
  /** Answer `generation` started thinking: one question, counted once. */
  asked(generation: number): void;
  /** An answer ended: the drag hint follows at the next quiet moment, once ever. */
  answered(): void;
  /** The hint to show at `now` on `scene`, or null. */
  next(scene: HintScene, now: number): TimedHint | null;
  /** When `next` will have a hint if the window stays as it is, or null. */
  dueAt(): number | null;
  /** `kind` went up at `now`. */
  shown(kind: TimedHint, now: number): void;
}

export function createHintPolicy(storage: StorageLike | null): HintPolicy {
  let memory = loadHintMemory(storage);
  let lastGeneration = -1;
  let dragPending = false;
  let quietSince: number | null = null;
  let lastTalk: number | null = null;
  const remember = (next: HintMemory): void => {
    memory = next;
    saveHintMemory(storage, memory);
  };
  const talkDue = (since: number): number | null => {
    if (memory.asked >= TALK_QUESTIONS) return null;
    return Math.max(since + TALK_IDLE_MS, lastTalk === null ? -Infinity : lastTalk + TALK_COOLDOWN_MS);
  };
  const dragDue = (): boolean => dragPending && !memory.dragShown;
  return {
    asked(generation) {
      if (!Number.isSafeInteger(generation) || generation <= lastGeneration) return;
      lastGeneration = generation;
      if (memory.asked < TALK_QUESTIONS) remember({ ...memory, asked: memory.asked + 1 });
    },
    answered() {
      dragPending = !memory.dragShown;
    },
    next(scene, now) {
      if (!scene.idle || scene.busy) {
        quietSince = null;
        return null;
      }
      quietSince ??= now;
      if (dragDue()) return "drag";
      const due = talkDue(quietSince);
      return due !== null && now >= due ? "talk" : null;
    },
    dueAt() {
      if (quietSince === null) return null;
      return dragDue() ? quietSince : talkDue(quietSince);
    },
    shown(kind, now) {
      if (kind === "talk") {
        lastTalk = now;
      } else {
        dragPending = false;
        remember({ ...memory, dragShown: true });
      }
    },
  };
}

export interface ConsentPhase {
  readonly phase: "picture" | "text";
  /** Until the other one's turn. */
  readonly leftMs: number;
}

/**
 * Whose turn it is `elapsedMs` after the consent question appeared: the picture for
 * `CONSENT_PICTURE_MS`, then the text for `textMs` (its reading time), and again.
 */
export function consentPhase(elapsedMs: number, textMs: number): ConsentPhase {
  const text = Number.isFinite(textMs) && textMs > 0 ? textMs : 0;
  const cycle = CONSENT_PICTURE_MS + text;
  const at = Number.isFinite(elapsedMs) && elapsedMs > 0 ? elapsedMs % cycle : 0;
  return at < CONSENT_PICTURE_MS
    ? { phase: "picture", leftMs: CONSENT_PICTURE_MS - at }
    : { phase: "text", leftMs: cycle - at };
}
