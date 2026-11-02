// The voice bar in the highlight window (the `"voice-bar"` event): while the person talks, a
// comic pill at the bottom centre of the monitor with a waveform that follows the microphone
// and the words heard so far above it; thinking calms the bars into a travelling shimmer before
// the pill fades. src/highlight/voicebar.ts holds the logic. The words are the person's own:
// written as text content only, never logged, and gone when the pill is.

import { num } from "./geometry.ts";
import {
  BAR_COUNT,
  BAR_MIN,
  FADE_MS,
  HISTORY_STEP_MS,
  THINKING_MS,
  barHeights,
  loudness,
  parseVoiceBarMsg,
  pushLevel,
  smoothLevel,
  stepBars,
  tailBudget,
  thinkingHeights,
  visibleTail,
} from "./voicebar.ts";
import type { VoiceBarMsg, VoiceBarState } from "./voicebar.ts";

export interface VoiceBar {
  /** Parses an untrusted `"voice-bar"` payload and applies it if valid; `true` when accepted. */
  handleRaw(raw: unknown): boolean;
  destroy(): void;
}

// The pill rises with a small overshoot; with reduced motion it is simply there.
const RISE: Keyframe[] = [
  { transform: "translateY(24px) scale(0.92)" },
  { transform: "translateY(0) scale(1)" },
];
const RISE_MS = 260;
const SINK: Keyframe[] = [
  { opacity: 1, transform: "translateY(0)" },
  { opacity: 0, transform: "translateY(12px)" },
];
const FADE: Keyframe[] = [{ opacity: 1 }, { opacity: 0 }];
// A long frame (a busy page, a hidden window) counts as this many ms at most.
const MAX_FRAME_MS = 100;
// The microphone sends ~12 levels a second while listening: this long without one means a
// lost state change, and the pill must not stay over the person's app.
const SILENT_LIMIT_MS = 4000;

function el(className: string, tag: "div" | "span" = "div"): HTMLElement {
  const created = document.createElement(tag);
  created.className = className;
  return created;
}

export function mountVoiceBar(reducedMotion: () => boolean): VoiceBar {
  const root = el("voice-bar");
  root.setAttribute("aria-hidden", "true");
  root.hidden = true;
  const heard = el("voice-bar-heard");
  heard.hidden = true;
  const words = el("voice-bar-words", "span");
  heard.append(words);
  const row = el("voice-bar-row");
  const wave = el("voice-bar-wave");
  const bars = Array.from({ length: BAR_COUNT }, () => el("voice-bar-bar", "span"));
  wave.append(...bars);
  row.append(el("voice-bar-dot", "span"), wave);
  root.append(heard, row);
  document.body.append(root);

  let state: VoiceBarState = "idle";
  let visible = false;
  let frame = 0;
  let lastFrame = 0;
  let sinceStep = 0;
  let thinkingSince = 0;
  let target = 0;
  let glow = 0;
  let history: number[] = [];
  let shown: number[] = [];
  let thinkingTimer: ReturnType<typeof setTimeout> | undefined;
  let lastMessage = 0;
  let fade: Animation | null = null;

  const draw = (): void => {
    shown.forEach((height, i) => bars[i]?.style.setProperty("--h", num(height)));
    root.style.setProperty("--level", num(glow));
  };

  const tick = (now: number): void => {
    const dt = lastFrame > 0 ? Math.min(MAX_FRAME_MS, Math.max(0, now - lastFrame)) : 0;
    lastFrame = now;
    const shimmer = !reducedMotion();
    let targets: number[];
    if (state === "listening" && now - lastMessage > SILENT_LIMIT_MS) {
      state = "idle";
      fadeOut();
    }
    if (state === "listening") {
      sinceStep += dt;
      if (sinceStep >= HISTORY_STEP_MS) {
        sinceStep %= HISTORY_STEP_MS;
        history = pushLevel(history, target);
      }
      targets = barHeights(history, BAR_COUNT, now, shimmer);
    } else if (state === "thinking") {
      targets = thinkingHeights(BAR_COUNT, now - thinkingSince, shimmer);
    } else {
      targets = new Array<number>(BAR_COUNT).fill(BAR_MIN);
    }
    glow = smoothLevel(glow, state === "listening" ? target : 0, dt);
    shown = stepBars(shown, targets, dt);
    draw();
    frame = requestAnimationFrame(tick);
  };

  const stopThinking = (): void => {
    if (thinkingTimer !== undefined) clearTimeout(thinkingTimer);
    thinkingTimer = undefined;
  };

  const hide = (): void => {
    stopThinking();
    if (frame) cancelAnimationFrame(frame);
    frame = 0;
    lastFrame = 0;
    state = "idle";
    visible = false;
    root.hidden = true;
    fade?.cancel();
    fade = null;
    root.classList.remove("is-thinking");
    words.textContent = "";
    heard.hidden = true;
  };

  const fadeOut = (): void => {
    stopThinking();
    if (!visible || fade) return;
    fade = root.animate(reducedMotion() ? FADE : SINK, {
      duration: FADE_MS, easing: "ease-out", fill: "forwards",
    });
    fade.onfinish = hide;
  };

  const listen = (): void => {
    stopThinking();
    const fresh = state !== "listening";
    fade?.cancel();
    fade = null;
    state = "listening";
    root.classList.remove("is-thinking");
    root.classList.toggle("reduce-motion", reducedMotion());
    if (fresh) {
      // A new turn: nothing heard or measured in the last one.
      target = 0;
      history = [];
      sinceStep = 0;
      words.textContent = "";
      heard.hidden = true;
    }
    if (!visible) {
      // Shown in this very frame: people need to see at once that Mate hears them.
      visible = true;
      glow = 0;
      shown = barHeights([], BAR_COUNT, performance.now(), !reducedMotion());
      draw();
      root.hidden = false;
      if (!reducedMotion()) {
        root.animate(RISE, { duration: RISE_MS, easing: "cubic-bezier(0.34, 1.56, 0.64, 1)" });
      }
    }
    if (!frame) frame = requestAnimationFrame(tick);
  };

  const think = (): void => {
    if (!visible || state !== "listening") return;
    state = "thinking";
    thinkingSince = performance.now();
    root.classList.add("is-thinking");
    thinkingTimer = setTimeout(fadeOut, THINKING_MS - FADE_MS);
  };

  const showWords = (text: string): void => {
    const font = Number.parseFloat(getComputedStyle(words).fontSize);
    const tail = visibleTail(text, tailBudget(row.clientWidth, font));
    // Text content only: the person's words are never markup.
    words.textContent = tail;
    heard.hidden = tail.length === 0;
  };

  const apply = (msg: VoiceBarMsg): void => {
    lastMessage = performance.now();
    if (msg.type === "state") {
      if (msg.state === "listening") listen();
      else if (msg.state === "thinking") think();
      else {
        state = "idle";
        fadeOut();
      }
      return;
    }
    // Only while the pill is up for this turn; never shown without a "listening".
    if (!visible || state === "idle") return;
    if (msg.type === "level") {
      if (state === "listening") target = loudness(msg.level);
      return;
    }
    showWords(msg.text);
  };

  return {
    handleRaw(raw) {
      const msg = parseVoiceBarMsg(raw);
      if (!msg) return false;
      apply(msg);
      return true;
    },
    destroy() {
      hide();
      root.remove();
    },
  };
}
