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
  BURST_EVENT,
  BURST_FADE_MS,
  BURST_HOLD_MS,
  BURST_INNER_RADIUS,
  BURST_MARGIN,
  BURST_OUTER_RADIUS,
  BURST_POP_MS,
  BURST_SPIKES,
  BURST_WORDS_EN,
  BURST_WORDS_RU,
  LOCK_MIN_SIDE,
  MAX_SCAN_RECTS,
  MAX_STEP_TOTAL,
  RING_PAD,
  SCAN_FADE_MS,
  SCAN_LIFETIME_MS,
  SCAN_SWEEP_MS,
  VIEWPORT_MARGIN,
  burstTiltAngle,
  clampBurstCenter,
  computeBadgePosition,
  computeBeamPolygon,
  computeBurstGeometry,
  computeBurstPolygon,
  formatPolygonPoints,
  formatStepBadge,
  lockCenter,
  nearestRectEdge,
  nextBurstWord,
  missPath,
  parseHighlightMsg,
  ringRect,
  scanDelay,
  shouldDim,
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
      dim: true,
      rect: RECT,
      beamFrom: { x: 320, y: 100 },
      step: { number: 2, total: 3 },
    },
    {
      type: "target",
      dim: true,
      rect: { left: -20.5, top: 0.25, right: 0.5, bottom: 10.75 },
      beamFrom: null,
      step: null,
    },
    {
      type: "target",
      dim: true,
      rect: RECT,
      beamFrom: { x: 0, y: 0 },
      step: { number: 1, total: 1 },
    },
    {
      type: "target",
      dim: true,
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
    dim: false,
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

test("miss payloads parse into fresh copies and reject malformed coordinates", () => {
  const raw = { type: "miss", from: { x: -20.5, y: 40 }, rect: RECT };
  const parsed = parseHighlightMsg(raw);
  assert.deepEqual(parsed, raw);
  assert.notEqual(parsed, raw);
  if (parsed?.type !== "miss") assert.fail("expected miss");
  assert.notEqual(parsed.from, raw.from);
  assert.notEqual(parsed.rect, raw.rect);
  for (const bad of [
    { type: "miss", rect: RECT },
    { ...raw, extra: true },
    { ...raw, from: null },
    { ...raw, from: { x: Infinity, y: 1 } },
    { ...raw, from: { x: 1, y: NaN } },
    { ...raw, from: { x: "1", y: 2 } },
    { ...raw, from: { x: 1, y: 2, z: 3 } },
    { ...raw, rect: { ...RECT, right: RECT.left } },
  ]) assert.equal(parseHighlightMsg(bad), null, inspect(bad));
});

test("missPath curves from the click to the facing edge in all four directions", () => {
  for (const [from, end] of [
    [{ x: -20, y: 100 }, { x: 100, y: 100 }],
    [{ x: 320, y: 100 }, { x: 220, y: 100 }],
    [{ x: 160, y: -10 }, { x: 160, y: 80 }],
    [{ x: 160, y: 200 }, { x: 160, y: 120 }],
    [{ x: -20, y: -10 }, { x: 100, y: 100 }],
  ]) {
    const d = missPath(from, RECT);
    assert.ok(d);
    const parts = d.split(" ");
    assert.equal(parts[0], "M");
    assert.deepEqual(parts.slice(1, 3).map(Number), [from.x, from.y]);
    assert.equal(parts[3], "Q");
    assert.deepEqual(parts.slice(6).map(Number), [end.x, end.y]);
    const [cx, cy] = parts.slice(4, 6).map(Number);
    assert.ok(Number.isFinite(cx) && Number.isFinite(cy));
    assert.ok(Math.abs((cx - from.x) * (end.y - from.y) -
      (cy - from.y) * (end.x - from.x)) > 0);
  }
});

test("missPath rejects inside, boundary and invalid geometry", () => {
  for (const from of [
    { x: 160, y: 100 }, { x: 100, y: 100 },
    { x: NaN, y: 100 }, { x: 0, y: Infinity },
  ]) assert.equal(missPath(from, RECT), null);
  assert.equal(missPath({ x: 0, y: 0 }, { ...RECT, right: RECT.left }), null);
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
test("spotlight is suppressed above half the monitor, including ring padding", () => {
  const viewport = { width: 100, height: 100 };
  assert.equal(shouldDim({ left: 6, top: 6, right: 44, bottom: 94 }, viewport), true);
  assert.equal(shouldDim({ left: 6, top: 6, right: 45, bottom: 94 }, viewport), false);
  assert.equal(shouldDim({ left: -20, top: -20, right: 120, bottom: 120 }, viewport), false);
  assert.equal(shouldDim(RECT, { width: 0, height: 100 }), false);
  assert.equal(shouldDim({ left: -30, top: 20, right: 10, bottom: 30 }, viewport), true);
});

test("scan payloads parse into fresh copies within MAX_SCAN_RECTS", () => {
  const field: CssRect = { left: 10, top: 300.5, right: 210, bottom: 330 };
  const raw = { type: "scan", rects: [RECT, { ...RECT, top: 0 }], locked: [field] };
  const parsed = parseHighlightMsg(raw);
  assert.deepEqual(parsed, raw);
  if (parsed?.type !== "scan") assert.fail("expected scan");
  assert.notEqual(parsed.rects, raw.rects);
  assert.notEqual(parsed.rects[0], raw.rects[0]);
  assert.notEqual(parsed.locked[0], field);
  assert.deepEqual(parseHighlightMsg({ type: "scan", rects: [], locked: [] }), {
    type: "scan", rects: [], locked: [],
  });
  // MAX_LISTED in src-tauri/src/screen.rs: outlines and locks together.
  assert.equal(MAX_SCAN_RECTS, 60);
  const full = Array.from({ length: MAX_SCAN_RECTS }, () => RECT);
  assert.equal(parseHighlightMsg({ type: "scan", rects: full, locked: [] })?.type, "scan");
  assert.equal(parseHighlightMsg({ type: "scan", rects: [], locked: full })?.type, "scan");
  const holed: unknown[] = [RECT];
  holed.length = 2;
  for (const bad of [
    { type: "scan", rects: [] },
    { type: "scan", rects: [], locked: [], extra: 1 },
    { type: "scan", rects: null, locked: [] },
    { type: "scan", rects: [], locked: RECT },
    { type: "scan", rects: [RECT, { ...RECT, bottom: RECT.top }], locked: [] },
    { type: "scan", rects: [], locked: [{ ...RECT, left: NaN }] },
    { type: "scan", rects: [{ ...RECT, name: "Password" }], locked: [] },
    { type: "scan", rects: holed, locked: [] },
    { type: "scan", rects: [...full, RECT], locked: [] },
    { type: "scan", rects: full.slice(1), locked: [RECT, RECT] },
  ]) assert.equal(parseHighlightMsg(bad), null, inspect(bad));
});

test("the scan sweeps top to bottom and locks only tiles with room for one", () => {
  const at = (top: number): CssRect => ({ ...RECT, top, bottom: top + 20 });
  assert.equal(scanDelay(at(0), 1000), 0);
  assert.equal(scanDelay(at(500), 1000), SCAN_SWEEP_MS / 2);
  assert.equal(scanDelay(at(1000), 1000), SCAN_SWEEP_MS);
  assert.ok(scanDelay(at(100), 1000) < scanDelay(at(200), 1000));
  // Above or below the monitor (clipped by Rust anyway) and a bad viewport stay in range.
  assert.equal(scanDelay(at(-40), 1000), 0);
  assert.equal(scanDelay(at(5000), 1000), SCAN_SWEEP_MS);
  assert.equal(scanDelay(at(500), 0), 0);
  assert.equal(scanDelay(at(500), NaN), 0);
  assert.equal(scanDelay(at(500), 1000, 2000), 1000);
  // The whole sweep and its fade fit in the scan's lifetime.
  assert.ok(SCAN_SWEEP_MS + SCAN_FADE_MS < SCAN_LIFETIME_MS && SCAN_LIFETIME_MS <= 4000);

  assert.deepEqual(lockCenter({ left: 10, top: 20, right: 210, bottom: 50 }), { x: 110, y: 35 });
  assert.deepEqual(
    lockCenter({ left: 0, top: 0, right: LOCK_MIN_SIDE, bottom: LOCK_MIN_SIDE }),
    { x: LOCK_MIN_SIDE / 2, y: LOCK_MIN_SIDE / 2 },
  );
  assert.equal(lockCenter({ left: 0, top: 0, right: LOCK_MIN_SIDE - 1, bottom: 40 }), null);
  assert.equal(lockCenter({ left: 0, top: 0, right: 200, bottom: LOCK_MIN_SIDE - 0.5 }), null);
});

test("warmth accepts exactly ten levels and rejects malformed fields", () => {
  for (let level = 0; level < 10; level++) {
    const msg = { type: "warmth", level, onTarget: level === 9 };
    assert.deepEqual(parseHighlightMsg(msg), msg);
  }
  for (const level of [-1, 10, 0.5, NaN, Infinity, "9", null]) {
    assert.equal(parseHighlightMsg({ type: "warmth", level, onTarget: false }), null);
  }
  for (const msg of [
    { type: "warmth", level: 2 },
    { type: "warmth", level: 2, onTarget: 1 },
    { type: "warmth", level: 2, onTarget: false, extra: true },
    { type: "target", rect: RECT, dim: "yes" },
  ]) assert.equal(parseHighlightMsg(msg), null);
  assert.deepEqual(parseHighlightMsg({ type: "target", rect: RECT, dim: false }), {
    type: "target", rect: RECT, dim: false, beamFrom: null, step: null,
  });
});

test("burst word rotation picks words in turn and handles bounds", () => {
  assert.equal(nextBurstWord(0, BURST_WORDS_EN), "CLICK!");
  assert.equal(nextBurstWord(1, BURST_WORDS_EN), "NICE!");
  assert.equal(nextBurstWord(2, BURST_WORDS_EN), "DONE!");
  assert.equal(nextBurstWord(3, BURST_WORDS_EN), "CLICK!");
  assert.equal(nextBurstWord(4, BURST_WORDS_EN), "NICE!");

  assert.equal(nextBurstWord(0, BURST_WORDS_RU), "КЛИК!");
  assert.equal(nextBurstWord(1, BURST_WORDS_RU), "ЕСТЬ!");
  assert.equal(nextBurstWord(2, BURST_WORDS_RU), "ГОТОВО!");
  assert.equal(nextBurstWord(3, BURST_WORDS_RU), "КЛИК!");

  assert.equal(nextBurstWord(-1, BURST_WORDS_EN), "DONE!");
  assert.equal(nextBurstWord(0, []), "");

  assert.equal(burstTiltAngle(0), -6);
  assert.equal(burstTiltAngle(1), 4);
  assert.equal(burstTiltAngle(2), -4);
  assert.equal(burstTiltAngle(3), 5);
  assert.equal(burstTiltAngle(4), -6);
});

test("clampBurstCenter keeps the burst within viewport margins", () => {
  const vp = { width: 800, height: 600 };
  const r = BURST_OUTER_RADIUS;
  const m = BURST_MARGIN;

  // Well inside viewport
  assert.deepEqual(clampBurstCenter({ x: 400, y: 300 }, vp, r, m), { x: 400, y: 300 });

  // Pushed past left/top edges
  assert.deepEqual(clampBurstCenter({ x: 10, y: 5 }, vp, r, m), { x: m + r, y: m + r });

  // Pushed past right/bottom edges
  assert.deepEqual(clampBurstCenter({ x: 900, y: 700 }, vp, r, m), {
    x: vp.width - m - r,
    y: vp.height - m - r,
  });

  // Zero / negative center
  assert.deepEqual(clampBurstCenter({ x: -100, y: -50 }, vp, r, m), { x: m + r, y: m + r });
});

test("computeBurstPolygon produces a closed star polygon around center", () => {
  const center = { x: 200, y: 200 };
  const points = computeBurstPolygon(center, 60, 36, 12);
  assert.equal(points.length, 24);

  // Polygon has positive non-zero area
  const area = polygonSignedArea(points);
  assert.ok(area > 0);

  // All points stay within outer radius bound
  for (const pt of points) {
    const dist = Math.hypot(pt.x - center.x, pt.y - center.y);
    assert.ok(dist >= 30 && dist <= 75, `Point distance ${dist} out of bounds`);
  }
});

test("computeBurstGeometry clamps target burst inside viewport", () => {
  const vp = { width: 1024, height: 768 };

  // Center target
  const midTarget: CssRect = { left: 400, top: 300, right: 500, bottom: 350 };
  const midGeo = computeBurstGeometry(midTarget, vp);
  assert.equal(midGeo.center.x, 450);
  assert.equal(midGeo.center.y, 325);
  assert.equal(midGeo.points.length, BURST_SPIKES * 2);

  // Top-left corner target
  const edgeTarget: CssRect = { left: 0, top: 0, right: 40, bottom: 30 };
  const edgeGeo = computeBurstGeometry(edgeTarget, vp);
  assert.ok(edgeGeo.center.x >= BURST_MARGIN + edgeGeo.radius);
  assert.ok(edgeGeo.center.y >= BURST_MARGIN + edgeGeo.radius);
  for (const pt of edgeGeo.points) {
    assert.ok(pt.x >= 0 && pt.x <= vp.width, `Point x ${pt.x} outside viewport`);
    assert.ok(pt.y >= 0 && pt.y <= vp.height, `Point y ${pt.y} outside viewport`);
  }
});

test("parseHighlightMsg validates burst payloads strictly", () => {
  const valid = {
    type: "burst",
    rect: { left: 50, top: 60, right: 150, bottom: 120 },
  };
  assert.deepEqual(parseHighlightMsg(valid), valid);

  // Reject missing rect
  assert.equal(parseHighlightMsg({ type: "burst" }), null);
  // Reject extra properties
  assert.equal(parseHighlightMsg({ ...valid, extra: 123 }), null);
  // Reject inverted/empty rect
  assert.equal(parseHighlightMsg({ type: "burst", rect: { left: 150, top: 60, right: 50, bottom: 120 } }), null);
  assert.equal(parseHighlightMsg({ type: "burst", rect: { left: 50, top: 60, right: 50, bottom: 120 } }), null);
  // Reject non-number coordinates
  assert.equal(parseHighlightMsg({ type: "burst", rect: { left: "50", top: 60, right: 150, bottom: 120 } }), null);
});
