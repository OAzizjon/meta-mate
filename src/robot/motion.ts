// Puts the springs' pose (spring.ts) on Mate's drawing: one style write per animation frame and
// only while something moves, so there are no frames at rest and no layout reads. Transform
// properties only, on groups that no CSS animation drives with the same property, so the
// body's loops and one-shots compose with them. The state animator (states.ts) holds him
// still while the beam is on, asleep, hidden and with reduced motion; mate.ts reports the
// window's moves.

import { bodyMatrix, createMotionModel } from "./spring.ts";
import type { MotionPose, SquashKind } from "./spring.ts";
import type { MateSvgRefs } from "./svg.ts";

export interface MotionController {
  /** The window's position on screen in CSS pixels, at each move (a drag or a flight). */
  moved(x: number, y: number): void;
  /** A drag ended: he lands if he was moving; a plain click does nothing. */
  release(): void;
  squash(kind: SquashKind): void;
  /** Still: the rest pose at once, and every input is ignored until he may move again. */
  setStill(still: boolean): void;
  destroy(): void;
}

/** For a drawing without springs (and the state animator's own tests). */
export const NO_MOTION: MotionController = {
  moved() {},
  release() {},
  squash() {},
  setStill() {},
  destroy() {},
};

export interface FrameClock {
  now(): number;
  request(callback: FrameRequestCallback): number;
  cancel(frame: number): void;
}

const BROWSER_CLOCK: FrameClock = {
  now: () => performance.now(),
  request: (callback) => requestAnimationFrame(callback),
  cancel: (frame) => cancelAnimationFrame(frame),
};

export type MotionParts = Pick<MateSvgRefs, "motion" | "cap" | "filament" | "layers">;

function round(value: number, digits: number): string {
  const k = 10 ** digits;
  return String(Math.round(value * k) / k);
}

/** Writes `pose`, or with null takes every spring style off. */
function draw(parts: MotionParts, pose: MotionPose | null): void {
  const turned = [parts.filament, parts.cap, parts.layers["arm-l"], parts.layers["arm-r"]];
  if (pose === null) {
    parts.motion.style.removeProperty("transform");
    for (const el of turned) el.style.removeProperty("rotate");
    parts.cap.style.removeProperty("translate");
    return;
  }
  const matrix = bodyMatrix(pose).map((n) => round(n, 4)).join(", ");
  parts.motion.style.setProperty("transform", `matrix(${matrix}, 0, 0)`);
  const turns = [pose.filamentDeg, pose.capDeg, pose.armLDeg, pose.armRDeg];
  turned.forEach((el, i) => el.style.setProperty("rotate", `${round(turns[i], 2)}deg`));
  parts.cap.style.setProperty("translate", `0px ${round(pose.capDrop, 2)}px`);
}

export function createMotionController(parts: MotionParts, clock: FrameClock = BROWSER_CLOCK): MotionController {
  const model = createMotionModel();
  let still = false;
  let posed = false;
  let frame: number | undefined;

  function unpose(): void {
    if (posed) draw(parts, null);
    posed = false;
  }

  function tick(time: number): void {
    frame = undefined;
    if (still) return;
    const pose = model.step(time, false);
    if (model.atRest()) {
      unpose();
      return;
    }
    draw(parts, pose);
    posed = true;
    schedule();
  }

  function schedule(): void {
    if (!still && frame === undefined) frame = clock.request(tick);
  }

  function stop(): void {
    if (frame !== undefined) clock.cancel(frame);
    frame = undefined;
    model.reset();
    unpose();
  }

  return {
    moved(x, y) {
      if (still) return;
      model.moved(x, y, clock.now());
      schedule();
    },
    release() {
      if (still) return;
      model.release();
      schedule();
    },
    squash(kind) {
      if (still) return;
      model.squash(kind);
      schedule();
    },
    setStill(next) {
      still = next;
      if (still) stop();
    },
    destroy() {
      still = true;
      stop();
    },
  };
}
