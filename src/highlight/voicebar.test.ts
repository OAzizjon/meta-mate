// Run with `npm test` (Node's own test runner; Node 24 strips the types).
// The voice bar's payloads, smoothing, bar heights and words tail (src/highlight/voicebar.ts).
import assert from "node:assert/strict";
import { test } from "node:test";

import {
  BAR_COUNT,
  BAR_MIN,
  HISTORY_SIZE,
  MAX_HEARD_CHARS,
  MIN_TAIL_CHARS,
  VOICE_BAR_EVENT,
  barHeights,
  loudness,
  parseVoiceBarMsg,
  pushLevel,
  smoothLevel,
  stepBars,
  tailBudget,
  thinkingHeights,
  visibleTail,
} from "./voicebar.ts";

test("valid voice bar payloads parse into fresh copies", () => {
  assert.equal(VOICE_BAR_EVENT, "voice-bar");
  const valid: unknown[] = [
    { type: "state", state: "listening" },
    { type: "state", state: "thinking" },
    { type: "state", state: "idle" },
    { type: "level", level: 0 },
    { type: "level", level: 0.37 },
    { type: "level", level: 1 },
    { type: "heard", text: "" },
    { type: "heard", text: "open my mail" },
  ];
  for (const raw of valid) {
    const msg = parseVoiceBarMsg(raw);
    assert.deepEqual(msg, raw);
    assert.notEqual(msg, raw);
  }
  assert.deepEqual(parseVoiceBarMsg(JSON.parse('{"type":"level","level":0.005}')), {
    type: "level",
    level: 0.005,
  });
  const negativeZero = parseVoiceBarMsg({ type: "level", level: -0 });
  assert.ok(negativeZero?.type === "level" && Object.is(negativeZero.level, 0));
});

test("malformed voice bar payloads are dropped without throwing", () => {
  const throwing = {
    type: "heard",
    get text(): string {
      throw new Error("boom");
    },
  };
  const invalid: unknown[] = [
    null,
    undefined,
    "listening",
    42,
    [],
    {},
    { type: "speaking" },
    { type: "state" },
    { type: "state", state: "speaking" },
    { type: "state", state: "listening", extra: true },
    { type: "level" },
    { type: "level", level: -0.01 },
    { type: "level", level: 1.01 },
    { type: "level", level: Number.NaN },
    { type: "level", level: Number.POSITIVE_INFINITY },
    { type: "level", level: "0.5" },
    { type: "level", level: 0.5, generation: 1 },
    { type: "heard" },
    { type: "heard", text: 5 },
    { type: "heard", text: "hi", extra: 1 },
    { type: "heard", text: "a".repeat(MAX_HEARD_CHARS + 1) },
    { type: "heard", text: "\u{1F642}".repeat(MAX_HEARD_CHARS + 1) },
    throwing,
  ];
  for (const raw of invalid) {
    assert.equal(parseVoiceBarMsg(raw), null);
  }
  // Rust's chars() limit: 200 characters even outside the BMP.
  const emoji = "\u{1F642}".repeat(MAX_HEARD_CHARS);
  assert.deepEqual(parseVoiceBarMsg({ type: "heard", text: emoji }), { type: "heard", text: emoji });
});

test("loudness maps silence to 0, loud speech to 1 and keeps quiet speech visible", () => {
  assert.equal(loudness(0), 0);
  assert.equal(loudness(0.005), 0);
  assert.equal(loudness(0.6), 1);
  assert.equal(loudness(1), 1);
  assert.equal(loudness(Number.NaN), 0);
  const quiet = loudness(0.05);
  assert.ok(quiet > 0.2 && quiet < 0.35, `quiet speech ${quiet}`);
  assert.ok(loudness(0.1) < loudness(0.3));
});

test("smoothLevel rises fast, falls slowly and never overshoots", () => {
  const up = smoothLevel(0, 1, 35);
  const down = smoothLevel(1, 0, 35);
  assert.ok(up > 0.6, `attack ${up}`);
  assert.ok(down > 0.8, `release ${down}`);
  assert.ok(1 - down < up);
  assert.equal(smoothLevel(0.5, 0.5, 100), 0.5);
  assert.equal(smoothLevel(0.3, 1, 0), 0.3);
  assert.equal(smoothLevel(0.3, 1, Number.NaN), 0.3);
  assert.equal(smoothLevel(0.3, 1, 16, 0), 1);
  assert.equal(smoothLevel(2, -1, Number.POSITIVE_INFINITY), 0);
  let value = 0;
  for (let i = 0; i < 100; i++) {
    value = smoothLevel(value, 0.7, 16);
    assert.ok(value <= 0.7);
  }
  assert.ok(value > 0.69);
});

test("pushLevel keeps the newest levels in a fresh array", () => {
  const history = [0.1, 0.2, 0.3];
  assert.deepEqual(pushLevel(history, 0.4, 3), [0.2, 0.3, 0.4]);
  assert.deepEqual(history, [0.1, 0.2, 0.3]);
  assert.deepEqual(pushLevel([], 2), [1]);
  let long: number[] = [];
  for (let i = 0; i < 40; i++) long = pushLevel(long, 0.5);
  assert.equal(long.length, HISTORY_SIZE);
});

test("barHeights puts the newest level in the middle and older ones toward the ends", () => {
  const silent = new Array<number>(HISTORY_SIZE).fill(0);
  assert.deepEqual(barHeights(silent, BAR_COUNT, 0, false), new Array<number>(BAR_COUNT).fill(BAR_MIN));
  assert.deepEqual(barHeights([], BAR_COUNT, 0, false), new Array<number>(BAR_COUNT).fill(BAR_MIN));

  const newest = barHeights([...silent.slice(1), 1], BAR_COUNT, 0, false);
  assert.equal(newest.length, BAR_COUNT);
  assert.ok(newest[13] > 0.5 && newest[14] > 0.5);
  assert.equal(newest[0], BAR_MIN);
  assert.equal(newest[BAR_COUNT - 1], BAR_MIN);

  const oldest = barHeights([1, ...silent.slice(1)], BAR_COUNT, 0, false);
  assert.ok(oldest[0] > BAR_MIN && oldest[BAR_COUNT - 1] > BAR_MIN);
  assert.equal(oldest[13], BAR_MIN);

  const loud = barHeights(new Array<number>(HISTORY_SIZE).fill(1), BAR_COUNT, 500, true);
  for (const height of loud) assert.ok(height >= BAR_MIN && height <= 1);
  assert.equal(barHeights(silent, 0, 0, false).length, BAR_COUNT);
  assert.equal(barHeights(silent, 7, 0, false).length, 7);
});

test("the idle shimmer is faint, moves with time and is off with reduced motion", () => {
  const silent = new Array<number>(HISTORY_SIZE).fill(0);
  const now = barHeights(silent, BAR_COUNT, 0, true);
  const later = barHeights(silent, BAR_COUNT, 300, true);
  assert.ok(now.some((height) => height > BAR_MIN));
  assert.ok(now.every((height) => height < BAR_MIN + 0.1));
  assert.notDeepEqual(now, later);
});

test("thinkingHeights travels one low hump left to right, flat without shimmer", () => {
  assert.deepEqual(thinkingHeights(BAR_COUNT, 300, false), new Array<number>(BAR_COUNT).fill(BAR_MIN));
  const peak = (heights: readonly number[]): number => heights.indexOf(Math.max(...heights));
  const early = thinkingHeights(BAR_COUNT, 300, true);
  const late = thinkingHeights(BAR_COUNT, 700, true);
  assert.ok(peak(early) < peak(late));
  for (const height of [...early, ...late]) assert.ok(height >= BAR_MIN && height < BAR_MIN + 0.25);
});

test("stepBars eases every bar toward its target", () => {
  const targets = [1, 0.5, BAR_MIN];
  const next = stepBars([], targets, 16);
  assert.equal(next.length, 3);
  assert.ok(next[0] > BAR_MIN && next[0] < 1);
  assert.equal(next[2], BAR_MIN);
  assert.deepEqual(stepBars(next, targets, 0), next);
});

test("tailBudget fits two lines of the pill and stays within bounds", () => {
  assert.equal(tailBudget(584, 26), 68);
  assert.equal(tailBudget(0, 26), MIN_TAIL_CHARS);
  assert.equal(tailBudget(584, Number.NaN), MIN_TAIL_CHARS);
  assert.equal(tailBudget(100_000, 26), MAX_HEARD_CHARS);
  assert.equal(tailBudget(60, 26), MIN_TAIL_CHARS);
});

test("visibleTail keeps the newest whole words after an ellipsis", () => {
  assert.equal(visibleTail("  open   my\n mail ", 40), "open my mail");
  assert.equal(visibleTail("one two three four five", 12), "…four five");
  assert.equal(visibleTail("abcdef ghijk", 6), "…ghijk");
  assert.equal(visibleTail("abcdefghijklmnop", 6), "…lmnop");
  assert.equal(visibleTail("", 10), "");
  const emoji = visibleTail("\u{1F642}".repeat(10), 4);
  assert.equal(emoji, `…${"\u{1F642}".repeat(3)}`);
  const long = "where is the button that sends this letter to my grandson ".repeat(4);
  for (const max of [5, 12, 30, 68]) {
    const tail = visibleTail(long, max);
    assert.ok(Array.from(tail).length <= max, `${max}: ${tail}`);
    assert.ok(tail.startsWith("…"));
    assert.ok(long.trim().endsWith(tail.slice(1)));
  }
});
