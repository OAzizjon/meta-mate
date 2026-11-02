// Mate's surprise entrance in the highlight window (the `"mate-jump"` event): a giant Mate, the
// same drawing as in his own window (src/robot/svg.ts), leaps up from the bottom edge with a
// flash of his bulb, a short shake of the whole overlay and a "BOO!" balloon, then winks and
// drops back out. src/highlight/jump.ts holds the cues; afterwards nothing is left in the page.

import "../robot/robot.css";
import { getLocale, setLocale, t } from "../i18n/index.ts";
import type { LocaleKey } from "../i18n/index.ts";
import { buildMateSvg, setArmPose, setEyes, setLeftHandRaised, setMouth, svgEl } from "../robot/svg.ts";
import type { MateSvgRefs } from "../robot/svg.ts";
import { computeBurstPolygon, formatPolygonPoints, num } from "./geometry.ts";
import {
  JUMP_EXIT_MS,
  JUMP_FADE_MS,
  JUMP_FLASH_MS,
  JUMP_LEAP_MS,
  JUMP_POP_MS,
  JUMP_SHAKE_MS,
  jumpTimeline,
  parseJumpMsg,
  shakeOffsets,
} from "./jump.ts";
import type { JumpCue, JumpLocale, JumpMsg } from "./jump.ts";

export interface MateJump {
  /** Parses an untrusted `"mate-jump"` payload and plays it if valid; `true` when accepted. */
  handleRaw(raw: unknown): boolean;
  destroy(): void;
}

interface Scene {
  readonly root: HTMLElement;
  readonly flash: HTMLElement;
  readonly rays: HTMLElement;
  readonly mate: HTMLElement;
  readonly refs: MateSvgRefs;
  readonly balloon: HTMLElement;
  readonly line: HTMLElement;
}

// The shout balloon: the lesson burst's jagged star in a 120 x 120 box, stretched wide by CSS.
const BALLOON_BOX = 120;
const BALLOON_POINTS = formatPolygonPoints(computeBurstPolygon({ x: 60, y: 60 }, 52, 40, 16));

// Squash and stretch around his feet (transform-origin in highlight.css).
const LEAP: Keyframe[] = [
  { transform: "translateY(105%) scale(0.8, 1.2)", easing: "cubic-bezier(0.2, 0.7, 0.4, 1)" },
  { transform: "translateY(-6%) scale(0.92, 1.1)", offset: 0.55, easing: "ease-in" },
  { transform: "translateY(0) scale(1.14, 0.86)", offset: 0.78, easing: "ease-out" },
  { transform: "translateY(0) scale(1, 1)" },
];
const DROP: Keyframe[] = [
  { transform: "translateY(0) scale(1, 1)", easing: "ease-out" },
  { transform: "translateY(-4%) scale(1.06, 0.94)", offset: 0.2, easing: "ease-in" },
  { transform: "translateY(115%) scale(0.55, 0.55)" },
];
const POP: Keyframe[] = [
  { opacity: 0, transform: "scale(0.3) rotate(-10deg)" },
  { opacity: 1, transform: "scale(1.12) rotate(3deg)", offset: 0.65 },
  { opacity: 1, transform: "scale(1) rotate(0deg)" },
];
const FLASH: Keyframe[] = [{ opacity: 0 }, { opacity: 0.9, offset: 0.25 }, { opacity: 0 }];
const FADE_IN: Keyframe[] = [{ opacity: 0 }, { opacity: 1 }];
const FADE_OUT: Keyframe[] = [{ opacity: 1 }, { opacity: 0 }];

function div(className: string): HTMLElement {
  const created = document.createElement("div");
  created.className = className;
  return created;
}

/** A line in the payload's language; the page's own language is back right after. */
function textIn(locale: JumpLocale, key: LocaleKey): string {
  const page = getLocale();
  setLocale(locale);
  try {
    return t(key);
  } finally {
    setLocale(page);
  }
}

function buildScene(locale: JumpLocale): Scene {
  const root = div("mate-jump");
  root.setAttribute("aria-hidden", "true");
  const flash = div("mate-jump-flash");
  const rays = div("mate-jump-rays");
  const mate = div("mate-jump-mate");
  const refs = buildMateSvg();
  mate.append(refs.svg);
  const balloon = div("mate-jump-balloon");
  balloon.hidden = true;
  const star = svgEl("svg", {
    viewBox: `0 0 ${BALLOON_BOX} ${BALLOON_BOX}`, preserveAspectRatio: "none", "aria-hidden": "true",
  });
  star.append(svgEl("polygon", { points: BALLOON_POINTS }));
  const shout = document.createElement("span");
  shout.className = "mate-jump-shout";
  shout.textContent = textIn(locale, "highlight.jump_shout");
  balloon.append(star, shout);
  const line = div("mate-jump-line");
  line.hidden = true;
  line.textContent = textIn(locale, "highlight.jump_line");
  root.append(flash, rays, mate, balloon, line);
  return { root, flash, rays, mate, refs, balloon, line };
}

export function mountJump(reducedMotion: () => boolean): MateJump {
  let scene: Scene | null = null;
  let timers: ReturnType<typeof setTimeout>[] = [];
  let animations: Animation[] = [];

  const run = (target: Element, keyframes: Keyframe[], options: KeyframeAnimationOptions): void => {
    animations.push(target.animate(keyframes, options));
  };

  const stop = (): void => {
    for (const timer of timers) clearTimeout(timer);
    timers = [];
    for (const animation of animations) animation.cancel();
    animations = [];
    scene?.root.remove();
    scene = null;
  };

  const reveal = (target: HTMLElement, reduced: boolean): void => {
    target.hidden = false;
    run(target, reduced ? FADE_IN : POP, { duration: reduced ? JUMP_FADE_MS : JUMP_POP_MS });
  };

  const cue = (s: Scene, name: JumpCue, reduced: boolean): void => {
    switch (name) {
      case "enter":
        // "BOO!": wide eyes, an open mouth, both palms up.
        setEyes(s.refs, "wide");
        setMouth(s.refs, "o");
        setArmPose(s.refs, "shrug");
        s.refs.svg.setAttribute("data-brows", "high");
        run(s.mate, reduced ? FADE_IN : LEAP, { duration: reduced ? JUMP_FADE_MS : JUMP_LEAP_MS });
        run(s.rays, FADE_IN, { duration: JUMP_FADE_MS });
        return;
      case "flash":
        run(s.flash, FLASH, { duration: JUMP_FLASH_MS, easing: "ease-out" });
        return;
      case "shake":
        // The whole overlay: the ring, the beam and the voice bar shake with him.
        run(document.body, shakeOffsets().map(({ x, y }) => ({
          transform: `translate(${num(x)}px, ${num(y)}px)`,
        })), { duration: JUMP_SHAKE_MS });
        return;
      case "balloon":
        reveal(s.balloon, reduced);
        return;
      case "line":
        reveal(s.line, reduced);
        return;
      case "wink":
        setEyes(s.refs, "open");
        setMouth(s.refs, "grin");
        setArmPose(s.refs, "rest");
        setLeftHandRaised(s.refs, true);
        s.refs.svg.setAttribute("data-brows", "up");
        s.mate.classList.add("is-winking");
        return;
      case "exit": {
        const fadeOut: KeyframeAnimationOptions = { duration: JUMP_FADE_MS, fill: "forwards" };
        if (reduced) {
          run(s.root, FADE_OUT, fadeOut);
          return;
        }
        run(s.mate, DROP, { duration: JUMP_EXIT_MS, fill: "forwards" });
        for (const part of [s.balloon, s.line, s.rays]) run(part, FADE_OUT, fadeOut);
        return;
      }
      case "done":
        stop();
        return;
    }
  };

  const play = (msg: JumpMsg): void => {
    const reduced = reducedMotion();
    const s = buildScene(msg.locale);
    s.root.classList.toggle("reduce-motion", reduced);
    document.body.append(s.root);
    scene = s;
    for (const step of jumpTimeline(reduced)) {
      // The first cue runs now, so his first frame is already the start of the leap.
      if (step.at === 0) cue(s, step.cue, reduced);
      else timers.push(setTimeout(() => cue(s, step.cue, reduced), step.at));
    }
  };

  return {
    handleRaw(raw) {
      const msg = parseJumpMsg(raw);
      if (!msg) return false;
      // One entrance at a time: another event while he is on screen changes nothing.
      if (!scene) play(msg);
      return true;
    },
    destroy: stop,
  };
}
