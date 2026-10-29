import assert from "node:assert/strict";
import { test } from "node:test";
import { canSend, parseHistory, sendsOnKey } from "./model.ts";

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
