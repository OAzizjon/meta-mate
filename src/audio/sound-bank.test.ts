import assert from "node:assert/strict";
import test from "node:test";
import { AudioMixer, LEVEL_GAIN_TABLE } from "./mixer.ts";
import { SoundBank } from "./sound-bank.ts";

class FakeParam {
  value = 1;
  setValueAtTime(value: number) { this.value = value; }
  linearRampToValueAtTime(value: number) { this.value = value; }
  setTargetAtTime(value: number) { this.value = value; }
  cancelScheduledValues() {}
}

class FakeNode {
  gain = new FakeParam();
  frequency = new FakeParam();
  Q = new FakeParam();
  threshold = new FakeParam();
  knee = new FakeParam();
  ratio = new FakeParam();
  attack = new FakeParam();
  release = new FakeParam();
  pan = new FakeParam();
  buffer: unknown = null;
  loop = false;
  started = false;
  stopped = false;
  disconnected = false;
  onended: (() => void) | null = null;
  connect(_target: unknown) {}
  disconnect() { this.disconnected = true; }
  start() { this.started = true; }
  stop() { this.stopped = true; this.onended?.(); }
}

function setup() {
  const sources: FakeNode[] = [];
  const gains: FakeNode[] = [];
  const ctx = {
    currentTime: 0,
    destination: new FakeNode(),
    createGain() { const node = new FakeNode(); gains.push(node); return node; },
    createBiquadFilter() { return new FakeNode(); },
    createDynamicsCompressor() { return new FakeNode(); },
    createStereoPanner() { return new FakeNode(); },
    createBufferSource() { const node = new FakeNode(); sources.push(node); return node; },
  } as unknown as AudioContext;
  const mixer = new AudioMixer(ctx);
  const bank = new SoundBank(ctx, mixer);
  // Bypass decoding only; exercise the real policy and playback lifecycle.
  bank.load = async () => ({} as AudioBuffer);
  return { bank, mixer, sources, gains };
}

async function settle() {
  // loadBuffer awaits load before its caller's .then runs.
  for (let i = 0; i < 6; i++) await Promise.resolve();
}

test("calm/fun budgets reserve concurrent SFX requests and reset only for a new generation", async () => {
  for (const mode of ["calm", "fun"] as const) {
    const { bank, mixer, sources } = setup();
    mixer.setSettings({ mode });
    bank.beginGeneration(1);
    for (let i = 0; i < 8; i++) bank.playSfx("cat");
    await settle();
    const budget = mode === "calm" ? 2 : 4;
    assert.equal(sources.length, budget);
    bank.beginGeneration(1);
    bank.playSfx("cat");
    await settle();
    assert.equal(sources.length, budget);
    bank.beginGeneration(2);
    bank.playSfx("cat");
    await settle();
    assert.equal(sources.length, budget + 1);
    bank.stopAllSfxAndCues();
  }
});

test("switching to calm stops existing effects and every ambience bed", async () => {
  const { bank, mixer, sources } = setup();
  mixer.setSettings({ mode: "fun" });
  bank.playSfx("thunder");
  bank.setAmbience("forest", "loud");
  await settle();
  bank.setAmbience("rain", "loud");
  await settle();
  assert.equal(sources.length, 3);
  mixer.setSettings({ mode: "calm" });
  assert.ok(sources.every((source) => source.stopped && source.disconnected));
});

test("pending fun-policy loads cannot start after switching to calm", async () => {
  const { bank, mixer, sources, gains } = setup();
  mixer.setSettings({ mode: "fun" });
  let finishLoad: (buffer: AudioBuffer) => void = () => {};
  const pending = new Promise<AudioBuffer>((resolve) => { finishLoad = resolve; });
  bank.load = () => pending;
  const handle = bank.playSfx("thunder");
  bank.setAmbience("forest", "loud");
  mixer.setSettings({ mode: "calm" });
  finishLoad({} as AudioBuffer);
  await settle();
  await handle.finished;
  assert.equal(sources.length, 0);
  bank.playSfx({ name: "cat", level: "loud" });
  bank.setAmbience("rain", "loud");
  await settle();
  assert.equal(sources.length, 2);
  assert.ok(gains.slice(-2).every((node) => node.gain.value === LEVEL_GAIN_TABLE.soft));
  bank.stopAllSfxAndCues();
  bank.stopAmbienceImmediately();
});

test("immediate ambience stop also stops the bed fading during a switch", async () => {
  const { bank, sources } = setup();
  bank.setAmbience("forest");
  await settle();
  bank.setAmbience("rain");
  await settle();
  assert.equal(sources.length, 2);
  assert.equal(sources[0].stopped, false);
  bank.stopAmbienceImmediately();
  assert.ok(sources.every((source) => source.stopped && source.disconnected));
  bank.stopAmbienceImmediately();
});
