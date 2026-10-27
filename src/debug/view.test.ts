// Run with `npm test` (Node's own test runner; Node 24 strips the types).
// Pure formatting and wire validation for the debug window (src/debug/view.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { inspect } from "node:util";

import type { TimelineItem } from "../ipc/timeline.ts";
import {
  DebugStore,
  MAX_GENERATIONS,
  computeTimingBars,
  escapeHtml,
  formatBoundChip,
  formatLookStats,
  formatPoint,
  formatSentencePcm,
  formatSound,
  formatTimelineItem,
  getDebugMsgGeneration,
  groupPcmBySentence,
  parseDebugAnswerMsg,
  parseDebugMsg,
  renderDebugViewHtml,
  renderGenerationHtml,
} from "./view.ts";
import type { DebugLookMsg, DebugMsg, DebugPcmMsg, DebugTimingMsg } from "./view.ts";

const VALID_ANSWER_STATE: DebugMsg = {
  type: "answer",
  msg: { type: "state", generation: 1, state: "listening" },
};

const VALID_ANSWER_ITEM: DebugMsg = {
  type: "answer",
  msg: {
    type: "item",
    generation: 1,
    item: {
      kind: "say",
      value: {
        id: 1,
        text: "It's right here.",
        bound: [{ kind: "point", value: { kind: "id", value: "e12" } }],
      },
    },
  },
};

const VALID_ANSWER_PCM: DebugMsg = {
  type: "answer",
  msg: {
    type: "pcm",
    generation: 1,
    sentence: 1,
    sampleRate: 16000,
    eos: true,
    samples: 24000,
  },
};

const VALID_ANSWER_CANCEL: DebugMsg = {
  type: "answer",
  msg: { type: "cancel", generation: 1 },
};

const VALID_ANSWER_LINE: DebugMsg = {
  type: "answer",
  msg: { type: "line", generation: 1, key: "robot.cant_find" },
};

const VALID_TIMING: DebugMsg = {
  type: "timing",
  generation: 1,
  name: "asr_final",
  ms: 350,
};

const VALID_LOOK: DebugMsg = {
  type: "look",
  generation: 1,
  elements: 142,
  listed: 35,
  painted: 35,
  ms: 180,
};

test("every valid DebugMsg variant parses into a fresh copy", () => {
  const cases: DebugMsg[] = [
    VALID_ANSWER_STATE,
    { type: "answer", msg: { type: "state", generation: 2, state: "thinking" } },
    { type: "answer", msg: { type: "state", generation: 3, state: "idle" } },
    VALID_ANSWER_ITEM,
    { type: "answer", msg: { type: "item", generation: 2, item: { kind: "pause", value: 400 } } },
    { type: "answer", msg: { type: "item", generation: 2, item: { kind: "end" } } },
    { type: "answer", msg: { type: "item", generation: 2, item: { kind: "wait" } } },
    VALID_ANSWER_PCM,
    {
      type: "answer",
      msg: { type: "pcm", generation: 1, sentence: 0, sampleRate: 8000, eos: false, samples: 0 },
    },
    {
      type: "answer",
      msg: { type: "pcm", generation: 1, sentence: 2, sampleRate: 48000, eos: true, samples: 48000 },
    },
    VALID_ANSWER_CANCEL,
    VALID_ANSWER_LINE,
    VALID_TIMING,
    { type: "timing", generation: 2, name: "first_text", ms: 820 },
    { type: "timing", generation: 2, name: "first_audio", ms: 1250 },
    VALID_LOOK,
    { type: "look", generation: 0, elements: 0, listed: 0, painted: 0, ms: 0 },
  ];

  for (const raw of cases) {
    const parsed = parseDebugMsg(raw);
    assert.deepEqual(parsed, raw, inspect(raw));
    assert.notEqual(parsed, raw);
  }
});

test("getDebugMsgGeneration returns the target generation", () => {
  assert.equal(getDebugMsgGeneration(VALID_ANSWER_STATE), 1);
  assert.equal(getDebugMsgGeneration(VALID_ANSWER_ITEM), 1);
  assert.equal(getDebugMsgGeneration(VALID_ANSWER_PCM), 1);
  assert.equal(getDebugMsgGeneration(VALID_TIMING), 1);
  assert.equal(getDebugMsgGeneration(VALID_LOOK), 1);
});

test("malformed DebugMsg payloads are rejected without throwing", () => {
  const malformed: unknown[] = [
    null,
    undefined,
    true,
    123,
    "debug",
    [],
    {},
    { type: "unknown" },
    // Missing fields
    { type: "answer" },
    { type: "timing", generation: 1, name: "asr_final" },
    { type: "look", generation: 1, elements: 10, listed: 5, painted: 5 },
    // Extra fields
    { ...VALID_TIMING, extra: 1 },
    { ...VALID_LOOK, extra: 1 },
    { type: "answer", msg: { type: "cancel", generation: 1 }, extra: true },
    // Wrong types or out-of-range numbers
    { type: "timing", generation: -1, name: "asr_final", ms: 100 },
    { type: "timing", generation: 1, name: "", ms: 100 },
    { type: "timing", generation: 1, name: 123, ms: 100 },
    { type: "timing", generation: 1, name: "asr_final", ms: -10 },
    { type: "timing", generation: 1, name: "asr_final", ms: Number.NaN },
    { type: "timing", generation: 1, name: "asr_final", ms: Number.POSITIVE_INFINITY },
    { type: "look", generation: 1.5, elements: 10, listed: 5, painted: 5, ms: 100 },
    { type: "look", generation: 1, elements: -1, listed: 5, painted: 5, ms: 100 },
    { type: "look", generation: 1, elements: 10, listed: "5", painted: 5, ms: 100 },
    { type: "look", generation: 1, elements: 10, listed: 5, painted: -5, ms: 100 },
    { type: "look", generation: 1, elements: 10, listed: 5, painted: 5, ms: Number.NaN },
    // Bad inner answer messages
    { type: "answer", msg: null },
    { type: "answer", msg: { type: "unknown", generation: 1 } },
    { type: "answer", msg: { type: "state", generation: 1, state: "speaking" } },
    { type: "answer", msg: { type: "pcm", generation: 1, sentence: 1, sampleRate: 7999, eos: true, samples: 100 } },
    { type: "answer", msg: { type: "pcm", generation: 1, sentence: 1, sampleRate: 48001, eos: true, samples: 100 } },
    { type: "answer", msg: { type: "pcm", generation: 1, sentence: -1, sampleRate: 16000, eos: true, samples: 100 } },
    { type: "answer", msg: { type: "pcm", generation: 1, sentence: 1, sampleRate: 16000, eos: "true", samples: 100 } },
    { type: "answer", msg: { type: "pcm", generation: 1, sentence: 1, sampleRate: 16000, eos: true, samples: -1 } },
    { type: "answer", msg: { type: "pcm", generation: 1, sentence: 1, sampleRate: 16000, eos: true, samples: 1.5 } },
    // PCM carries samplesB64 instead of samples: rejected
    {
      type: "answer",
      msg: {
        type: "pcm",
        generation: 1,
        sentence: 1,
        sampleRate: 16000,
        eos: true,
        samplesB64: "AAD/fw==",
      },
    },
    { type: "answer", msg: { type: "line", generation: 1, key: "" } },
    { type: "answer", msg: { type: "item", generation: 1, item: { kind: "invalid_kind" } } },
  ];

  for (const raw of malformed) {
    assert.equal(parseDebugMsg(raw), null, inspect(raw));
  }
});

test("hostile objects are rejected without throwing", () => {
  const boom = (): never => {
    throw new Error("boom");
  };
  const hostile: unknown[] = [
    new Proxy({}, { ownKeys: boom }),
    new Proxy({ ...VALID_TIMING }, { get: boom }),
    { get type() { return boom(); } },
    { type: "timing", get generation() { return boom(); } },
    { type: "answer", get msg() { return boom(); } },
    { type: "answer", msg: new Proxy({ type: "cancel", generation: 1 }, { get: boom }) },
  ];

  for (const raw of hostile) {
    assert.equal(parseDebugMsg(raw), null);
    assert.equal(parseDebugAnswerMsg(raw), null);
  }
});

test("formatPoint formats id and pixel targets", () => {
  assert.equal(formatPoint({ kind: "id", value: "e12" }), "e12");
  assert.equal(formatPoint({ kind: "pixel", value: [100, 250] }), "100,250");
});

test("formatSound formats sfx name, level, and solo modifier", () => {
  assert.equal(formatSound({ name: "ping", level: "normal", solo: false }), "ping");
  assert.equal(formatSound({ name: "ping", level: "soft", solo: false }), "ping soft");
  assert.equal(formatSound({ name: "rimshot", level: "loud", solo: true }), "rimshot loud solo");
  assert.equal(formatSound({ name: "rimshot", level: "normal", solo: true }), "rimshot solo");
});

test("formatBoundChip formats bound tags into chips", () => {
  assert.deepEqual(formatBoundChip({ kind: "point", value: { kind: "id", value: "e12" } }), {
    label: "<point e12>",
    kind: "point",
  });
  assert.deepEqual(formatBoundChip({ kind: "mood", value: "happy" }), {
    label: "<mood happy>",
    kind: "mood",
  });
  assert.deepEqual(
    formatBoundChip({ kind: "sfx", value: { name: "ping", level: "soft", solo: false } }),
    { label: "<sfx ping soft>", kind: "sfx" },
  );
  assert.deepEqual(
    formatBoundChip({
      kind: "step",
      value: { number: 1, total: 3, target: { kind: "id", value: "e4" } },
    }),
    { label: "<step 1/3 e4>", kind: "step" },
  );
  assert.deepEqual(formatBoundChip({ kind: "show", value: "125%" }), {
    label: "<show 125%>",
    kind: "show",
  });
});

test("formatTimelineItem formats MateScript-like chips and sentence text", () => {
  // say #1 with bound point
  const sayItem: TimelineItem = {
    kind: "say",
    value: {
      id: 1,
      text: "It's right here.",
      bound: [{ kind: "point", value: { kind: "id", value: "e12" } }],
    },
  };
  const sayView = formatTimelineItem(sayItem);
  assert.deepEqual(sayView.chips, [
    { label: "<point e12>", kind: "point" },
    { label: "say #1", kind: "say" },
  ]);
  assert.equal(sayView.text, "It's right here.");

  // pause 400
  assert.deepEqual(formatTimelineItem({ kind: "pause", value: 400 }), {
    chips: [{ label: "<pause 400>", kind: "pause" }],
  });

  // end
  assert.deepEqual(formatTimelineItem({ kind: "end" }), {
    chips: [{ label: "end", kind: "end" }],
  });

  // wait
  assert.deepEqual(formatTimelineItem({ kind: "wait" }), {
    chips: [{ label: "<wait click>", kind: "wait" }],
  });

  // lang
  assert.deepEqual(formatTimelineItem({ kind: "lang", value: "en" }), {
    chips: [{ label: "<lang en>", kind: "lang" }],
  });

  // standalone sfx
  assert.deepEqual(
    formatTimelineItem({
      kind: "sfx",
      value: { name: "rimshot", level: "normal", solo: true },
    }),
    { chips: [{ label: "<sfx rimshot solo>", kind: "sfx" }] },
  );

  // amb on and off
  assert.deepEqual(
    formatTimelineItem({ kind: "amb", value: { name: "rain", level: "soft" } }),
    { chips: [{ label: "<amb rain soft>", kind: "amb" }] },
  );
  assert.deepEqual(formatTimelineItem({ kind: "amb", value: { name: null, level: "soft" } }), {
    chips: [{ label: "<amb off>", kind: "amb" }],
  });

  // point_rejected and bad_tag
  assert.deepEqual(formatTimelineItem({ kind: "point_rejected" }), {
    chips: [{ label: "<point_rejected>", kind: "point_rejected" }],
  });
  assert.deepEqual(formatTimelineItem({ kind: "bad_tag" }), {
    chips: [{ label: "<bad_tag>", kind: "bad_tag" }],
  });
});

test("groupPcmBySentence groups multiple chunks and formats sample counts", () => {
  const pcmList: DebugPcmMsg[] = [
    { type: "pcm", generation: 1, sentence: 1, sampleRate: 16000, eos: false, samples: 8000 },
    { type: "pcm", generation: 1, sentence: 0, sampleRate: 16000, eos: true, samples: 16000 },
    { type: "pcm", generation: 1, sentence: 1, sampleRate: 16000, eos: true, samples: 16000 },
  ];

  const grouped = groupPcmBySentence(pcmList);
  assert.equal(grouped.length, 2);

  // Sentence 0 comes first (sorted by sentence id)
  assert.equal(grouped[0].sentence, 0);
  assert.equal(grouped[0].totalSamples, 16000);
  assert.equal(grouped[0].chunks, 1);
  assert.equal(grouped[0].eos, true);
  assert.equal(formatSentencePcm(grouped[0]), "Sentence #0 (cached): 16,000 samples (1 chunk @ 16 kHz, eos)");

  // Sentence 1 accumulated both chunks
  assert.equal(grouped[1].sentence, 1);
  assert.equal(grouped[1].totalSamples, 24000);
  assert.equal(grouped[1].chunks, 2);
  assert.equal(grouped[1].eos, true);
  assert.deepEqual(grouped[1].arrivals, [8000, 16000]);
  assert.equal(formatSentencePcm(grouped[1]), "Sentence #1: 24,000 samples (2 chunks @ 16 kHz, eos)");
});

test("computeTimingBars scales widths against max ms (key-up t = 0)", () => {
  const timings: DebugTimingMsg[] = [
    { type: "timing", generation: 1, name: "asr_final", ms: 350 },
    { type: "timing", generation: 1, name: "first_text", ms: 820 },
    { type: "timing", generation: 1, name: "first_audio", ms: 1400 },
  ];

  const bars = computeTimingBars(timings);
  assert.equal(bars.length, 3);
  assert.equal(bars[0].name, "asr_final");
  assert.equal(bars[0].ms, 350);
  assert.equal(bars[0].percentage, 25); // 350 / 1400 = 25%

  assert.equal(bars[1].name, "first_text");
  assert.equal(bars[1].ms, 820);
  assert.equal(bars[1].percentage, 58.6); // 820 / 1400 = 58.57% -> 58.6%

  assert.equal(bars[2].name, "first_audio");
  assert.equal(bars[2].ms, 1400);
  assert.equal(bars[2].percentage, 100);

  // Empty timings
  assert.deepEqual(computeTimingBars([]), []);
});

test("formatLookStats formats element counts, painted counts, and duration", () => {
  const look: DebugLookMsg = {
    type: "look",
    generation: 1,
    elements: 142,
    listed: 35,
    painted: 35,
    ms: 180,
  };
  assert.equal(formatLookStats(look), "142 elements, 35 listed, 35 painted in 180 ms");
});

test("escapeHtml escapes XML/HTML control characters", () => {
  assert.equal(escapeHtml('<say "hello" & \'goodbye\'>'), "&lt;say &quot;hello&quot; &amp; &#39;goodbye&#39;&gt;");
});

test("DebugStore maintains generations newest on top and caps at 20", () => {
  const store = new DebugStore();

  // Add generations 1 through 25
  for (let i = 1; i <= 25; i++) {
    const ok = store.handleRaw({
      type: "timing",
      generation: i,
      name: "asr_final",
      ms: i * 50,
    });
    assert.equal(ok, true);
  }

  const generations = store.getGenerations();
  assert.equal(generations.length, MAX_GENERATIONS);
  assert.equal(generations.length, 20);

  // Newest on top: generation 25 is first, 6 is last
  assert.equal(generations[0].generation, 25);
  assert.equal(generations[generations.length - 1].generation, 6);

  // Dropped generations 1..5 are gone
  assert.ok(!generations.some((g) => g.generation <= 5));
});

test("DebugStore aggregates items, pcm, timings, look, state, and cancellation", () => {
  const store = new DebugStore();
  const gen = 42;

  store.addMsg({ type: "answer", msg: { type: "state", generation: gen, state: "listening" } });
  store.addMsg({
    type: "answer",
    msg: {
      type: "item",
      generation: gen,
      item: {
        kind: "say",
        value: {
          id: 1,
          text: "Click Save.",
          bound: [{ kind: "point", value: { kind: "id", value: "e10" } }],
        },
      },
    },
  });
  store.addMsg({
    type: "answer",
    msg: { type: "item", generation: gen, item: { kind: "end" } },
  });
  store.addMsg({
    type: "answer",
    msg: { type: "pcm", generation: gen, sentence: 1, sampleRate: 16000, eos: true, samples: 16000 },
  });
  store.addMsg({ type: "timing", generation: gen, name: "first_text", ms: 400 });
  store.addMsg({ type: "timing", generation: gen, name: "first_audio", ms: 800 });
  store.addMsg({
    type: "look",
    generation: gen,
    elements: 50,
    listed: 10,
    painted: 10,
    ms: 95,
  });
  store.addMsg({ type: "answer", msg: { type: "cancel", generation: gen } });
  store.addMsg({ type: "answer", msg: { type: "line", generation: gen, key: "robot.cant_find" } });

  const records = store.getGenerations();
  assert.equal(records.length, 1);
  const rec = records[0];
  assert.equal(rec.generation, gen);
  assert.equal(rec.state, "listening");
  assert.equal(rec.cancelled, true);
  assert.deepEqual(rec.lines, ["robot.cant_find"]);
  assert.equal(rec.items.length, 2);
  assert.equal(rec.pcm.length, 1);
  assert.equal(rec.timings.length, 2);
  assert.ok(rec.look);
  assert.equal(rec.look.elements, 50);

  // Updating a timing with the same name replaces it
  store.addMsg({ type: "timing", generation: gen, name: "first_text", ms: 420 });
  const updated = store.getGenerations()[0];
  assert.equal(updated.timings.length, 2);
  assert.equal(updated.timings.find((t) => t.name === "first_text")?.ms, 420);
});

test("renderDebugViewHtml and renderGenerationHtml generate complete HTML markup", () => {
  const store = new DebugStore();
  assert.match(renderDebugViewHtml(store.getGenerations()), /Waiting for events/);

  store.addMsg(VALID_LOOK);
  store.addMsg(VALID_TIMING);
  store.addMsg(VALID_ANSWER_ITEM);
  store.addMsg(VALID_ANSWER_PCM);

  const html = renderDebugViewHtml(store.getGenerations());

  // Generation card header
  assert.match(html, /Generation #1/);
  // Look stats
  assert.match(html, /142 elements, 35 listed, 35 painted in 180 ms/);
  // Timings bar
  assert.match(html, /asr_final/);
  assert.match(html, /350 ms/);
  // Timeline chips
  assert.match(html, /&lt;point e12&gt;/);
  assert.match(html, /say #1/);
  // Muted sentence text
  assert.match(html, /class="sentence-text muted">It&#39;s right here\./);
  // PCM summary
  assert.match(html, /Sentence #1: 24,000 samples/);
});

test("renderGenerationHtml renders empty notice when generation has no sections", () => {
  const emptyGenHtml = renderGenerationHtml({
    generation: 99,
    items: [],
    pcm: [],
    timings: [],
    look: null,
    lines: [],
  });
  assert.match(emptyGenHtml, /Generation #99/);
  assert.match(emptyGenHtml, /No events recorded/);
});
