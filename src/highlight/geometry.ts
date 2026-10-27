// Pure geometry and wire validation for the click-through highlight window (docs/architecture.md
// section 2, WP5): parses `HighlightMsg` payloads from Rust, inflates the target ring, builds the
// light-beam polygon from `beamFrom` to the target's nearest edge, and keeps the step badge inside
// the viewport. No DOM or Tauri imports, so Node tests run this module directly.

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
  readonly rect: CssRect;
  readonly beamFrom: CssPoint | null;
  readonly step: HighlightStep | null;
}

export interface HighlightClearMsg {
  readonly type: "clear";
}

/** Wire payload of the `"highlight"` Tauri event sent to the highlight window. */
export type HighlightMsg = HighlightTargetMsg | HighlightClearMsg;

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
export const BADGE_WIDTH = 48;
export const BADGE_HEIGHT = 22;
/** Vertical gap between the step badge and the top of the ring (CSS px). */
export const BADGE_GAP = 4;
/** Minimum distance from the step badge to the viewport edges (CSS px). */
export const VIEWPORT_MARGIN = 4;
/** Duration of the fade-out when `"clear"` arrives (ms). */
export const CLEAR_FADE_MS = 200;

type JsonObject = Readonly<Record<string, unknown>>;

const TARGET_ALLOWED_KEYS = ["type", "rect", "beamFrom", "step"] as const;
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

/** True when `value` is a plain `{ x, y }` object with finite coordinates. */
export function isValidPoint(value: unknown): value is CssPoint {
  return parsePoint(value) !== null;
}

/** True when `value` is a plain `{ left, top, right, bottom }` object with finite bounds and positive size. */
export function isValidRect(value: unknown): value is CssRect {
  return parseRect(value) !== null;
}

/** True when `value` is a plain `{ number, total }` object with integers `1 <= number <= total <= 20`. */
export function isValidStep(value: unknown): value is HighlightStep {
  return parseStep(value) !== null;
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

  return { type: "target", rect, beamFrom, step };
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

export const inflateRect = ringRect;

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

export const beamPolygon = computeBeamPolygon;

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

export const badgePosition = computeBadgePosition;
