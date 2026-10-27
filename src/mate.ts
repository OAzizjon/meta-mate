// Mate's window: mounts the robot (src/robot) idle and calm. In development only, the console
// gets `window.__mate` to try states, moods and the beam by hand, e.g.
// __mate.setState("pointing"); __mate.aimAt({ left: 20, top: 20, right: 120, bottom: 60 }).
import { isRobotMood, isRobotState, mountRobot } from "./robot";
import type { CssRect, RobotController } from "./robot";

interface MateDevHook {
  setState(state: string): void;
  setMood(mood: string): void;
  aimAt(target: CssRect | null): void;
}

declare global {
  interface Window {
    __mate?: MateDevHook;
  }
}

function devHook(robot: RobotController): MateDevHook {
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
  };
}

const container = document.getElementById("mate");
if (container) {
  const robot = mountRobot(container);
  robot.setMood("calm");
  robot.setState("idle");
  if (import.meta.env.DEV) window.__mate = devHook(robot);
}
