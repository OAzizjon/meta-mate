// Pure sound-cue policy for Mate's window: maps answer, beam, lesson, presence, and menu events to cues.

import { LEVEL_GAIN_TABLE, levelGain, panFromViewportX } from "./mixer.ts";
import type {
  AudioSettings,
  CueName,
  CueOptions,
  SoundLevel,
  SoundMode,
} from "./types.ts";
import type { AnswerLineKey, AnswerState } from "../ipc/answer-stream.ts";
import type { CssRect } from "../robot/types.ts";
import type { TimelineItem } from "../ipc/timeline.ts";
import type { HighlightMsg as HighlightPayload } from "../highlight/geometry.ts";

/** Minimum spacing between two played cues in milliseconds. */
export const CUE_COOLDOWN_MS = 400;

const PRAISE_KEYS: ReadonlySet<AnswerLineKey> = new Set<AnswerLineKey>([
  "robot.right_click",
  "robot.right_click_2",
  "robot.right_click_3",
  "robot.right_click_4",
]);

const LESSON_MARKER_KEYS: ReadonlySet<AnswerLineKey> = new Set<AnswerLineKey>([
  "robot.right_click",
  "robot.right_click_2",
  "robot.right_click_3",
  "robot.right_click_4",
  "robot.wrong_place",
  "robot.done",
]);

const LESSON_REFUSAL_KEYS: ReadonlySet<AnswerLineKey> = new Set<AnswerLineKey>([
  "robot.cant_find",
  "robot.cant_look",
  "robot.protected_window",
  "robot.window_closed",
  "robot.private_window",
  "robot.cant_speak",
  "robot.consent_expired",
  "robot.too_many_turns",
  "robot.wont_press",
  "robot.press_cancelled",
  "robot.press_failed",
  "robot.sleep",
  "robot.network_error",
  "robot.sign_in_again",
  "robot.sign_in_first",
  "robot.guest_limit",
]);

export type CueEvent =
  | { type: "listening"; generation?: number }
  | { type: "looking"; generation?: number }
  | { type: "thinking"; generation: number }
  | { type: "answer-state"; generation: number; state: AnswerState }
  | { type: "answer-line"; generation: number; key: AnswerLineKey }
  | { type: "answer-item"; generation: number; item: TimelineItem }
  | { type: "answer-done"; generation: number }
  | { type: "answer-cancel"; generation: number }
  | { type: "beam-target"; rect: CssRect | null; viewportWidth?: number }
  | { type: "point-resolved"; rect: CssRect | null; viewportWidth?: number }
  | { type: "highlight"; payload: HighlightPayload; viewportWidth?: number }
  | { type: "lesson-step-right" }
  | { type: "click-miss" }
  | { type: "lesson-done"; generation?: number }
  | { type: "wake" }
  | { type: "sleep" }
  | { type: "menu-open" };

export interface CueDecision {
  readonly cue: CueName;
  readonly options: CueOptions;
}

export interface CuePolicyState {
  readonly lastCueAt: number;
  readonly activeGeneration: number;
  readonly thinkPlayedGenerations: ReadonlySet<number>;
  readonly lessonGenerations: ReadonlySet<number>;
  readonly failedGenerations: ReadonlySet<number>;
  readonly donePlayedGenerations: ReadonlySet<number>;
  readonly now: number;
  readonly speaking: boolean;
  readonly volume: number;
  readonly mode: SoundMode;
  readonly reduceMotion?: boolean;
  readonly viewportWidth: number;
}

export interface CuePolicyContext {
  readonly now?: number;
  readonly speaking?: boolean;
  readonly settings?: Partial<AudioSettings>;
  readonly reduceMotion?: boolean;
  readonly viewportWidth?: number;
}

export interface CuePolicyOptions {
  readonly now?: () => number;
  readonly isSpeaking?: () => boolean;
  readonly getSettings?: () => AudioSettings;
  readonly getViewportWidth?: () => number;
  readonly playCue?: (name: CueName, opts?: CueOptions) => unknown;
}

export interface CuePolicy {
  decide(event: CueEvent, ctx?: CuePolicyContext): CueDecision | null;
  trigger(event: CueEvent, ctx?: CuePolicyContext): CueDecision | null;
  markLesson(generation?: number): void;
  reset(): void;
}

/** Map Settings sound mode to a base sound level ("calm" -> "soft", "fun" -> "normal"). */
export function cueLevelForMode(mode: SoundMode): SoundLevel {
  return mode === "calm" ? "soft" : "normal";
}

/** Linear gain for a cue with requested base level under the active sound mode. */
export function cueGainForMode(baseLevel: SoundLevel, mode: SoundMode): number {
  if (baseLevel === "soft") {
    return mode === "calm"
      ? LEVEL_GAIN_TABLE.soft * LEVEL_GAIN_TABLE.soft
      : LEVEL_GAIN_TABLE.soft;
  }
  return levelGain(baseLevel, mode);
}

/** Whether a canned line key belongs to a multi-step lesson flow. */
export function isLessonLineKey(key: AnswerLineKey): boolean {
  return LESSON_MARKER_KEYS.has(key);
}

/** Whether a canned line key ends a flow in a refusal or error rather than completion. */
export function isLessonRefusalLineKey(key: AnswerLineKey): boolean {
  return LESSON_REFUSAL_KEYS.has(key);
}

/** Whether a timeline item indicates an interactive lesson (a `<step>` or `wait` item). */
export function isLessonItem(item: TimelineItem): boolean {
  if (item.kind === "wait") return true;
  if (item.kind === "say") {
    return item.value.bound.some((b) => b.kind === "step");
  }
  if (item.kind === "bound") {
    return item.value.kind === "step";
  }
  return false;
}

/** Creates an initial pure state snapshot for `decideCue`. */
export function createCuePolicyState(
  overrides: Partial<CuePolicyState> = {},
): CuePolicyState {
  return {
    lastCueAt: overrides.lastCueAt ?? -Infinity,
    activeGeneration: overrides.activeGeneration ?? -1,
    thinkPlayedGenerations: new Set(overrides.thinkPlayedGenerations),
    lessonGenerations: new Set(overrides.lessonGenerations),
    failedGenerations: new Set(overrides.failedGenerations),
    donePlayedGenerations: new Set(overrides.donePlayedGenerations),
    now: overrides.now ?? 0,
    speaking: overrides.speaking ?? false,
    volume: overrides.volume ?? 1,
    mode: overrides.mode ?? "fun",
    reduceMotion: overrides.reduceMotion ?? false,
    viewportWidth: overrides.viewportWidth ?? 0,
  };
}

/**
 * Pure transition: takes an event and policy state, returns `{ decision, state }`.
 */
export function decideCue(
  event: CueEvent,
  state: CuePolicyState,
): { decision: CueDecision | null; state: CuePolicyState } {
  let activeGeneration = state.activeGeneration;
  const thinkPlayedGenerations = new Set(state.thinkPlayedGenerations);
  const lessonGenerations = new Set(state.lessonGenerations);
  const failedGenerations = new Set(state.failedGenerations);
  const donePlayedGenerations = new Set(state.donePlayedGenerations);

  const trackGeneration = (generation: number | undefined): void => {
    if (typeof generation === "number" && generation > activeGeneration) {
      activeGeneration = generation;
    }
  };

  let cue: CueName | null = null;
  let baseLevel: SoundLevel = "normal";
  let pan: number | undefined;
  let loop: boolean | undefined;
  let markThinkGen: number | null = null;
  let markDoneGen: number | null = null;

  switch (event.type) {
    case "listening":
      trackGeneration(event.generation);
      cue = "listen";
      break;

    case "looking":
      trackGeneration(event.generation);
      cue = "whoosh";
      baseLevel = "soft";
      break;

    case "thinking":
      trackGeneration(event.generation);
      if (!thinkPlayedGenerations.has(event.generation)) {
        cue = "think";
        loop = false;
        markThinkGen = event.generation;
      }
      break;

    case "answer-state":
      trackGeneration(event.generation);
      if (event.state === "listening") {
        cue = "listen";
      } else if (event.state === "looking") {
        cue = "whoosh";
        baseLevel = "soft";
      } else if (event.state === "thinking") {
        if (!thinkPlayedGenerations.has(event.generation)) {
          cue = "think";
          loop = false;
          markThinkGen = event.generation;
        }
      }
      break;

    case "answer-line":
      trackGeneration(event.generation);
      if (isLessonLineKey(event.key)) {
        lessonGenerations.add(event.generation);
      }
      if (isLessonRefusalLineKey(event.key)) {
        failedGenerations.add(event.generation);
      }
      if (PRAISE_KEYS.has(event.key)) {
        cue = "success";
      } else if (event.key === "robot.wrong_place") {
        cue = "oops";
      } else if (event.key === "robot.done") {
        if (!donePlayedGenerations.has(event.generation)) {
          cue = "done";
          markDoneGen = event.generation;
        }
      }
      break;

    case "answer-item":
      trackGeneration(event.generation);
      if (isLessonItem(event.item)) {
        lessonGenerations.add(event.generation);
      }
      break;

    case "answer-done":
      trackGeneration(event.generation);
      if (
        lessonGenerations.has(event.generation) &&
        !failedGenerations.has(event.generation) &&
        !donePlayedGenerations.has(event.generation)
      ) {
        cue = "done";
        markDoneGen = event.generation;
      }
      break;

    case "answer-cancel":
      failedGenerations.add(event.generation);
      break;

    case "beam-target":
    case "point-resolved":
      if (event.rect) {
        const width = event.viewportWidth ?? state.viewportWidth;
        const centerX = (event.rect.left + event.rect.right) / 2;
        pan = panFromViewportX(centerX, width);
        cue = "ping";
      }
      break;

    case "highlight": {
      const { payload } = event;
      if (payload.type === "target") {
        const width = event.viewportWidth ?? state.viewportWidth;
        const centerX = (payload.rect.left + payload.rect.right) / 2;
        pan = panFromViewportX(centerX, width);
        cue = "ping";
      } else if (payload.type === "burst") {
        if (activeGeneration >= 0) lessonGenerations.add(activeGeneration);
        cue = "success";
      } else if (payload.type === "miss") {
        if (activeGeneration >= 0) lessonGenerations.add(activeGeneration);
        cue = "oops";
      } else if (payload.type === "warmth") {
        if (activeGeneration >= 0) lessonGenerations.add(activeGeneration);
      }
      break;
    }

    case "lesson-step-right":
      if (activeGeneration >= 0) lessonGenerations.add(activeGeneration);
      cue = "success";
      break;

    case "click-miss":
      if (activeGeneration >= 0) lessonGenerations.add(activeGeneration);
      cue = "oops";
      break;

    case "lesson-done": {
      const gen = event.generation ?? activeGeneration;
      if (gen >= 0) {
        if (!donePlayedGenerations.has(gen)) {
          cue = "done";
          markDoneGen = gen;
        }
      } else {
        cue = "done";
      }
      break;
    }

    case "wake":
      cue = "wake";
      break;

    case "sleep":
      cue = "sleep";
      break;

    case "menu-open":
      cue = "pop";
      break;
  }

  const nextBaseState = (lastCueAt: number): CuePolicyState => ({
    ...state,
    lastCueAt,
    activeGeneration,
    thinkPlayedGenerations,
    lessonGenerations,
    failedGenerations,
    donePlayedGenerations,
  });

  if (!cue) {
    return { decision: null, state: nextBaseState(state.lastCueAt) };
  }

  // Zero volume suppresses cues completely; reduced motion does not affect audio.
  if (!(state.volume > 0)) {
    return { decision: null, state: nextBaseState(state.lastCueAt) };
  }

  // Never play a cue over Mate's own speech, except "listen".
  if (state.speaking && cue !== "listen") {
    return { decision: null, state: nextBaseState(state.lastCueAt) };
  }

  // Rate-limit to at most one cue per 400 ms.
  if (state.now - state.lastCueAt < CUE_COOLDOWN_MS) {
    return { decision: null, state: nextBaseState(state.lastCueAt) };
  }

  if (markThinkGen !== null) {
    thinkPlayedGenerations.add(markThinkGen);
  }
  if (markDoneGen !== null) {
    donePlayedGenerations.add(markDoneGen);
  }

  const effectiveLevel: SoundLevel =
    baseLevel === "soft" ? "soft" : cueLevelForMode(state.mode);
  const gain = cueGainForMode(baseLevel, state.mode);

  const options: CueOptions = {
    level: effectiveLevel,
    gain,
    ...(pan !== undefined ? { pan } : {}),
    ...(loop !== undefined ? { loop } : {}),
  };

  return {
    decision: { cue, options },
    state: nextBaseState(state.now),
  };
}

export function createCuePolicy(options: CuePolicyOptions = {}): CuePolicy {
  let state = createCuePolicyState();

  function decide(
    event: CueEvent,
    ctx?: CuePolicyContext,
  ): CueDecision | null {
    const baseSettings = options.getSettings?.();
    const volume = ctx?.settings?.volume ?? baseSettings?.volume ?? 1;
    const mode: SoundMode = ctx?.settings?.mode ?? baseSettings?.mode ?? "fun";
    const reduceMotion =
      ctx?.reduceMotion ??
      ctx?.settings?.reduceMotion ??
      baseSettings?.reduceMotion ??
      false;
    const speaking = ctx?.speaking ?? options.isSpeaking?.() ?? false;
    const now = ctx?.now ?? options.now?.() ?? performance.now();
    const viewportWidth =
      ctx?.viewportWidth ??
      options.getViewportWidth?.() ??
      (typeof window !== "undefined" ? window.innerWidth : 0);

    const result = decideCue(event, {
      ...state,
      now,
      speaking,
      volume,
      mode,
      reduceMotion,
      viewportWidth,
    });
    state = result.state;
    return result.decision;
  }

  function trigger(
    event: CueEvent,
    ctx?: CuePolicyContext,
  ): CueDecision | null {
    const decision = decide(event, ctx);
    if (decision) {
      options.playCue?.(decision.cue, decision.options);
    }
    return decision;
  }

  function markLesson(generation?: number): void {
    const gen = generation ?? state.activeGeneration;
    if (gen >= 0) {
      const lessonGenerations = new Set(state.lessonGenerations);
      lessonGenerations.add(gen);
      state = { ...state, lessonGenerations };
    }
  }

  function reset(): void {
    state = createCuePolicyState();
  }

  return {
    decide,
    trigger,
    markLesson,
    reset,
  };
}
