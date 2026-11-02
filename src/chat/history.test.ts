// Run with npm test (Node's test runner strips the TypeScript types).
import assert from "node:assert/strict";
import { test } from "node:test";
import { canReuseMessages, mateAnswerGrew, nextAwaitingReply, parseMateBlocks, parseMateInline } from "./history.ts";
import type { AwaitingReply } from "./history.ts";
import { HISTORY_LIMIT, parseHistory } from "./model.ts";
import type { Message } from "./model.ts";

const question: Message = { role: "person", text: "question", timestamp: 1 };
const answer: Message = { role: "mate", text: "answer", timestamp: 2 };

test("quoted chips preserve English and Russian text, apostrophes and unfinished quotes", () => {
  const text = 'Click "Save", «Печать» or “Close”. Don\'t click "unfinished';
  const nodes = parseMateInline(text);
  assert.deepEqual(nodes.filter((node) => node.kind === "key").map((node) => node.text), [
    '"Save"', '«Печать»', '“Close”',
  ]);
  assert.equal(nodes.map((node) => node.text).join(""), text);
});

test("URLs remain passive tokens and keep balanced parentheses and literal markup", () => {
  const text = '<img src=x> (https://example.org/a(b)), www.example.org. javascript:alert(1)';
  const nodes = parseMateInline(text);
  assert.deepEqual(nodes.filter((node) => node.kind === "url").map((node) => node.text), [
    'https://example.org/a(b)', 'www.example.org',
  ]);
  assert.equal(nodes.map((node) => node.text).join(""), text);
  assert.deepEqual(parseMateInline("Do not click if you want to keep this window open."), [
    { kind: "text", text: "Do not click if you want to keep this window open." },
  ]);
});

test("step lists preserve gaps, restarts, blank lines and following prose", () => {
  assert.deepEqual(parseMateBlocks('Steps:\n3. Open "Settings"\n5) Choose display\n1. Restart\n\nDone.'), [
    { kind: "paragraph", children: [{ kind: "text", text: "Steps:" }] },
    { kind: "list", items: [
      { number: 3, children: [{ kind: "text", text: "Open " }, { kind: "key", text: '"Settings"' }] },
      { number: 5, children: [{ kind: "text", text: "Choose display" }] },
      { number: 1, children: [{ kind: "text", text: "Restart" }] },
    ] },
    { kind: "paragraph", children: [] },
    { kind: "paragraph", children: [{ kind: "text", text: "Done." }] },
  ]);
});

test("mood-only streaming updates keep bubble identity", () => {
  assert.equal(canReuseMessages([answer], [{ ...answer, mood: "sad" }]), true);
});

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

test("only a new or longer Mate answer counts as a new answer", () => {
  assert.equal(mateAnswerGrew([question], [question, answer]), true);
  assert.equal(mateAnswerGrew([question, answer], [question, { ...answer, text: "answer more" }]), true);
  assert.equal(mateAnswerGrew([], [answer]), true);
  assert.equal(mateAnswerGrew([question, answer], [question, { ...answer, model: "muse-spark-1.3" }]), false);
  assert.equal(mateAnswerGrew([question, answer], [question, answer]), false);
  assert.equal(mateAnswerGrew([answer], [answer, question]), false);
  assert.equal(mateAnswerGrew([question, answer], [{ ...answer, timestamp: 3 }]), false);
  assert.equal(mateAnswerGrew([question], []), false);
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
