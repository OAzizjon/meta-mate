// Mate's window: mounts the robot (src/robot) idle and calm. Inside Tauri it plays live answers
// from Rust's answer stream (src/ipc/answer-stream.ts) on him, the audio engine and a speech
// bubble. In development only, the console gets `window.__mate` to try states, moods, the beam,
// the safety stamp, the hints and the UI language by hand, e.g.
// __mate.setState("pointing"); __mate.aimAt({ left: 20, top: 20, right: 120, bottom: 60 });
// __mate.setStamp("scam"); __mate.hint("talk").
import { parseHighlightMsg } from "./highlight/geometry.ts";
import { createAudioEngine } from "./audio";
import { bubbleLingerMs, bubbleReadMs } from "./bubble-timing";
import { mountHints } from "./hints/view.ts";
import type { Hints } from "./hints/view.ts";
import { getLocale, setLocale, t } from "./i18n";
import { hideReturnTime, mountMateMenu } from "./mate-menu";
import { pttKeycapLabel, pttKeyLabel } from "./settings/ptt-key";
import { applyTheme } from "./settings/theme.ts";
import type { LocaleCode } from "./i18n";
import { decodePcm, pointAt, reportPlayback, subscribeAnswerStream } from "./ipc/answer-stream";
import type { AnswerModel, AnswerMsg } from "./ipc/answer-stream";
import { awakeBubble, bubbleShape, createMateAnswerRouter, createMateWindowEvents, guestTrialView, interruptLesson, signedIn } from "./mate-logic.ts";
import type { Bubble, BubbleHint, BubbleShape, Stamp } from "./mate-logic.ts";
import { SAFETY_CHECK_EVENT, STAMP_KEYS, stampClearOf } from "./safety-stamp.ts";
import type { SafetyVerdict } from "./safety-stamp.ts";
import type { LocaleKey } from "./i18n";
import { mountOnboarding, setOnboardingTranslator, setOnboardingPttKey, stopOnboarding } from "./onboarding";
import { createTimelinePlayer } from "./player";
import type { TimelinePlayer } from "./player";
import { isRobotMood, isRobotState, mountRobot } from "./robot";
import type { CssRect, RobotController, RobotMood, RobotState } from "./robot";
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

const LESSON_INTERRUPT_EVENT = "lesson-interrupt";
const REDUCED_MOTION_QUERY = "(prefers-reduced-motion: reduce)";
const MATE_STATE_EVENT = "mate-state-changed";

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
  setStamp(verdict: string | null): void;
  hint(kind: string | null): void;
}

declare global {
  interface Window {
    __mate?: MateDevHook;
  }
}

function devHook(
  robot: RobotController,
  stamp: LiveStamp,
  hints: Hints | null,
  setUiLocale: (locale: LocaleCode) => void,
): MateDevHook {
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
      stamp.avoid(target);
      hints?.avoid(target);
    },
    setLocale(locale) {
      if (locale !== "en" && locale !== "ru") throw new TypeError(`unknown locale: ${locale}`);
      setUiLocale(locale);
    },
    setStamp(verdict) {
      if (verdict !== null && !Object.hasOwn(STAMP_KEYS, verdict)) {
        throw new TypeError(`unknown verdict: ${verdict}`);
      }
      stamp.show(verdict as SafetyVerdict | null);
    },
    hint(kind) {
      if (kind !== null && kind !== "talk" && kind !== "drag") throw new TypeError(`unknown hint: ${kind}`);
      hints?.show(kind);
    },
  };
}

function mountBubble(
  element: HTMLElement,
  isSleeping: () => boolean,
  onConsent: (text: string | null) => void,
): Bubble {
  const content = document.createElement("div");
  content.className = "mate-bubble-content";
  const textElement = document.createElement("div");
  textElement.className = "mate-bubble-text";
  const chip = document.createElement("span");
  chip.className = "mate-model-chip";
  chip.hidden = true;
  content.append(textElement, chip);
  let shape: BubbleShape = "speech";
  let currentText: string | null = null;
  let showingDots = false;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let shownAt = 0;
  element.dataset.shape = shape;
  const ensureContent = (): void => {
    // Onboarding shares this element and may have replaced its children. Otherwise the
    // nodes stay put: re-inserting the chip would restart its fade on every sentence.
    if (content.parentNode !== element) {
      element.replaceChildren(content);
    }
  };
  const syncDots = (): void => {
    const wantDots = currentText === null && shape === "thought" && !isSleeping();
    if (wantDots) {
      ensureContent();
      showingDots = true;
      element.classList.add("mate-bubble-dots");
      element.hidden = false;
    } else if (showingDots) {
      showingDots = false;
      element.classList.remove("mate-bubble-dots");
      if (currentText === null) element.hidden = true;
    } else {
      element.classList.remove("mate-bubble-dots");
    }
  };
  const setModel = (model: AnswerModel | null): void => {
    chip.textContent = model === null ? "" : t(MODEL_KEYS[model]);
    chip.hidden = model === null;
  };
  const setShape = (next: BubbleShape): void => {
    shape = next;
    if (element.dataset.shape !== next) element.dataset.shape = next;
    syncDots();
  };
  const show = (text: string | null, ms?: number, hint?: BubbleHint): void => {
    clearTimeout(timer);
    timer = undefined;
    currentText = text;
    if (text !== null) shownAt = performance.now();
    // textContent only: answer text is never parsed as markup.
    textElement.textContent = text ?? "";
    ensureContent();
    // The chip goes with the bubble: a later line in the same answer shows none.
    if (text === null) setModel(null);
    if (text !== null) {
      showingDots = false;
      element.classList.remove("mate-bubble-dots");
      element.hidden = false;
      if (ms !== undefined) timer = setTimeout(() => show(null), ms);
    } else if (shape === "thought" && !isSleeping()) {
      syncDots();
    } else {
      showingDots = false;
      element.classList.remove("mate-bubble-dots");
      element.hidden = true;
    }
    // The consent picture lives exactly as long as its question (src/hints/view.ts).
    onConsent(hint === "consent" ? text : null);
  };
  return {
    setModel,
    setShape,
    show,
    hideAfter(ms) {
      clearTimeout(timer);
      timer = undefined;
      // Nothing lingers when the bubble has no text: the chip goes now.
      if (currentText === null || element.hidden) {
        setModel(null);
        return;
      }
      timer = setTimeout(
        () => show(null),
        bubbleLingerMs(currentText, performance.now() - shownAt, ms),
      );
    },
  };
}

/** The stamp, and the beam's target it must never cover. */
interface LiveStamp extends Stamp {
  avoid(target: CssRect | null): void;
}

function mountStamp(
  element: HTMLElement,
  isSleeping: () => boolean,
  onVerdict?: (verdict: SafetyVerdict | null) => void,
): LiveStamp {
  let verdict: SafetyVerdict | null = null;
  let target: CssRect | null = null;
  let timer: ReturnType<typeof setTimeout> | undefined;
  const render = (): void => {
    if (verdict !== null) {
      element.textContent = t(STAMP_KEYS[verdict]);
      element.dataset.verdict = verdict;
    }
    // Invisible rather than removed, so it does not stamp down again once the beam moves on.
    element.classList.toggle("is-covering", !stampClearOf(target, window.innerWidth, window.innerHeight));
    element.hidden = verdict === null;
  };
  const setVerdict = (next: SafetyVerdict | null, syncBubble: boolean): void => {
    clearTimeout(timer);
    timer = undefined;
    verdict = isSleeping() ? null : next;
    render();
    if (syncBubble) onVerdict?.(verdict);
  };
  return {
    show: (next) => setVerdict(next, true),
    hideAfter(ms) {
      clearTimeout(timer);
      timer = verdict === null ? undefined : setTimeout(() => setVerdict(null, false), ms);
    },
    avoid(next) {
      target = next;
      render();
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
  stamp: LiveStamp,
  hints: Hints | null,
  settings: MateSettingsBinding,
): LiveAnswers {
  let uiLocale = getLocale();
  let unsubscribe: (() => void) | null = null;
  const ready = createRouter(robot, bubble, stamp, hints, settings);
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
  stamp: LiveStamp,
  hints: Hints | null,
  settings: MateSettingsBinding,
): Promise<(msg: AnswerMsg) => void> {
  const audio = await createAudioEngine({
    settings: toAudioSettings(settings.get()),
  });
  settings.setAudio(audio);
  const updateAudioActivity = (): void => {
    const state = document.documentElement.dataset.mateState;
    audio.setIdle(state === "idle" || state === "sleeping");
    audio.setInactive(document.hidden || state === "sleeping");
  };
  document.addEventListener("visibilitychange", updateAudioActivity);
  document.addEventListener(MATE_STATE_EVENT, updateAudioActivity);
  updateAudioActivity();
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
      interruptLesson(event.payload, (generation) => audio.interrupt(generation));
    })
    .catch((error: unknown) => {
      console.error("[mate] lesson-interrupt listener failed", error);
    });
  const router = createMateAnswerRouter({
    robot, bubble, stamp,
    player: {
      push: (generation, item) => player.push(generation, item),
      cancel: (generation) => player.cancel(generation),
    },
    t, stopOnboarding, reportPlayback,
    sleep: (ms) => new Promise<void>((resolve) => setTimeout(resolve, ms)),
    onPcm: (msg) => {
      const samples = decodePcm(msg.samplesB64);
      audio.pushPcm({
        generationId: msg.generation,
        sentenceId: msg.sentence,
        sampleRate: msg.sampleRate,
        eos: msg.eos,
        samples,
      });
      void reportPlayback("pcm", msg.generation, `s=${msg.sentence} n=${samples.length}`);
    },
  });
  // "Is this safe?" (src-tauri/src/voice.rs): only this event says an answer is a verdict.
  void getCurrentWebviewWindow()
    .listen<unknown>(SAFETY_CHECK_EVENT, (event) => router.markSafety(event.payload))
    .catch((error: unknown) => {
      console.error("[mate] safety-check listener failed", error);
    });
  const player: TimelinePlayer = createTimelinePlayer({
    // Answer tags select speech voices; UI labels keep the person's preference.
    setLocale: () => setLocale(settings.get().language),
    robot: {
      setState: (state) => router.present("state", () => router.setState(state)),
      setMood: (mood) => router.present("mood", () => router.setMood(mood)),
      aimAt: (target) => router.present("aim", () => {
        robot.aimAt(target);
        stamp.avoid(target);
        hints?.avoid(target);
      }),
    },
    audio,
    resolveTarget: (point, generation, step) => pointAt(generation, point, step),
    openTarget: async (generation) => {
      const { invoke } = await import("@tauri-apps/api/core");
      const result = await invoke<{ name: string; opened: boolean } | null>(
        "open_answer_target", { generation },
      );
      return result ? t(result.opened ? "open.opening" : "open.failed", { name: result.name }) : null;
    },
    showBubble: router.showBubble,
    onLine: router.onLine,
    onDone: (generation) => {
      router.onDone(generation);
      hints?.answered();
    },
  });
  return (msg: AnswerMsg) => {
    // A question: the talk hint stops after three (src/hints/logic.ts).
    if (msg.type === "state" && msg.state === "thinking") hints?.asked(msg.generation);
    router.route(msg);
  };
}

const container = document.getElementById("mate");
if (container) {
  const mountedRobot = mountRobot(container);
  let sleeping = false;
  let currentState: RobotState = "idle";
  let currentMood: RobotMood = "calm";
  let currentVerdict: SafetyVerdict | null = null;
  // Mounted once the settings are (below): the bubble tells them about the consent question.
  let hints: Hints | null = null;
  const bubble = document.getElementById("mate-bubble");
  const mountedBubble = bubble
    ? mountBubble(bubble, () => sleeping, (text) => hints?.consent(text))
    : null;
  const syncBubbleShape = (): void => {
    mountedBubble?.setShape?.(bubbleShape(currentState, currentMood, currentVerdict));
  };
  const robot: RobotController = {
    ...mountedRobot,
    setState: (state) => {
      const shown = sleeping ? "sleeping" : state;
      currentState = shown;
      mountedRobot.setState(shown);
      document.documentElement.dataset.mateState = shown;
      syncBubbleShape();
      document.dispatchEvent(new Event(MATE_STATE_EVENT));
    },
    setMood: (mood) => {
      currentMood = mood;
      mountedRobot.setMood(mood);
      syncBubbleShape();
    },
  };
  const updateVisibility = (): void => {
    document.documentElement.classList.toggle("mate-page-hidden", document.hidden);
  };
  document.addEventListener("visibilitychange", updateVisibility);
  updateVisibility();
  robot.setMood("calm");
  robot.setState("idle");
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
    // Native polling distinguishes a click from a drag and never activates the window. The
    // command returns when the drag (or click) is over: he lands if the drag moved him.
    void tauriInvoke("mate_drag_start", { x: event.clientX, y: event.clientY })
      .then(() => robot.release())
      .catch((error: unknown) => {
        console.error("mate: drag:", error);
      });
  });
  if (inTauri) {
    // Rust moves the window (a drag or a flight); he leans into it (src/robot/motion.ts).
    void import("@tauri-apps/api/webviewWindow")
      .then(({ getCurrentWebviewWindow }) =>
        getCurrentWebviewWindow().onMoved(({ payload }) => {
          const ratio = window.devicePixelRatio || 1;
          robot.moved(payload.x / ratio, payload.y / ratio);
        }),
      )
      .catch((error: unknown) => {
        console.error("mate: moves:", error);
      });
  }
  const motionQuery =
    typeof window.matchMedia === "function" ? window.matchMedia(REDUCED_MOTION_QUERY) : null;
  let refreshTrial = (): void => {};
  let shownLanguage: string | null = null;
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
      onChange: (updated) => {
        applyTheme(updated, document.documentElement);
        // Reapplying on every resize, the menu's own included, must not close the menu it
        // just opened (it opened and closed at once); only a new language rebuilds its labels.
        const languageChanged = updated.language !== shownLanguage;
        shownLanguage = updated.language;
        setLocale(updated.language);
        document.documentElement.lang = getLocale();
        setOnboardingPttKey(updated.pttKey);
        refreshTrial();
        if (languageChanged) menu?.close();
      },
    },
  );
  void settings.ready.then(() => mountOnboarding(undefined, null));
  const onEnvironmentChange = (): void => {
    settings.reapply();
  };
  motionQuery?.addEventListener("change", onEnvironmentChange);
  window.addEventListener("resize", onEnvironmentChange);
  // The consent picture and the talk and drag reminders (src/hints/view.ts).
  const hintElement = document.getElementById("mate-hint");
  hints = hintElement
    ? mountHints({
        element: hintElement,
        t,
        talkKey: () => {
          const current = settings.get();
          return { label: pttKeycapLabel(current.pttKey, t), hold: current.ptt === "hold" };
        },
      })
    : null;
  // An answer already playing must not replace the hide explanation or sleeping state.
  const liveBubble = mountedBubble ? awakeBubble(mountedBubble, () => sleeping) : null;
  const stampElement = document.getElementById("mate-stamp");
  const stamp: LiveStamp = stampElement
    ? mountStamp(stampElement, () => sleeping, (verdict) => {
        currentVerdict = verdict;
        syncBubbleShape();
      })
    : { show() {}, hideAfter() {}, avoid() {} };
  const live =
    liveBubble && inTauri ? startLiveAnswers(robot, liveBubble, stamp, hints, settings) : null;
  if (mountedBubble && inTauri) {
    const events = createMateWindowEvents({
      bubble: mountedBubble, robot,
      isSleeping: () => sleeping,
      setSleeping: (value) => { sleeping = value; },
      closeMenu: () => menu?.close(),
      stopOnboarding,
      hidingText: (minutes) => t("mate.hiding", {
        time: hideReturnTime(minutes, Date.now(), getLocale()),
        key: pttKeyLabel(settings.get().pttKey, t),
      }),
      explainText: (control) => t("mate.explain_offer", { control }),
    });
    void import("@tauri-apps/api/event").then(async ({ listen }) => {
      await listen<unknown>("highlight", (event) => {
        const msg = parseHighlightMsg(event.payload);
        if (msg?.type === "warmth") robot.setWarmth(msg.level, msg.onTarget);
        else if (msg?.type === "clear") robot.setWarmth(null, false);
      });
      await listen("mate-menu-outside", () => menu?.close());
      await listen<{ minutes: number }>("mate-hiding", (event) => {
        stamp.show(null);
        events.hide(event.payload.minutes);
      });
      await listen("mate-woke", () => events.wake());
      // The guest trial's count (src-tauri/src/signin.rs): always in view while it runs.
      const trial = document.getElementById("mate-trial");
      let trialUsage: unknown = null;
      const showTrial = (usage: unknown): void => {
        trialUsage = usage;
        if (!trial) return;
        const counted = guestTrialView(usage);
        trial.hidden = counted === null;
        if (counted) {
          trial.textContent = t("mate.trial", { left: counted.left, limit: counted.limit });
          trial.classList.toggle("is-empty", counted.empty);
        }
      };
      refreshTrial = () => showTrial(trialUsage);
      await listen<unknown>("guest-usage", (event) => showTrial(event.payload));
      await listen<unknown>("auth-changed", (event) => {
        if (signedIn(event.payload)) showTrial(null);
      });
      const { invoke } = await import("@tauri-apps/api/core");
      showTrial(await invoke("guest_usage").catch(() => null));
    }).catch((error: unknown) => {
      console.error("mate: presence events:", error);
    });
    void import("@tauri-apps/api/event").then(({ listen }) =>
      listen("home-closed-hint", () => {
        if (sleeping) return;
        const text = t("home.closed_hint");
        robot.setMood("calm");
        mountedBubble.setModel(null);
        mountedBubble.show(text, bubbleReadMs(text));
      }),
    ).catch((error: unknown) => {
      console.error("mate: home hint:", error);
    });
    // Mate let go on a control (src-tauri/src/explain.rs): one click on him asks what it does.
    void import("@tauri-apps/api/event").then(({ listen }) =>
      listen<{ control?: unknown; ms?: unknown } | null>("explain-offer", (event) => {
        events.explain(event.payload);
      }),
    ).catch((error: unknown) => {
      console.error("mate: explain offer:", error);
    });
  }
  if (import.meta.env.DEV) {
    window.__mate = devHook(robot, stamp, hints, (locale) => {
      if (live) live.setUiLocale(locale);
      else setLocale(locale);
    });
  }
}
