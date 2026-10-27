// Run with `npm test` (Node's own test runner; Node 24 strips the types).
// The wire format of src/ipc/timeline.ts against the Rust snapshot.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

import { parseTimelineItem } from "../ipc/timeline.ts";

const SNAPSHOT = new URL(
  "../../crates/mm-core/tests/snapshots/timeline_wire__wire_format.snap",
  import.meta.url,
);

/** The snapshot's JSON lines, after the insta header. */
function snapshotLines(): string[] {
  const text = readFileSync(SNAPSHOT, "utf8").replace(/\r\n/g, "\n");
  const body = text.split("---\n").slice(2).join("---\n");
  return body.split("\n").filter((line) => line.trim() !== "");
}

function assertRoundTrip(line: string) {
  const item = parseTimelineItem(JSON.parse(line));
  assert.ok(item, `rejected: ${line}`);
  // Same keys, same order, same values: the types mirror the wire exactly.
  assert.equal(JSON.stringify(item), line);
}

test("every line of the snapshot parses and serializes back unchanged", () => {
  const lines = snapshotLines();
  assert.equal(lines.length, 12);
  for (const line of lines) assertRoundTrip(line);
});

test("variants the snapshot doesn't show parse too", () => {
  for (const line of [
    '{"kind":"bad_tag"}',
    '{"kind":"bound","value":{"kind":"point","value":{"kind":"pixel","value":[812,344]}}}',
    '{"kind":"bound","value":{"kind":"step","value":{"number":3,"total":3,"target":{"kind":"pixel","value":[0,0]}}}}',
    '{"kind":"bound","value":{"kind":"sfx","value":{"name":"success","level":"loud","solo":false}}}',
    '{"kind":"bound","value":{"kind":"show","value":"example.com"}}',
    '{"kind":"amb","value":{"name":"forest","level":"normal"}}',
    '{"kind":"say","value":{"id":9007199254740991,"text":"","bound":[]}}',
    '{"kind":"pause","value":4294967295}',
  ]) {
    assertRoundTrip(line);
  }
});

test("the parser returns a copy, not the input", () => {
  const json = JSON.parse('{"kind":"amb","value":{"name":"rain","level":"soft"}}');
  const item = parseTimelineItem(json);
  assert.deepEqual(item, json);
  assert.notEqual(item, json);
});

const ID = { kind: "id", value: "e12" };

function say(value: Record<string, unknown>) {
  return { kind: "say", value: { id: 1, text: "Hi.", bound: [], ...value } };
}

const MALFORMED_ITEMS: unknown[] = [
  null,
  undefined,
  true,
  42,
  "wait",
  [],
  {},
  [{ kind: "wait" }],
  { kind: "nope" },
  { kind: "Wait" },
  { kind: 1 },
  { value: "en" },
  // Unit variants carry no value; the others need one; no extra keys anywhere.
  { kind: "wait", value: null },
  { kind: "end", extra: 1 },
  { kind: "lang" },
  { kind: "lang", value: "en", extra: true },
  { kind: "lang", value: 5 },
  { kind: "lang", value: null },
  // The contract draft's shapes are not the wire.
  { kind: "lang", code: "en" },
  { kind: "pause", ms: 400 },
  { kind: "pause", value: -1 },
  { kind: "pause", value: 1.5 },
  { kind: "pause", value: "400" },
  { kind: "pause", value: 2 ** 32 },
  { kind: "pause", value: Number.NaN },
  { kind: "pause", value: Number.POSITIVE_INFINITY },
  { kind: "sfx", value: "rimshot" },
  { kind: "sfx", value: { name: "rimshot", level: "normal" } },
  { kind: "amb", value: null },
  { kind: "amb", value: { name: "rain" } },
  { kind: "amb", value: { level: "soft" } },
  { kind: "amb", value: { name: 5, level: "soft" } },
  { kind: "amb", value: { name: "rain", level: "quiet" } },
  { kind: "say", value: null },
  say({ id: 0 }),
  say({ id: -1 }),
  say({ id: 1.5 }),
  say({ id: "1" }),
  say({ id: 2 ** 53 }),
  say({ text: null }),
  say({ bound: {} }),
  say({ bound: [null] }),
  say({ extra: 1 }),
  { kind: "say", value: { id: 1, text: "Hi." } },
];

const MALFORMED_BOUND: unknown[] = [
  { kind: "wait" },
  { kind: "mood" },
  { kind: "mood", value: 1 },
  { kind: "mood", mood: "happy" },
  { kind: "show", value: null },
  { kind: "sfx", value: { name: "ping", level: "max", solo: false } },
  { kind: "sfx", value: { name: "ping", level: "soft" } },
  { kind: "sfx", value: { name: "ping", level: "soft", solo: "no" } },
  { kind: "sfx", value: { name: "ping", level: "soft", solo: false, pan: 0 } },
  { kind: "sfx", value: { name: 1, level: "soft", solo: false } },
  { kind: "point", value: { kind: "id", value: 12 } },
  { kind: "point", value: { kind: "id", id: "e12" } },
  { kind: "point", value: { Id: "e12" } },
  { kind: "point", value: { kind: "pixel", value: [1] } },
  { kind: "point", value: { kind: "pixel", value: [1, 2, 3] } },
  { kind: "point", value: { kind: "pixel", value: [-1, 2] } },
  { kind: "point", value: { kind: "pixel", value: [1.5, 2] } },
  { kind: "point", value: { kind: "pixel", value: ["1", 2] } },
  { kind: "point", value: { kind: "pixel", value: { x: 1, y: 2 } } },
  { kind: "point", value: { kind: "pixel", x: 1, y: 2 } },
  { kind: "point", value: { kind: "cell", value: "e12" } },
  { kind: "step", value: { number: 0, total: 2, target: ID } },
  { kind: "step", value: { number: 3, total: 2, target: ID } },
  { kind: "step", value: { number: 1, total: 2 } },
  { kind: "step", value: { number: 1, total: 2, target: { kind: "id" } } },
  { kind: "step", value: { number: "1", total: 2, target: ID } },
];

test("malformed items are rejected", () => {
  for (const json of MALFORMED_ITEMS) {
    assert.equal(parseTimelineItem(json), null, JSON.stringify(json));
  }
});

test("a malformed bound action rejects its item, alone or inside a sentence", () => {
  for (const bound of MALFORMED_BOUND) {
    assert.equal(parseTimelineItem({ kind: "bound", value: bound }), null, JSON.stringify(bound));
    assert.equal(parseTimelineItem(say({ bound: [bound] })), null, JSON.stringify(bound));
  }
});

test("hostile objects are rejected without throwing", () => {
  const throwingKeys = new Proxy({}, {
    ownKeys() {
      throw new Error("boom");
    },
  });
  const throwingGetter = {
    kind: "lang",
    get value() {
      throw new Error("boom");
    },
  };
  assert.equal(parseTimelineItem(throwingKeys), null);
  assert.equal(parseTimelineItem(throwingGetter), null);
  assert.equal(parseTimelineItem({ kind: "say", value: throwingGetter }), null);
});
