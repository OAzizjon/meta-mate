// Run with `npm test` (Node's own test runner; Node 24 strips the types).
// Pure geometry and payload validation for the highlight window (src/highlight/geometry.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { inspect } from "node:util";

import {
  BADGE_GAP,
  BADGE_HEIGHT,
  BADGE_WIDTH,
  BEAM_HALF_WIDTH,
  MAX_STEP_TOTAL,
  RING_PAD,
  VIEWPORT_MARGIN,
  computeBadgePosition,
  computeBeamPolygon,
  formatPolygonPoints,
  formatStepBadge,
  nearestRectEdge,
  parseHighlightMsg,
  ringRect,
} from "./geometry.ts";
import type { CssPoint, CssRect } from "./geometry.ts";

const RECT: CssRect = { left: 100, top: 80, right: 220, bottom: 120 };
const VIEWPORT = { width: 1920, height: 1080 };

function polygonSignedArea(points: readonly CssPoint[]): number {
  let sum = 0;
  for (let i = 0; i < points.length; i++) {
    const a = points[i];
    const b = points[(i + 1) % points.length];
    sum += a.x * b.y - b.x * a.y;
  }
  return sum / 2;
}

function segmentsProperlyIntersect(
  a: CssPoint,
  b: CssPoint,
  c: CssPoint,
  d: CssPoint,
): boolean {
  const cross = (p: CssPoint, q: CssPoint, r: CssPoint): number =>
    (q.x - p.x) * (r.y - p.y) - (q.y - p.y) * (r.x - p.x);
  const d1 = cross(a, b, c);
  const d2 = cross(a, b, d);
  const d3 = cross(c, d, a);
  const d4 = cross(c, d, b);
  return ((d1 > 0 && d2 < 0) || (d1 < 0 && d2 > 0)) && ((d3 > 0 && d4 < 0) || (d3 < 0 && d4 > 0));
}

test("valid target and clear payloads parse into fresh copies", () => {
  const cases = [
    {
      type: "target",
      rect: RECT,
      beamFrom: { x: 320, y: 100 },
      step: { number: 2, total: 3 },
    },
    {
      type: "target",
      rect: { left: -20.5, top: 0.25, right: 0.5, bottom: 10.75 },
      beamFrom: null,
      step: null,
    },
    {
      type: "target",
      rect: RECT,
      beamFrom: { x: 0, y: 0 },
      step: { number: 1, total: 1 },
    },
    {
      type: "target",
      rect: RECT,
      beamFrom: null,
      step: { number: MAX_STEP_TOTAL, total: MAX_STEP_TOTAL },
    },
    { type: "clear" },
  ];

  for (const raw of cases) {
    const parsed = parseHighlightMsg(raw);
    assert.deepEqual(parsed, raw, inspect(raw));
    assert.notEqual(parsed, raw);
  }

  // Omitted optional nullable fields normalize to null.
  assert.deepEqual(parseHighlightMsg({ type: "target", rect: RECT }), {
    type: "target",
    rect: RECT,
    beamFrom: null,
    step: null,
  });
});

test("malformed highlight payloads are rejected without throwing", () => {
  const badRects: unknown[] = [
    null,
    undefined,
    [],
    {},
    { left: 10, top: 10, right: 10, bottom: 20 },
    { left: 12, top: 10, right: 10, bottom: 20 },
    { left: 10, top: 20, right: 20, bottom: 20 },
    { left: 10, top: 25, right: 20, bottom: 20 },
    { left: Number.NaN, top: 0, right: 10, bottom: 10 },
    { left: 0, top: Number.POSITIVE_INFINITY, right: 10, bottom: 10 },
    { left: 0, top: 0, right: Number.NEGATIVE_INFINITY, bottom: 10 },
    { left: "0", top: 0, right: 10, bottom: 10 },
    { left: 0, top: 0, right: 10 },
    { left: 0, top: 0, right: 10, bottom: 10, extra: 1 },
  ];

  const badBeams: unknown[] = [
    undefined,
    false,
    "10,20",
    [10, 20],
    {},
    { x: 10 },
    { x: 10, y: 20, z: 30 },
    { x: Number.NaN, y: 20 },
    { x: 10, y: Number.POSITIVE_INFINITY },
    { x: "10", y: 20 },
  ];

  const badSteps: unknown[] = [
    undefined,
    "2 / 3",
    [2, 3],
    {},
    { number: 0, total: 3 },
    { number: -1, total: 3 },
    { number: 4, total: 3 },
    { number: 1, total: 21 },
    { number: 21, total: 21 },
    { number: 1.5, total: 3 },
    { number: 1, total: 2.5 },
    { number: Number.NaN, total: 3 },
    { number: 1, total: Number.POSITIVE_INFINITY },
    { number: "1", total: 3 },
    { number: 1 },
    { number: 1, total: 2, extra: true },
  ];

  const malformed: unknown[] = [
    null,
    undefined,
    true,
    42,
    "clear",
    [],
    {},
    { type: "unknown" },
    { type: "clear", extra: 1 },
    { type: "target" },
    { type: "target", rect: RECT, extra: 1 },
    { type: "target", rect: RECT, beam_from: { x: 1, y: 2 } },
    Object.assign(Object.create({ type: "clear" }), {}),
    ...badRects.map((rect) => ({ type: "target", rect, beamFrom: null, step: null })),
    ...badBeams.map((beamFrom) => ({ type: "target", rect: RECT, beamFrom, step: null })),
    ...badSteps.map((step) => ({ type: "target", rect: RECT, beamFrom: null, step })),
  ];

  for (const raw of malformed) {
    assert.equal(parseHighlightMsg(raw), null, inspect(raw));
  }

  const boom = (): never => {
    throw new Error("boom");
  };
  assert.equal(parseHighlightMsg(new Proxy({}, { ownKeys: boom })), null);
  assert.equal(parseHighlightMsg({ get type() { return boom(); } }), null);
  assert.equal(
    parseHighlightMsg({ type: "target", get rect() { return boom(); }, beamFrom: null, step: null }),
    null,
  );
});

test("ringRect inflates the target by 6 px on every side", () => {
  const ring = ringRect(RECT);
  assert.deepEqual(ring, {
    left: RECT.left - RING_PAD,
    top: RECT.top - RING_PAD,
    right: RECT.right + RING_PAD,
    bottom: RECT.bottom + RING_PAD,
  });
  assert.equal(ring.right - ring.left, RECT.right - RECT.left + 2 * RING_PAD);
  assert.equal(ring.bottom - ring.top, RECT.bottom - RECT.top + 2 * RING_PAD);
});

test("computeBeamPolygon widens from beamFrom to the nearest edge in all four directions", () => {
  // Mate to the right -> right edge (x = 220, y = 80..120).
  const fromRight: CssPoint = { x: 320, y: 100 };
  assert.equal(nearestRectEdge(fromRight, RECT)?.side, "right");
  const rightPoly = computeBeamPolygon(fromRight, RECT);
  assert.deepEqual(rightPoly, [
    { x: 320, y: 100 + BEAM_HALF_WIDTH },
    { x: 220, y: 120 },
    { x: 220, y: 80 },
    { x: 320, y: 100 - BEAM_HALF_WIDTH },
  ]);

  // Mate to the left -> left edge (x = 100, y = 80..120).
  const fromLeft: CssPoint = { x: 20, y: 100 };
  assert.equal(nearestRectEdge(fromLeft, RECT)?.side, "left");
  const leftPoly = computeBeamPolygon(fromLeft, RECT);
  assert.deepEqual(leftPoly, [
    { x: 20, y: 100 - BEAM_HALF_WIDTH },
    { x: 100, y: 80 },
    { x: 100, y: 120 },
    { x: 20, y: 100 + BEAM_HALF_WIDTH },
  ]);

  // Mate above -> top edge (x = 100..220, y = 80).
  const fromAbove: CssPoint = { x: 160, y: 10 };
  assert.equal(nearestRectEdge(fromAbove, RECT)?.side, "top");
  const topPoly = computeBeamPolygon(fromAbove, RECT);
  assert.deepEqual(topPoly, [
    { x: 160 + BEAM_HALF_WIDTH, y: 10 },
    { x: 220, y: 80 },
    { x: 100, y: 80 },
    { x: 160 - BEAM_HALF_WIDTH, y: 10 },
  ]);

  // Mate below -> bottom edge (x = 100..220, y = 120).
  const fromBelow: CssPoint = { x: 160, y: 200 };
  assert.equal(nearestRectEdge(fromBelow, RECT)?.side, "bottom");
  const bottomPoly = computeBeamPolygon(fromBelow, RECT);
  assert.deepEqual(bottomPoly, [
    { x: 160 - BEAM_HALF_WIDTH, y: 200 },
    { x: 100, y: 120 },
    { x: 220, y: 120 },
    { x: 160 + BEAM_HALF_WIDTH, y: 200 },
  ]);
});

test("diagonal beamFrom points produce simple non-self-intersecting quads", () => {
  const origins: CssPoint[] = [
    { x: 10, y: 20 },
    { x: 80, y: 10 },
    { x: 300, y: 20 },
    { x: 240, y: 10 },
    { x: 300, y: 180 },
    { x: 240, y: 220 },
    { x: 10, y: 180 },
    { x: 80, y: 220 },
  ];

  for (const origin of origins) {
    const poly = computeBeamPolygon(origin, RECT);
    assert.ok(poly, inspect(origin));
    assert.equal(poly.length, 4);
    assert.ok(Math.abs(polygonSignedArea(poly)) > 0, `zero area for ${inspect(origin)}`);
    assert.equal(segmentsProperlyIntersect(poly[0], poly[1], poly[2], poly[3]), false);
    assert.equal(segmentsProperlyIntersect(poly[1], poly[2], poly[3], poly[0]), false);

    const baseWidth = Math.hypot(poly[3].x - poly[0].x, poly[3].y - poly[0].y);
    const targetEdgeWidth = Math.hypot(poly[2].x - poly[1].x, poly[2].y - poly[1].y);
    assert.ok(baseWidth <= targetEdgeWidth + 1e-9);
    assert.match(formatPolygonPoints(poly), /^-?\d+(\.\d+)?,-?\d+(\.\d+)?( -?\d+(\.\d+)?,-?\d+(\.\d+)?){3}$/);
  }
});

test("computeBeamPolygon returns null when beamFrom is inside or on the target rect", () => {
  for (const point of [
    { x: 160, y: 100 },
    { x: RECT.left, y: 100 },
    { x: RECT.right, y: 100 },
    { x: 160, y: RECT.top },
    { x: 160, y: RECT.bottom },
    { x: RECT.left, y: RECT.top },
    { x: RECT.right, y: RECT.bottom },
  ]) {
    assert.equal(computeBeamPolygon(point, RECT), null, inspect(point));
  }
  assert.equal(computeBeamPolygon({ x: 300, y: 100 }, RECT, 0), null);
  assert.equal(computeBeamPolygon({ x: 300, y: 100 }, RECT, -4), null);
});

test("step badge text is digits only and badge position stays inside the viewport", () => {
  assert.equal(formatStepBadge({ number: 2, total: 3 }), "2 / 3");
  assert.doesNotMatch(formatStepBadge({ number: 1, total: 20 }), /[A-Za-z\u0400-\u04FF]/);

  // Interior target: sits directly at the ring's top-left, above the ring by BADGE_GAP.
  const interior = computeBadgePosition(RECT, VIEWPORT);
  assert.deepEqual(interior, {
    x: RECT.left - RING_PAD,
    y: RECT.top - RING_PAD - BADGE_HEIGHT - BADGE_GAP,
  });

  // Targets near every edge and corner stay within the viewport margin.
  const edgeRects: CssRect[] = [
    { left: 0, top: 0, right: 40, bottom: 24 },
    { left: 2, top: 12, right: 80, bottom: 40 },
    { left: 1900, top: 4, right: 1918, bottom: 28 },
    { left: 1890, top: 1060, right: 1918, bottom: 1078 },
    { left: 4, top: 1065, right: 60, bottom: 1079 },
    { left: -20, top: -10, right: 20, bottom: 20 },
  ];

  for (const rect of edgeRects) {
    const pos = computeBadgePosition(rect, VIEWPORT);
    assert.ok(pos.x >= VIEWPORT_MARGIN, `x=${pos.x} for ${inspect(rect)}`);
    assert.ok(pos.y >= VIEWPORT_MARGIN, `y=${pos.y} for ${inspect(rect)}`);
    assert.ok(
      pos.x + BADGE_WIDTH <= VIEWPORT.width - VIEWPORT_MARGIN,
      `right=${pos.x + BADGE_WIDTH} for ${inspect(rect)}`,
    );
    assert.ok(
      pos.y + BADGE_HEIGHT <= VIEWPORT.height - VIEWPORT_MARGIN,
      `bottom=${pos.y + BADGE_HEIGHT} for ${inspect(rect)}`,
    );
  }
});
