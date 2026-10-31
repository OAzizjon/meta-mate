// Run with `npm test` (Node's own test runner; Node 24 strips the types).
// Unit tests for "My lessons" on Home (src/home/lessons.ts): the cards_list payload, the
// order, the date and the "Show all" rule.
import assert from "node:assert/strict";
import { test } from "node:test";
import { inspect } from "node:util";

import {
  CARDS_LIST_COMMAND,
  CARDS_OPEN_FOLDER_COMMAND,
  CARD_SHOW_COMMAND,
  LESSON_STEPS_SHOWN,
  formatLessonDate,
  loadLessonCards,
  parseLessonCard,
  parseLessonCards,
  shownLessonSteps,
} from "./lessons.ts";

const PNG = "data:image/png;base64,iVBORw0KGgo=";
// 2026-11-01 12:00 UTC.
const NOV_1 = 1_793_534_400;

test("the commands match the cards contract", () => {
  assert.equal(CARDS_LIST_COMMAND, "cards_list");
  assert.equal(CARD_SHOW_COMMAND, "card_show");
  assert.equal(CARDS_OPEN_FOLDER_COMMAND, "cards_open_folder");
});

test("a card keeps its fields; only a PNG data URL is a thumbnail", () => {
  assert.deepEqual(
    parseLessonCard({ id: "c1", title: " Send a photo ", steps: ["Open Mail", " ", 3, " Click Attach "], created: NOV_1, thumb: PNG }),
    { id: "c1", title: "Send a photo", steps: ["Open Mail", "Click Attach"], created: NOV_1, thumb: PNG },
  );
  for (const thumb of [null, undefined, "", "https://example.com/a.png", "file:///C:/a.png", "data:image/svg+xml;base64,PHN2Zz4=", 7]) {
    assert.equal(parseLessonCard({ id: "c1", title: "T", steps: [], created: NOV_1, thumb })?.thumb, null, inspect(thumb));
  }
  // Missing or odd fields are empty, not fatal.
  assert.deepEqual(parseLessonCard({ id: "c2" }), { id: "c2", title: "", steps: [], created: 0, thumb: null });
  for (const created of [-5, Number.NaN, Number.POSITIVE_INFINITY, "1793534400", null]) {
    assert.equal(parseLessonCard({ id: "c3", created })?.created, 0, inspect(created));
  }
});

test("a card without an id is dropped, and odd payloads never throw", () => {
  for (const bad of [null, undefined, "c1", 7, [], { id: "" }, { id: "  " }, { id: 7, title: "T" }]) {
    assert.equal(parseLessonCard(bad), null, inspect(bad));
  }
  const boom = new Proxy({}, { get: () => { throw new Error("boom"); } });
  assert.equal(parseLessonCard(boom), null);
  for (const bad of [null, undefined, {}, "[]", 0, { cards: [] }]) {
    assert.deepEqual(parseLessonCards(bad), [], inspect(bad));
  }
});

test("the list is newest first, keeps same-second order and drops repeated ids", () => {
  const cards = parseLessonCards([
    { id: "old", title: "Old", created: NOV_1 - 86_400 },
    { id: "a", title: "A", created: NOV_1 },
    null,
    { id: "undated", title: "Undated" },
    { id: "b", title: "B", created: NOV_1 },
    { id: "a", title: "A again", created: NOV_1 + 60 },
    { title: "No id", created: NOV_1 + 120 },
  ]);
  assert.deepEqual(cards.map((card) => card.id), ["a", "b", "old", "undated"]);
  assert.equal(cards[0].title, "A");
});

test("loadLessonCards asks cards_list and shows no cards when it is missing or fails", async () => {
  const calls: string[] = [];
  const cards = await loadLessonCards(async (cmd) => {
    calls.push(cmd);
    return [{ id: "a", title: "A", steps: ["One"], created: NOV_1, thumb: null }];
  });
  assert.deepEqual(calls, ["cards_list"]);
  assert.deepEqual(cards.map((card) => card.id), ["a"]);

  assert.deepEqual(await loadLessonCards(null), []);
  assert.deepEqual(await loadLessonCards(async () => { throw new Error("command cards_list not found"); }), []);
  assert.deepEqual(await loadLessonCards(async () => "not a list"), []);
});

test("the date is the card's day in the page language, empty when unknown", () => {
  const en = formatLessonDate(NOV_1, "en", "UTC");
  assert.match(en, /November/);
  assert.match(en, /\b1\b/);
  assert.match(en, /2026/);
  assert.match(formatLessonDate(NOV_1, "ru", "UTC"), /ноября/);
  for (const created of [0, -1, Number.NaN, Number.POSITIVE_INFINITY, 1e20]) {
    assert.equal(formatLessonDate(created, "en", "UTC"), "", inspect(created));
  }
});

test("steps collapse to the first two with Show all only when there are more", () => {
  assert.equal(LESSON_STEPS_SHOWN, 2);
  assert.deepEqual(shownLessonSteps(0, false), { shown: 0, canExpand: false });
  assert.deepEqual(shownLessonSteps(2, false), { shown: 2, canExpand: false });
  assert.deepEqual(shownLessonSteps(3, false), { shown: 2, canExpand: true });
  assert.deepEqual(shownLessonSteps(3, true), { shown: 3, canExpand: true });
  assert.deepEqual(shownLessonSteps(9, false), { shown: 2, canExpand: true });
  assert.deepEqual(shownLessonSteps(-1, true), { shown: 0, canExpand: false });
});
