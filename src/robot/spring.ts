// Mate's springs (docs/robot.md "Flexibility"): the body leans into the window's motion,
// stretches along it and squashes on hello, mood jumps and landings, always keeping its area;
// the arms, the cap and the filament trail the body. Pure: no DOM and no clock, so motion.ts
// drives it from animation frames and Node tests drive it directly. Angles in degrees, squash
// and stretch as a ratio minus 1, positions in CSS pixels, times in milliseconds.

import { clamp } from "./geometry.ts";

export interface SpringParams {
  readonly stiffness: number;
  readonly damping: number;
}

export interface Spring {
  readonly value: number;
  readonly velocity: number;
}

export const AT_REST: Spring = { value: 0, velocity: 0 };

/** The body: ~2.5 Hz at damping ratio 0.25, about two visible swings before rest. */
export const WOBBLE: SpringParams = { stiffness: 250, damping: 8 };

/**
 * A part that trails its leader by `lagMs` (a steady motion's lag is 2ζ/ω) and overshoots a
 * little when the leader stops (ζ = 0.55: about 12 %).
 */
export function followSpring(lagMs: number, dampingRatio = 0.55): SpringParams {
  const omega = (2 * dampingRatio * 1000) / lagMs;
  return { stiffness: omega * omega, damping: 2 * dampingRatio * omega };
}

/** How far each part trails the body (ms): the arms first, the filament last (overlap). */
export const FOLLOW_LAG_MS = { arms: 65, cap: 90, filament: 115 } as const;

/** Integration step, small enough for the stiffest spring here. */
const STEP_MS = 4;
/** A longer gap (a slow or skipped frame) is not simulated: the springs just carry on. */
const MAX_FRAME_MS = 64;

/** Advances `spring` toward `target` by `dtMs` (semi-implicit Euler in small steps). */
export function stepSpring(spring: Spring, target: number, params: SpringParams, dtMs: number): Spring {
  let { value, velocity } = spring;
  let left = clamp(dtMs, 0, MAX_FRAME_MS);
  while (left > 0) {
    const h = Math.min(left, STEP_MS) / 1000;
    velocity += (-params.stiffness * (value - target) - params.damping * velocity) * h;
    value += velocity * h;
    left -= STEP_MS;
  }
  return { value, velocity };
}

export interface MoveSample {
  readonly x: number;
  readonly y: number;
  readonly t: number;
}

export interface Velocity {
  readonly x: number;
  readonly y: number;
}

const NO_VELOCITY: Velocity = { x: 0, y: 0 };
/** Velocity is the mean over this much of the newest moves (IPC delivers them unevenly). */
export const VELOCITY_WINDOW_MS = 100;
/** Moves closer together than this give no velocity yet. */
const MIN_SPAN_MS = 12;
/** No move for this long: he has stopped. */
export const STALE_MS = 80;
/** Faster than any drag or flight: the window jumped (a new home, another monitor's scale). */
export const JUMP_PX_PER_S = 10000;

/** The samples to keep after `next`: the newest window, or `next` alone after a jump. */
export function addSample(samples: readonly MoveSample[], next: MoveSample): MoveSample[] {
  if (![next.x, next.y, next.t].every(Number.isFinite)) return [...samples];
  const last = samples[samples.length - 1];
  if (last) {
    const dt = next.t - last.t;
    if (dt < 0) return [next];
    if (dt > 0 && (Math.hypot(next.x - last.x, next.y - last.y) * 1000) / dt > JUMP_PX_PER_S) return [next];
  }
  return [...samples.filter((sample) => next.t - sample.t <= VELOCITY_WINDOW_MS), next];
}

/** CSS pixels per second over the kept samples; none once they are stale at `now`. */
export function velocityOf(samples: readonly MoveSample[], now: number): Velocity {
  const first = samples[0];
  const last = samples[samples.length - 1];
  if (!first || !last || now - last.t > STALE_MS) return NO_VELOCITY;
  const span = last.t - first.t;
  if (span < MIN_SPAN_MS) return NO_VELOCITY;
  return { x: ((last.x - first.x) * 1000) / span, y: ((last.y - first.y) * 1000) / span };
}

/** Lean per CSS pixel per second of sideways speed, and the most either way. */
export const LEAN_DEG_PER_PX_S = 0.012;
export const MAX_LEAN_DEG = 12;
/** Stretch along the motion per pixel per second, and the most (0.08 = 8 % longer). */
const STRETCH_PER_PX_S = 0.00006;
export const MAX_STRETCH = 0.08;
/** No squash or stretch goes past this, whatever kicks add up. */
export const MAX_SQUASH = 0.15;
/** Slower moves neither start a landing nor turn the stretch axis. */
const MIN_SPEED = 40;

export function leanFor(vx: number): number {
  return clamp(vx * LEAN_DEG_PER_PX_S, -MAX_LEAN_DEG, MAX_LEAN_DEG);
}

export function stretchFor(speed: number): number {
  return clamp(speed * STRETCH_PER_PX_S, 0, MAX_STRETCH);
}

/** A volume-keeping pair: `long` along an axis, `short` across it, long × short = 1. */
export function scalePair(amount: number): { readonly long: number; readonly short: number } {
  const a = clamp(amount, -MAX_SQUASH, MAX_SQUASH);
  return { long: 1 + a, short: 1 / (1 + a) };
}

export interface MotionPose {
  /** The body's lean (+ leans right, toward a rightward move). */
  readonly leanDeg: number;
  /** Longer along `axisDeg` (the motion's direction, y down) and thinner across it. */
  readonly stretch: number;
  readonly axisDeg: number;
  /** Taller and thinner (+) or shorter and wider (-). */
  readonly squash: number;
  /** The parts' own turns against the body, and the cap's lag downward (viewBox units). */
  readonly filamentDeg: number;
  readonly capDeg: number;
  readonly capDrop: number;
  readonly armLDeg: number;
  readonly armRDeg: number;
}

export const REST_POSE: MotionPose = {
  leanDeg: 0,
  stretch: 0,
  axisDeg: 0,
  squash: 0,
  filamentDeg: 0,
  capDeg: 0,
  capDrop: 0,
  armLDeg: 0,
  armRDeg: 0,
};

/**
 * The body's CSS matrix(a, b, c, d): lean · stretch along the axis · vertical squash. Every
 * factor has determinant 1, so the glass keeps its area.
 */
export function bodyMatrix(pose: MotionPose): readonly [number, number, number, number] {
  const along = scalePair(pose.stretch);
  const up = scalePair(pose.squash);
  const axis = (pose.axisDeg * Math.PI) / 180;
  const ac = Math.cos(axis);
  const as = Math.sin(axis);
  // R(axis) · diag(long, short) · R(-axis), then · diag(1 / up, up).
  const s11 = along.long * ac * ac + along.short * as * as;
  const s12 = (along.long - along.short) * ac * as;
  const s22 = along.long * as * as + along.short * ac * ac;
  const m11 = s11 * up.short;
  const m12 = s12 * up.long;
  const m21 = s12 * up.short;
  const m22 = s22 * up.long;
  const lean = (clamp(pose.leanDeg, -MAX_LEAN_DEG, MAX_LEAN_DEG) * Math.PI) / 180;
  const lc = Math.cos(lean);
  const ls = Math.sin(lean);
  // x' = a x + c y, y' = b x + d y.
  return [lc * m11 - ls * m21, ls * m11 + lc * m21, lc * m12 - ls * m22, ls * m12 + lc * m22];
}

export type SquashKind = "hello" | "jump" | "flinch" | "swell" | "wiggle";

/** Kicks to the squash spring (ratio per second): negative squashes first (anticipation). */
export const SQUASH_KICKS: Readonly<Record<SquashKind, number>> = {
  hello: -1,
  jump: -2,
  flinch: -1.2,
  swell: 1.5,
  wiggle: 1,
};
const MAX_KICK = 4;
/** Landing: a slow drag squashes a little, one at this speed (px/s) or faster the most. */
const LAND_KICK_MIN = -0.8;
const LAND_KICK_MAX = -2.4;
const LAND_FULL_SPEED = 1500;

/** How much the trailing parts show their lag. */
const FILAMENT_SWING = 1.4;
const CAP_SWING = 0.6;
const CAP_DROP_PER_SQUASH = 30;
const ARM_FLAP_DEG = 60;
const MAX_PART_DEG = 10;
const MAX_CAP_DROP = 4;

/** Below these the springs snap to rest and the frames stop. */
const ANGLE_EPS = 0.02;
const RATIO_EPS = 0.0005;

export interface MotionModel {
  /** The window's position on screen in CSS pixels, at `t`. */
  moved(x: number, y: number, t: number): void;
  /** A drag ended: if he was moving he lands now; after a plain click nothing happens. */
  release(): void;
  squash(kind: SquashKind): void;
  /** Advances to `t`; still (beam on, reduced motion, asleep, hidden) is the rest pose. */
  step(t: number, still: boolean): MotionPose;
  /** Nothing moves: no frames are needed until the next input. */
  atRest(): boolean;
  reset(): void;
}

interface Trail {
  readonly params: SpringParams;
  lean: Spring;
  squash: Spring;
}

function settled(spring: Spring, eps: number): boolean {
  return Math.abs(spring.value) < eps && Math.abs(spring.velocity) < eps * 10;
}

export function createMotionModel(): MotionModel {
  const trail = (lagMs: number): Trail => ({ params: followSpring(lagMs), lean: AT_REST, squash: AT_REST });
  const arms = trail(FOLLOW_LAG_MS.arms);
  const cap = trail(FOLLOW_LAG_MS.cap);
  const filament = trail(FOLLOW_LAG_MS.filament);
  const trails = [arms, cap, filament];
  let samples: MoveSample[] = [];
  let moving = false;
  let peakSpeed = 0;
  let axisDeg = 0;
  let lean = AT_REST;
  let stretch = AT_REST;
  let squash = AT_REST;
  let lastStep: number | null = null;

  function kick(velocity: number): void {
    squash = { value: squash.value, velocity: clamp(squash.velocity + velocity, -MAX_KICK, MAX_KICK) };
  }

  function land(): void {
    const k = clamp(peakSpeed / LAND_FULL_SPEED, 0, 1);
    kick(LAND_KICK_MIN + (LAND_KICK_MAX - LAND_KICK_MIN) * k);
    samples = [];
    moving = false;
    peakSpeed = 0;
  }

  function atRest(): boolean {
    const angles = [lean, ...trails.map((part) => part.lean)];
    const ratios = [stretch, squash, ...trails.map((part) => part.squash)];
    return !moving && angles.every((s) => settled(s, ANGLE_EPS)) && ratios.every((s) => settled(s, RATIO_EPS));
  }

  /** The springs at rest; the moves seen so far stay, the next one may make a velocity. */
  function snap(): void {
    lean = AT_REST;
    stretch = AT_REST;
    squash = AT_REST;
    for (const part of trails) {
      part.lean = AT_REST;
      part.squash = AT_REST;
    }
    lastStep = null;
  }

  function reset(): void {
    samples = [];
    moving = false;
    peakSpeed = 0;
    axisDeg = 0;
    snap();
  }

  function pose(): MotionPose {
    const swing = (part: Trail): number => part.lean.value - lean.value;
    const lag = (part: Trail): number => part.squash.value - squash.value;
    const turn = (deg: number): number => clamp(deg, -MAX_PART_DEG, MAX_PART_DEG);
    return {
      leanDeg: clamp(lean.value, -MAX_LEAN_DEG, MAX_LEAN_DEG),
      stretch: clamp(stretch.value, -MAX_STRETCH, MAX_STRETCH),
      axisDeg,
      squash: clamp(squash.value, -MAX_SQUASH, MAX_SQUASH),
      filamentDeg: turn(swing(filament) * FILAMENT_SWING),
      capDeg: turn(swing(cap) * CAP_SWING),
      capDrop: clamp(lag(cap) * CAP_DROP_PER_SQUASH, -MAX_CAP_DROP, MAX_CAP_DROP),
      // Lagging behind a squash, both hands rise (the left turns clockwise, the right back).
      armLDeg: turn(swing(arms) + lag(arms) * ARM_FLAP_DEG),
      armRDeg: turn(swing(arms) - lag(arms) * ARM_FLAP_DEG),
    };
  }

  return {
    moved(x, y, t) {
      samples = addSample(samples, { x, y, t });
      const v = velocityOf(samples, t);
      const speed = Math.hypot(v.x, v.y);
      if (speed < MIN_SPEED) return;
      moving = true;
      peakSpeed = Math.max(peakSpeed, speed);
    },
    release() {
      if (moving) land();
      else samples = [];
    },
    squash(kind) {
      kick(SQUASH_KICKS[kind]);
    },
    step(t, still) {
      if (still || !Number.isFinite(t)) {
        reset();
        return REST_POSE;
      }
      const dt = lastStep === null ? 0 : t - lastStep;
      lastStep = t;
      const newest = samples[samples.length - 1];
      if (moving && newest && t - newest.t > STALE_MS) land();
      const v = velocityOf(samples, t);
      const speed = Math.hypot(v.x, v.y);
      if (speed >= MIN_SPEED) axisDeg = (Math.atan2(v.y, v.x) * 180) / Math.PI;
      lean = stepSpring(lean, leanFor(v.x), WOBBLE, dt);
      stretch = stepSpring(stretch, stretchFor(speed), WOBBLE, dt);
      squash = stepSpring(squash, 0, WOBBLE, dt);
      for (const part of trails) {
        part.lean = stepSpring(part.lean, lean.value, part.params, dt);
        part.squash = stepSpring(part.squash, squash.value, part.params, dt);
      }
      if (atRest()) {
        snap();
        return REST_POSE;
      }
      return pose();
    },
    atRest,
    reset,
  };
}
