import assert from "node:assert/strict";
import { test } from "node:test";
import { AnswerGenerations } from "./answer-generation.ts";

test("a follow-up recording keeps the lesson's speech, state and model", () => {
  const ids = new AnswerGenerations();
  assert.ok(ids.accept({ type: "item", generation: 1, item: { kind: "end" } }));
  assert.ok(ids.accept({ type: "state", generation: 2, state: "listening" }));
  assert.ok(ids.accept({ type: "heard", generation: 2, text: "next" }));
  assert.ok(ids.accept({ type: "state", generation: 2, state: "thinking" }));
  assert.ok(ids.accept({ type: "state", generation: 1, state: "thinking" }));
  assert.ok(ids.accept({ type: "model", generation: 1, model: null }));
  assert.ok(ids.accept({ type: "item", generation: 1, item: { kind: "end" } }));
  assert.ok(ids.accept({
    type: "pcm", generation: 1, sentence: 3, sampleRate: 16000, eos: true, samplesB64: "",
  }));
  assert.equal(ids.answer, 1);
});

test("barge-in retires ordinary answers even after a newer recording starts", () => {
  const ids = new AnswerGenerations();
  ids.accept({ type: "model", generation: 4, model: "muse-spark-1.3" });
  ids.accept({ type: "state", generation: 5, state: "listening" });
  assert.ok(ids.accept({ type: "cancel", generation: 4 }));
  assert.equal(ids.accept({ type: "model", generation: 4, model: null }), false);
  assert.equal(ids.accept({ type: "state", generation: 4, state: "idle" }), false);
  assert.ok(ids.accept({ type: "model", generation: 5, model: "muse-wonder-1.0" }));
  assert.equal(ids.answer, 5);
  assert.equal(ids.accept({ type: "line", generation: 3, key: "robot.cant_find" }), false);
  assert.equal(ids.accept({ type: "cancel", generation: 4 }), false);
  assert.ok(ids.accept({ type: "state", generation: 5, state: "idle" }));
});

test("a late recording cannot replace the newest microphone's state or words", () => {
  const ids = new AnswerGenerations();
  ids.accept({ type: "model", generation: 1, model: "muse-spark-1.3" });
  ids.accept({ type: "state", generation: 2, state: "listening" });
  ids.accept({ type: "state", generation: 3, state: "listening" });
  assert.equal(ids.accept({ type: "heard", generation: 2, text: "yes" }), false);
  assert.equal(ids.accept({ type: "level", generation: 2, level: 0.5 }), false);
  assert.equal(ids.accept({ type: "state", generation: 2, state: "idle" }), false);
  assert.ok(ids.accept({ type: "state", generation: 1, state: "thinking" }));
});
