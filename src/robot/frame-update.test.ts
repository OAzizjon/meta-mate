import assert from "node:assert/strict";
import test from "node:test";
import { createFrameUpdate } from "./frame-update.ts";

test("bursts render only the latest voice level once per frame, without an idle loop", () => {
  const pending = new Map<number, FrameRequestCallback>();
  let id = 0;
  let level = 0;
  const rendered: number[] = [];
  const update = createFrameUpdate(
    () => rendered.push(level),
    (callback) => { pending.set(++id, callback); return id; },
    (frame) => { pending.delete(frame); },
  );
  for (let i = 0; i < 100; i++) { level = i / 100; update.schedule(); }
  assert.equal(pending.size, 1);
  const flush = pending.get(id)!;
  pending.delete(id);
  flush(0);
  assert.deepEqual(rendered, [0.99]);
  assert.equal(pending.size, 0);
  update.schedule();
  update.cancel();
  assert.equal(pending.size, 0);
  update.schedule();
  assert.equal(pending.size, 1);
  update.cancel();
});
