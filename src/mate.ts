// Mate's window: mounts the robot (src/robot) idle and calm. Inside Tauri it plays live answers
// from Rust's answer stream (src/ipc/answer-stream.ts) on him, the audio engine and a speech
// bubble. In development only, the console gets `window.__mate` to try states, moods, the beam
// and the UI language by hand, e.g.
// __mate.setState("pointing"); __mate.aimAt({ left: 20, top: 20, right: 120, bottom: 60 }).
import { createAudioEngine } from "./audio";
import { getLocale, setLocale, t } from "./i18n";
import type { LocaleCode } from "./i18n";
import { decodePcm, reportPlayback, subscribeAnswerStream } from "./ipc/answer-stream";
import type { AnswerMsg } from "./ipc/answer-stream";
import { createTimelinePlayer } from "./player";
import { isRobotMood, isRobotState, mountRobot } from "./robot";
import type { CssRect, RobotController } from "./robot";

/** How long the bubble shows a line from Rust, the "can't find" line, and stays after `End`. */
const LINE_BUBBLE_MS = 4000;
const CANT_FIND_BUBBLE_MS = 3000;
const DONE_BUBBLE_MS = 1500;

interface MateDevHook {
  setState(state: string): void;
  setMood(mood: string): void;
  aimAt(target: CssRect | null): void;
  setLocale(locale: string): void;
}

declare global {
  interface Window {
    __mate?: MateDevHook;
  }
}

function devHook(robot: RobotController, setUiLocale: (locale: LocaleCode) => void): MateDevHook {
  return {
    setState(state) {
      if (!isRobotState(state)) throw new TypeError(`unknown state: ${state}`);
      robot.setState(state);
    },
    setMood(mood) {
      if (!isRobotMood(mood)) throw new TypeError(`unknown mood: ${mood}`);
      robot.setMood(mood);
    },
    aimAt(target) {
      robot.aimAt(target);
    },
    setLocale(locale) {
      if (locale !== "en" && locale !== "ru") throw new TypeError(`unknown locale: ${locale}`);
      setUiLocale(locale);
    },
  };
}

interface Bubble {
  /** Shows `text` (null hides the bubble), for `ms` when given. */
  show(text: string | null, ms?: number): void;
  /** Hides the bubble after `ms`, unless something else is shown first. */
  hideAfter(ms: number): void;
}

function mountBubble(element: HTMLElement): Bubble {
  let timer: ReturnType<typeof setTimeout> | undefined;
  const show = (text: string | null, ms?: number): void => {
    clearTimeout(timer);
    timer = undefined;
    // textContent only: answer text is never parsed as markup.
    element.textContent = text ?? "";
    element.hidden = text === null;
    if (text !== null && ms !== undefined) timer = setTimeout(() => show(null), ms);
  };
  return {
    show,
    hideAfter(ms) {
      clearTimeout(timer);
      timer = element.hidden ? undefined : setTimeout(() => show(null), ms);
    },
  };
}

interface LiveAnswers {
  /** Switches the UI language; Rust voices its cached phrases in it. */
  setUiLocale(locale: LocaleCode): void;
}

/**
 * Plays Rust's answers. The UI language is tracked here, apart from i18n's current locale,
 * which an answer's `<lang>` also moves.
 */
function startLiveAnswers(robot: RobotController, bubble: Bubble): LiveAnswers {
  let uiLocale = getLocale();
  let unsubscribe: (() => void) | null = null;
  const ready = createRouter(robot, bubble);
  // (Re)subscriptions run one at a time, in call order, once the engine is up.
  let queue: Promise<void> = Promise.resolve();
  const subscribe = (): void => {
    queue = queue
      .then(async () => {
        const route = await ready;
        const next = await subscribeAnswerStream(uiLocale, route);
        unsubscribe?.();
        unsubscribe = next;
      })
      .catch((error: unknown) => {
        console.error("[mate] live answers failed", error);
      });
  };
  subscribe();
  return {
    setUiLocale(locale) {
      setLocale(locale);
      if (locale === uiLocale) return;
      uiLocale = locale;
      subscribe();
    },
  };
}

/** Starts the audio engine and the timeline player; resolves to the answer message handler. */
async function createRouter(
  robot: RobotController,
  bubble: Bubble,
): Promise<(msg: AnswerMsg) => void> {
  const audio = await createAudioEngine();
  // The window runs with --autoplay-policy=no-user-gesture-required, so this should pass.
  const running = await audio.ensureResumed();
  if (!running) {
    console.warn("[mate] audio is suspended (autoplay blocked)");
  }
  const clock = (): string =>
    `state=${audio.ctx.state} t=${audio.ctx.currentTime.toFixed(3)} rate=${audio.ctx.sampleRate}`;
  void reportPlayback(running ? "audio_running" : "audio_suspended", 0, clock());
  // A second look: a context that starts late still counts.
  setTimeout(() => {
    const late = audio.ctx.state === "running" && audio.ctx.currentTime > 0;
    void reportPlayback(late ? "audio_running" : "audio_suspended", 0, clock());
  }, 1500);
  audio.onWorkletMessage((msg) => {
    if (msg.type === "sentence-start" || msg.type === "sentence-end" || msg.type === "overflow") {
      void reportPlayback("trace", 0, msg.type);
    }
  });
  const player = createTimelinePlayer({
    robot,
    audio,
    resolveTarget: () => null,
    showBubble: (text) => bubble.show(text),
    onLine(line) {
      // The only line a timeline raises today; nothing speaks it here.
      if (line !== "CantFind") return;
      bubble.show(t("robot.cant_find"), CANT_FIND_BUBBLE_MS);
      return new Promise<void>((resolve) => setTimeout(resolve, CANT_FIND_BUBBLE_MS));
    },
    onDone(generation) {
      robot.setState("idle");
      bubble.hideAfter(DONE_BUBBLE_MS);
      // Every sentence before `End` was played out: the worklet reported its end.
      void reportPlayback("done", generation);
    },
  });
  return (msg) => {
    switch (msg.type) {
      case "state":
        robot.setState(msg.state);
        if (msg.state === "listening") bubble.show(null);
        return;
      case "item":
        player.push(msg.generation, msg.item);
        void reportPlayback("trace", msg.generation, `item ${msg.item.kind}`);
        return;
      case "pcm": {
        const samples = decodePcm(msg.samplesB64);
        audio.pushPcm({
          generationId: msg.generation,
          sentenceId: msg.sentence,
          sampleRate: msg.sampleRate,
          eos: msg.eos,
          samples,
        });
        void reportPlayback("pcm", msg.generation, `s=${msg.sentence} n=${samples.length}`);
        return;
      }
      case "cancel":
        // Cancels the audio engine too.
        player.cancel(msg.generation);
        robot.setState("idle");
        return;
      case "line":
        bubble.show(t(msg.key), LINE_BUBBLE_MS);
        return;
    }
  };
}

const container = document.getElementById("mate");
if (container) {
  const robot = mountRobot(container);
  robot.setMood("calm");
  robot.setState("idle");
  const bubble = document.getElementById("mate-bubble");
  const live =
    bubble && "__TAURI_INTERNALS__" in window ? startLiveAnswers(robot, mountBubble(bubble)) : null;
  if (import.meta.env.DEV) {
    window.__mate = devHook(robot, (locale) => {
      if (live) live.setUiLocale(locale);
      else setLocale(locale);
    });
  }
}
