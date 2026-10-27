// Moves Mate's box across the page with Web Animations: he tilts toward where he is going and
// arrives with a small bounce (docs/robot.md "flying"). With reduced motion he fades out,
// jumps and fades in: no positions in between, no tilt.

import { clamp } from "./geometry";
import type { CssPoint } from "./types";

export interface FlightOptions {
  readonly reduceMotion: boolean;
}

export interface FlightController {
  /**
   * Moves the element's translation from `from` to `to` (CSS pixels). Resolves when he has
   * arrived or the flight was cancelled; either way the element ends at `to`.
   */
  moveTo(from: CssPoint, to: CssPoint, opts: FlightOptions): Promise<void>;
  cancel(): void;
}

const FADE_MS = 90;
const TILT_DEG_PER_PX = 0.04;
const TILT_MAX_DEG = 14;
const MS_PER_PX = 0.45;
const MIN_MS = 220;
const MAX_MS = 520;
/** Overshoots a little: the arrival bounce. */
const ARRIVAL_EASING = "cubic-bezier(0.22, 1.28, 0.36, 1)";

function transform(at: CssPoint, tiltDeg = 0): string {
  return `translate3d(${at.x}px, ${at.y}px, 0) rotate(${tiltDeg}deg)`;
}

function lerp(a: CssPoint, b: CssPoint, t: number): CssPoint {
  return { x: a.x + (b.x - a.x) * t, y: a.y + (b.y - a.y) * t };
}

export function createFlightController(el: HTMLElement): FlightController {
  let running: Animation | null = null;
  let flight = 0;

  /** Plays one animation; false if it was cancelled. */
  async function play(keyframes: Keyframe[], options: KeyframeAnimationOptions): Promise<boolean> {
    const animation = el.animate(keyframes, options);
    running = animation;
    try {
      await animation.finished;
      return true;
    } catch {
      return false;
    } finally {
      if (running === animation) running = null;
    }
  }

  function cancel(): void {
    flight += 1;
    running?.cancel();
    running = null;
    el.style.removeProperty("opacity");
  }

  async function moveTo(from: CssPoint, to: CssPoint, opts: FlightOptions): Promise<void> {
    cancel();
    const mine = flight;
    // The end position is set first: the animations only draw the way there.
    el.style.transform = transform(to);
    const dx = to.x - from.x;
    const dy = to.y - from.y;
    if (dx === 0 && dy === 0) return;

    if (opts.reduceMotion) {
      el.style.opacity = "0";
      try {
        const out = [{ transform: transform(from), opacity: 1 }, { transform: transform(from), opacity: 0 }];
        if (await play(out, { duration: FADE_MS, easing: "ease-in" })) {
          await play([{ opacity: 0 }, { opacity: 1 }], { duration: FADE_MS, easing: "ease-out" });
        }
      } finally {
        if (mine === flight) el.style.removeProperty("opacity");
      }
      return;
    }

    const tilt = clamp(dx * TILT_DEG_PER_PX, -TILT_MAX_DEG, TILT_MAX_DEG);
    const duration = clamp(Math.hypot(dx, dy) * MS_PER_PX, MIN_MS, MAX_MS);
    await play(
      [
        { transform: transform(from) },
        { transform: transform(lerp(from, to, 0.2), tilt), offset: 0.2 },
        { transform: transform(lerp(from, to, 0.8), tilt), offset: 0.8 },
        { transform: transform(to) },
      ],
      { duration, easing: ARRIVAL_EASING },
    );
  }

  return { moveTo, cancel };
}
