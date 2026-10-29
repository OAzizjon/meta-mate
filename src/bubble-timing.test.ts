// Run with `npm test` (Node's own test runner; Node 24 strips the types).
// Tests for how long Mate's speech bubble stays (src/bubble-timing.ts, WP85).
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

import {
  BUBBLE_LINGER_MS,
  BUBBLE_MAX_MS,
  BUBBLE_MIN_MS,
  BUBBLE_MS_PER_CHAR,
  BUBBLE_NOTICE_MS,
  bubbleLingerMs,
  bubbleReadMs,
} from "./bubble-timing.ts";

// The locale files read directly: src/i18n imports them as JSON modules, which Node's test
// runner cannot load.
type Strings = Record<string, string>;
const LOCALES: Record<string, Strings> = {
  en: JSON.parse(readFileSync(new URL("../locales/en.json", import.meta.url), "utf8")) as Strings,
  ru: JSON.parse(readFileSync(new URL("../locales/ru.json", import.meta.url), "utf8")) as Strings,
};

test("bubbleReadMs gives a short text the minimum and grows with the text", () => {
  assert.equal(bubbleReadMs(""), BUBBLE_MIN_MS);
  assert.equal(bubbleReadMs("One moment."), BUBBLE_MIN_MS);

  const sentence = "a".repeat(57);
  assert.equal(bubbleReadMs(sentence), BUBBLE_NOTICE_MS + 57 * BUBBLE_MS_PER_CHAR);
  assert.ok(bubbleReadMs(sentence + "a") > bubbleReadMs(sentence));
});

test("bubbleReadMs stops at the maximum and ignores surrounding spaces", () => {
  assert.equal(bubbleReadMs("a".repeat(1000)), BUBBLE_MAX_MS);
  assert.equal(bubbleReadMs(`  ${"a".repeat(57)}\n`), bubbleReadMs("a".repeat(57)));
});

test("bubbleReadMs counts characters, so Russian reads as long as English of the same length", () => {
  const english = "e".repeat(60);
  const russian = "ж".repeat(60);
  const emoji = "\u{1F44D}".repeat(60);
  assert.equal(bubbleReadMs(russian), bubbleReadMs(english));
  assert.equal(bubbleReadMs(emoji), bubbleReadMs(english));
});

test("every line Mate shows by itself stays longer than the old 4 s, in both languages", () => {
  for (const [locale, strings] of Object.entries(LOCALES)) {
    for (const [key, text] of Object.entries(strings)) {
      if (!key.startsWith("robot.")) continue;
      const ms = bubbleReadMs(text.replaceAll("{name}", "Mate"));
      assert.ok(ms > 4000, `${locale} ${key}: ${ms}`);
      assert.ok(ms <= BUBBLE_MAX_MS, `${locale} ${key}: ${ms}`);
    }
  }
});

test("bubbleLingerMs keeps the rest of the reading time, and at least the minimum", () => {
  const sentence = "a".repeat(57);
  const read = bubbleReadMs(sentence);
  assert.equal(bubbleLingerMs(sentence, 0), read);
  assert.equal(bubbleLingerMs(sentence, 3000), read - 3000);
  assert.equal(bubbleLingerMs(sentence, read), BUBBLE_LINGER_MS);
  assert.equal(bubbleLingerMs(sentence, 60_000), BUBBLE_LINGER_MS);
  assert.equal(bubbleLingerMs(sentence, 60_000, 1500), 1500);
});

test("bubbleLingerMs reads a bad shown time as just shown", () => {
  const read = bubbleReadMs("Here it is.");
  for (const shown of [Number.NaN, Number.POSITIVE_INFINITY, Number.NEGATIVE_INFINITY, -5]) {
    assert.equal(bubbleLingerMs("Here it is.", shown), read, String(shown));
  }
});
