// The robot's public face: mounts Mate (svg.ts), his beam (beam.ts), the state and mood
// machine (states.ts) and flight (flight.ts) into one container. Reduce motion is on when the
// setting or the OS (prefers-reduced-motion) asks for it.

import "./robot.css";
import { createBeamController } from "./beam";
import { createFlightController } from "./flight";
import { VIEW_H, VIEW_W } from "./geometry";
import { createStateAnimator } from "./states";
import { buildMateSvg, svgEl } from "./svg";
import { DEFAULT_ROBOT_SIZE, ROBOT_SIZE_PX } from "./types";
import type { CssPoint, CssRect, RobotMood, RobotSizePreset, RobotState } from "./types";

export type { CssPoint, CssRect, RobotMood, RobotSizePreset, RobotState } from "./types";
export { ROBOT_MOODS, ROBOT_STATES, isRobotMood, isRobotState } from "./types";

export interface RobotController {
  setState(state: RobotState): void;
  setMood(mood: RobotMood): void;
  /** Voice level 0..1 while speaking. */
  setVoiceLevel(rms: number): void;
  /** The mouse in client CSS pixels, or null. */
  setEyeTarget(cursor: CssPoint | null): void;
  /** The beam's target in client CSS pixels; it shines in pointing, waiting_for_click and oops. */
  aimAt(target: CssRect | null): void;
  /** Flies so that the centre of Mate's box lands on `center` (client CSS pixels). */
  flyTo(center: CssPoint): Promise<void>;
  setSize(size: RobotSizePreset): void;
  /** The "reduce motion" setting; the OS preference counts too. */
  setReduceMotion(reduce: boolean): void;
  setFunMode(fun: boolean): void;
  destroy(): void;
}

const REDUCED_MOTION_QUERY = "(prefers-reduced-motion: reduce)";

export function mountRobot(container: HTMLElement): RobotController {
  const refs = buildMateSvg();
  // Below the body in paint order, so the flashlight covers the start of its beam.
  const beamLayer = svgEl("svg", { class: "mate-beam-layer", "aria-hidden": "true" });
  const box = document.createElement("div");
  box.className = "mate-box";
  box.append(refs.svg);
  const root = document.createElement("div");
  root.className = "mate-root";
  root.append(beamLayer, box);
  container.replaceChildren(root);

  const beam = createBeamController(beamLayer);
  const animator = createStateAnimator(refs, beam);
  const flight = createFlightController(box);
  const motionQuery = window.matchMedia(REDUCED_MOTION_QUERY);
  let reduceSetting = false;
  let offset: CssPoint = { x: 0, y: 0 };

  const reduceMotion = (): boolean => reduceSetting || motionQuery.matches;
  const applyMotion = (): void => animator.setReduceMotion(reduceMotion());
  const relayout = (): void => animator.relayout();

  function setSize(size: RobotSizePreset): void {
    const width = ROBOT_SIZE_PX[size];
    refs.svg.setAttribute("width", String(width));
    refs.svg.setAttribute("height", String(Math.round((width * VIEW_H) / VIEW_W)));
    relayout();
  }

  setSize(DEFAULT_ROBOT_SIZE);
  applyMotion();
  motionQuery.addEventListener("change", applyMotion);
  window.addEventListener("resize", relayout);

  return {
    setState: (state) => animator.setState(state),
    setMood: (mood) => animator.setMood(mood),
    setVoiceLevel: (rms) => animator.setVoiceLevel(rms),
    setEyeTarget: (cursor) => animator.setEyeTarget(cursor),
    aimAt: (target) => animator.aimAt(target),
    async flyTo(center) {
      flight.cancel();
      const rect = box.getBoundingClientRect();
      const from = offset;
      const to = {
        x: offset.x + center.x - (rect.left + rect.width / 2),
        y: offset.y + center.y - (rect.top + rect.height / 2),
      };
      offset = to;
      await flight.moveTo(from, to, { reduceMotion: reduceMotion() });
      relayout();
    },
    setSize,
    setReduceMotion(reduce) {
      reduceSetting = reduce;
      applyMotion();
    },
    setFunMode: (fun) => animator.setFunMode(fun),
    destroy() {
      motionQuery.removeEventListener("change", applyMotion);
      window.removeEventListener("resize", relayout);
      flight.cancel();
      animator.destroy();
      root.remove();
    },
  };
}
