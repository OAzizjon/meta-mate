// Run with `npm test` (Node's own test runner; Node 24 strips the types).
// The answer stream's wire format (src-tauri/src/answer_stream.rs), its PCM and the subscription.
import assert from "node:assert/strict";
import { Buffer } from "node:buffer";
import { test } from "node:test";
import { inspect } from "node:util";

import {
  ANSWER_LINE_KEYS,
  ANSWER_STATES,
  decodePcm,
  parseAnswerMsg,
  subscribeAnswerStream,
} from "./answer-stream.ts";

/** PCM16LE samples as base64, the way Rust's `STANDARD` engine writes them. */
function encodePcm(samples: readonly number[]): string {
  const bytes = new Uint8Array(samples.length * 2);
  samples.forEach((sample, i) => {
    bytes[2 * i] = sample & 0xff;
    bytes[2 * i + 1] = (sample >> 8) & 0xff;
  });
  return Buffer.from(bytes).toString("base64");
}

const STATE = { type: "state", generation: 1, state: "thinking" };
const LEVEL = { type: "level", generation: 1, level: 0.25 };
const ITEM = { type: "item", generation: 1, item: { kind: "end" } };
const PCM = { type: "pcm", generation: 2, sentence: 1, sampleRate: 16000, eos: true, samplesB64: "AAD/fw==" };
const CANCEL = { type: "cancel", generation: 1 };
const LINE = { type: "line", generation: 1, key: "robot.cant_find" };

test("every message type parses into a fresh copy", () => {
  for (const raw of [
    ...ANSWER_STATES.map((state) => ({ type: "state", generation: 0, state })),
    LEVEL,
    { type: "level", generation: 0, level: 0 },
    { type: "level", generation: 2, level: 1 },
    ITEM,
    { type: "item", generation: 3, item: { kind: "say", value: { id: 1, text: "Hi.", bound: [] } } },
    PCM,
    { ...PCM, sentence: 0, sampleRate: 8000, eos: false },
    { ...PCM, sampleRate: 48000, samplesB64: "" },
    CANCEL,
    { type: "cancel", generation: Number.MAX_SAFE_INTEGER },
    ...ANSWER_LINE_KEYS.map((key) => ({ type: "line", generation: 1, key })),
  ]) {
    const msg = parseAnswerMsg(raw);
    assert.deepEqual(msg, raw, inspect(raw));
    assert.notEqual(msg, raw);
  }
});

test("Rust's JSON parses and serializes back unchanged", () => {
  for (const line of [
    '{"type":"state","generation":4,"state":"listening"}',
    '{"type":"state","generation":4,"state":"looking"}',
    '{"type":"level","generation":4,"level":0}',
    '{"type":"level","generation":4,"level":0.375}',
    '{"type":"level","generation":4,"level":1}',
    '{"type":"item","generation":4,"item":{"kind":"lang","value":"ru"}}',
    '{"type":"item","generation":4,"item":{"kind":"point_rejected"}}',
    '{"type":"pcm","generation":4,"sentence":2,"sampleRate":22050,"eos":true,"samplesB64":"AID//w=="}',
    // TTS failed: an empty chunk still ends the sentence.
    '{"type":"pcm","generation":4,"sentence":3,"sampleRate":16000,"eos":true,"samplesB64":""}',
    '{"type":"cancel","generation":4}',
    '{"type":"line","generation":5,"key":"robot.network_error"}',
  ]) {
    const msg = parseAnswerMsg(JSON.parse(line));
    assert.ok(msg, `rejected: ${line}`);
    assert.equal(JSON.stringify(msg), line);
  }
});

test("PCM decodes little-endian", () => {
  assert.deepEqual(decodePcm("AAD/fw=="), Int16Array.from([0, 32767]));
  assert.deepEqual(decodePcm("AID//w=="), Int16Array.from([-32768, -1]));
  assert.deepEqual(decodePcm(""), new Int16Array(0));
});

test("base64 round trip, padded or not, every tail length", () => {
  const values = [0, 1, -1, 32767, -32768, 12345, -12345, 256, -256];
  for (let count = 0; count <= values.length; count++) {
    const samples = values.slice(0, count);
    const padded = encodePcm(samples);
    for (const b64 of [padded, padded.replace(/=+$/, "")]) {
      assert.deepEqual(decodePcm(b64), Int16Array.from(samples), b64);
      assert.ok(parseAnswerMsg({ ...PCM, samplesB64: b64 }), b64);
    }
  }
});

test("a second of 48 kHz speech round-trips", () => {
  const samples = Array.from({ length: 48000 }, (_, i) => ((i * 7919) % 65536) - 32768);
  const b64 = encodePcm(samples);
  assert.ok(parseAnswerMsg({ ...PCM, sampleRate: 48000, samplesB64: b64 }));
  assert.deepEqual(decodePcm(b64), Int16Array.from(samples));
});

test("odd byte counts are rejected", () => {
  for (const length of [1, 3, 5, 7, 99]) {
    const padded = Buffer.alloc(length, 0x41).toString("base64");
    for (const b64 of [padded, padded.replace(/=+$/, "")]) {
      assert.equal(parseAnswerMsg({ ...PCM, samplesB64: b64 }), null, b64);
      assert.throws(() => decodePcm(b64), TypeError, b64);
    }
  }
});

test("anything but canonical standard base64 is rejected", () => {
  for (const b64 of [
    "A",
    "AAAAA",
    "AA=",
    "AAA==",
    "AAAA=",
    "AAAA==",
    "====",
    "A===",
    "AA==AA==",
    "A=AA",
    " AAA=",
    "AAA=\n",
    "AA A",
    // URL-safe alphabet.
    "AA-=",
    "AA_=",
    // Non-ASCII.
    "AAé=",
    "ＡＡＡ=",
    // Unused bits set.
    "AAB=",
    "AAAAAB==",
  ]) {
    assert.equal(parseAnswerMsg({ ...PCM, samplesB64: b64 }), null, inspect(b64));
    assert.throws(() => decodePcm(b64), TypeError, inspect(b64));
  }
});

const { eos: _eos, ...PCM_WITHOUT_EOS } = PCM;
const NOT_IDS: unknown[] = [-1, 0.5, 2 ** 53, Number.MAX_VALUE, Infinity, -Infinity, NaN, "1", 1n, null, true, {}, [1]];

const MALFORMED: unknown[] = [
  null,
  undefined,
  true,
  42,
  "cancel",
  [],
  {},
  [CANCEL],
  { type: "nope", generation: 1 },
  { type: "Cancel", generation: 1 },
  { type: 1, generation: 1 },
  { generation: 1 },
  // `type` only inherited.
  Object.assign(Object.create({ type: "cancel" }), { generation: 1 }),
  // Missing and extra keys; snake_case is not the wire.
  { type: "cancel" },
  { ...CANCEL, extra: 1 },
  { type: "state", generation: 1 },
  { ...STATE, state: undefined },
  { type: "level", generation: 1 },
  { ...LEVEL, extra: 1 },
  { ...LEVEL, level: undefined },
  { type: "item", generation: 1 },
  PCM_WITHOUT_EOS,
  { ...PCM, text: "Hi." },
  { type: "pcm", generation: 2, sentence: 1, sample_rate: 16000, eos: true, samples_b64: "" },
  { type: "line", generation: 1 },
  ...NOT_IDS.map((generation) => ({ ...CANCEL, generation })),
  ...NOT_IDS.map((generation) => ({ ...STATE, generation })),
  ...NOT_IDS.map((generation) => ({ ...LEVEL, generation })),
  ...NOT_IDS.map((sentence) => ({ ...PCM, sentence })),
  ...["speaking", "Listening", "Looking", "idle ", "", null, 1].map((state) => ({ ...STATE, state })),
  ...[-0.001, -1, 1.001, 2, Number.MAX_VALUE, Infinity, -Infinity, NaN, "0.5", null, true, {}, [0.5]].map(
    (level) => ({ ...LEVEL, level }),
  ),
  ...[{ kind: "nope" }, { kind: "end", value: null }, null, '{"kind":"end"}'].map((item) => ({ ...ITEM, item })),
  ...[7999, 48001, 16000.5, 1e9, -16000, "16000", NaN, Infinity, null].map((sampleRate) => ({ ...PCM, sampleRate })),
  ...["true", 1, null].map((eos) => ({ ...PCM, eos })),
  ...[null, 4, ["AAAA"], new Uint8Array(4)].map((samplesB64) => ({ ...PCM, samplesB64 })),
  ...["robot.found_it", "robot.cant_find ", "CantFind", "", null, 1].map((key) => ({ ...LINE, key })),
];

test("malformed messages are rejected", () => {
  for (const raw of MALFORMED) {
    assert.equal(parseAnswerMsg(raw), null, inspect(raw));
  }
});

test("hostile objects are rejected without throwing", () => {
  const boom = (): never => {
    throw new Error("boom");
  };
  const hostile: unknown[] = [
    new Proxy({}, { ownKeys: boom }),
    new Proxy({ ...CANCEL }, { get: boom }),
    new Proxy({ ...CANCEL }, { getOwnPropertyDescriptor: boom }),
    { get type() { return boom(); }, generation: 1 },
    { type: "cancel", get generation() { return boom(); } },
    { ...LEVEL, get level() { return boom(); } },
    { ...PCM, get samplesB64() { return boom(); } },
    { ...ITEM, item: new Proxy({ kind: "end" }, { ownKeys: boom }) },
    { ...STATE, state: { toString: boom } },
  ];
  for (const raw of hostile) {
    assert.equal(parseAnswerMsg(raw), null);
  }
});

test("each field is read once and copied as a plain value", () => {
  let reads = 0;
  const raw = {
    type: "cancel",
    get generation() {
      reads += 1;
      return reads === 1 ? 3 : -1;
    },
  };
  const msg = parseAnswerMsg(raw);
  assert.deepEqual(msg, { type: "cancel", generation: 3 });
  assert.equal(Object.getOwnPropertyDescriptor(msg, "generation")?.value, 3);

  let levelReads = 0;
  const rawLevel = {
    type: "level",
    generation: 2,
    get level() {
      levelReads += 1;
      return levelReads === 1 ? 0.5 : 2;
    },
  };
  const levelMsg = parseAnswerMsg(rawLevel);
  assert.deepEqual(levelMsg, { type: "level", generation: 2, level: 0.5 });
  assert.equal(Object.getOwnPropertyDescriptor(levelMsg, "level")?.value, 0.5);
});

/** A fake Tauri connection: records the locale and lets the test send raw messages. */
function fakeConnect() {
  const connection = {
    locale: "",
    send: (_raw: unknown): void => {},
    async connect(locale: "en" | "ru", onRaw: (raw: unknown) => void) {
      connection.locale = locale;
      connection.send = onRaw;
    },
  };
  return connection;
}

test("the subscription routes parsed messages and drops the rest without their text", async (t) => {
  const warn = t.mock.method(console, "warn", () => {});
  const fake = fakeConnect();
  const received: unknown[] = [];
  const unsubscribe = await subscribeAnswerStream("ru", (msg) => received.push(msg), fake.connect);
  assert.equal(fake.locale, "ru");
  fake.send(CANCEL);
  fake.send({ type: "line", generation: 1, key: "Private words" });
  fake.send({ type: "Private words" });
  fake.send(STATE);
  assert.deepEqual(received, [CANCEL, STATE]);
  assert.equal(warn.mock.callCount(), 2);
  for (const call of warn.mock.calls) {
    assert.ok(!inspect(call.arguments).includes("Private"));
  }
  unsubscribe();
  fake.send(CANCEL);
  assert.equal(received.length, 2);
});

test("messages that arrive while subscribing are delivered", async () => {
  const received: unknown[] = [];
  await subscribeAnswerStream(
    "en",
    (msg) => received.push(msg),
    async (_locale, onRaw) => {
      onRaw(CANCEL);
    },
  );
  assert.deepEqual(received, [CANCEL]);
});

test("a throwing handler doesn't stop the messages behind it", async (t) => {
  const error = t.mock.method(console, "error", () => {});
  const fake = fakeConnect();
  const received: string[] = [];
  await subscribeAnswerStream(
    "en",
    (msg) => {
      if (msg.type === "cancel") throw new Error("boom");
      received.push(msg.type);
    },
    fake.connect,
  );
  assert.doesNotThrow(() => fake.send(CANCEL));
  fake.send(STATE);
  assert.deepEqual(received, ["state"]);
  assert.equal(error.mock.callCount(), 1);
});

test("a failed subscription rejects and ignores late messages", async () => {
  let send: (raw: unknown) => void = () => {};
  const received: unknown[] = [];
  await assert.rejects(
    subscribeAnswerStream(
      "en",
      (msg) => received.push(msg),
      async (_locale, onRaw) => {
        send = onRaw;
        throw new Error("no such command");
      },
    ),
  );
  send(CANCEL);
  assert.deepEqual(received, []);
});
