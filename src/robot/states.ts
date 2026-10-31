// Mate's two channels (docs/robot.md "States"): the state (what he is doing, set by the app)
// and the mood (his face, set by MateScript <mood>). A state may override the face, some only
// briefly (oops, success), then the mood returns. Loops live in robot.css, keyed on the
// data-state / data-mood / data-cheeks / data-brows attributes set here; one-shot moves use
// Web Animations; squash, stretch and lean are springs (motion.ts), held still here.

import type { BeamController } from "./beam.ts";
import { createFrameUpdate } from "./frame-update.ts";
import { NO_MOTION } from "./motion.ts";
import type { MotionController } from "./motion.ts";
import type { SquashKind } from "./spring.ts";
import {
  BEAM_HALF_WIDTH,
  MOOD_FACES,
  SPARK_ANGLES,
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
import { setArmPose, setAttr, setEyes, setGaze, setLeftHandRaised, setMouth, setMouthOpen } from "./svg.ts";
import type { MateSvgRefs } from "./svg.ts";
import type { CssPoint, CssRect, RobotMood, RobotState } from "./types.ts";

export const LOOKING_SWEEP_MS = 600;

export interface StateAnimator {
  setState(state: RobotState): void;
  setMood(mood: RobotMood): void;
  /** The voice level 0..1 while speaking (RMS, at animation-frame rate). */
  setVoiceLevel(rms: number): void;
  setWarmth(level: number | null, onTarget: boolean): void;
  /** The mouse in client CSS pixels; the eyes follow it a little in idle. */
  setEyeTarget(cursor: CssPoint | null): void;
  /** What the beam shines on in pointing, waiting_for_click and oops (client CSS pixels). */
  aimAt(target: CssRect | null): void;
  /** Re-aims after Mate moved or changed size. */
  relayout(): void;
  setFunMode(fun: boolean): void;
  setReduceMotion(reduce: boolean): void;
  setHidden(hidden: boolean): void;
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
  /** The body's squash and stretch that goes with the move. */
  readonly squash?: SquashKind;
}

// Moves of the whole body, skipped when motion is reduced. Jumps crouch first (anticipation).
const BODY_MOVES: Partial<Record<RobotState, OneShot>> = {
  success: {
    keyframes: [
      { transform: "translateY(0)", easing: "ease-in-out" },
      { transform: "translateY(3px)", offset: 0.2, easing: "ease-out" },
      { transform: "translateY(-14px)", offset: 0.5, easing: "ease-in" },
      { transform: "translateY(0)" },
    ],
    options: { duration: 480 },
    squash: "jump",
  },
  oops: {
    keyframes: [
      { transform: "translateY(0)" },
      { transform: "translateY(3px)", offset: 0.25 },
      { transform: "translateY(3px)", offset: 0.7 },
      { transform: "translateY(0)" },
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

// A mood's little reaction when it arrives (a scam verdict flinches, "be careful" jumps, joy
// wiggles). Individual transform properties, so they ride on the idle float; skipped when motion
// is reduced.
const MOOD_MOVES: Partial<Record<RobotMood, OneShot>> = {
  surprised: {
    keyframes: [
      { translate: "0 0", easing: "ease-in-out" },
      { translate: "0 2px", offset: 0.2, easing: "ease-out" },
      { translate: "0 -10px", offset: 0.5, easing: "ease-in" },
      { translate: "0 0" },
    ],
    options: { duration: 440 },
    squash: "jump",
  },
  sad: {
    keyframes: [
      { translate: "0 0" },
      { translate: "-2px 1px", offset: 0.2 },
      { translate: "2px 2px", offset: 0.4 },
      { translate: "-2px 2px", offset: 0.6 },
      { translate: "1px 2px", offset: 0.8 },
      { translate: "0 0" },
    ],
    options: { duration: 520, easing: "ease-in-out" },
    squash: "flinch",
  },
  happy: {
    keyframes: [{ rotate: "0deg" }, { rotate: "-6deg", offset: 0.25 }, { rotate: "6deg", offset: 0.6 }, { rotate: "0deg" }],
    options: { duration: 560, easing: "ease-in-out" },
    squash: "wiggle",
  },
  proud: {
    keyframes: [{ scale: "1" }, { scale: "1.06", offset: 0.4 }, { scale: "1" }],
    options: { duration: 500, easing: "ease-out" },
    squash: "swell",
  },
};

// Hello: the raised left hand winds up the other way, then rocks twice (on start, on waking,
// on coming back from hiding).
const WAVE: OneShot = {
  keyframes: [
    { transform: "rotate(0deg)" },
    { transform: "rotate(6deg)", offset: 0.1 },
    { transform: "rotate(-14deg)", offset: 0.28 },
    { transform: "rotate(10deg)", offset: 0.5 },
    { transform: "rotate(-14deg)", offset: 0.73 },
    { transform: "rotate(0deg)" },
  ],
  options: { duration: 1200, easing: "ease-in-out" },
};

/** How far the success sparks fly out from their places (viewBox units). */
const SPARK_FLIGHT = 16;

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

export function createStateAnimator(
  refs: MateSvgRefs,
  beam: BeamController,
  motion: MotionController = NO_MOTION,
): StateAnimator {
  let state: RobotState = "idle";
  let mood: RobotMood = "calm";
  let voice = 0;
  let warmth = 0;
  let warmthActive = false;
  let onTarget = false;
  let cursor: CssPoint | null = null;
  let target: CssRect | null = null;
  let reduceMotion = false;
  let hidden = false;
  let blinking = false;
  let overrideOn = false;
  let overrideTimer: ReturnType<typeof setTimeout> | undefined;
  let blinkTimer: ReturnType<typeof setTimeout> | undefined;
  let oneShots: Animation[] = [];
  // Mood reactions and the wave outlive a state change: a mood often arrives just before
  // "speaking", which would otherwise cancel its flinch at once.
  let gestures: Animation[] = [];
  let waving: Animation | null = null;
  let greeted = false;
  const voiceFrame = createFrameUpdate(() => {
    if (hidden || state !== "speaking") return;
    renderMouth();
    renderAura();
  });

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
    renderMouth();
    setAttr(refs.svg, "data-cheeks", f.cheeks);
    setAttr(refs.svg, "data-brows", f.brows);
    setGaze(refs, BLINKABLE.has(f.eyes) ? gaze() : NO_GAZE);
  }

  function renderMouth(): void {
    if (state === "speaking" && voice >= VOICE_OPEN) setMouthOpen(refs, voice);
    else setMouth(refs, face().mouth);
  }

  function renderAura(): void {
    const aura = refs.layers.aura;
    if (state === "speaking") {
      const opacity = String(Math.round((AURA_REST + voice * AURA_VOICE) * 100) / 100);
      if (aura.style.opacity !== opacity) aura.style.opacity = opacity;
    }
    else if (state === "waiting_for_click" && warmthActive) {
      aura.style.opacity = String(AURA_REST + warmth * 0.5);
    }
    else aura.style.removeProperty("opacity");
    refs.svg.classList.toggle("has-warmth", warmthActive);
    refs.svg.classList.toggle("warmth-steady", onTarget);
    refs.svg.style.setProperty("--warmth-period", `${1.8 - warmth * 1.2}s`);
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
    beam.setPulse(state === "waiting_for_click" && !onTarget && !isMotionReduced());
  }

  // The springs never pose him while the beam is on: it starts where the flashlight is drawn
  // at rest. Asleep, hidden and with reduced motion he holds still too.
  function syncMotion(): void {
    motion.setStill(hidden || state === "sleeping" || BEAM_STATES.has(state) || isMotionReduced());
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

  function stopWave(): void {
    waving?.cancel();
    waving = null;
    setLeftHandRaised(refs, false);
  }

  function stopGestures(): void {
    for (const animation of gestures) animation.cancel();
    gestures = [];
    stopWave();
  }

  function gesture(animation: Animation, after?: () => void): void {
    gestures.push(animation);
    animation.finished
      ?.then(() => {
        gestures = gestures.filter((other) => other !== animation);
        after?.();
      })
      .catch(() => {});
  }

  function wave(): void {
    if (hidden || state !== "idle" || isMotionReduced()) return;
    stopWave();
    motion.squash("hello");
    setLeftHandRaised(refs, true);
    const animation = refs.layers["arm-l"].animate(WAVE.keyframes, WAVE.options);
    waving = animation;
    animation.finished
      ?.then(() => {
        if (waving === animation) stopWave();
      })
      .catch(() => {});
  }

  function burstSparks(): void {
    const sparks = Array.from(refs.layers.sparks.children) as SVGElement[];
    sparks.forEach((spark, i) => {
      const angle = ((SPARK_ANGLES[i] ?? 0) * Math.PI) / 180;
      const dx = Math.round(Math.cos(angle) * SPARK_FLIGHT);
      const dy = Math.round(Math.sin(angle) * SPARK_FLIGHT);
      oneShots.push(
        spark.animate(
          [
            { transform: "translate(0px, 0px) scale(0)", opacity: 0 },
            { transform: `translate(${dx / 2}px, ${dy / 2}px) scale(1.25)`, opacity: 1, offset: 0.35 },
            { transform: `translate(${dx}px, ${dy}px) scale(0)`, opacity: 0 },
          ],
          { duration: 800, delay: i * 40, easing: "ease-out", fill: "both" },
        ),
      );
    });
  }

  function playOneShots(): void {
    stopOneShots();
    if (hidden || state === "sleeping") return;
    const reduced = isMotionReduced();
    const move = BODY_MOVES[state];
    if (move && !reduced) {
      oneShots.push(refs.body.animate(move.keyframes, move.options));
      if (move.squash) motion.squash(move.squash);
    }
    const eyeMove = EYE_MOVES[state];
    if (eyeMove && !reduced) oneShots.push(refs.eyes.animate(eyeMove.keyframes, eyeMove.options));
    if (state === "success") oneShots.push(refs.layers.aura.animate(FLASH.keyframes, FLASH.options));
    if (state === "success" && !reduced) burstSparks();
  }

  function scheduleBlink(): void {
    if (hidden || state === "sleeping") return;
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
  syncMotion();
  scheduleBlink();

  return {
    setState(next) {
      // A repeated state replays its one-shot (a second oops dips again).
      const woke = state === "sleeping" && next !== "sleeping";
      if (next !== "idle") stopWave();
      // A mood move already running would pull the flashlight off the target.
      if (BEAM_STATES.has(next)) stopGestures();
      state = next;
      syncMotion();
      voiceFrame.cancel();
      clearTimeout(blinkTimer);
      blinking = false;
      scheduleBlink();
      voice = 0;
      startOverride();
      render();
      playOneShots();
      if (next === "idle" && (woke || !greeted)) {
        greeted = true;
        wave();
      }
    },
    setMood(next) {
      const changed = next !== mood;
      mood = next;
      setAttr(refs.svg, "data-mood", mood);
      renderFace();
      const move = MOOD_MOVES[mood];
      if (changed && move && !hidden && state !== "sleeping" && !BEAM_STATES.has(state) && !isMotionReduced()) {
        // Never while the beam is on a target: the body would pull the flashlight off it.
        gesture(refs.body.animate(move.keyframes, move.options));
        if (move.squash) motion.squash(move.squash);
      }
    },
    setWarmth(level, steady) {
      warmthActive = level !== null;
      warmth = level === null ? 0 : clamp(Number.isFinite(level) ? level / 9 : 0, 0, 1);
      onTarget = steady;
      renderAura();
      renderPose();
    },
    setVoiceLevel(rms) {
      const next = Math.round(clamp(Number.isFinite(rms) ? rms : 0, 0, 1) * 100) / 100;
      if (voice === next) return;
      voice = next;
      if (state === "speaking" && !hidden) voiceFrame.schedule();
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
      if (isMotionReduced()) {
        stopOneShots();
        stopGestures();
      }
      syncMotion();
      renderPose();
    },
    setHidden(next) {
      if (hidden === next) return;
      hidden = next;
      syncMotion();
      clearTimeout(blinkTimer);
      blinking = false;
      voiceFrame.cancel();
      if (hidden) {
        stopOneShots();
        stopGestures();
      } else {
        renderFace();
        renderAura();
        scheduleBlink();
        wave();
      }
    },
    destroy() {
      voiceFrame.cancel();
      clearTimeout(overrideTimer);
      clearTimeout(blinkTimer);
      stopOneShots();
      stopGestures();
      beam.hide();
    },
  };
}
