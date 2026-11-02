import assert from "node:assert/strict";
import { test } from "node:test";
import { canSend, parseHistory, sendsOnKey } from "./model.ts";

test("legacy history loads and only supported Mate moods survive validation", () => {
  assert.deepEqual(parseHistory([
    { role: "mate", text: "old", timestamp: 1 },
    { role: "mate", text: "hello", timestamp: 2, mood: "happy" },
    { role: "mate", text: "unknown", timestamp: 3, mood: "<img>" },
    { role: "person", text: "hello", timestamp: 4, mood: "sad" },
  ]), [
    { role: "mate", text: "old", timestamp: 1 },
    { role: "mate", text: "hello", timestamp: 2, mood: "happy" },
    { role: "mate", text: "unknown", timestamp: 3 },
    { role: "person", text: "hello", timestamp: 4 },
  ]);
});

test("only Mate answers keep a model the chat can name", () => {
  assert.deepEqual(parseHistory([
    { role: "mate", text: "spark", timestamp: 1, model: "muse-spark-1.3", mood: "happy" },
    { role: "mate", text: "wonder", timestamp: 2, model: "muse-wonder-1.0" },
    { role: "mate", text: "card", timestamp: 3, model: "muse-glimmer-30b" },
    { role: "mate", text: "raw", timestamp: 4, model: "muse-spark-preview" },
    { role: "person", text: "hello", timestamp: 5, model: "muse-spark-1.3" },
  ]), [
    { role: "mate", text: "spark", timestamp: 1, mood: "happy", model: "muse-spark-1.3" },
    { role: "mate", text: "wonder", timestamp: 2, model: "muse-wonder-1.0" },
    { role: "mate", text: "card", timestamp: 3, model: "muse-glimmer-30b" },
    { role: "mate", text: "raw", timestamp: 4 },
    { role: "person", text: "hello", timestamp: 5 },
  ]);
});

test("only nonempty questions of at most 1000 Unicode characters send", () => {
  for (const text of ["", " \n\t", "a".repeat(1001)]) assert.equal(canSend(text), false);
  for (const text of [" hello ", "one\ntwo", "😀".repeat(1000)]) assert.equal(canSend(text), true);
  assert.equal(canSend("😀".repeat(1001)), false);
});

test("Enter sends, Shift+Enter and IME composition preserve input", () => {
  assert.equal(sendsOnKey("Enter", false, false), true);
  assert.equal(sendsOnKey("Enter", true, false), false);
  assert.equal(sendsOnKey("Enter", false, true), false);
  assert.equal(sendsOnKey("a", false, false), false);
});

test("history rejects malformed entries, preserves literal text, and keeps the last 200", () => {
  assert.deepEqual(parseHistory(null), []);
  assert.deepEqual(parseHistory([
    {}, { role: "person", text: "x", timestamp: -1 },
    { role: "mate", text: "x", timestamp: Number.MAX_SAFE_INTEGER },
  ]), []);
  const entries = Array.from({ length: 205 }, (_, timestamp) => ({
    role: timestamp % 2 ? "person" : "mate", text: "<b>literal</b>\nline", timestamp,
  }));
  const parsed = parseHistory(entries);
  assert.equal(parsed.length, 200);
  assert.equal(parsed[0].timestamp, 5);
  assert.equal(parsed[199].text, "<b>literal</b>\nline");
  assert.notEqual(parsed[0], entries[5]);
});
