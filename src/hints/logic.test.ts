// Run with npm test (Node's test runner strips the TypeScript types).
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import {
  CONSENT_PICTURE_MS, consentPhase, createHintPolicy, HINT_ASKED_KEY, HINT_DRAG_KEY, HINT_KEYS,
  loadHintMemory, TALK_COOLDOWN_MS, TALK_IDLE_MS, TALK_QUESTIONS,
} from "./logic.ts";
import type { StorageLike } from "../onboarding.ts";

const QUIET = { idle: true, busy: false };
const BUSY = { idle: true, busy: true };
const SPEAKING = { idle: false, busy: false };

function memoryStorage(initial: Record<string, string> = {}): StorageLike & { data: Map<string, string> } {
  const data = new Map(Object.entries(initial));
  return {
    data,
    getItem: (key) => data.get(key) ?? null,
    setItem: (key, value) => { data.set(key, value); },
  };
}

const throwing: StorageLike = {
  getItem() { throw new Error("denied"); },
  setItem() { throw new Error("denied"); },
};

test("the talk hint comes after 20 quiet seconds, never while busy or not idle", () => {
  const policy = createHintPolicy(memoryStorage());
  assert.equal(policy.next(BUSY, 0), null);
  assert.equal(policy.dueAt(), null);
  assert.equal(policy.next(SPEAKING, 1000), null);
  assert.equal(policy.next(QUIET, 2000), null);
  assert.equal(policy.dueAt(), 2000 + TALK_IDLE_MS);
  assert.equal(policy.next(QUIET, 2000 + TALK_IDLE_MS - 1), null);
  // Something showing on the window starts the 20 s again.
  assert.equal(policy.next(BUSY, 2000 + TALK_IDLE_MS - 1), null);
  assert.equal(policy.next(QUIET, 30_000), null);
  assert.equal(policy.dueAt(), 30_000 + TALK_IDLE_MS);
  assert.equal(policy.next(QUIET, 30_000 + TALK_IDLE_MS), "talk");
});

test("the talk hint comes at most once in two minutes", () => {
  const policy = createHintPolicy(memoryStorage());
  policy.next(QUIET, 0);
  assert.equal(policy.next(QUIET, TALK_IDLE_MS), "talk");
  policy.shown("talk", TALK_IDLE_MS);
  assert.equal(policy.next(QUIET, TALK_IDLE_MS + 1), null);
  assert.equal(policy.dueAt(), TALK_IDLE_MS + TALK_COOLDOWN_MS);
  policy.next(BUSY, 60_000);
  policy.next(QUIET, 61_000);
  assert.equal(policy.next(QUIET, 61_000 + TALK_IDLE_MS), null);
  assert.equal(policy.next(QUIET, TALK_IDLE_MS + TALK_COOLDOWN_MS), "talk");
});

test("three questions end the talk hint for good, counted once per answer and remembered", () => {
  const storage = memoryStorage();
  const policy = createHintPolicy(storage);
  policy.asked(4);
  policy.asked(4);
  policy.asked(3);
  assert.equal(storage.data.get(HINT_ASKED_KEY), "1");
  policy.asked(5);
  policy.next(QUIET, 0);
  assert.equal(policy.next(QUIET, TALK_IDLE_MS), "talk");
  policy.asked(9);
  assert.equal(storage.data.get(HINT_ASKED_KEY), String(TALK_QUESTIONS));
  assert.equal(policy.next(QUIET, 10 * TALK_COOLDOWN_MS), null);
  assert.equal(policy.dueAt(), null);
  policy.asked(10);
  assert.equal(storage.data.get(HINT_ASKED_KEY), String(TALK_QUESTIONS));
  const relaunched = createHintPolicy(storage);
  relaunched.next(QUIET, 0);
  assert.equal(relaunched.next(QUIET, 10 * TALK_COOLDOWN_MS), null);
});

test("the drag hint shows once ever, at the first quiet moment after an answer", () => {
  const storage = memoryStorage();
  const policy = createHintPolicy(storage);
  assert.equal(policy.next(QUIET, 0), null);
  policy.answered();
  // The answer's bubble still lingers.
  assert.equal(policy.next(BUSY, 100), null);
  assert.equal(policy.dueAt(), null);
  assert.equal(policy.next(QUIET, 200), "drag");
  policy.shown("drag", 200);
  assert.equal(storage.data.get(HINT_DRAG_KEY), "1");
  policy.answered();
  assert.equal(policy.next(QUIET, 300), null);
  const relaunched = createHintPolicy(storage);
  relaunched.answered();
  assert.equal(relaunched.next(QUIET, 0), null);
});

test("storage that is missing, broken or throws remembers nothing and breaks nothing", () => {
  for (const storage of [null, throwing, memoryStorage({ [HINT_ASKED_KEY]: "-2", [HINT_DRAG_KEY]: "yes" })]) {
    assert.deepEqual(loadHintMemory(storage), { asked: 0, dragShown: false });
    const policy = createHintPolicy(storage);
    policy.asked(1);
    policy.answered();
    assert.equal(policy.next(QUIET, 0), "drag");
    policy.shown("drag", 0);
    assert.equal(policy.next(QUIET, TALK_IDLE_MS), "talk");
  }
  assert.deepEqual(loadHintMemory(memoryStorage({ [HINT_ASKED_KEY]: "99" })), { asked: TALK_QUESTIONS, dragShown: false });
});

test("the consent picture and the question's text take turns, the picture first", () => {
  const textMs = 12_000;
  assert.deepEqual(consentPhase(0, textMs), { phase: "picture", leftMs: CONSENT_PICTURE_MS });
  assert.deepEqual(consentPhase(CONSENT_PICTURE_MS - 1, textMs), { phase: "picture", leftMs: 1 });
  assert.deepEqual(consentPhase(CONSENT_PICTURE_MS, textMs), { phase: "text", leftMs: textMs });
  const cycle = CONSENT_PICTURE_MS + textMs;
  assert.deepEqual(consentPhase(cycle + 500, textMs), { phase: "picture", leftMs: CONSENT_PICTURE_MS - 500 });
  for (const bad of [Number.NaN, -1, Number.POSITIVE_INFINITY]) {
    assert.equal(consentPhase(bad, textMs).phase, "picture");
    assert.equal(consentPhase(1000, bad).phase, "picture");
  }
});

test("every hint word exists in both languages, and the consent question names both ways", () => {
  const locales = ["en", "ru"].map((code) => [code, JSON.parse(
    readFileSync(new URL(`../../locales/${code}.json`, import.meta.url), "utf8"),
  ) as Record<string, string>] as const);
  for (const [code, strings] of locales) {
    for (const key of HINT_KEYS) assert.ok(strings[key], `${code}: missing ${key}`);
    assert.equal(strings["hints.key_fn"], "Fn");
    assert.equal(strings["hints.key_f8"], "F8");
    assert.ok(strings["robot.look_consent"].includes(`${strings["hints.key_fn"]} + ${strings["hints.key_f8"]}`), code);
  }
});
