// Mate's locked B1 "classic glow" look (D29) with the v1.1 refresh (docs/robot.md), ported from
// bulbMate('classic') in assets/art/src/mate-lib.js as plain data: the same 260 x 240 viewBox,
// paths and colours. svg.ts turns the nodes into elements. The functions at the end map the
// viewBox to CSS pixels and aim the arm, the beam and the eyes.

import type { CssPoint, CssRect, RobotMood } from "./types.ts";

export const INK = "#1F2A44";
export const GLASS_PATH =
  "M110 20 C160 20 186 56 180 96 C176 122 156 138 148 166 L72 166 C64 138 44 122 40 96 C34 56 60 20 110 20 Z";
export const VIEW_W = 260;
export const VIEW_H = 240;

/** A point in viewBox units; `space` is a phantom tag like `CssPoint`'s. */
export interface ViewPoint {
  readonly x: number;
  readonly y: number;
  readonly space?: "view";
}

export type SvgTag = "g" | "ellipse" | "circle" | "path" | "rect" | "text";
export type SvgAttrs = Readonly<Record<string, string | number>>;

export interface SvgNode {
  readonly tag: SvgTag;
  readonly attrs: SvgAttrs;
  readonly children?: readonly SvgNode[];
  readonly text?: string;
}

export interface GradientSpec {
  readonly tag: "radialGradient" | "linearGradient";
  readonly attrs: SvgAttrs;
  readonly stops: readonly SvgAttrs[];
}

// --- gradients (svg.ts gives them per-drawing ids, like the art's `gid`) ---------------

export const GLASS_GRADIENT: GradientSpec = {
  tag: "radialGradient",
  attrs: { cx: "42%", cy: "32%", r: "74%" },
  stops: [
    { offset: 0, "stop-color": "#FFFDF0" },
    { offset: 0.68, "stop-color": "#FFEFAE" },
    { offset: 1, "stop-color": "#FFD866" },
  ],
};

// The aura has two radial layers: a wide soft haze and a warm core that hugs the glass. Their
// stops only shape the falloff; the strength is the aura layer's CSS opacity (the art's group
// opacity: .45, or .8 with `glow`), so it renders the same and animates.
export const AURA_GRADIENT: GradientSpec = {
  tag: "radialGradient",
  attrs: {},
  stops: [
    { offset: 0, "stop-color": "#FFE08A", "stop-opacity": 0.55 },
    { offset: 0.6, "stop-color": "#FFE08A", "stop-opacity": 0.26 },
    { offset: 1, "stop-color": "#FFE08A", "stop-opacity": 0 },
  ],
};

export const AURA_CORE_GRADIENT: GradientSpec = {
  tag: "radialGradient",
  attrs: {},
  stops: [
    { offset: 0, "stop-color": "#FFF1B8", "stop-opacity": 0.7 },
    { offset: 0.8, "stop-color": "#FFE6A0", "stop-opacity": 0.38 },
    { offset: 1, "stop-color": "#FFE08A", "stop-opacity": 0 },
  ],
};

// A soft sheen on top of each ring.
export const METAL_GRADIENT: GradientSpec = {
  tag: "linearGradient",
  attrs: { x1: 0, y1: 0, x2: 0, y2: 1 },
  stops: [
    { offset: 0, "stop-color": "#4A7899" },
    { offset: 0.45, "stop-color": "#3B6688" },
    { offset: 1, "stop-color": "#2E5070" },
  ],
};

// The art's beam colour and strength at the lens; it fades to .15 rather than 0 so the light
// still lands on the target.
export const BEAM_COLOR = "#FFE08A";
export const BEAM_STOPS: readonly SvgAttrs[] = [
  { offset: 0, "stop-color": BEAM_COLOR, "stop-opacity": 0.95 },
  { offset: 1, "stop-color": BEAM_COLOR, "stop-opacity": 0.15 },
];
export const CONTOUR_COLOR = "#FFC94A";

// --- body ---------------------------------------------------------------------------

export const SHADOW: readonly SvgNode[] = [
  { tag: "ellipse", attrs: { cx: 112, cy: 226, rx: 50, ry: 7, fill: INK, opacity: 0.13 } },
];

export function auraNodes(haze: string, core: string): readonly SvgNode[] {
  return [
    { tag: "circle", attrs: { cx: 110, cy: 96, r: 104, fill: haze } },
    { tag: "circle", attrs: { cx: 110, cy: 96, r: 86, fill: core } },
  ];
}

export function baseNodes(metal: string): readonly SvgNode[] {
  return [
    { tag: "rect", attrs: { x: 80, y: 166, width: 60, height: 15, rx: 6, fill: metal, stroke: INK, "stroke-width": 5 } },
    { tag: "rect", attrs: { x: 84, y: 179, width: 52, height: 14, rx: 6, fill: metal, stroke: INK, "stroke-width": 5 } },
    { tag: "path", attrs: { d: "M96 192 Q110 208 124 192 Z", fill: INK } },
  ];
}

export const STATUS_LIGHT: readonly SvgNode[] = [
  { tag: "circle", attrs: { cx: 128, cy: 173, r: 3, fill: "#8FF5E9" } },
  { tag: "circle", attrs: { cx: 127, cy: 172, r: 1.1, fill: "#fff", opacity: 0.9 } },
];

/** A zigzag on two supports, low in the glass under the face. */
const FILAMENT_PATH = "M100 161 L98 148 L102 142 L106 148 L110 142 L114 148 L118 142 L122 148 L120 161";

export function glassNodes(fill: string): readonly SvgNode[] {
  const wire = { d: FILAMENT_PATH, fill: "none", "stroke-linecap": "round", "stroke-linejoin": "round" };
  return [
    { tag: "path", attrs: { d: GLASS_PATH, fill, stroke: INK, "stroke-width": 6 } },
    // Dim at rest; robot.css brightens it and its glow in listening and speaking.
    {
      tag: "g",
      attrs: { class: "mate-filament", opacity: 0.45 },
      children: [
        { tag: "path", attrs: { ...wire, class: "mate-filament-glow", stroke: "#FFC94A", "stroke-width": 8, opacity: 0 } },
        { tag: "path", attrs: { ...wire, stroke: "#F28C28", "stroke-width": 3 } },
      ],
    },
    { tag: "path", attrs: { d: "M62 72 Q64 47 88 38", stroke: "#fff", "stroke-width": 6, "stroke-linecap": "round", fill: "none" } },
    { tag: "circle", attrs: { cx: 61, cy: 85, r: 3.2, fill: "#fff" } },
  ];
}

// Decorative, the only text in the drawing (invariant 7 allows it).
export const SLEEP_Z: readonly SvgNode[] = [
  {
    tag: "text",
    attrs: { x: 172, y: 40, "font-size": 30, "font-weight": 700, "font-family": "Segoe UI, Arial, sans-serif", fill: INK },
    text: "z",
  },
];

/** Where the sparks circle and burst from: the middle of the glass. */
export const SPARK_CENTER: ViewPoint = { x: 110, y: 96 };
const SPARK_RADIUS = 84;
/** Six little four-point stars around the glass: thinking orbits three of them, success
 *  bursts all six outward (robot.css, states.ts). Hidden at rest. */
export const SPARK_ANGLES: readonly number[] = [-90, -30, 30, 90, 150, 210];

function star(cx: number, cy: number, s: number): string {
  const k = Math.round(s * 0.32 * 10) / 10;
  return (
    `M${cx} ${cy - s} L${cx + k} ${cy - k} L${cx + s} ${cy} L${cx + k} ${cy + k} ` +
    `L${cx} ${cy + s} L${cx - k} ${cy + k} L${cx - s} ${cy} L${cx - k} ${cy - k} Z`
  );
}

export const SPARKS: readonly SvgNode[] = SPARK_ANGLES.map((deg, i) => {
  const angle = (deg * Math.PI) / 180;
  const cx = Math.round(SPARK_CENTER.x + SPARK_RADIUS * Math.cos(angle));
  const cy = Math.round(SPARK_CENTER.y + SPARK_RADIUS * Math.sin(angle));
  return {
    tag: "path",
    attrs: {
      class: "mate-spark",
      d: star(cx, cy, i % 2 === 0 ? 8 : 6),
      fill: i % 2 === 0 ? "#FFC94A" : "#FFE58A",
      stroke: INK,
      "stroke-width": 1.5,
      "stroke-linejoin": "round",
    },
  };
});

// --- face ---------------------------------------------------------------------------

const EX1 = 89;
const EX2 = 131;
const EY = 96;
const MY = EY + 21;

export type EyeShape = "open" | "wide" | "happy" | "squint" | "closed";
export type MouthShape = "smile" | "grin" | "flat" | "o" | "sad" | "small" | "open";
export type CheekStyle = "normal" | "faded" | "puffed";
/** robot.css moves the brows for each style: `raised` lifts the right one, `tilted` lifts the inner ends. */
export type BrowStyle = "rest" | "up" | "raised" | "tilted" | "high" | "low";

export interface Face {
  readonly eyes: EyeShape;
  readonly mouth: MouthShape;
  readonly cheeks: CheekStyle;
  readonly brows: BrowStyle;
}

function line(d: string, width: number, join?: "round"): SvgNode {
  const attrs = { d, stroke: INK, "stroke-width": width, "stroke-linecap": "round", fill: "none" };
  return { tag: "path", attrs: join ? { ...attrs, "stroke-linejoin": join } : attrs };
}

export const EYES: Readonly<Record<EyeShape, readonly SvgNode[]>> = {
  open: [
    { tag: "ellipse", attrs: { cx: EX1, cy: EY, rx: 11, ry: 14, fill: INK } },
    { tag: "ellipse", attrs: { cx: EX2, cy: EY, rx: 11, ry: 14, fill: INK } },
    { tag: "circle", attrs: { cx: EX1 + 3.5, cy: EY - 5.5, r: 4.2, fill: "#fff" } },
    { tag: "circle", attrs: { cx: EX2 + 3.5, cy: EY - 5.5, r: 4.2, fill: "#fff" } },
    { tag: "circle", attrs: { cx: EX1 - 3.5, cy: EY + 5.5, r: 2, fill: "#fff", opacity: 0.85 } },
    { tag: "circle", attrs: { cx: EX2 - 3.5, cy: EY + 5.5, r: 2, fill: "#fff", opacity: 0.85 } },
  ],
  wide: [
    { tag: "ellipse", attrs: { cx: EX1, cy: EY - 1, rx: 13, ry: 17, fill: INK } },
    { tag: "ellipse", attrs: { cx: EX2, cy: EY - 1, rx: 13, ry: 17, fill: INK } },
    { tag: "circle", attrs: { cx: EX1 + 4.5, cy: EY - 8, r: 5.2, fill: "#fff" } },
    { tag: "circle", attrs: { cx: EX2 + 4.5, cy: EY - 8, r: 5.2, fill: "#fff" } },
    { tag: "circle", attrs: { cx: EX1 - 4, cy: EY + 6, r: 2.2, fill: "#fff", opacity: 0.85 } },
    { tag: "circle", attrs: { cx: EX2 - 4, cy: EY + 6, r: 2.2, fill: "#fff", opacity: 0.85 } },
  ],
  happy: [
    line(`M${EX1 - 11} ${EY + 5} Q${EX1} ${EY - 13} ${EX1 + 11} ${EY + 5}`, 6),
    line(`M${EX2 - 11} ${EY + 5} Q${EX2} ${EY - 13} ${EX2 + 11} ${EY + 5}`, 6),
  ],
  squint: [
    line(`M${EX1 - 9} ${EY - 8} L${EX1 + 8} ${EY} L${EX1 - 9} ${EY + 8}`, 6, "round"),
    line(`M${EX2 + 9} ${EY - 8} L${EX2 - 8} ${EY} L${EX2 + 9} ${EY + 8}`, 6, "round"),
  ],
  // Not in the art: closed lids for blinking and sleep.
  closed: [
    line(`M${EX1 - 11} ${EY} Q${EX1} ${EY + 10} ${EX1 + 11} ${EY}`, 6),
    line(`M${EX2 - 11} ${EY} Q${EX2} ${EY + 10} ${EX2 + 11} ${EY}`, 6),
  ],
};

/** The brows' ends; each arc peaks 3.5 units higher. */
const BROW_Y = EY - 24;

// No transform attribute: robot.css moves the brows (data-brows), as the art's transforms do.
function brow(cx: number, side: "l" | "r"): SvgNode {
  const { attrs } = line(`M${cx - 10} ${BROW_Y} Q${cx} ${BROW_Y - 7} ${cx + 10} ${BROW_Y}`, 4);
  return { tag: "path", attrs: { ...attrs, class: `mate-brow mate-brow-${side}` } };
}

export const BROWS: readonly SvgNode[] = [brow(EX1, "l"), brow(EX2, "r")];

const OPEN_MOUTH_RY = 3;
const OPEN_MOUTH_RANGE = 9;

export const MOUTHS: Readonly<Record<MouthShape, readonly SvgNode[]>> = {
  smile: [line(`M98 ${MY} Q110 ${MY + 10} 122 ${MY}`, 4.5)],
  grin: [{ tag: "path", attrs: { d: `M96 ${MY - 2} Q110 ${MY + 15} 124 ${MY - 2} Z`, fill: INK } }],
  flat: [{ tag: "path", attrs: { d: `M101 ${MY + 3} H119`, stroke: INK, "stroke-width": 4.5, "stroke-linecap": "round" } }],
  // Not in the art: the moods and the speaking mouth of docs/robot.md.
  o: [{ tag: "ellipse", attrs: { cx: 110, cy: MY + 2, rx: 6, ry: 7, fill: INK } }],
  sad: [line(`M98 ${MY + 6} Q110 ${MY - 2} 122 ${MY + 6}`, 4.5)],
  small: [{ tag: "path", attrs: { d: `M104 ${MY + 3} H116`, stroke: INK, "stroke-width": 4.5, "stroke-linecap": "round" } }],
  open: [{ tag: "ellipse", attrs: { cx: 110, cy: MY + 2, rx: 8, ry: OPEN_MOUTH_RY, fill: INK } }],
};

export const CHEEKS: readonly SvgNode[] = [
  { tag: "ellipse", attrs: { cx: 70, cy: MY - 2, rx: 8, ry: 5, fill: "#FFB3A7", opacity: 0.9 } },
  { tag: "ellipse", attrs: { cx: 150, cy: MY - 2, rx: 8, ry: 5, fill: "#FFB3A7", opacity: 0.9 } },
];

export const MOOD_FACES: Readonly<Record<RobotMood, Face>> = {
  calm: { eyes: "open", mouth: "smile", cheeks: "normal", brows: "rest" },
  happy: { eyes: "happy", mouth: "grin", cheeks: "normal", brows: "up" },
  thinking: { eyes: "open", mouth: "flat", cheeks: "normal", brows: "raised" },
  surprised: { eyes: "wide", mouth: "o", cheeks: "normal", brows: "high" },
  sad: { eyes: "open", mouth: "sad", cheeks: "faded", brows: "tilted" },
  proud: { eyes: "happy", mouth: "grin", cheeks: "puffed", brows: "up" },
  sleepy: { eyes: "closed", mouth: "small", cheeks: "normal", brows: "low" },
};

// --- arms and flashlight ------------------------------------------------------------

export type ArmPose = "rest" | "point" | "shrug";

/** The bulb's vertical axis: drawings mirror around it. */
const AXIS_X = 110;
export const MIRROR_TRANSFORM = `translate(${2 * AXIS_X} 0) scale(-1 1)`;

function limb(d: string): readonly SvgNode[] {
  return [
    { tag: "path", attrs: { d, stroke: INK, "stroke-width": 15, "stroke-linecap": "round", fill: "none" } },
    { tag: "path", attrs: { d, stroke: "#FFF0B3", "stroke-width": 6, "stroke-linecap": "round", fill: "none" } },
  ];
}

function stub(cx: number, cy: number, rot: number): SvgNode {
  const transform = `rotate(${rot} ${cx} ${cy})`;
  return { tag: "ellipse", attrs: { cx, cy, rx: 10, ry: 16, transform, fill: "#FFF0B3", stroke: INK, "stroke-width": 5 } };
}

const STOP_ARM: readonly SvgNode[] = [
  ...limb("M168 118 Q192 102 198 76"),
  { tag: "circle", attrs: { cx: 199, cy: 66, r: 14, fill: "#FFF0B3", stroke: INK, "stroke-width": 5 } },
];

export const ARM_L: Readonly<Record<ArmPose, readonly SvgNode[]>> = {
  rest: [stub(40, 130, 30)],
  point: [stub(40, 130, 30)],
  // The art raises only the right hand ("stop"); a shrug raises both, the left one mirrored.
  shrug: [{ tag: "g", attrs: { transform: MIRROR_TRANSFORM }, children: STOP_ARM }],
};

export const ARM_R: Readonly<Record<ArmPose, readonly SvgNode[]>> = {
  rest: [stub(180, 130, -30)],
  point: limb("M170 120 Q194 110 208 100"),
  shrug: STOP_ARM,
};

// The art also draws a short beam here (beam(222, 96)); the real one is beam.ts, in CSS pixels.
export const FLASHLIGHT: readonly SvgNode[] = [
  {
    tag: "rect",
    attrs: { x: 204, y: 84, width: 22, height: 24, rx: 6, transform: "rotate(-20 215 96)", fill: "#3B6688", stroke: INK, "stroke-width": 4 },
  },
  { tag: "circle", attrs: { cx: 224, cy: 92, r: 4, fill: "#FFF6D6" } },
];

// --- aiming -------------------------------------------------------------------------

/** Where the pointing limb starts and where the lens sits (the beam's origin). */
export const SHOULDER: ViewPoint = { x: 170, y: 120 };
export const NOZZLE: ViewPoint = { x: 224, y: 92 };
/** The art's beam is 22 units tall at the lens. */
export const BEAM_HALF_WIDTH = 11;
const AIM_LIMIT_DEG = 45;
const EYES_CENTER: ViewPoint = { x: (EX1 + EX2) / 2, y: EY };
const EYE_REACH = 3.5;
export const THINKING_GAZE: ViewPoint = { x: 3, y: -4 };

export interface ArmAim {
  readonly angleDeg: number;
  readonly mirrored: boolean;
}
export const STRAIGHT_AIM: ArmAim = { angleDeg: 0, mirrored: false };

/** `value` limited to [min, max]; NaN becomes `min`. */
export function clamp(value: number, min: number, max: number): number {
  return Number.isNaN(value) ? min : Math.min(max, Math.max(min, value));
}

export function rectCenter(rect: CssRect): CssPoint {
  return { x: (rect.left + rect.right) / 2, y: (rect.top + rect.bottom) / 2 };
}

/** CSS pixels per viewBox unit for the drawing laid out in `box` (xMidYMid meet); 0 if unlaid. */
export function viewScale(box: CssRect): number {
  return Math.max(0, Math.min((box.right - box.left) / VIEW_W, (box.bottom - box.top) / VIEW_H));
}

function viewOrigin(box: CssRect, scale: number): CssPoint {
  return { x: (box.left + box.right - VIEW_W * scale) / 2, y: (box.top + box.bottom - VIEW_H * scale) / 2 };
}

export function viewToCss(box: CssRect, point: ViewPoint): CssPoint {
  const scale = viewScale(box);
  const origin = viewOrigin(box, scale);
  return { x: origin.x + point.x * scale, y: origin.y + point.y * scale };
}

/** The inverse of `viewToCss`; callers check `viewScale(box) > 0` first. */
export function cssToView(box: CssRect, point: CssPoint): ViewPoint {
  const scale = viewScale(box);
  const origin = viewOrigin(box, scale);
  return { x: (point.x - origin.x) / scale, y: (point.y - origin.y) / scale };
}

/** Turns the right arm toward `target` (±45°), mirrored onto the left side for targets there. */
export function armAim(target: ViewPoint): ArmAim {
  const mirrored = target.x < AXIS_X;
  const x = mirrored ? 2 * AXIS_X - target.x : target.x;
  const angle = (Math.atan2(target.y - SHOULDER.y, x - SHOULDER.x) * 180) / Math.PI;
  return { angleDeg: clamp(angle, -AIM_LIMIT_DEG, AIM_LIMIT_DEG), mirrored };
}

/** The lens after `aim`: the same rotate-then-mirror that svg.ts applies to the arm. */
export function nozzleInView(aim: ArmAim): ViewPoint {
  const a = (aim.angleDeg * Math.PI) / 180;
  const dx = NOZZLE.x - SHOULDER.x;
  const dy = NOZZLE.y - SHOULDER.y;
  const x = SHOULDER.x + dx * Math.cos(a) - dy * Math.sin(a);
  const y = SHOULDER.y + dx * Math.sin(a) + dy * Math.cos(a);
  return { x: aim.mirrored ? 2 * AXIS_X - x : x, y };
}

/** How far the eyes shift toward `cursor`: up to 3.5 units, less when it is close. */
export function gazeToward(cursor: ViewPoint): ViewPoint {
  const dx = cursor.x - EYES_CENTER.x;
  const dy = cursor.y - EYES_CENTER.y;
  const length = Math.hypot(dx, dy);
  if (!(length >= 1)) return { x: 0, y: 0 };
  const reach = Math.min(EYE_REACH, length / 20);
  return { x: (dx / length) * reach, y: (dy / length) * reach };
}

/** The speaking mouth's height for a voice level 0..1. */
export function openMouthRy(level: number): number {
  return OPEN_MOUTH_RY + clamp(level, 0, 1) * OPEN_MOUTH_RANGE;
}

/**
 * The light cone from `nozzle` to `target`: two points `halfWidth` either side of the lens,
 * then the two corners of `target` that bound it as seen from the lens. Null when the lens is
 * inside the target (there is no cone to draw).
 */
export function computeBeamPolygon(nozzle: CssPoint, target: CssRect, halfWidth: number): readonly CssPoint[] | null {
  const inside = nozzle.x > target.left && nozzle.x < target.right && nozzle.y > target.top && nozzle.y < target.bottom;
  if (inside) return null;
  const center = rectCenter(target);
  const axis = Math.atan2(center.y - nozzle.y, center.x - nozzle.x);
  const corners: readonly CssPoint[] = [
    { x: target.left, y: target.top },
    { x: target.right, y: target.top },
    { x: target.right, y: target.bottom },
    { x: target.left, y: target.bottom },
  ];
  let first: CssPoint = { x: target.left, y: target.top };
  let last: CssPoint = first;
  let min = Infinity;
  let max = -Infinity;
  for (const corner of corners) {
    const raw = Math.atan2(corner.y - nozzle.y, corner.x - nozzle.x) - axis;
    const relative = Math.atan2(Math.sin(raw), Math.cos(raw));
    if (relative < min) {
      min = relative;
      first = corner;
    }
    if (relative > max) {
      max = relative;
      last = corner;
    }
  }
  // (-sin, cos) points toward larger angles, so the lens points pair with `first` and `last`.
  const px = -Math.sin(axis) * halfWidth;
  const py = Math.cos(axis) * halfWidth;
  return [{ x: nozzle.x - px, y: nozzle.y - py }, first, last, { x: nozzle.x + px, y: nozzle.y + py }];
}
