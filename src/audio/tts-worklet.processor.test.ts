import assert from "node:assert/strict";
import test from "node:test";
import { TtsRingProcessor } from "./tts-worklet.processor.ts";
import type { WorkletOutboundMsg } from "./types.ts";

function setup(sampleRate = 48_000) {
  const processor = new TtsRingProcessor({ processorOptions: { sampleRate } });
  const messages: WorkletOutboundMsg[] = [];
  processor.port.postMessage = (message) => messages.push(message);
  return { processor, messages };
}

function render(processor: TtsRingProcessor, frames: number) {
  const output = new Float32Array(frames);
  assert.equal(processor.process([], [[output]]), true);
  return output;
}

test("EOS arriving after PCM drains ends the sentence exactly once", () => {
  const { processor, messages } = setup();
  processor.pushPcm(1, 7, 48_000, false, new Int16Array([8192, 16384]));
  assert.deepEqual([...render(processor, 2)], [0.25, 0.5]);
  assert.equal(messages.some((message) => message.type === "sentence-end"), false);
  processor.handleMessage({ type: "push", generationId: 1, sentenceId: 7,
    sampleRate: 48_000, eos: true, pcm: new ArrayBuffer(0) });
  render(processor, 128);
  render(processor, 128);
  assert.deepEqual(messages.filter((message) => message.type.startsWith("sentence-")), [
    { type: "sentence-start", generationId: 1, sentenceId: 7 },
    { type: "sentence-end", generationId: 1, sentenceId: 7 },
  ]);
});

test("stale cancellation leaves the newer generation playing", () => {
  const { processor, messages } = setup();
  processor.pushPcm(2, 1, 48_000, true, new Int16Array([16384, 8192]));
  processor.handleMessage({ type: "cancel", generationId: 1 });
  assert.equal(processor.currentGenerationId, 2);
  assert.deepEqual([...render(processor, 2)], [0.5, 0.25]);
  assert.ok(messages.some((message) => message.type === "sentence-end" && message.generationId === 2));
});

test("late PCM and EOS from canceled generations are dropped", () => {
  const { processor, messages } = setup();
  processor.pushPcm(1, 1, 48_000, false, new Int16Array([16384]));
  processor.handleMessage({ type: "cancel", generationId: 1 });
  processor.pushPcm(1, 1, 48_000, true, new Int16Array([16384]));
  processor.pushPcm(0, 2, 48_000, true, new Int16Array());
  assert.equal(processor.bufferedFrames, 0);
  assert.deepEqual([...render(processor, 4)], [0, 0, 0, 0]);
  assert.equal(messages.some((message) => message.type.startsWith("sentence-")), false);
  processor.pushPcm(2, 1, 48_000, true, new Int16Array([8192]));
  assert.equal(render(processor, 1)[0], 0.25);
});

for (const rate of [24_000, 44_100, 96_000]) {
  test(`resampling at ${rate} Hz preserves phase and boundary samples across chunks`, () => {
    const samples = Int16Array.from({ length: 257 }, (_, index) => (index % 17) * 1000 - 8000);
    const whole = setup().processor;
    whole.pushPcm(1, 1, rate, true, samples);
    const expected = render(whole, whole.bufferedFrames);
    const { processor, messages } = setup();
    const actual: number[] = [];
    for (let offset = 0; offset < samples.length; offset++) {
      processor.pushPcm(1, 1, rate, false, samples.subarray(offset, offset + 1));
      actual.push(...render(processor, processor.bufferedFrames));
    }
    processor.pushPcm(1, 1, rate, true, new Int16Array());
    actual.push(...render(processor, processor.bufferedFrames));
    assert.deepEqual(actual, [...expected]);
    assert.equal(messages.filter((message) => message.type === "sentence-end").length, 1);
  });
}

test("cancellation and sentence changes reset the resampler", () => {
  const { processor } = setup();
  processor.pushPcm(1, 1, 24_000, false, new Int16Array([30000]));
  processor.handleMessage({ type: "cancel", generationId: 1 });
  processor.pushPcm(2, 1, 24_000, true, new Int16Array([8192, 16384]));
  assert.deepEqual([...render(processor, processor.bufferedFrames)], [0.25, 0.375, 0.5, 0.5]);
  processor.pushPcm(2, 2, 24_000, true, new Int16Array([-8192]));
  assert.deepEqual([...render(processor, processor.bufferedFrames)], [-0.25, -0.25]);
});

test("overflow rejects the whole chunk, preserves queued speech, and ends the skipped sentence", () => {
  // A small target rate gives a ring of a few hundred frames instead of 120 seconds of PCM.
  const { processor, messages } = setup(4);
  const capacity = processor.bufferedFrames + 4 * 120; // RING_BUFFER_SECONDS at 4 Hz
  const accepted = new Int16Array(capacity - 8).fill(8192);
  processor.pushPcm(1, 1, 4, true, accepted);
  const rejected = new Int16Array(9).fill(16384);
  processor.pushPcm(1, 2, 4, true, rejected);
  assert.equal(processor.bufferedFrames, capacity - 8);
  assert.deepEqual(messages.slice(0, 1), [{ type: "overflow", generationId: 1, sentenceId: 2,
    requiredFrames: 9, availableFrames: 8 }]);
  // Nothing retries it: the skipped sentence still ends, so the player never hangs on it.
  assert.ok(messages.some((message) => message.type === "sentence-end" && message.sentenceId === 2));
  assert.deepEqual([...render(processor, capacity - 8)], Array(capacity - 8).fill(0.25));
});

test("interrupt mid-sentence ends dropped sentences and plays the next pushed sentence", () => {
  const { processor, messages } = setup();
  processor.pushPcm(1, 1, 48_000, false, new Int16Array([8192, 16384, 8192, 16384]));
  processor.pushPcm(1, 2, 48_000, true, new Int16Array([16384, 16384]));
  assert.deepEqual([...render(processor, 2)], [0.25, 0.5]);
  assert.equal(messages.some((message) => message.type === "sentence-end"), false);

  processor.handleMessage({ type: "interrupt", generationId: 1 });
  assert.equal(processor.bufferedFrames, 0);
  assert.equal(processor.currentGenerationId, 1);
  assert.deepEqual(
    messages.filter((message) => message.type === "sentence-end"),
    [
      { type: "sentence-end", generationId: 1, sentenceId: 1 },
      { type: "sentence-end", generationId: 1, sentenceId: 2 },
    ],
  );
  assert.deepEqual([...render(processor, 2)], [0, 0]);

  processor.pushPcm(1, 3, 48_000, true, new Int16Array([-8192, 8192]));
  assert.deepEqual([...render(processor, 2)], [-0.25, 0.25]);
  assert.ok(
    messages.some(
      (message) =>
        message.type === "sentence-end" &&
        message.generationId === 1 &&
        message.sentenceId === 3,
    ),
  );
});

test("interrupt for another generation leaves the playing generation unaffected", () => {
  const { processor, messages } = setup();
  processor.pushPcm(2, 1, 48_000, true, new Int16Array([16384, 8192]));
  processor.handleMessage({ type: "interrupt", generationId: 1 });
  processor.handleMessage({ type: "interrupt", generationId: 3 });
  assert.equal(processor.currentGenerationId, 2);
  assert.equal(processor.bufferedFrames, 2);
  assert.deepEqual([...render(processor, 2)], [0.5, 0.25]);
  assert.deepEqual(
    messages.filter((message) => message.type === "sentence-end"),
    [{ type: "sentence-end", generationId: 2, sentenceId: 1 }],
  );
});
