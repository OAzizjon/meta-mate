// The robot's vocabulary, shared with Rust: states (mm-platform overlay.rs `RobotState`),
// moods (mm-core matescript/vocabulary.rs `MOODS`), sizes (docs/robot.md) and CSS-pixel
// geometry (mm-core geom.rs `CssPoint`, `CssRect`).

export const ROBOT_STATES = [
  "idle",
  "listening",
  "looking",
  "thinking",
  "speaking",
  "flying",
  "pointing",
  "waiting_for_click",
  "success",
  "oops",
  "cant_find",
  "sleeping",
] as const;
export type RobotState = (typeof ROBOT_STATES)[number];

export const ROBOT_MOODS = ["calm", "happy", "thinking", "surprised", "sad", "proud", "sleepy"] as const;
export type RobotMood = (typeof ROBOT_MOODS)[number];

export type RobotSizePreset = "S" | "M" | "L" | "XL";

/** Width in CSS pixels; the height follows the 260 x 240 viewBox. */
export const ROBOT_SIZE_PX: Readonly<Record<RobotSizePreset, number>> = { S: 64, M: 96, L: 160, XL: 220 };
export const DEFAULT_ROBOT_SIZE: RobotSizePreset = "M";

/**
 * CSS pixels of this WebView (client coordinates). `space` is a phantom tag, never set: it
 * only stops a point in another space (see `ViewPoint`) from being passed here (convention 2).
 */
export interface CssPoint {
  readonly x: number;
  readonly y: number;
  readonly space?: "css";
}

export interface CssRect {
  readonly left: number;
  readonly top: number;
  readonly right: number;
  readonly bottom: number;
}

export function isRobotState(value: unknown): value is RobotState {
  return typeof value === "string" && (ROBOT_STATES as readonly string[]).includes(value);
}

export function isRobotMood(value: unknown): value is RobotMood {
  return typeof value === "string" && (ROBOT_MOODS as readonly string[]).includes(value);
}
