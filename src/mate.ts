// Mate's window: mounts the robot (src/robot) idle and calm. Inside Tauri it plays live answers
// from Rust's answer stream (src/ipc/answer-stream.ts) on him, the audio engine and a speech
// bubble. In development only, the console gets `window.__mate` to try states, moods, the beam
// and the UI language by hand, e.g.
// __mate.setState("pointing"); __mate.aimAt({ left: 20, top: 20, right: 120, bottom: 60 }).
import { createAudioEngine } from "./audio";
import { BUBBLE_LINGER_MS, bubbleLingerMs, bubbleReadMs } from "./bubble-timing";
import { getLocale, setLocale, t } from "./i18n";
import { hideReturnTime, mountMateMenu } from "./mate-menu";
import { pttKeyLabel } from "./settings/ptt-key";
import type { LocaleCode } from "./i18n";
import { decodePcm, pointAt, reportPlayback, subscribeAnswerStream } from "./ipc/answer-stream";
import type { AnswerModel, AnswerMsg } from "./ipc/answer-stream";
import { AnswerGenerations, HeldPresentation } from "./ipc/answer-generation";
import type { LocaleKey } from "./i18n";
import { mountOnboarding, setOnboardingTranslator, setOnboardingPttKey, stopOnboarding } from "./onboarding";
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

/** How long the timeline holds on "can't find"; the bubble stays its reading time (WP85). */
const CANT_FIND_HOLD_MS = 3000;
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
  /**
   * Hides the bubble once its text was on screen for its reading time, and no sooner than
   * `ms` from now, unless something else is shown first.
   */
  hideAfter(ms: number): void;
}

function mountBubble(element: HTMLElement): Bubble {
  const textElement = document.createElement("div");
  const chip = document.createElement("span");
  chip.className = "mate-model-chip";
  chip.hidden = true;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let shownAt = 0;
  const setModel = (model: AnswerModel | null): void => {
    chip.textContent = model === null ? "" : t(MODEL_KEYS[model]);
    chip.hidden = model === null;
  };
  const show = (text: string | null, ms?: number): void => {
    clearTimeout(timer);
    timer = undefined;
    if (text !== null) shownAt = performance.now();
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
      if (element.hidden) {
        setModel(null);
        return;
      }
      const text = textElement.textContent ?? "";
      timer = setTimeout(() => show(null), bubbleLingerMs(text, performance.now() - shownAt, ms));
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
      setState: (state) => present("state", () => setState(state)),
      setMood: (mood) => present("mood", () => robot.setMood(mood)),
      aimAt: (target) => present("aim", () => robot.aimAt(target)),
    },
    audio,
    resolveTarget: (point, generation, step) => pointAt(generation, point, step),
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
      return new Promise<void>((resolve) => setTimeout(resolve, CANT_FIND_HOLD_MS));
    },
    onDone(doneGeneration) {
      if (doneGeneration !== generations.answer) return;
      present("state", () => setState("idle"));
      present("bubble-hide", () => bubble.hideAfter(BUBBLE_LINGER_MS));
      // Every sentence before `End` was played out: the worklet reported its end.
      void reportPlayback("done", doneGeneration);
    },
  });
  return (msg) => {
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
        if (generations.canIdle(msg.generation)) setState("idle");
        return;
      case "line":
        present("bubble", () => {
          showingHeard = false;
          const text = t(msg.key);
          bubble.show(text, bubbleReadMs(text));
        });
        return;
    }
  };
}

const container = document.getElementById("mate");
if (container) {
  const mountedRobot = mountRobot(container);
  let sleeping = false;
  const robot: RobotController = {
    ...mountedRobot,
    setState: (state) => mountedRobot.setState(sleeping ? "sleeping" : state),
  };
  robot.setMood("calm");
  robot.setState("idle");
  const bubble = document.getElementById("mate-bubble");
  setOnboardingTranslator((key) => t(key));
  const inTauri = "__TAURI_INTERNALS__" in window;
  const menu = inTauri ? mountMateMenu(tauriInvoke, t) : null;
  container.addEventListener("contextmenu", (event) => {
    event.preventDefault();
    menu?.open();
  });
  container.querySelector(".mate-svg")?.addEventListener("pointerdown", (event) => {
    if (!(event instanceof PointerEvent) || event.button !== 0 || !inTauri) return;
    event.preventDefault();
    void tauriInvoke("mate_drag_start").catch((error: unknown) => {
      console.error("mate: drag:", error);
    });
  });
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
      onChange: (updated) => setOnboardingPttKey(updated.pttKey),
    },
  );
  void settings.ready.then(() => mountOnboarding(undefined, null));
  const onEnvironmentChange = (): void => {
    settings.reapply();
  };
  motionQuery?.addEventListener("change", onEnvironmentChange);
  window.addEventListener("resize", onEnvironmentChange);
  const mountedBubble = bubble ? mountBubble(bubble) : null;
  // An answer already playing must not replace the hide explanation or sleeping state.
  const liveBubble: Bubble | null = mountedBubble ? {
    setModel: (model) => { if (!sleeping) mountedBubble.setModel(model); },
    show: (text, ms) => { if (!sleeping) mountedBubble.show(text, ms); },
    hideAfter: (ms) => { if (!sleeping) mountedBubble.hideAfter(ms); },
  } : null;
  const live =
    liveBubble && inTauri ? startLiveAnswers(robot, liveBubble, settings) : null;
  if (mountedBubble && inTauri) {
    void import("@tauri-apps/api/event").then(async ({ listen }) => {
      await listen("mate-menu-outside", () => menu?.close());
      await listen<{ minutes: number }>("mate-hiding", (event) => {
        menu?.close();
        stopOnboarding();
        sleeping = true;
        robot.setState("sleeping");
        mountedBubble.setModel(null);
        mountedBubble.show(t("mate.hiding", {
          time: hideReturnTime(event.payload.minutes, Date.now(), getLocale()),
          key: pttKeyLabel(settings.get().pttKey, t),
        }), 3000);
      });
      await listen("mate-woke", () => {
        sleeping = false;
        mountedBubble.show(null);
        robot.setState("idle");
      });
    }).catch((error: unknown) => {
      console.error("mate: presence events:", error);
    });
    void import("@tauri-apps/api/event").then(({ listen }) =>
      listen("home-closed-hint", () => {
        if (sleeping) return;
        const text = t("home.closed_hint");
        mountedBubble.setModel(null);
        mountedBubble.show(text, bubbleReadMs(text));
      }),
    ).catch((error: unknown) => {
      console.error("mate: home hint:", error);
    });
  }
  if (import.meta.env.DEV) {
    window.__mate = devHook(robot, (locale) => {
      if (live) live.setUiLocale(locale);
      else setLocale(locale);
    });
  }
}
