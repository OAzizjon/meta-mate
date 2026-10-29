// Mate's two channels (docs/robot.md "States"): the state (what he is doing, set by the app)
// and the mood (his face, set by MateScript <mood>). A state may override the face, some only
// briefly (oops, success), then the mood returns. Loops live in robot.css, keyed on the
// data-state / data-mood / data-cheeks / data-brows attributes set here; one-shot moves use
// Web Animations.

import type { BeamController } from "./beam.ts";
import {
  BEAM_HALF_WIDTH,
  MOOD_FACES,
  THINKING_GAZE,
  armAim,
  clamp,
  cssToView,
  gazeToward,
  nozzleInView,
  rectCenter,
  viewScale,
  viewToCss,
} from "./geometry.ts";
import type { BrowStyle, CheekStyle, EyeShape, Face, MouthShape, ViewPoint } from "./geometry.ts";
import { setArmPose, setAttr, setEyes, setGaze, setMouth, setMouthOpen } from "./svg.ts";
import type { MateSvgRefs } from "./svg.ts";
import type { CssPoint, CssRect, RobotMood, RobotState } from "./types.ts";

export const LOOKING_SWEEP_MS = 600;

export interface StateAnimator {
  setState(state: RobotState): void;
  setMood(mood: RobotMood): void;
  /** The voice level 0..1 while speaking (RMS, at animation-frame rate). */
  setVoiceLevel(rms: number): void;
  /** The mouse in client CSS pixels; the eyes follow it a little in idle. */
  setEyeTarget(cursor: CssPoint | null): void;
  /** What the beam shines on in pointing, waiting_for_click and oops (client CSS pixels). */
  aimAt(target: CssRect | null): void;
  /** Re-aims after Mate moved or changed size. */
  relayout(): void;
  setFunMode(fun: boolean): void;
  setReduceMotion(reduce: boolean): void;
  destroy(): void;
}

interface FaceOverride {
  readonly eyes?: EyeShape;
  readonly mouth?: MouthShape;
  readonly cheeks?: CheekStyle;
  readonly brows?: BrowStyle;
  /** Shown this long after entering the state, then the mood's face returns. */
  readonly ms?: number;
}

const STATE_FACES: Partial<Record<RobotState, FaceOverride>> = {
  listening: { eyes: "wide", brows: "up" },
  looking: { eyes: "open", brows: "rest" },
  thinking: { brows: "raised" },
  success: { eyes: "happy", mouth: "grin", brows: "up", ms: 1200 },
  oops: { eyes: "squint", brows: "raised", ms: 600 },
  cant_find: { brows: "tilted" },
  sleeping: { eyes: "closed", mouth: "small", brows: "low" },
};

interface OneShot {
  readonly keyframes: Keyframe[];
  readonly options: KeyframeAnimationOptions;
}

// Moves of the whole body, skipped when motion is reduced.
const BODY_MOVES: Partial<Record<RobotState, OneShot>> = {
  success: {
    keyframes: [{ transform: "translateY(0)" }, { transform: "translateY(-14px)", offset: 0.4 }, { transform: "translateY(0)" }],
    options: { duration: 400, easing: "ease-out" },
  },
  oops: {
    keyframes: [
      { transform: "rotate(0deg)" },
      { transform: "rotate(8deg)", offset: 0.25 },
      { transform: "rotate(8deg)", offset: 0.7 },
      { transform: "rotate(0deg)" },
    ],
    options: { duration: 600, easing: "ease-in-out" },
  },
};

// Eye moves, skipped when motion is reduced.
const EYE_MOVES: Partial<Record<RobotState, OneShot>> = {
  looking: {
    keyframes: [
      { transform: "translateX(0)" },
      { transform: "translateX(-4px)", offset: 0.3 },
      { transform: "translateX(4px)", offset: 0.7 },
      { transform: "translateX(0)" },
    ],
    options: { duration: LOOKING_SWEEP_MS, easing: "ease-in-out", fill: "forwards" },
  },
};

/** The glow at rest (robot.css) and how much the voice adds while speaking. */
const AURA_REST = 0.45;
const AURA_VOICE = 0.35;

// success's bright flash: a fade, so it stays with reduced motion.
const FLASH: OneShot = { keyframes: [{ opacity: 1 }, { opacity: AURA_REST }], options: { duration: 600, easing: "ease-out" } };

const BEAM_STATES: ReadonlySet<RobotState> = new Set<RobotState>(["pointing", "waiting_for_click", "oops"]);
const BLINKABLE: ReadonlySet<EyeShape> = new Set<EyeShape>(["open", "wide"]);
const BLINK_MS = 140;
const BLINK_MIN_GAP_MS = 3000;
const BLINK_GAP_SPREAD_MS = 3000;
/** Below this voice level the mouth rests in the mood's shape. */
const VOICE_OPEN = 0.06;
const NO_GAZE: ViewPoint = { x: 0, y: 0 };

function boxOf(el: Element): CssRect {
  const r = el.getBoundingClientRect();
  return { left: r.left, top: r.top, right: r.right, bottom: r.bottom };
}

function isFiniteRect(rect: CssRect): boolean {
  return [rect.left, rect.top, rect.right, rect.bottom].every(Number.isFinite);
}

export function createStateAnimator(refs: MateSvgRefs, beam: BeamController): StateAnimator {
  let state: RobotState = "idle";
  let mood: RobotMood = "calm";
  let voice = 0;
  let cursor: CssPoint | null = null;
  let target: CssRect | null = null;
  let reduceMotion = false;
  let blinking = false;
  let overrideOn = false;
  let overrideTimer: ReturnType<typeof setTimeout> | undefined;
  let blinkTimer: ReturnType<typeof setTimeout> | undefined;
  let oneShots: Animation[] = [];

  function isMotionReduced(): boolean {
    if (reduceMotion) return true;
    if (typeof refs.svg.classList?.contains === "function" && refs.svg.classList.contains("reduce-motion")) {
      return true;
    }
    if (typeof refs.svg.closest === "function" && refs.svg.closest(".reduce-motion") !== null) {
      return true;
    }
    return false;
  }

  function face(): Face {
    const base = MOOD_FACES[mood];
    const own = STATE_FACES[state];
    if (!own || (own.ms !== undefined && !overrideOn)) return base;
    return {
      eyes: own.eyes ?? base.eyes,
      mouth: own.mouth ?? base.mouth,
      cheeks: own.cheeks ?? base.cheeks,
      brows: own.brows ?? base.brows,
    };
  }

  function gaze(): ViewPoint {
    if (state === "looking") return NO_GAZE;
    if (state === "thinking" || mood === "thinking") return THINKING_GAZE;
    if (state !== "idle" || !cursor) return NO_GAZE;
    const box = boxOf(refs.svg);
    return viewScale(box) > 0 ? gazeToward(cssToView(box, cursor)) : NO_GAZE;
  }

  function renderFace(): void {
    const f = face();
    setEyes(refs, blinking && state !== "looking" && BLINKABLE.has(f.eyes) ? "closed" : f.eyes);
    if (state === "speaking" && voice >= VOICE_OPEN) setMouthOpen(refs, voice);
    else setMouth(refs, f.mouth);
    setAttr(refs.svg, "data-cheeks", f.cheeks);
    setAttr(refs.svg, "data-brows", f.brows);
    setGaze(refs, BLINKABLE.has(f.eyes) ? gaze() : NO_GAZE);
  }

  function renderAura(): void {
    const aura = refs.layers.aura;
    if (state === "speaking") aura.style.opacity = (AURA_REST + voice * AURA_VOICE).toFixed(3);
    else aura.style.removeProperty("opacity");
  }

  function renderPose(): void {
    if (state === "cant_find") {
      setArmPose(refs, "shrug");
      beam.hide();
      return;
    }
    if (!BEAM_STATES.has(state)) {
      setArmPose(refs, "rest");
      beam.hide();
      return;
    }
    const box = boxOf(refs.svg);
    const scale = viewScale(box);
    if (!target || scale <= 0) {
      setArmPose(refs, "point");
      beam.hide();
      return;
    }
    const aim = armAim(cssToView(box, rectCenter(target)));
    setArmPose(refs, "point", aim);
    beam.aimAt(viewToCss(box, nozzleInView(aim)), target, BEAM_HALF_WIDTH * scale);
    beam.setPulse(state === "waiting_for_click" && !isMotionReduced());
  }

  function render(): void {
    setAttr(refs.svg, "data-state", state);
    setAttr(refs.svg, "data-mood", mood);
    renderFace();
    renderAura();
    renderPose();
  }

  function startOverride(): void {
    clearTimeout(overrideTimer);
    const ms = STATE_FACES[state]?.ms;
    overrideOn = ms !== undefined;
    if (ms === undefined) return;
    overrideTimer = setTimeout(() => {
      overrideOn = false;
      renderFace();
    }, ms);
  }

  function stopOneShots(): void {
    for (const animation of oneShots) animation.cancel();
    oneShots = [];
  }

  function playOneShots(): void {
    stopOneShots();
    const reduced = isMotionReduced();
    const move = BODY_MOVES[state];
    if (move && !reduced) oneShots.push(refs.body.animate(move.keyframes, move.options));
    const eyeMove = EYE_MOVES[state];
    if (eyeMove && !reduced) oneShots.push(refs.eyes.animate(eyeMove.keyframes, eyeMove.options));
    if (state === "success") oneShots.push(refs.layers.aura.animate(FLASH.keyframes, FLASH.options));
  }

  function scheduleBlink(): void {
    blinkTimer = setTimeout(blink, BLINK_MIN_GAP_MS + Math.random() * BLINK_GAP_SPREAD_MS);
  }

  function blink(): void {
    blinking = true;
    renderFace();
    blinkTimer = setTimeout(() => {
      blinking = false;
      renderFace();
      scheduleBlink();
    }, BLINK_MS);
  }

  render();
  scheduleBlink();

  return {
    setState(next) {
      // A repeated state replays its one-shot (a second oops tilts again).
      state = next;
      voice = 0;
      startOverride();
      render();
      playOneShots();
    },
    setMood(next) {
      mood = next;
      setAttr(refs.svg, "data-mood", mood);
      renderFace();
    },
    setVoiceLevel(rms) {
      voice = clamp(rms, 0, 1);
      if (state !== "speaking") return;
      renderFace();
      renderAura();
    },
    setEyeTarget(next) {
      cursor = next && Number.isFinite(next.x) && Number.isFinite(next.y) ? next : null;
      if (state === "idle") renderFace();
    },
    aimAt(next) {
      target = next && isFiniteRect(next) ? next : null;
      renderPose();
    },
    relayout() {
      renderPose();
    },
    setFunMode(fun) {
      refs.svg.classList.toggle("is-fun", fun);
    },
    setReduceMotion(reduce) {
      reduceMotion = reduce;
      refs.svg.classList.toggle("reduce-motion", reduce);
      if (isMotionReduced()) stopOneShots();
      renderPose();
    },
    destroy() {
      clearTimeout(overrideTimer);
      clearTimeout(blinkTimer);
      stopOneShots();
      beam.hide();
    },
  };
}
