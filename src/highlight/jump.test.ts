// Run with `npm test` (Node's own test runner; Node 24 strips the types).
// Mate's surprise entrance: payload, timeline and shake (src/highlight/jump.ts).
import assert from "node:assert/strict";
import { test } from "node:test";

import {
  JUMP_EXIT_MS,
  JUMP_FADE_MS,
  JUMP_LEAP_MS,
  JUMP_MAX_MS,
  JUMP_SHAKE_PX,
  MATE_JUMP_EVENT,
  jumpTimeline,
  parseJumpMsg,
  shakeOffsets,
} from "./jump.ts";
import type { JumpCue, JumpStep } from "./jump.ts";

const at = (steps: readonly JumpStep[], cue: JumpCue): number => {
  const step = steps.find((candidate) => candidate.cue === cue);
  assert.ok(step, `missing ${cue}`);
  return step.at;
};

test("valid jump payloads parse into fresh copies", () => {
  assert.equal(MATE_JUMP_EVENT, "mate-jump");
  for (const raw of [{ type: "jump", locale: "en" }, { type: "jump", locale: "ru" }]) {
    const msg = parseJumpMsg(raw);
    assert.deepEqual(msg, raw);
    assert.notEqual(msg, raw);
  }
  assert.deepEqual(parseJumpMsg(JSON.parse('{"type":"jump","locale":"ru"}')), { type: "jump", locale: "ru" });
});

test("malformed jump payloads are dropped without throwing", () => {
  const throwing = {
    type: "jump",
    get locale(): string {
      throw new Error("boom");
    },
  };
  const invalid: unknown[] = [
    null,
    undefined,
    "jump",
    [],
    {},
    { type: "jump" },
    { locale: "en" },
    { type: "jump", locale: "de" },
    { type: "jump", locale: "EN" },
    { type: "jump", locale: 1 },
    { type: "boo", locale: "en" },
    { type: "jump", locale: "en", extra: true },
    throwing,
  ];
  for (const raw of invalid) {
    assert.equal(parseJumpMsg(raw), null);
  }
});

test("the timeline runs in order and ends within 3 s, leaving time for the exit", () => {
  for (const reduced of [false, true]) {
    const steps = jumpTimeline(reduced);
    assert.equal(steps[0].cue, "enter");
    assert.equal(steps[0].at, 0);
    assert.equal(steps[steps.length - 1].cue, "done");
    for (let i = 1; i < steps.length; i++) assert.ok(steps[i - 1].at <= steps[i].at);
    assert.equal(new Set(steps.map((step) => step.cue)).size, steps.length);
    assert.ok(at(steps, "balloon") < at(steps, "line"));
    assert.ok(at(steps, "line") < at(steps, "wink"));
    assert.ok(at(steps, "wink") < at(steps, "exit"));
    assert.ok(at(steps, "done") <= JUMP_MAX_MS);
  }
});

test("full motion leaps, flashes and shakes as he lands", () => {
  const steps = jumpTimeline(false);
  assert.ok(at(steps, "flash") < JUMP_LEAP_MS);
  assert.ok(at(steps, "shake") < JUMP_LEAP_MS);
  assert.ok(Math.abs(at(steps, "wink") - 1600) <= 200);
  assert.ok(at(steps, "exit") + JUMP_EXIT_MS <= at(steps, "done"));
});

test("reduced motion has no flash and no shake, only fades", () => {
  const steps = jumpTimeline(true);
  const cues = steps.map((step) => step.cue);
  assert.ok(!cues.includes("flash"));
  assert.ok(!cues.includes("shake"));
  assert.ok(at(steps, "balloon") >= JUMP_FADE_MS);
  assert.ok(at(steps, "exit") + JUMP_FADE_MS <= at(steps, "done"));
});

test("the shake jolts 6 to 8 px and settles back to rest", () => {
  const offsets = shakeOffsets();
  assert.deepEqual(offsets[0], { x: 0, y: 0 });
  assert.deepEqual(offsets[offsets.length - 1], { x: 0, y: 0 });
  const strongest = Math.max(...offsets.map((offset) => Math.hypot(offset.x, offset.y)));
  assert.ok(strongest >= 6 && strongest <= 8.5, `strongest ${strongest}`);
  for (const offset of offsets) {
    assert.ok(Math.abs(offset.x) <= JUMP_SHAKE_PX && Math.abs(offset.y) <= JUMP_SHAKE_PX);
  }
  assert.deepEqual(shakeOffsets(Number.NaN), offsets);
  assert.ok(Math.abs(shakeOffsets(2)[1].x) <= 2);
});
