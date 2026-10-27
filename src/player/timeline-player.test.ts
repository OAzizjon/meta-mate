// Run with `npm test` (Node's own test runner; Node 24 strips the types).
// The player against fake robot and audio objects that log every call in order.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

import { parseTimelineItem } from "../ipc/timeline.ts";
import { TimelinePlayer } from "./timeline-player.ts";

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

function items(...lines: string[]) {
  return lines.map((line) => {
    const item = parseTimelineItem(JSON.parse(line));
    assert.ok(item, `rejected: ${line}`);
    return item;
  });
}

const RECT = { left: 10, top: 20, right: 110, bottom: 60 };

/** Runs every pending promise chain (the fakes use no timers). */
function settle() {
  return new Promise((resolve) => setImmediate(resolve));
}

function setup() {
  const log: string[] = [];
  const sentences = new Map<string, () => void>();
  const sounds = new Map<string, () => void>();
  const sleeps: (() => void)[] = [];
  const robot = {
    setState(state: string) { log.push(`state ${state}`); },
    setMood(mood: string) { log.push(`mood ${mood}`); },
    aimAt(rect: unknown) { log.push(rect ? "aim rect" : "aim null"); },
  };
  const audio = {
    // Like the engine: cancelling resolves the sentence waits of that generation and older.
    cancel(generation: number) {
      log.push(`audio.cancel ${generation}`);
      for (const [key, resolve] of sentences) {
        if (Number(key.split(":")[0]) <= generation) {
          sentences.delete(key);
          resolve();
        }
      }
    },
    waitForSentenceEnd(generation: number, id: number) {
      log.push(`sentence ${generation}:${id}`);
      return new Promise<void>((resolve) => sentences.set(`${generation}:${id}`, resolve));
    },
    // Every sound, bound or solo, plays until the test ends it.
    playSfx(sound: { name: string }, generation: number) {
      log.push(`sfx ${sound.name} ${generation}`);
      const finished = new Promise<void>((resolve) => sounds.set(sound.name, resolve));
      return { stop() {}, finished };
    },
    setAmbience(name: string | null, level: string) { log.push(`amb ${name} ${level}`); },
  };
  const player = new TimelinePlayer({
    robot,
    audio,
    setLocale(locale: string) { log.push(`locale ${locale}`); },
    resolveTarget(point: { kind: string; value: unknown }) {
      log.push(`resolve ${point.kind}:${String(point.value)}`);
      return point.kind === "id" ? RECT : null;
    },
    showBubble(text: string | null) { log.push(`bubble ${text}`); },
    onLine(line: string) { log.push(`line ${line}`); },
    onDone(generation: number) { log.push(`done ${generation}`); },
    onWaitReached(generation: number) { log.push(`wait ${generation}`); },
    sleep(ms: number) {
      log.push(`sleep ${ms}`);
      return new Promise<void>((resolve) => sleeps.push(resolve));
    },
  });

  function finish(map: Map<string, () => void>, key: string) {
    const resolve = map.get(key);
    assert.ok(resolve, `nothing waits for ${key}`);
    map.delete(key);
    resolve();
  }

  return {
    player,
    robot,
    /** The calls since the last `take()`. */
    take: () => log.splice(0),
    endSentence: (generation: number, id: number) => finish(sentences, `${generation}:${id}`),
    endSound: (name: string) => finish(sounds, name),
    endSleep: () => {
      const resolve = sleeps.shift();
      assert.ok(resolve, "nothing sleeps");
      resolve();
    },
  };
}

function pushAll(player: TimelinePlayer, generation: number, lines: string[]) {
  for (const item of items(...lines)) player.push(generation, item);
}

test("the snapshot timeline drives the robot and the audio in order", async () => {
  const h = setup();
  pushAll(h.player, 1, snapshotLines());
  await settle();
  assert.deepEqual(h.take(), [
    "locale en",
    "mood happy",
    "resolve id:e12",
    "aim rect",
    "sfx ping 1",
    "state pointing",
    "sentence 1:1",
  ]);
  h.endSentence(1, 1);
  await settle();
  assert.deepEqual(h.take(), [
    "resolve id:e12",
    "aim rect",
    "bubble 125%",
    "state pointing",
    "sentence 1:2",
  ]);
  h.endSentence(1, 2);
  await settle();
  assert.deepEqual(h.take(), ["state waiting_for_click", "wait 1"]);
  assert.equal(h.player.resolveWait(), true);
  await settle();
  assert.deepEqual(h.take(), ["sleep 400"]);
  h.endSleep();
  await settle();
  assert.deepEqual(h.take(), ["amb rain soft", "sfx rimshot 1"]);
  h.endSound("rimshot");
  await settle();
  assert.deepEqual(h.take(), [
    "aim null",
    "state cant_find",
    "line CantFind",
    "state speaking",
    "sentence 1:3",
  ]);
  h.endSentence(1, 3);
  await settle();
  assert.deepEqual(h.take(), ["amb null soft", "mood proud", "done 1"]);
});

test("solo sounds, sentences and pauses are barriers; bound sounds are not", async () => {
  const h = setup();
  pushAll(h.player, 1, [
    '{"kind":"sfx","value":{"name":"rimshot","level":"normal","solo":true}}',
    '{"kind":"say","value":{"id":1,"text":"One.","bound":[{"kind":"sfx","value":{"name":"ping","level":"soft","solo":false}}]}}',
    '{"kind":"pause","value":5000}',
    '{"kind":"amb","value":{"name":"rain","level":"soft"}}',
    '{"kind":"end"}',
  ]);
  await settle();
  assert.deepEqual(h.take(), ["sfx rimshot 1"]);
  h.endSound("rimshot");
  await settle();
  // The ping never ends here, yet the sentence goes on; the pause waits for the sentence.
  assert.deepEqual(h.take(), ["sfx ping 1", "state speaking", "sentence 1:1"]);
  h.endSentence(1, 1);
  await settle();
  assert.deepEqual(h.take(), ["sleep 3000"]);
  h.endSleep();
  await settle();
  assert.deepEqual(h.take(), ["amb rain soft", "done 1"]);
});

test("a wait holds the timeline until resolveWait, which remembers nothing early", async () => {
  const h = setup();
  assert.equal(h.player.resolveWait(), false);
  pushAll(h.player, 1, [
    '{"kind":"say","value":{"id":1,"text":"Click it.","bound":[]}}',
    '{"kind":"wait"}',
    '{"kind":"end"}',
  ]);
  await settle();
  assert.deepEqual(h.take(), ["state speaking", "sentence 1:1"]);
  assert.equal(h.player.resolveWait(), false);
  h.endSentence(1, 1);
  await settle();
  assert.deepEqual(h.take(), ["state waiting_for_click", "wait 1"]);
  await settle();
  assert.deepEqual(h.take(), []);
  assert.equal(h.player.resolveWait(), true);
  assert.equal(h.player.resolveWait(), false);
  await settle();
  assert.deepEqual(h.take(), ["done 1"]);
});

test("cancel mid-sentence stops the answer and clears its beam and bubble", async () => {
  const h = setup();
  pushAll(h.player, 1, [
    '{"kind":"say","value":{"id":1,"text":"Type this.","bound":[{"kind":"point","value":{"kind":"id","value":"e12"}},{"kind":"show","value":"125%"}]}}',
    '{"kind":"say","value":{"id":2,"text":"Then this.","bound":[]}}',
    '{"kind":"end"}',
  ]);
  await settle();
  assert.deepEqual(h.take(), ["resolve id:e12", "aim rect", "bubble 125%", "state pointing", "sentence 1:1"]);
  h.player.cancel(1);
  assert.deepEqual(h.take(), ["audio.cancel 1", "aim null", "bubble null"]);
  await settle();
  // The engine released the sentence wait, but sentence 2 and End never play.
  assert.deepEqual(h.take(), []);
  pushAll(h.player, 1, ['{"kind":"end"}']);
  await settle();
  assert.deepEqual(h.take(), []);

  pushAll(h.player, 2, [
    '{"kind":"lang","value":"ru"}',
    '{"kind":"say","value":{"id":1,"text":"Next.","bound":[]}}',
  ]);
  await settle();
  assert.deepEqual(h.take(), ["locale ru", "state speaking", "sentence 2:1"]);
});

test("cancel aborts a pause; an older cancel leaves a newer answer alone", async () => {
  const h = setup();
  pushAll(h.player, 3, [
    '{"kind":"pause","value":400}',
    '{"kind":"pause","value":100}',
    '{"kind":"end"}',
  ]);
  await settle();
  assert.deepEqual(h.take(), ["sleep 400"]);
  h.player.cancel(2);
  assert.deepEqual(h.take(), ["audio.cancel 2"]);
  h.endSleep();
  await settle();
  assert.deepEqual(h.take(), ["sleep 100"]);
  h.player.cancel(3);
  assert.deepEqual(h.take(), ["audio.cancel 3"]);
  h.endSleep();
  await settle();
  assert.deepEqual(h.take(), []);
});

test("a newer generation replaces a waiting one; older items are dropped", async () => {
  const h = setup();
  pushAll(h.player, 1, ['{"kind":"wait"}']);
  await settle();
  assert.deepEqual(h.take(), ["state waiting_for_click", "wait 1"]);
  pushAll(h.player, 2, ['{"kind":"say","value":{"id":1,"text":"Next.","bound":[]}}']);
  await settle();
  assert.deepEqual(h.take(), ["audio.cancel 1", "state speaking", "sentence 2:1"]);
  assert.equal(h.player.resolveWait(), false);
  pushAll(h.player, 1, ['{"kind":"end"}']);
  await settle();
  assert.deepEqual(h.take(), []);
});

test("lang maps only English and Russian; unknown moods and unplaceable targets do nothing", async () => {
  const h = setup();
  pushAll(h.player, 1, [
    '{"kind":"lang","value":"rus"}',
    '{"kind":"lang","value":"es"}',
    '{"kind":"lang","value":"en"}',
    '{"kind":"bound","value":{"kind":"mood","value":"grumpy"}}',
    '{"kind":"bound","value":{"kind":"point","value":{"kind":"pixel","value":[812,344]}}}',
    '{"kind":"bad_tag"}',
    '{"kind":"end"}',
  ]);
  await settle();
  assert.deepEqual(h.take(), ["locale ru", "locale en", "resolve pixel:812,344", "aim null", "done 1"]);
});

test("a failing action is reported without its text and the sentence still plays", async (t) => {
  const errors = t.mock.method(console, "error", () => {});
  const h = setup();
  h.robot.setMood = () => {
    throw new Error("boom");
  };
  pushAll(h.player, 1, [
    '{"kind":"say","value":{"id":1,"text":"Secret words.","bound":[{"kind":"mood","value":"happy"},{"kind":"show","value":"125%"}]}}',
    '{"kind":"end"}',
  ]);
  await settle();
  assert.deepEqual(h.take(), ["bubble 125%", "state speaking", "sentence 1:1"]);
  h.endSentence(1, 1);
  await settle();
  assert.deepEqual(h.take(), ["done 1"]);
  assert.equal(errors.mock.callCount(), 1);
  const logged = errors.mock.calls[0].arguments.map(String).join(" ");
  assert.match(logged, /mood failed/);
  assert.doesNotMatch(logged, /Secret|125%/);
});
