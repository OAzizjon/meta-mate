// Mate's window: mounts the robot (src/robot) idle and calm. Inside Tauri it plays live answers
// from Rust's answer stream (src/ipc/answer-stream.ts) on him, the audio engine and a speech
// bubble. In development only, the console gets `window.__mate` to try states, moods, the beam
// and the UI language by hand, e.g.
// __mate.setState("pointing"); __mate.aimAt({ left: 20, top: 20, right: 120, bottom: 60 }).
import { createAudioEngine } from "./audio";
import { getLocale, setLocale, t } from "./i18n";
import type { LocaleCode } from "./i18n";
import { decodePcm, pointAt, reportPlayback, subscribeAnswerStream } from "./ipc/answer-stream";
import type { AnswerModel, AnswerMsg } from "./ipc/answer-stream";
import { AnswerGenerations } from "./ipc/answer-generation";
import type { LocaleKey } from "./i18n";
import { mountOnboarding, setOnboardingTranslator, stopOnboarding } from "./onboarding";
import { createTimelinePlayer } from "./player";
import { isRobotMood, isRobotState, mountRobot } from "./robot";
import type { CssRect, RobotController, RobotState } from "./robot";
import {
  SETTINGS_CHANGED_EVENT,
  startMateSettings,
  toAudioSettings,
} from "./settings/apply";
import type {
  MateSettingsBinding,
  SettingsInvoke,
  SettingsSubscribe,
} from "./settings/apply";

/** How long the bubble shows a line from Rust, the "can't find" line, and stays after `End`. */
const LINE_BUBBLE_MS = 4000;
const CANT_FIND_BUBBLE_MS = 3000;
const DONE_BUBBLE_MS = 1500;
const LESSON_INTERRUPT_EVENT = "lesson-interrupt";
const REDUCED_MOTION_QUERY = "(prefers-reduced-motion: reduce)";

const MODEL_KEYS: Readonly<Record<AnswerModel, LocaleKey>> = {
  "muse-spark-1.3": "model.muse_spark",
  "muse-wonder-1.0": "model.muse_wonder",
  "sam-3.1": "model.sam",
  "muse-voice-transcribe-1.0": "model.voice_transcribe",
};

const tauriInvoke: SettingsInvoke = async (cmd, args) => {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke(cmd, args);
};

const tauriSubscribe: SettingsSubscribe = async (onChanged) => {
  const { listen } = await import("@tauri-apps/api/event");
  return listen<unknown>(SETTINGS_CHANGED_EVENT, (event) => {
    onChanged(event.payload);
  });
};

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
  /** Puts the model chip under the text (null clears it); it goes when the bubble hides. */
  setModel(model: AnswerModel | null): void;
  /** Shows `text` (null hides the bubble), for `ms` when given; the chip is left as it is. */
  show(text: string | null, ms?: number): void;
  /** Hides the bubble after `ms`, unless something else is shown first. */
  hideAfter(ms: number): void;
}

function mountBubble(element: HTMLElement): Bubble {
  const textElement = document.createElement("div");
  const chip = document.createElement("span");
  chip.className = "mate-model-chip";
  chip.hidden = true;
  let timer: ReturnType<typeof setTimeout> | undefined;
  const setModel = (model: AnswerModel | null): void => {
    chip.textContent = model === null ? "" : t(MODEL_KEYS[model]);
    chip.hidden = model === null;
  };
  const show = (text: string | null, ms?: number): void => {
    clearTimeout(timer);
    timer = undefined;
    // textContent only: answer text is never parsed as markup.
    textElement.textContent = text ?? "";
    // Onboarding shares this element and may have replaced its children. Otherwise the
    // nodes stay put: re-inserting the chip would restart its fade on every sentence.
    if (textElement.parentNode !== element || chip.parentNode !== element) {
      element.replaceChildren(textElement, chip);
    }
    element.hidden = text === null;
    // The chip goes with the bubble: a later line in the same answer shows none.
    if (text === null) setModel(null);
    if (text !== null && ms !== undefined) timer = setTimeout(() => show(null), ms);
  };
  return {
    setModel,
    show,
    hideAfter(ms) {
      clearTimeout(timer);
      timer = undefined;
      // Nothing lingers when the bubble is already hidden: the chip goes now.
      if (element.hidden) setModel(null);
      else timer = setTimeout(() => show(null), ms);
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
function startLiveAnswers(
  robot: RobotController,
  bubble: Bubble,
  settings: MateSettingsBinding,
): LiveAnswers {
  let uiLocale = getLocale();
  let unsubscribe: (() => void) | null = null;
  const ready = createRouter(robot, bubble, settings);
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
  settings: MateSettingsBinding,
): Promise<(msg: AnswerMsg) => void> {
  const audio = await createAudioEngine({
    settings: toAudioSettings(settings.get()),
  });
  settings.setAudio(audio);
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
  const { getCurrentWebviewWindow } = await import("@tauri-apps/api/webviewWindow");
  void getCurrentWebviewWindow()
    .listen<unknown>(LESSON_INTERRUPT_EVENT, (event) => {
      if (
        typeof event.payload === "number" &&
        Number.isSafeInteger(event.payload) &&
        event.payload >= 0
      ) {
        audio.interrupt(event.payload);
      }
    })
    .catch((error: unknown) => {
      console.error("[mate] lesson-interrupt listener failed", error);
    });
  let listening = false;
  let listeningGeneration: number | null = null;
  let showingHeard = false;
  const generations = new AnswerGenerations();
  let idle = false;
  const setState = (state: RobotState): void => {
    idle = state === "idle";
    // The chip stays while the bubble lingers after the answer; the bubble clears it.
    if (listening && state !== "listening") {
      listening = false;
      listeningGeneration = null;
      if (showingHeard) bubble.show(null);
      showingHeard = false;
      robot.setVoiceLevel(0);
    } else {
      listening = state === "listening";
    }
    robot.setState(state);
  };
  const player = createTimelinePlayer({
    robot: {
      setState,
      setMood: (mood) => robot.setMood(mood),
      aimAt: (target) => robot.aimAt(target),
    },
    audio,
    resolveTarget: (point, generation, step) => pointAt(generation, point, step),
    showBubble: (text) => {
      showingHeard = false;
      bubble.show(text);
    },
    onLine(line) {
      // The only line a timeline raises today; nothing speaks it here.
      if (line !== "CantFind") return;
      showingHeard = false;
      bubble.show(t("robot.cant_find"), CANT_FIND_BUBBLE_MS);
      return new Promise<void>((resolve) => setTimeout(resolve, CANT_FIND_BUBBLE_MS));
    },
    onDone(doneGeneration) {
      if (doneGeneration !== generations.answer) return;
      setState("idle");
      bubble.hideAfter(DONE_BUBBLE_MS);
      // Every sentence before `End` was played out: the worklet reported its end.
      void reportPlayback("done", doneGeneration);
    },
  });
  return (msg) => {
    const newest = generations.newest;
    if (!generations.accept(msg)) return;
    if (generations.newest > newest) {
      idle = false;
      bubble.setModel(null);
    }
    // The first sign of a real answer ends the first-run examples.
    stopOnboarding();
    switch (msg.type) {
      case "model":
        if (!idle) bubble.setModel(msg.model);
        return;
      case "state":
        setState(msg.state);
        if (msg.state === "listening") {
          listeningGeneration = msg.generation;
          showingHeard = false;
          bubble.show(null);
        }
        return;
      case "heard":
        if (listening && msg.generation === listeningGeneration) {
          bubble.show(msg.text || null);
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
        setState("idle");
        return;
      case "line":
        showingHeard = false;
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
  setOnboardingTranslator((key) => t(key));
  mountOnboarding();
  const inTauri = "__TAURI_INTERNALS__" in window;
  const motionQuery =
    typeof window.matchMedia === "function" ? window.matchMedia(REDUCED_MOTION_QUERY) : null;
  const settings = startMateSettings(
    {
      roots: [document.documentElement, container],
      robot,
      viewport: () => ({
        width: container.clientWidth || window.innerWidth,
        height: container.clientHeight || window.innerHeight,
      }),
      prefersReducedMotion: () => Boolean(motionQuery?.matches),
    },
    {
      invoke: inTauri ? tauriInvoke : null,
      subscribe: inTauri ? tauriSubscribe : null,
    },
  );
  const onEnvironmentChange = (): void => {
    settings.reapply();
  };
  motionQuery?.addEventListener("change", onEnvironmentChange);
  window.addEventListener("resize", onEnvironmentChange);
  const live =
    bubble && inTauri ? startLiveAnswers(robot, mountBubble(bubble), settings) : null;
  if (import.meta.env.DEV) {
    window.__mate = devHook(robot, (locale) => {
      if (live) live.setUiLocale(locale);
      else setLocale(locale);
    });
  }
}
