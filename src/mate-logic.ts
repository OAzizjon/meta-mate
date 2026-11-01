// Mate's answer presentation policy, independent of the DOM, Tauri and the audio engine.
// Effects are injected so the same event sequence can be replayed in Node tests.
import { BUBBLE_LINGER_MS, bubbleReadMs } from "./bubble-timing.ts";
import { AnswerGenerations, HeldPresentation } from "./ipc/answer-generation.ts";
import { guestUsageOf } from "./mate-menu.ts";
import { nextStamp } from "./safety-stamp.ts";
import type { HintKind } from "./hints/logic.ts";
import type { AnswerModel, AnswerMsg } from "./ipc/answer-stream.ts";
import type { TimelineItem } from "./ipc/timeline.ts";
import type { LocaleKey } from "./i18n/index.ts";
import type { RobotController, RobotMood, RobotState } from "./robot/index.ts";
import type { TimelinePlayer, TimelinePlayerOptions } from "./player/timeline-player.ts";
import type { SafetyVerdict, StampState } from "./safety-stamp.ts";

export const CANT_FIND_HOLD_MS = 3000;
/** As long as Rust waits for a click on Mate (or F8) before an unclear window is looked at (screen.rs confirm_look). */
export const LOOK_CONSENT_MS = 30_000;

export type BubbleShape = "thought" | "speech" | "shout";

/** The bubble's text has a hint picture (src/hints/logic.ts): today only the look-consent question. */
export type BubbleHint = Extract<HintKind, "consent">;

/** Comic balloon shape from Mate's state, the answer's mood and any safety stamp. */
export function bubbleShape(
  state: RobotState,
  mood: RobotMood,
  stamp: SafetyVerdict | null = null,
): BubbleShape {
  if (state === "thinking" || state === "looking") return "thought";
  if (
    state !== "listening" &&
    state !== "sleeping" &&
    (mood === "surprised" || mood === "sad" || stamp === "careful" || stamp === "scam")
  ) {
    return "shout";
  }
  return "speech";
}

export interface Bubble {
  /** The chip goes when the bubble hides; showing text leaves it as it is. */
  setModel(model: AnswerModel | null): void;
  /** Comic balloon shape; optional so text-only test doubles stay small. */
  setShape?(shape: BubbleShape): void;
  /**
   * Null hides the bubble; a duration schedules its removal. `hint` names the picture that
   * goes with this text (src/hints/view.ts); any other show, the timed removal included, ends it.
   */
  show(text: string | null, ms?: number, hint?: BubbleHint): void;
  /** Keep the remaining reading time, and at least this many ms from now. */
  hideAfter(ms: number): void;
}

/** The safety check's comic stamp (src/safety-stamp.ts). */
export interface Stamp {
  /** Null hides it. */
  show(verdict: SafetyVerdict | null): void;
  /** Hides it in `ms` unless it is shown or hidden before. */
  hideAfter(ms: number): void;
}

interface AnswerEffects {
  robot: Pick<RobotController, "setState" | "setVoiceLevel"> &
    Partial<Pick<RobotController, "setMood">>;
  bubble: Bubble;
  stamp: Stamp;
  player: Pick<TimelinePlayer, "push" | "cancel">;
  t: (key: LocaleKey) => string;
  stopOnboarding: () => void;
  onPcm: (msg: Extract<AnswerMsg, { type: "pcm" }>) => void;
  reportPlayback: (kind: "trace" | "done", generation: number, detail?: string) => unknown;
  sleep: (ms: number) => Promise<void>;
}

export function createMateAnswerRouter(effects: AnswerEffects) {
  const { robot, bubble, stamp, player, t, stopOnboarding, onPcm, reportPlayback, sleep } = effects;
  let listening = false;
  let listeningGeneration: number | null = null;
  let clickListening = false;
  let showingHeard = false;
  let lookConsentGeneration: number | null = null;
  let currentState: RobotState = "idle";
  let currentMood: RobotMood = "calm";
  let currentStamp: SafetyVerdict | null = null;
  const syncShape = (): void => {
    bubble.setShape?.(bubbleShape(currentState, currentMood, currentStamp));
  };
  const generations = new AnswerGenerations();
  const presentation = new HeldPresentation();
  const present = (key: string, update: () => void): void => {
    presentation.apply(key, generations.holdPresentation, update);
  };
  // The answer Rust named a safety check (`markSafety`) and its stamp so far.
  let safety: { generation: number; state: StampState } | null = null;
  let stamped = false;
  const unstamp = (): void => {
    safety = null;
    currentStamp = null;
    if (stamped) {
      stamp.show(null);
      syncShape();
    }
    stamped = false;
  };
  const stampItem = (generation: number, item: TimelineItem): void => {
    if (safety?.generation !== generation) return;
    const before = safety.state ?? null;
    safety.state = nextStamp(safety.state, item);
    const verdict = safety.state ?? null;
    if (verdict === before) return;
    stamped = verdict !== null;
    present("stamp", () => {
      currentStamp = verdict;
      stamp.show(verdict);
      syncShape();
    });
  };
  let idle = false;
  const setMood = (mood: RobotMood): void => {
    currentMood = mood;
    robot.setMood?.(mood);
    syncShape();
  };
  const setState = (state: RobotState): void => {
    idle = state === "idle";
    currentState = state;
    // The chip stays while the bubble lingers after the answer; the bubble clears it.
    if (listening && state !== "listening") {
      listening = false;
      listeningGeneration = null;
      clickListening = false;
      if (showingHeard) bubble.show(null);
      showingHeard = false;
      robot.setVoiceLevel(0);
    } else {
      listening = state === "listening";
    }
    robot.setState(state);
    syncShape();
  };
  const route = (msg: AnswerMsg): void => {
    const newest = generations.newest;
    const answer = generations.answer;
    if (!generations.accept(msg)) return;
    if (msg.type === "cancel" || generations.answer > answer) presentation.clear();
    if (generations.answer > answer && currentMood !== "calm") {
      present("mood", () => setMood("calm"));
    }
    if (!generations.holdPresentation) presentation.release();
    if (generations.newest > newest) {
      idle = false;
      bubble.setModel(null);
      // A newer press ends the verdict's stamp at once, like the chip.
      if (safety && safety.generation < generations.newest) unstamp();
    }
    // The first sign of a real answer ends the first-run examples.
    stopOnboarding();
    switch (msg.type) {
      case "model":
        present("model", () => {
          if (!idle) bubble.setModel(msg.model);
        });
        return;
      case "state":
        if (msg.generation === lookConsentGeneration && (msg.state === "thinking" || msg.state === "idle")) {
          lookConsentGeneration = null;
          // A held consent question is replaced, never shown after its window closed.
          present("bubble", () => bubble.show(null));
        }
        if (msg.generation !== generations.listening && generations.holdPresentation) {
          present("state", () => setState(msg.state));
          return;
        }
        setState(msg.state);
        if (msg.state === "listening") {
          listeningGeneration = msg.generation;
          clickListening = false;
          showingHeard = false;
          bubble.show(null);
        }
        return;
      case "heard":
        if (listening && msg.generation === listeningGeneration) {
          bubble.show(clickListening
            ? [msg.text, t("robot.click_stop")].filter(Boolean).join("\n")
            : msg.text || null);
          showingHeard = true;
        }
        return;
      case "level":
        if (listening) robot.setVoiceLevel(msg.level);
        return;
      case "item":
        player.push(msg.generation, msg.item);
        stampItem(msg.generation, msg.item);
        void reportPlayback("trace", msg.generation, `item ${msg.item.kind}`);
        return;
      case "pcm":
        onPcm(msg);
        return;
      case "cancel":
        if (safety && safety.generation <= msg.generation) unstamp();
        if (msg.generation >= generations.answer && currentMood !== "calm") setMood("calm");
        // Cancels the audio engine too.
        player.cancel(msg.generation);
        if (generations.canIdle(msg.generation)) setState("idle");
        return;
      case "line":
        if (msg.key === "robot.click_stop") {
          if (listening && msg.generation === listeningGeneration) {
            clickListening = true;
            showingHeard = true;
            bubble.show(t(msg.key));
          }
          return;
        }
        // The consent question must stay up until its answer window closes; tracked on
        // arrival, so a close that comes while presentation is held still finds it.
        lookConsentGeneration = msg.key === "robot.look_consent" ? msg.generation : null;
        present("bubble", () => {
          showingHeard = false;
          const text = t(msg.key);
          // The consent question brings its picture: where Fn and F8 are, or a click on Mate.
          if (msg.key === "robot.look_consent") bubble.show(text, LOOK_CONSENT_MS, "consent");
          else bubble.show(text, bubbleReadMs(text));
        });
        return;
    }
  };
  const timeline: Pick<TimelinePlayerOptions, "showBubble" | "onLine" | "onDone"> = {
    showBubble: (text) => {
      present("bubble", () => {
        showingHeard = false;
        bubble.show(text);
      });
    },
    onLine(line) {
      // The only line a timeline raises today; nothing speaks it here.
      if (line !== "CantFind") return;
      present("bubble", () => {
        showingHeard = false;
        const text = t("robot.cant_find");
        bubble.show(text, bubbleReadMs(text));
      });
      return sleep(CANT_FIND_HOLD_MS);
    },
    onDone(doneGeneration) {
      if (doneGeneration !== generations.answer) return;
      present("state", () => setState("idle"));
      present("bubble-hide", () => bubble.hideAfter(BUBBLE_LINGER_MS));
      if (stamped && safety?.generation === doneGeneration) {
        present("stamp-hide", () => stamp.hideAfter(BUBBLE_LINGER_MS));
      }
      // Every sentence before `End` was played out: the worklet reported its end.
      void reportPlayback("done", doneGeneration);
    },
  };
  /** Rust's `safety-check` event: answer `payload` is "Is this safe?", so its verdict may be stamped. */
  const markSafety = (payload: unknown): void => {
    if (typeof payload !== "number" || !Number.isSafeInteger(payload) || payload < 0) return;
    // A newer press has moved on; a repeat must not restart the verdict.
    if (payload < generations.newest || safety?.generation === payload) return;
    unstamp();
    safety = { generation: payload, state: undefined };
  };
  return { route, present, setState, setMood, markSafety, ...timeline };
}

/** Lesson audio is interrupted independently of the answer's presentation generation. */
export function interruptLesson(payload: unknown, interrupt: (generation: number) => void): void {
  if (typeof payload === "number" && Number.isSafeInteger(payload) && payload >= 0) {
    interrupt(payload);
  }
}

/** Suppress live answer bubbles while the hide explanation owns the window. */
export function awakeBubble(bubble: Bubble, isSleeping: () => boolean): Bubble {
  return {
    setModel: (model) => { if (!isSleeping()) bubble.setModel(model); },
    setShape: (shape) => { if (!isSleeping()) bubble.setShape?.(shape); },
    // Only the arguments given: a hint is passed on only with the consent question.
    show: (...args) => { if (!isSleeping()) bubble.show(...args); },
    hideAfter: (ms) => { if (!isSleeping()) bubble.hideAfter(ms); },
  };
}

interface WindowEffects {
  bubble: Bubble;
  robot: Pick<RobotController, "setState"> & Partial<Pick<RobotController, "setMood">>;
  isSleeping: () => boolean;
  setSleeping: (sleeping: boolean) => void;
  closeMenu: () => void;
  stopOnboarding: () => void;
  hidingText: (minutes: number) => string;
  explainText: (control: string) => string;
}

export function createMateWindowEvents(effects: WindowEffects) {
  const { bubble, robot } = effects;
  return {
    hide(minutes: number): void {
      effects.closeMenu();
      effects.stopOnboarding();
      effects.setSleeping(true);
      robot.setMood?.("calm");
      robot.setState("sleeping");
      bubble.setShape?.("speech");
      bubble.setModel(null);
      bubble.show(effects.hidingText(minutes), 3000);
    },
    wake(): void {
      effects.setSleeping(false);
      bubble.show(null);
      robot.setState("idle");
    },
    explain(payload: { control?: unknown; ms?: unknown } | null): void {
      const control = payload?.control;
      const ms = payload?.ms;
      if (effects.isSleeping() || typeof control !== "string" || typeof ms !== "number" || !(ms > 0)) return;
      effects.stopOnboarding();
      robot.setMood?.("calm");
      bubble.setShape?.("speech");
      bubble.setModel(null);
      bubble.show(effects.explainText(control), ms);
    },
  };
}

/** Locale-ready trial view; auth clears it only for an explicit signed-in event. */
export function guestTrialView(usage: unknown) {
  const counted = guestUsageOf(usage);
  return counted === null ? null : {
    left: String(counted.left),
    limit: String(counted.limit),
    empty: counted.left === 0,
  };
}

export function signedIn(payload: unknown): boolean {
  return (payload as { signedIn?: unknown } | null)?.signedIn === true;
}

