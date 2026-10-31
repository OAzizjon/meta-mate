import assert from "node:assert/strict";
import { registerHooks } from "node:module";
import test from "node:test";

// Only Vite's asset URL import needs a loader; exercise the real engine and idle policy.
const hooks = registerHooks({
  resolve(specifier, context, nextResolve) {
    if (specifier.endsWith("?worker&url")) {
      return { url: 'data:text/javascript,export default "worklet.js"', shortCircuit: true };
    }
    return nextResolve(specifier, context);
  },
});
const { createAudioEngine } = await import("./index.ts");
hooks.deregister();

class FakeNode {
  gain = { value: 1 };
  frequency = { value: 0 };
  Q = { value: 0 };
  threshold = { value: 0 };
  knee = { value: 0 };
  ratio = { value: 0 };
  attack = { value: 0 };
  release = { value: 0 };
  connect() {}
  disconnect() {}
}

test("blocked audio resume completes EOS without worklet output, including queued chunks", async (t) => {
  let resumes = 0;
  let posted = 0;
  class FakeContext {
    state = "suspended";
    sampleRate = 48000;
    currentTime = 0;
    destination = new FakeNode();
    audioWorklet = { async addModule() {} };
    createGain() { return new FakeNode(); }
    createBiquadFilter() { return new FakeNode(); }
    createDynamicsCompressor() { return new FakeNode(); }
    async resume() {
      // Initial startup fails immediately; playback resume resolves but remains suspended.
      if (++resumes === 1) throw new Error("blocked");
    }
    async close() { this.state = "closed"; }
    addEventListener() {}
    removeEventListener() {}
  }
  class FakeWorklet extends FakeNode {
    port = { onmessage: null, postMessage() { posted++; } };
  }
  for (const [name, value] of [["AudioContext", FakeContext], ["AudioWorkletNode", FakeWorklet]] as const) {
    const original = Object.getOwnPropertyDescriptor(globalThis, name);
    Object.defineProperty(globalThis, name, { configurable: true, writable: true, value });
    t.after(() => {
      if (original) Object.defineProperty(globalThis, name, original);
      else Reflect.deleteProperty(globalThis, name);
    });
  }
  const engine = await createAudioEngine();
  t.after(() => engine.dispose());
  const chunk = { generationId: 1, sentenceId: 1, sampleRate: 24000, samples: new Int16Array([100]) };
  let completed: boolean | undefined;
  void engine.waitForSentenceEnd(1, 1).then((played) => { completed = played; });
  engine.pushPcm({ ...chunk, eos: false });
  engine.pushPcm({ ...chunk, samples: new Int16Array(), eos: true });
  await new Promise((resolve) => setImmediate(resolve));
  assert.equal(completed, false);
  assert.equal(await engine.waitForSentenceEnd(1, 1), false);
  engine.pushPcm({ ...chunk, sentenceId: 2, eos: true });
  await new Promise((resolve) => setImmediate(resolve));
  assert.equal(await engine.waitForSentenceEnd(1, 2), false);
  assert.equal(posted, 0);
  assert.equal(engine.fallbackNeeded, true);
  assert.ok(resumes >= 3);
});
