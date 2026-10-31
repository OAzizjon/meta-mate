// Mate's answer presentation policy, independent of the DOM, Tauri and the audio engine.
// Effects are injected so the same event sequence can be replayed in Node tests.
import { BUBBLE_LINGER_MS, bubbleReadMs } from "./bubble-timing.ts";
import { AnswerGenerations, HeldPresentation } from "./ipc/answer-generation.ts";
import { guestUsageOf } from "./mate-menu.ts";
import type { AnswerModel, AnswerMsg } from "./ipc/answer-stream.ts";
import type { LocaleKey } from "./i18n/index.ts";
import type { RobotController, RobotState } from "./robot/index.ts";
import type { TimelinePlayer, TimelinePlayerOptions } from "./player/timeline-player.ts";

export const CANT_FIND_HOLD_MS = 3000;
/** As long as Rust waits for F8 before an unclear window is looked at (screen.rs confirm_look). */
export const LOOK_CONSENT_MS = 30_000;

export interface Bubble {
  /** The chip goes when the bubble hides; showing text leaves it as it is. */
  setModel(model: AnswerModel | null): void;
  /** Null hides the bubble; a duration schedules its removal. */
  show(text: string | null, ms?: number): void;
  /** Keep the remaining reading time, and at least this many ms from now. */
  hideAfter(ms: number): void;
}

interface AnswerEffects {
  robot: Pick<RobotController, "setState" | "setVoiceLevel">;
  bubble: Bubble;
  player: Pick<TimelinePlayer, "push" | "cancel">;
  t: (key: LocaleKey) => string;
  stopOnboarding: () => void;
  onPcm: (msg: Extract<AnswerMsg, { type: "pcm" }>) => void;
  reportPlayback: (kind: "trace" | "done", generation: number, detail?: string) => unknown;
  sleep: (ms: number) => Promise<void>;
}

export function createMateAnswerRouter(effects: AnswerEffects) {
  const { robot, bubble, player, t, stopOnboarding, onPcm, reportPlayback, sleep } = effects;
  let listening = false;
  let listeningGeneration: number | null = null;
  let clickListening = false;
  let showingHeard = false;
  const generations = new AnswerGenerations();
  const presentation = new HeldPresentation();
  const present = (key: string, update: () => void): void => {
    presentation.apply(key, generations.holdPresentation, update);
  };
  let idle = false;
  const setState = (state: RobotState): void => {
    idle = state === "idle";
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
  };
  const route = (msg: AnswerMsg): void => {
    const newest = generations.newest;
    const answer = generations.answer;
    if (!generations.accept(msg)) return;
    if (msg.type === "cancel" || generations.answer > answer) presentation.clear();
    if (!generations.holdPresentation) presentation.release();
    if (generations.newest > newest) {
      idle = false;
      bubble.setModel(null);
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
        void reportPlayback("trace", msg.generation, `item ${msg.item.kind}`);
        return;
      case "pcm":
        onPcm(msg);
        return;
      case "cancel":
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
        present("bubble", () => {
          showingHeard = false;
          const text = t(msg.key);
          // The consent question must stay up until its answer window closes.
          bubble.show(text, msg.key === "robot.look_consent" ? LOOK_CONSENT_MS : bubbleReadMs(text));
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
      // Every sentence before `End` was played out: the worklet reported its end.
      void reportPlayback("done", doneGeneration);
    },
  };
  return { route, present, setState, ...timeline };
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
    show: (text, ms) => { if (!isSleeping()) bubble.show(text, ms); },
    hideAfter: (ms) => { if (!isSleeping()) bubble.hideAfter(ms); },
  };
}

interface WindowEffects {
  bubble: Bubble;
  robot: Pick<RobotController, "setState">;
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
      robot.setState("sleeping");
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

