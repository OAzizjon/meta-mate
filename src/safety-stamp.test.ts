// Run with npm test (Node's test runner strips the TypeScript types).
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { nextStamp, STAMP_KEYS, stampClearOf } from "./safety-stamp.ts";
import type { StampState } from "./safety-stamp.ts";
import type { Bound, TimelineItem } from "./ipc/timeline.ts";

const locales = Object.fromEntries(["en", "ru"].map((code) => [
  code, JSON.parse(readFileSync(new URL(`../locales/${code}.json`, import.meta.url), "utf8")) as Record<string, string>,
]));

const lang: TimelineItem = { kind: "lang", value: "en" };
const mood = (value: string): Bound => ({ kind: "mood", value });
const point: Bound = { kind: "point", value: { kind: "id", value: "e7" } };
const say = (...bound: Bound[]): TimelineItem => ({ kind: "say", value: { id: 1, text: "Text.", bound } });

function stampOf(items: readonly TimelineItem[]): StampState {
  return items.reduce<StampState>(nextStamp, undefined);
}

test("the first sentence's mood after <lang> is the verdict", () => {
  assert.equal(stampOf([lang]), undefined);
  assert.equal(stampOf([lang, say(mood("sad"))]), "scam");
  assert.equal(stampOf([lang, say(mood("surprised"))]), "careful");
  // "Looks fine" gets no stamp at all (D73).
  assert.equal(stampOf([lang, say(mood("calm"))]), null);
  // The prompt's own example: the warning first, the red flag pointed at after it.
  assert.equal(stampOf([lang, say(mood("sad")), say(point), say(), { kind: "end" }]), "scam");
});

test("anything else first leaves no stamp, for good", () => {
  for (const first of [
    say(),
    say(mood("happy")),
    say(mood("thinking")),
    say(mood("constructor")),
    say(mood("calm"), point),
    { kind: "bound", value: mood("sad") },
    { kind: "bad_tag" },
    { kind: "point_rejected" },
    { kind: "end" },
  ] satisfies TimelineItem[]) {
    assert.equal(stampOf([lang, first]), null, JSON.stringify(first));
    assert.equal(stampOf([lang, first, say(mood("sad"))]), null, JSON.stringify(first));
  }
});

test("a calm first sentence never stamps; a warning stays for the answer", () => {
  for (const later of [say(mood("sad")), say(mood("surprised")), say(point), { kind: "point_rejected" }] satisfies TimelineItem[]) {
    assert.equal(stampOf([lang, say(mood("calm")), later]), null, JSON.stringify(later));
  }
  assert.equal(stampOf([lang, say(mood("sad")), say(mood("calm"))]), "scam");
  assert.equal(stampOf([lang, say(mood("surprised")), say(mood("sad"))]), "careful");
});

test("the stamp yields to a beam target anywhere inside the window", () => {
  const rect = (left: number, top: number, right: number, bottom: number) => ({ left, top, right, bottom });
  assert.equal(stampClearOf(null, 220, 220), true);
  for (const outside of [rect(-90, 10, 0, 40), rect(220, 10, 300, 40), rect(10, -50, 60, 0), rect(10, 220, 60, 260)]) {
    assert.equal(stampClearOf(outside, 220, 220), true, JSON.stringify(outside));
  }
  for (const inside of [rect(-90, 10, 1, 40), rect(219, 219, 300, 300), rect(50, 150, 120, 180), rect(-10, -10, 400, 400)]) {
    assert.equal(stampClearOf(inside, 220, 220), false, JSON.stringify(inside));
  }
  assert.equal(stampClearOf(rect(NaN, 0, NaN, 10), 220, 220), false);
});

test("every stamp word exists, short and distinct, in both languages", () => {
  for (const [locale, strings] of Object.entries(locales)) {
    const words = Object.values(STAMP_KEYS).map((key) => strings[key]);
    for (const word of words) {
      assert.ok(word && word.length <= 20 && word === word.toUpperCase(), `${locale}: ${word}`);
    }
    assert.equal(new Set(words).size, words.length, locale);
  }
});
