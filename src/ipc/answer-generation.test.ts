import assert from "node:assert/strict";
import { test } from "node:test";
import { AnswerGenerations, HeldPresentation } from "./answer-generation.ts";

test("click instructions belong to the recording and preserve an interrupted lesson", () => {
  const ids = new AnswerGenerations();
  ids.accept({ type: "model", generation: 1, model: null });
  ids.accept({ type: "state", generation: 2, state: "listening" });
  assert.ok(ids.accept({ type: "line", generation: 2, key: "robot.click_stop" }));
  assert.equal(ids.answer, 1);
  assert.equal(ids.holdPresentation, true);
  assert.ok(ids.accept({ type: "item", generation: 1, item: { kind: "end" } }));
  ids.accept({ type: "state", generation: 3, state: "listening" });
  assert.equal(ids.accept({ type: "line", generation: 2, key: "robot.click_stop" }), false);
});

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

test("cancelling an older lesson keeps a newer recording thinking", () => {
  const ids = new AnswerGenerations();
  ids.accept({ type: "model", generation: 1, model: null });
  ids.accept({ type: "state", generation: 2, state: "thinking" });
  assert.ok(ids.accept({ type: "cancel", generation: 1 }));
  assert.equal(ids.canIdle(1), false);
  assert.equal(ids.canIdle(2), true);
});

test("a recognition failure under the lesson generation keeps later lesson output", () => {
  const ids = new AnswerGenerations();
  ids.accept({ type: "model", generation: 1, model: null });
  ids.accept({ type: "state", generation: 2, state: "thinking" });
  assert.ok(ids.accept({ type: "line", generation: 1, key: "robot.network_error" }));
  assert.ok(ids.accept({ type: "state", generation: 1, state: "idle" }));
  assert.ok(ids.accept({ type: "item", generation: 1, item: { kind: "end" } }));
  assert.equal(ids.answer, 1);
});

test("lesson visuals wait for listening to end while its items are accepted", () => {
  const ids = new AnswerGenerations();
  const presentation = new HeldPresentation();
  const shown: string[] = [];
  ids.accept({ type: "model", generation: 1, model: null });
  ids.accept({ type: "state", generation: 2, state: "listening" });
  assert.equal(ids.holdPresentation, true);
  assert.ok(ids.accept({ type: "state", generation: 1, state: "looking" }));
  assert.ok(ids.accept({ type: "item", generation: 1, item: { kind: "end" } }));
  presentation.apply("state", ids.holdPresentation, () => shown.push("looking"));
  presentation.apply("state", ids.holdPresentation, () => shown.push("idle"));
  presentation.apply("bubble", ids.holdPresentation, () => shown.push("lesson"));
  assert.deepEqual(shown, []);
  assert.ok(ids.accept({ type: "heard", generation: 2, text: "which one?" }));
  assert.ok(ids.accept({ type: "level", generation: 2, level: 0.5 }));
  assert.equal(ids.listening, 2);
  ids.accept({ type: "state", generation: 2, state: "thinking" });
  assert.equal(ids.holdPresentation, false);
  presentation.release();
  assert.deepEqual(shown, ["idle", "lesson"]);
  presentation.release();
  assert.deepEqual(shown, ["idle", "lesson"]);
});

test("superseding a lesson discards its held visual changes", () => {
  const presentation = new HeldPresentation();
  const shown: string[] = [];
  presentation.apply("state", true, () => shown.push("idle"));
  presentation.apply("bubble", true, () => shown.push("old lesson"));
  presentation.clear();
  presentation.release();
  assert.deepEqual(shown, []);
  presentation.apply("bubble", false, () => shown.push("new answer"));
  assert.deepEqual(shown, ["new answer"]);
});

test("a held closing bubble shows before its hide timer starts", () => {
  const presentation = new HeldPresentation();
  const shown: string[] = [];
  presentation.apply("bubble", true, () => shown.push("cached line"));
  presentation.apply("bubble", true, () => shown.push("closing sentence"));
  presentation.apply("bubble-hide", true, () => shown.push("hide after"));
  assert.deepEqual(shown, []);
  presentation.release();
  assert.deepEqual(shown, ["closing sentence", "hide after"]);
});
