// Pure geometry and wire validation for the click-through highlight window (docs/architecture.md
// section 2, WP5): parses `HighlightMsg` payloads from Rust, inflates the target ring, builds the
// light-beam polygon from `beamFrom` to the target's nearest edge, keeps the step badge inside
// the viewport, and times the scan's sweep and places its locks. No DOM or Tauri imports, so Node
// tests run this module directly.

import type { CssPoint, CssRect } from "../robot/types.ts";

export type { CssPoint, CssRect } from "../robot/types.ts";

export interface CssSize {
  readonly width: number;
  readonly height: number;
}

/** Step progress shown on the ring's top-left badge (`1 <= number <= total <= 20`). */
export interface HighlightStep {
  readonly number: number;
  readonly total: number;
}

export interface HighlightTargetMsg {
  readonly type: "target";
  readonly dim: boolean;
  readonly rect: CssRect;
  readonly beamFrom: CssPoint | null;
  readonly step: HighlightStep | null;
}

export interface HighlightClearMsg {
  readonly type: "clear";
}

/** Wire payload of the `"highlight"` Tauri event sent to the highlight window. */
export interface HighlightWarmthMsg {
  readonly type: "warmth";
  readonly level: number;
  readonly onTarget: boolean;
}

export interface HighlightMissMsg {
  readonly type: "miss";
  readonly from: CssPoint;
  readonly rect: CssRect;
}

/** What Mate read while the model thinks: outlines, and hidden fields as locked tiles. */
export interface HighlightScanMsg {
  readonly type: "scan";
  readonly rects: readonly CssRect[];
  readonly locked: readonly CssRect[];
}

/** A short comic onomatopoeia burst when the person does a lesson step right. */
export interface HighlightBurstMsg {
  readonly type: "burst";
  readonly rect: CssRect;
}

export type HighlightMsg =
  | HighlightTargetMsg
  | HighlightClearMsg
  | HighlightWarmthMsg
  | HighlightMissMsg
  | HighlightScanMsg
  | HighlightBurstMsg;

export type RectEdgeSide = "left" | "right" | "top" | "bottom";

export interface RectEdge {
  readonly side: RectEdgeSide;
  readonly start: CssPoint;
  readonly end: CssPoint;
}

/** Outward padding of the glowing ring around the target rectangle (CSS px). */
export const RING_PAD = 6;
/** Corner radius of the glowing target ring (CSS px). */
export const RING_RADIUS = 10;
/** Semi-transparent warm white of the highlight beam. */
export const BEAM_COLOR = "#fff6c8";
/** Half-width of the light beam at `beamFrom` (CSS px). */
export const BEAM_HALF_WIDTH = 6;
/** Maximum allowed `total` in a step badge (`1 <= number <= total <= 20`). */
export const MAX_STEP_TOTAL = 20;
/** Default step badge box for viewport clamping (CSS px). */
export const BADGE_WIDTH = 64;
export const BADGE_HEIGHT = 30;
/** Vertical gap between the step badge and the top of the ring (CSS px). */
export const BADGE_GAP = 4;
/** Minimum distance from the step badge to the viewport edges (CSS px). */
export const VIEWPORT_MARGIN = 4;
/** Duration of the fade-out when `"clear"` arrives (ms). */
export const CLEAR_FADE_MS = 200;
/** Most rectangles in one scan, outlines and locked tiles together (`MAX_LISTED` in src-tauri). */
export const MAX_SCAN_RECTS = 60;
/** Time for the scan to sweep from the top of the monitor to the bottom (ms). */
export const SCAN_SWEEP_MS = 900;
/** The scan is gone this long after it arrives, its fade-out included (ms). */
export const SCAN_LIFETIME_MS = 4000;
/** Duration of the scan's fade-out (ms). */
export const SCAN_FADE_MS = 300;
/** A locked tile gets its lock only when both sides are at least this long (CSS px). */
export const LOCK_MIN_SIDE = 16;
/** Name of the Tauri event signaling a comic burst. */
export const BURST_EVENT = "highlight-burst";
/** Default outer radius of the comic star burst (CSS px). */
export const BURST_OUTER_RADIUS = 60;
/** Default inner radius of the comic star burst valleys (CSS px). */
export const BURST_INNER_RADIUS = 36;
/** Number of outer spikes on the star burst. */
export const BURST_SPIKES = 12;
/** Minimum clearance from viewport edges (CSS px). */
export const BURST_MARGIN = 16;
/** Overshoot pop-in duration (ms). */
export const BURST_POP_MS = 200;
/** Hold duration on screen (ms). */
export const BURST_HOLD_MS = 600;
/** Fade-out duration (ms). */
export const BURST_FADE_MS = 250;
export const BURST_WORDS_EN = ["CLICK!", "NICE!", "DONE!"] as const;
export const BURST_WORDS_RU = ["КЛИК!", "ЕСТЬ!", "ГОТОВО!"] as const;

export interface BurstGeometry {
  readonly center: CssPoint;
  readonly points: readonly CssPoint[];
  readonly radius: number;
}

type JsonObject = Readonly<Record<string, unknown>>;

const TARGET_ALLOWED_KEYS = ["type", "rect", "beamFrom", "step", "dim"] as const;
const RECT_KEYS = ["left", "top", "right", "bottom"] as const;
const POINT_KEYS = ["x", "y"] as const;
const STEP_KEYS = ["number", "total"] as const;

/** `value` limited to `[min, max]`; `NaN` becomes `min`. */
export function clamp(value: number, min: number, max: number): number {
  return Number.isNaN(value) ? min : Math.min(max, Math.max(min, value));
}

/** Short SVG/CSS attribute numbers rounded to 0.01. */
export function num(value: number): string {
  return String(Math.round(value * 100) / 100);
}

/**
 * Validates a `"highlight"` event payload against the wire contract and returns a fresh copy, or
 * `null` when anything is malformed (unknown type, extra/missing keys, non-finite numbers,
 * zero/negative rect size, or step outside `1 <= number <= total <= 20`). Never throws.
 */
export function parseHighlightMsg(raw: unknown): HighlightMsg | null {
  try {
    return parseMsg(raw);
  } catch {
    return null;
  }
}

function parseMsg(raw: unknown): HighlightMsg | null {
  if (!isPlainObject(raw)) return null;
  const { type } = raw;
  if (type === "clear") {
    return exact(raw, ["type"]) ? { type: "clear" } : null;
  }
  if (type === "warmth") {
    const { level, onTarget } = raw;
    return exact(raw, ["type", "level", "onTarget"]) &&
      typeof level === "number" && Number.isInteger(level) && level >= 0 && level <= 9 &&
      typeof onTarget === "boolean"
      ? { type, level, onTarget } : null;
  }
  if (type === "miss") {
    if (!exact(raw, ["type", "from", "rect"])) return null;
    const from = parsePoint(raw.from);
    const rect = parseRect(raw.rect);
    return from && rect ? { type, from, rect } : null;
  }
  if (type === "scan") {
    if (!exact(raw, ["type", "rects", "locked"])) return null;
    const rects = parseRects(raw.rects);
    const locked = parseRects(raw.locked);
    return rects && locked && rects.length + locked.length <= MAX_SCAN_RECTS
      ? { type, rects, locked } : null;
  }
  if (type === "burst") {
    if (!exact(raw, ["type", "rect"])) return null;
    const rect = parseRect(raw.rect);
    return rect ? { type: "burst", rect } : null;
  }
  if (type !== "target") return null;

  const own = Object.keys(raw);
  if (
    !own.includes("type") ||
    !own.includes("rect") ||
    !own.every((key) => (TARGET_ALLOWED_KEYS as readonly string[]).includes(key))
  ) {
    return null;
  }

  const rect = parseRect(raw.rect);
  if (!rect) return null;

  let beamFrom: CssPoint | null = null;
  if (Object.hasOwn(raw, "beamFrom") && raw.beamFrom !== null) {
    beamFrom = parsePoint(raw.beamFrom);
    if (!beamFrom) return null;
  }

  let step: HighlightStep | null = null;
  if (Object.hasOwn(raw, "step") && raw.step !== null) {
    step = parseStep(raw.step);
    if (!step) return null;
  }

  const dim = Object.hasOwn(raw, "dim") ? raw.dim : false;
  if (typeof dim !== "boolean") return null;
  return { type: "target", rect, beamFrom, step, dim };
}

/** Only the visible part of the padded ring counts against half this monitor. */
export function shouldDim(rect: CssRect, viewport: CssSize): boolean {
  if (!(viewport.width > 0 && viewport.height > 0)) return false;
  const ring = ringRect(rect);
  const width = Math.max(0, Math.min(viewport.width, ring.right) - Math.max(0, ring.left));
  const height = Math.max(0, Math.min(viewport.height, ring.bottom) - Math.max(0, ring.top));
  return width * height <= viewport.width * viewport.height / 2;
}

function parseRect(value: unknown): CssRect | null {
  const obj = exact(value, RECT_KEYS);
  if (!obj) return null;
  const { left, top, right, bottom } = obj;
  if (
    typeof left !== "number" ||
    typeof top !== "number" ||
    typeof right !== "number" ||
    typeof bottom !== "number" ||
    !Number.isFinite(left) ||
    !Number.isFinite(top) ||
    !Number.isFinite(right) ||
    !Number.isFinite(bottom) ||
    left >= right ||
    top >= bottom
  ) {
    return null;
  }
  return { left, top, right, bottom };
}

/** A plain array of at most `MAX_SCAN_RECTS` valid rects; one bad entry or hole rejects it. */
function parseRects(value: unknown): CssRect[] | null {
  if (!Array.isArray(value) || value.length > MAX_SCAN_RECTS) return null;
  const rects: CssRect[] = [];
  for (let i = 0; i < value.length; i++) {
    const rect = parseRect(value[i]);
    if (!rect) return null;
    rects.push(rect);
  }
  return rects;
}

function parsePoint(value: unknown): CssPoint | null {
  const obj = exact(value, POINT_KEYS);
  if (!obj) return null;
  const { x, y } = obj;
  if (typeof x !== "number" || typeof y !== "number" || !Number.isFinite(x) || !Number.isFinite(y)) {
    return null;
  }
  return { x, y };
}

function parseStep(value: unknown): HighlightStep | null {
  const obj = exact(value, STEP_KEYS);
  if (!obj) return null;
  const { number, total } = obj;
  if (
    typeof number !== "number" ||
    typeof total !== "number" ||
    !Number.isInteger(number) ||
    !Number.isInteger(total) ||
    number < 1 ||
    number > total ||
    total > MAX_STEP_TOTAL
  ) {
    return null;
  }
  return { number, total };
}

function isPlainObject(value: unknown): value is JsonObject {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function exact(value: unknown, keys: readonly string[]): JsonObject | null {
  if (!isPlainObject(value)) return null;
  const own = Object.keys(value);
  return own.length === keys.length && own.every((key) => keys.includes(key)) ? value : null;
}

/** Inflates `rect` outward by `pad` CSS pixels (defaults to `RING_PAD = 6`). */
export function ringRect(rect: CssRect, pad: number = RING_PAD): CssRect {
  return {
    left: rect.left - pad,
    top: rect.top - pad,
    right: rect.right + pad,
    bottom: rect.bottom + pad,
  };
}

/**
 * Picks the edge of `rect` that faces `from`. Returns `null` when `from` is inside or on the
 * boundary of `rect`, or when the inputs are not valid.
 */
export function nearestRectEdge(from: CssPoint, rect: CssRect): RectEdge | null {
  if (
    !Number.isFinite(from.x) ||
    !Number.isFinite(from.y) ||
    !Number.isFinite(rect.left) ||
    !Number.isFinite(rect.top) ||
    !Number.isFinite(rect.right) ||
    !Number.isFinite(rect.bottom) ||
    rect.left >= rect.right ||
    rect.top >= rect.bottom
  ) {
    return null;
  }

  const leftGap = rect.left - from.x;
  const rightGap = from.x - rect.right;
  const topGap = rect.top - from.y;
  const bottomGap = from.y - rect.bottom;
  if (leftGap <= 0 && rightGap <= 0 && topGap <= 0 && bottomGap <= 0) {
    return null;
  }

  const horizontalGap = Math.max(leftGap, rightGap);
  const verticalGap = Math.max(topGap, bottomGap);
  if (horizontalGap >= verticalGap) {
    if (leftGap >= rightGap) {
      return {
        side: "left",
        start: { x: rect.left, y: rect.top },
        end: { x: rect.left, y: rect.bottom },
      };
    }
    return {
      side: "right",
      start: { x: rect.right, y: rect.top },
      end: { x: rect.right, y: rect.bottom },
    };
  }
  if (topGap >= bottomGap) {
    return {
      side: "top",
      start: { x: rect.left, y: rect.top },
      end: { x: rect.right, y: rect.top },
    };
  }
  return {
    side: "bottom",
    start: { x: rect.left, y: rect.bottom },
    end: { x: rect.right, y: rect.bottom },
  };
}

/**
 * Computes the 4-vertex SVG beam polygon from `beamFrom` (narrow base of half-width `halfWidth`,
 * capped at half the target edge so the beam never narrows toward the target) widening to the
 * nearest edge of `rect`. Returns `null` when `beamFrom` is inside or on `rect` or inputs are
 * invalid.
 */
export function computeBeamPolygon(
  beamFrom: CssPoint,
  rect: CssRect,
  halfWidth: number = BEAM_HALF_WIDTH,
): readonly [CssPoint, CssPoint, CssPoint, CssPoint] | null {
  if (!Number.isFinite(halfWidth) || halfWidth <= 0) return null;
  const edge = nearestRectEdge(beamFrom, rect);
  if (!edge) return null;

  const midX = (edge.start.x + edge.end.x) / 2;
  const midY = (edge.start.y + edge.end.y) / 2;
  const dx = midX - beamFrom.x;
  const dy = midY - beamFrom.y;
  const dist = Math.hypot(dx, dy);
  if (!(dist > 0)) return null;

  const nx = -dy / dist;
  const ny = dx / dist;
  const edgeLength = Math.hypot(edge.end.x - edge.start.x, edge.end.y - edge.start.y);
  const narrowHalf = Math.min(halfWidth, edgeLength / 2);

  const baseFirst: CssPoint = {
    x: beamFrom.x - nx * narrowHalf,
    y: beamFrom.y - ny * narrowHalf,
  };
  const baseLast: CssPoint = {
    x: beamFrom.x + nx * narrowHalf,
    y: beamFrom.y + ny * narrowHalf,
  };

  const edgeOrder = (edge.end.x - edge.start.x) * nx + (edge.end.y - edge.start.y) * ny;
  const [edgeFirst, edgeLast] =
    edgeOrder >= 0 ? [edge.start, edge.end] : [edge.end, edge.start];

  return [baseFirst, edgeFirst, edgeLast, baseLast];
}

/** A softly bowed quadratic curve to the middle of the facing edge. */
export function missPath(from: CssPoint, rect: CssRect): string | null {
  const edge = nearestRectEdge(from, rect);
  if (!edge) return null;
  const x = edge.start.x / 2 + edge.end.x / 2;
  const y = edge.start.y / 2 + edge.end.y / 2;
  const dx = x - from.x;
  const dy = y - from.y;
  const distance = Math.hypot(dx, dy);
  if (!Number.isFinite(distance) || distance <= 0) return null;
  const bend = Math.min(48, distance * 0.18);
  const cx = from.x / 2 + x / 2 - dy / distance * bend;
  const cy = from.y / 2 + y / 2 + dx / distance * bend;
  return `M ${num(from.x)} ${num(from.y)} Q ${num(cx)} ${num(cy)} ${num(x)} ${num(y)}`;
}

/** When the sweep reaches `rect`: its top's share of the monitor's height of `sweepMs`. */
export function scanDelay(
  rect: CssRect,
  viewportHeight: number,
  sweepMs: number = SCAN_SWEEP_MS,
): number {
  if (!Number.isFinite(viewportHeight) || viewportHeight <= 0) return 0;
  return Math.round(clamp(rect.top / viewportHeight, 0, 1) * sweepMs);
}

/** The middle of a locked tile, where its lock goes; `null` when the tile is too small. */
export function lockCenter(rect: CssRect): CssPoint | null {
  if (!(rect.right - rect.left >= LOCK_MIN_SIDE && rect.bottom - rect.top >= LOCK_MIN_SIDE)) {
    return null;
  }
  return { x: rect.left / 2 + rect.right / 2, y: rect.top / 2 + rect.bottom / 2 };
}

/** Formats polygon vertices for an SVG `points` attribute. */
export function formatPolygonPoints(points: readonly CssPoint[]): string {
  return points.map((p) => `${num(p.x)},${num(p.y)}`).join(" ");
}

/**
 * Formats the step badge text as digits only (`"2 / 3"`, invariant 7: no words).
 */
export function formatStepBadge(step: HighlightStep): string {
  return `${step.number} / ${step.total}`;
}

/**
 * Places the step badge at the top-left of the target's inflated ring and clamps its top-left
 * corner so the whole badge stays inside `viewport` (with `margin` px padding).
 */
export function computeBadgePosition(
  rect: CssRect,
  viewport: CssSize,
  badgeSize: CssSize = { width: BADGE_WIDTH, height: BADGE_HEIGHT },
  margin: number = VIEWPORT_MARGIN,
): CssPoint {
  const ring = ringRect(rect, RING_PAD);
  const bw = Number.isFinite(badgeSize.width) && badgeSize.width > 0 ? badgeSize.width : BADGE_WIDTH;
  const bh =
    Number.isFinite(badgeSize.height) && badgeSize.height > 0 ? badgeSize.height : BADGE_HEIGHT;
  const vw = Number.isFinite(viewport.width) && viewport.width > 0 ? viewport.width : bw;
  const vh = Number.isFinite(viewport.height) && viewport.height > 0 ? viewport.height : bh;
  const pad = Number.isFinite(margin) && margin >= 0 ? margin : VIEWPORT_MARGIN;

  const idealX = ring.left;
  const idealY = ring.top - bh - BADGE_GAP;

  const maxX = Math.max(0, vw - bw - pad);
  const maxY = Math.max(0, vh - bh - pad);
  const minX = Math.min(pad, maxX);
  const minY = Math.min(pad, maxY);

  return {
    x: clamp(idealX, minX, maxX),
    y: clamp(idealY, minY, maxY),
  };
}


/** Picks the word for this step in turn (0 -> CLICK!, 1 -> NICE!, 2 -> DONE!, 3 -> CLICK!, ...). */
export function nextBurstWord(turn: number, words: readonly string[]): string {
  if (words.length === 0) return "";
  const index = ((Math.trunc(turn) % words.length) + words.length) % words.length;
  return words[index];
}

/** Slight comic tilt angle in degrees (-6, +4, -4, +5) varying with turn. */
export function burstTiltAngle(turn: number): number {
  const angles = [-6, 4, -4, 5];
  const index = ((Math.trunc(turn) % angles.length) + angles.length) % angles.length;
  return angles[index];
}

/** Clamps the burst center so the star burst with outer `radius` stays within `viewport` margin. */
export function clampBurstCenter(
  center: CssPoint,
  viewport: CssSize,
  radius: number = BURST_OUTER_RADIUS,
  margin: number = BURST_MARGIN,
): CssPoint {
  const vw = Number.isFinite(viewport.width) && viewport.width > 0 ? viewport.width : radius * 2;
  const vh = Number.isFinite(viewport.height) && viewport.height > 0 ? viewport.height : radius * 2;
  const r = Number.isFinite(radius) && radius > 0 ? radius : BURST_OUTER_RADIUS;
  const pad = Number.isFinite(margin) && margin >= 0 ? margin : BURST_MARGIN;

  const minX = pad + r;
  const maxX = Math.max(minX, vw - pad - r);
  const minY = pad + r;
  const maxY = Math.max(minY, vh - pad - r);

  return {
    x: clamp(center.x, minX, maxX),
    y: clamp(center.y, minY, maxY),
  };
}

/** Computes the points of a jagged comic star burst polygon around `center`. */
export function computeBurstPolygon(
  center: CssPoint,
  outerRadius: number = BURST_OUTER_RADIUS,
  innerRadius: number = BURST_INNER_RADIUS,
  spikes: number = BURST_SPIKES,
): readonly CssPoint[] {
  const points: CssPoint[] = [];
  const nSpikes = Number.isInteger(spikes) && spikes >= 3 ? spikes : BURST_SPIKES;
  const rOut = Number.isFinite(outerRadius) && outerRadius > 0 ? outerRadius : BURST_OUTER_RADIUS;
  const rIn = Number.isFinite(innerRadius) && innerRadius > 0 ? innerRadius : BURST_INNER_RADIUS;
  const total = nSpikes * 2;

  for (let i = 0; i < total; i++) {
    const angle = (i * Math.PI) / nSpikes - Math.PI / 2;
    const isOuter = i % 2 === 0;
    const r = isOuter
      ? rOut * (1 + 0.12 * Math.sin(i * 1.5))
      : rIn * (1 - 0.08 * Math.cos(i * 1.5));
    points.push({
      x: Math.round((center.x + r * Math.cos(angle)) * 100) / 100,
      y: Math.round((center.y + r * Math.sin(angle)) * 100) / 100,
    });
  }
  return points;
}

/** Computes the clamped center and burst polygon for `target` within `viewport`. */
export function computeBurstGeometry(
  target: CssRect,
  viewport: CssSize,
  outerRadius: number = BURST_OUTER_RADIUS,
  innerRadius: number = BURST_INNER_RADIUS,
  spikes: number = BURST_SPIKES,
  margin: number = BURST_MARGIN,
): BurstGeometry {
  const rawCenter = {
    x: (target.left + target.right) / 2,
    y: (target.top + target.bottom) / 2,
  };
  const maxRadius = outerRadius * 1.15;
  const center = clampBurstCenter(rawCenter, viewport, maxRadius, margin);
  const points = computeBurstPolygon(center, outerRadius, innerRadius, spikes);
  return { center, points, radius: maxRadius };
}
