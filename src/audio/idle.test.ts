import assert from "node:assert/strict";
import test from "node:test";
import { AUDIO_IDLE_MS, AudioIdleController } from "./idle.ts";

function clock() {
  return {
    state: "running" as AudioContextState,
    suspends: 0,
    resumes: 0,
    async suspend() { this.suspends++; this.state = "suspended"; },
    async resume() { this.resumes++; this.state = "running"; },
  };
}

async function settle() {
  for (let i = 0; i < 12; i++) await Promise.resolve();
}

test("suspends three seconds after both idle and playback completion", async (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const ctx = clock();
  const policy = new AudioIdleController(ctx);
  t.after(() => policy.dispose());
  policy.setIdle(false);
  t.mock.timers.tick(AUDIO_IDLE_MS);
  await settle();
  assert.equal(ctx.suspends, 0);
  const speech = policy.begin();
  const cue = policy.begin();
  await Promise.all([speech.ready, cue.ready]);
  policy.setIdle(true);
  speech.release();
  t.mock.timers.tick(AUDIO_IDLE_MS * 2);
  await settle();
  assert.equal(ctx.suspends, 0);
  cue.release();
  cue.release(); // Completion and stop may race; release is idempotent.
  t.mock.timers.tick(AUDIO_IDLE_MS - 1);
  await settle();
  assert.equal(ctx.suspends, 0);
  t.mock.timers.tick(1);
  await settle();
  assert.equal(ctx.state, "suspended");
  assert.equal(policy.wantsRunning, false);
});

test("new playback cancels an idle suspension already queued in a microtask", async (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const ctx = clock();
  const policy = new AudioIdleController(ctx);
  t.after(() => policy.dispose());
  t.mock.timers.tick(AUDIO_IDLE_MS);
  const playback = policy.begin();
  await playback.ready;
  assert.equal(ctx.suspends, 0);
  playback.release();
});

test("playback awaits an in-flight suspension and resume before scheduling", async (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const ctx = clock();
  let finishSuspend: () => void = () => {};
  ctx.suspend = () => new Promise<void>((resolve) => {
    finishSuspend = () => { ctx.state = "suspended"; resolve(); };
  });
  let finishResume: () => void = () => {};
  ctx.resume = () => new Promise<void>((resolve) => {
    finishResume = () => { ctx.state = "running"; resolve(); };
  });
  const policy = new AudioIdleController(ctx);
  t.after(() => policy.dispose());
  t.mock.timers.tick(AUDIO_IDLE_MS);
  await settle();
  const playback = policy.begin();
  let scheduled = false;
  void playback.ready.then(() => { scheduled = true; });
  await settle();
  assert.equal(scheduled, false);
  finishSuspend();
  await settle();
  assert.equal(scheduled, false);
  finishResume();
  await playback.ready;
  assert.equal(scheduled, true);
  playback.release();
});

test("hidden/sleeping drains audible playback then suspends without an idle delay", async (t) => {
  const ctx = clock();
  const policy = new AudioIdleController(ctx);
  t.after(() => policy.dispose());
  policy.setIdle(false);
  const cue = policy.begin();
  await cue.ready;
  policy.setInactive(true);
  await settle();
  assert.equal(ctx.suspends, 0);
  cue.release();
  await settle();
  assert.equal(ctx.state, "suspended");
  const nextCue = policy.begin();
  await nextCue.ready;
  assert.equal(ctx.resumes, 1);
  nextCue.release();
  await settle();
  assert.equal(ctx.state, "suspended");
});

test("resume rejection can be released and disposed clocks never resume", async () => {
  const ctx = clock();
  ctx.state = "suspended";
  ctx.resume = async () => { throw new Error("blocked"); };
  const policy = new AudioIdleController(ctx);
  const playback = policy.begin();
  await assert.rejects(playback.ready, /blocked/);
  playback.release();
  policy.dispose();
  const late = policy.begin();
  await assert.rejects(late.ready, /disposed/);
  late.release();
});
