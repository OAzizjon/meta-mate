// Run with npm test (Node's test runner strips the TypeScript types).
import assert from "node:assert/strict";
import { test } from "node:test";
import { canReuseMessages, nextAwaitingReply } from "./history.ts";
import type { AwaitingReply } from "./history.ts";
import { HISTORY_LIMIT, parseHistory } from "./model.ts";
import type { Message } from "./model.ts";

const question: Message = { role: "person", text: "question", timestamp: 1 };
const answer: Message = { role: "mate", text: "answer", timestamp: 2 };

test("empty, unchanged and appended histories keep existing bubbles", () => {
  assert.equal(canReuseMessages([], []), true);
  assert.equal(canReuseMessages([], [question]), true);
  assert.equal(canReuseMessages([question], [{ ...question }]), true);
  assert.equal(canReuseMessages([question], [question, answer]), true);
});

test("streamed text changes keep bubbles without mutating either history", () => {
  const previous = Object.freeze([question, Object.freeze({ ...answer, text: "first" })]);
  const next = Object.freeze([question, Object.freeze({ ...answer, text: "first second" })]);
  assert.equal(canReuseMessages(previous, next), true);
  assert.equal(previous[1].text, "first");
  assert.equal(next[1].text, "first second");
});

test("cleared, shortened, reordered or replaced histories rebuild bubbles", () => {
  assert.equal(canReuseMessages([question], []), false);
  assert.equal(canReuseMessages([question, answer], [question]), false);
  assert.equal(canReuseMessages([question, answer], [answer, question]), false);
  assert.equal(canReuseMessages([question], [{ ...question, role: "mate" }]), false);
  assert.equal(canReuseMessages([question], [{ ...question, timestamp: 3 }]), false);
  assert.equal(canReuseMessages([question, answer], [question, { ...answer, timestamp: 3 }]), false);
});

test("history limit rollover rebuilds bubbles even when the message count stays the same", () => {
  const raw = Array.from({ length: HISTORY_LIMIT + 1 }, (_, timestamp) => ({ ...question, timestamp }));
  const previous = parseHistory(raw.slice(0, HISTORY_LIMIT));
  const next = parseHistory(raw);
  assert.equal(previous.length, next.length);
  assert.equal(canReuseMessages(previous, next), false);
});

test("every waiting state handles empty, person and Mate history tails", () => {
  const cases: [AwaitingReply, AwaitingReply[]][] = [
    [null, [null, null, null]],
    ["sent", ["sent", "asked", "sent"]],
    ["asked", [null, "asked", null]],
  ];
  const histories = [[], [answer, question], [question, answer]];
  for (const [state, expected] of cases) {
    histories.forEach((history, index) => {
      assert.equal(nextAwaitingReply(state, history), expected[index], `${state}, history ${index}`);
    });
  }
});

test("a stale snapshot cannot finish a new wait before the question appears", () => {
  let state: AwaitingReply = "sent";
  for (const history of [[], [answer], [question], [question], [question, answer]]) {
    state = nextAwaitingReply(state, history);
    if (history.at(-1) === question) assert.equal(state, "asked");
    else if (history.length < 2) assert.equal(state, "sent");
  }
  assert.equal(state, null);
  assert.equal(nextAwaitingReply(state, [question]), null);
});

test("clearing after the question appears ends the wait, including invalid history payloads", () => {
  const state = nextAwaitingReply("sent", parseHistory([question]));
  assert.equal(state, "asked");
  for (const raw of [[], null, [{ role: "mate", text: "", timestamp: 2 }]]) {
    assert.equal(nextAwaitingReply(state, parseHistory(raw)), null);
  }
});
