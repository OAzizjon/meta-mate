// Run with `npm test` (Node's own test runner; Node 24 strips the types).
// Mate's springs (spring.ts): they settle, stay within their clamps, keep the glass's area,
// trail by their documented lag, and give the rest pose when he must hold still.
import assert from "node:assert/strict";
import { test } from "node:test";

import {
  AT_REST,
  FOLLOW_LAG_MS,
  MAX_LEAN_DEG,
  MAX_SQUASH,
  MAX_STRETCH,
  REST_POSE,
  STALE_MS,
  VELOCITY_WINDOW_MS,
  WOBBLE,
  addSample,
  bodyMatrix,
  createMotionModel,
  followSpring,
  leanFor,
  scalePair,
  stepSpring,
  stretchFor,
  velocityOf,
} from "./spring.ts";
import type { MotionPose, MoveSample, Spring, SquashKind } from "./spring.ts";

const FRAME_MS = 16;

function determinant(pose: MotionPose): number {
  const [a, b, c, d] = bodyMatrix(pose);
  return a * d - b * c;
}

test("the body's wobble overshoots, swings about twice and comes to rest", () => {
  let spring: Spring = { value: 1, velocity: 0 };
  const extremes: number[] = [];
  let previous = 0;
  for (let t = 0; t < 2000; t += FRAME_MS) {
    spring = stepSpring(spring, 0, WOBBLE, FRAME_MS);
    if (previous !== 0 && Math.sign(spring.velocity) !== Math.sign(previous)) extremes.push(spring.value);
    previous = spring.velocity;
  }
  const visible = extremes.filter((value) => Math.abs(value) >= 0.05);
  assert.ok(visible.length >= 2 && visible.length <= 4, `visible swings: ${visible.join(", ")}`);
  assert.ok(visible[0] < -0.3 && visible[0] > -0.6, `first overshoot ${visible[0]}`);
  for (let i = 1; i < visible.length; i++) {
    assert.ok(Math.sign(visible[i]) !== Math.sign(visible[i - 1]), "swings alternate");
    assert.ok(Math.abs(visible[i]) < Math.abs(visible[i - 1]), "and shrink");
  }
  assert.ok(Math.abs(spring.value) < 0.005 && Math.abs(spring.velocity) < 0.05);
});

test("a spring step survives a missing or huge frame time", () => {
  const moving: Spring = { value: 1, velocity: 0 };
  assert.deepEqual(stepSpring(moving, 0, WOBBLE, Number.NaN), moving);
  assert.deepEqual(stepSpring(moving, 0, WOBBLE, -5), moving);
  // A background tab's long gap is simulated as one short frame, never as an explosion.
  const after = stepSpring(moving, 0, WOBBLE, 60_000);
  assert.ok(Number.isFinite(after.value) && Math.abs(after.value) <= 1);
});

test("the arms, the cap and the filament trail the body by 60-120 ms and overshoot a little", () => {
  for (const [part, lagMs] of Object.entries(FOLLOW_LAG_MS)) {
    const params = followSpring(lagMs);
    // A leader moving steadily at 1 unit per second: the follower settles lagMs behind.
    let ramp = AT_REST;
    for (let t = 1; t <= 1500; t += 1) ramp = stepSpring(ramp, t / 1000, params, 1);
    const measured = (1.5 - ramp.value) * 1000;
    assert.ok(Math.abs(measured - lagMs) < 5, `${part}: lag ${measured} ms`);
    assert.ok(measured >= 60 && measured <= 120, `${part}: lag ${measured} ms`);
    // A leader that stops: a small overshoot, then rest.
    let step = AT_REST;
    let peak = 0;
    for (let t = 0; t < 1500; t += FRAME_MS) {
      step = stepSpring(step, 1, params, FRAME_MS);
      peak = Math.max(peak, step.value);
    }
    assert.ok(peak - 1 > 0.03 && peak - 1 < 0.2, `${part}: overshoot ${peak - 1}`);
    assert.ok(Math.abs(step.value - 1) < 0.001);
  }
});

test("lean and stretch follow the speed and are clamped", () => {
  assert.ok(Math.abs(leanFor(500) - 6) < 1e-9);
  assert.equal(leanFor(5000), MAX_LEAN_DEG);
  assert.equal(leanFor(-5000), -MAX_LEAN_DEG);
  assert.equal(stretchFor(-10), 0);
  assert.equal(stretchFor(100_000), MAX_STRETCH);

  // A fling at 3000 px/s, then a stop: the overshoot on the way back is clamped too.
  const model = createMotionModel();
  let t = 0;
  for (; t <= 300; t += FRAME_MS) {
    model.moved(t * 3, -t, t);
    model.step(t, false);
  }
  model.release();
  for (const end = t + 2000; t <= end; t += FRAME_MS) {
    const pose = model.step(t, false);
    assert.ok(Math.abs(pose.leanDeg) <= MAX_LEAN_DEG);
    assert.ok(Math.abs(pose.stretch) <= MAX_STRETCH);
    assert.ok(Math.abs(pose.squash) <= MAX_SQUASH);
    for (const part of [pose.filamentDeg, pose.capDeg, pose.armLDeg, pose.armRDeg]) assert.ok(Math.abs(part) <= 10);
  }
});

test("squash and stretch keep the glass's area (scaleX x scaleY = 1)", () => {
  for (const amount of [-0.5, -0.1, 0, 0.07, 0.5, Number.NaN]) {
    const { long, short } = scalePair(amount);
    assert.ok(Math.abs(long * short - 1) < 1e-12, String(amount));
    assert.ok(long >= 1 - MAX_SQUASH && long <= 1 + MAX_SQUASH, String(amount));
  }
  for (const leanDeg of [-12, -5, 0, 7, 12]) {
    for (const stretch of [-0.08, 0, 0.05, 0.08]) {
      for (const axisDeg of [0, 33, 90, -135]) {
        for (const squash of [-0.15, -0.04, 0, 0.1]) {
          const pose = { ...REST_POSE, leanDeg, stretch, axisDeg, squash };
          assert.ok(Math.abs(determinant(pose) - 1) < 1e-9, JSON.stringify(pose));
        }
      }
    }
  }
  // Squash + is taller and thinner; the rest pose is the identity.
  const [a, , , d] = bodyMatrix({ ...REST_POSE, squash: 0.1 });
  assert.ok(Math.abs(d - 1.1) < 1e-9 && Math.abs(a - 1 / 1.1) < 1e-9);
  const rest = bodyMatrix(REST_POSE);
  [1, 0, 0, 1].forEach((expected, i) => assert.ok(Math.abs(rest[i] - expected) < 1e-12));
});

test("velocity: the mean over the newest moves, none when stale, a jump starts over", () => {
  let samples: MoveSample[] = addSample([], { x: 0, y: 0, t: 0 });
  assert.deepEqual(velocityOf(samples, 0), { x: 0, y: 0 }, "one move is no velocity");
  samples = addSample(samples, { x: 10, y: -5, t: 20 });
  const v = velocityOf(samples, 20);
  assert.ok(Math.abs(v.x - 500) < 1e-9 && Math.abs(v.y + 250) < 1e-9);
  assert.deepEqual(velocityOf(samples, 20 + STALE_MS + 1), { x: 0, y: 0 });
  // A new home or another monitor's scale: faster than any drag, so it starts over.
  assert.deepEqual(addSample(samples, { x: 5000, y: 0, t: 36 }), [{ x: 5000, y: 0, t: 36 }]);
  assert.equal(addSample(samples, { x: Number.NaN, y: 0, t: 40 }).length, samples.length);

  let many: MoveSample[] = [];
  for (let t = 0; t <= 300; t += 10) many = addSample(many, { x: t, y: 0, t });
  assert.ok(many.every((sample) => 300 - sample.t <= VELOCITY_WINDOW_MS));
  assert.ok(Math.abs(velocityOf(many, 300).x - 1000) < 1e-9);
});

test("a drag: he leans into it, the parts trail, he lands with a squash and wobbles to rest", () => {
  const model = createMotionModel();
  const poses: MotionPose[] = [];
  let t = 0;
  // 400 ms to the right at 750 px/s, one move per frame.
  for (; t <= 400; t += FRAME_MS) {
    model.moved(t * 0.75, 0, t);
    poses.push(model.step(t, false));
  }
  const leaning = poses[poses.length - 1];
  assert.ok(leaning.leanDeg > 4, `lean ${leaning.leanDeg}`);
  assert.ok(leaning.stretch > 0.02, `stretch ${leaning.stretch}`);
  assert.ok(Math.abs(leaning.axisDeg) < 1e-9, "stretched along the motion");
  assert.ok(poses.some((pose) => pose.filamentDeg < -0.5), "the filament trails the lean");
  assert.ok(poses.some((pose) => pose.armLDeg < -0.5 && pose.armRDeg < -0.5), "so do the arms");

  model.release();
  let landed = false;
  let swungBack = false;
  for (const end = t + 3000; t <= end; t += FRAME_MS) {
    const pose = model.step(t, false);
    if (pose.squash < -0.02) landed = true;
    if (pose.leanDeg < -0.5) swungBack = true;
    assert.ok(Math.abs(determinant(pose) - 1) < 1e-9);
  }
  assert.ok(landed, "squashes on landing");
  assert.ok(swungBack, "wobbles past upright");
  assert.equal(model.atRest(), true);
});

test("a move that stops on its own (a flight) lands too; a plain click does nothing", () => {
  const flight = createMotionModel();
  let t = 0;
  for (; t <= 200; t += FRAME_MS) {
    flight.moved(0, t * 2, t);
    flight.step(t, false);
  }
  let landed = false;
  for (const end = t + STALE_MS + 300; t <= end; t += FRAME_MS) {
    if (flight.step(t, false).squash < -0.02) landed = true;
  }
  assert.ok(landed);

  const click = createMotionModel();
  click.moved(100, 100, 0);
  click.release();
  assert.equal(click.step(16, false), REST_POSE);
  assert.equal(click.atRest(), true);
});

test("jumps and hellos crouch first (anticipation), swells and wiggles stretch first", () => {
  const firstWay: Record<SquashKind, number> = { hello: -1, jump: -1, flinch: -1, swell: 1, wiggle: 1 };
  for (const [kind, sign] of Object.entries(firstWay) as [SquashKind, number][]) {
    const model = createMotionModel();
    model.squash(kind);
    model.step(0, false);
    assert.equal(Math.sign(model.step(FRAME_MS, false).squash), sign, kind);
  }
  // A jump: crouch, then stretch on the way up.
  const jump = createMotionModel();
  jump.squash("jump");
  let crouchedAt = -1;
  let stretchedAt = -1;
  for (let t = 0; t <= 600; t += FRAME_MS) {
    const { squash } = jump.step(t, false);
    if (crouchedAt < 0 && squash < -0.05) crouchedAt = t;
    if (stretchedAt < 0 && squash > 0.02) stretchedAt = t;
  }
  assert.ok(crouchedAt >= 0 && stretchedAt > crouchedAt, `crouch ${crouchedAt} ms, stretch ${stretchedAt} ms`);
});

test("held still (reduced motion, the beam on, asleep) he is in the rest pose and forgets the motion", () => {
  const model = createMotionModel();
  let t = 0;
  for (let i = 0; i < 10; i++, t += FRAME_MS) {
    model.moved(i * FRAME_MS, 0, t);
    model.step(t, false);
  }
  model.squash("jump");
  const moving = model.step(t, false);
  assert.ok(moving.leanDeg > 1, `lean ${moving.leanDeg}`);
  assert.equal(model.step(t + FRAME_MS, true), REST_POSE);
  assert.equal(model.atRest(), true);
  // Nothing carries over once he may move again.
  assert.equal(model.step(t + 2 * FRAME_MS, false), REST_POSE);
  model.moved(500, 0, t + 3 * FRAME_MS);
  assert.equal(model.step(t + 3 * FRAME_MS, false), REST_POSE, "one move is no velocity");
});
