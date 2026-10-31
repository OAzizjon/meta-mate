// Run with npm test (Node's test runner strips the TypeScript types).
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import {
  awakeBubble, CANT_FIND_HOLD_MS, createMateAnswerRouter, createMateWindowEvents, LOOK_CONSENT_MS,
  guestTrialView, interruptLesson, signedIn,
} from "./mate-logic.ts";
import { BUBBLE_LINGER_MS, bubbleReadMs } from "./bubble-timing.ts";
import type { AnswerMsg } from "./ipc/answer-stream.ts";
import {
  ONBOARDING_STEP_MS, resetOnboardingSession, startOnboarding, stopOnboarding,
} from "./onboarding.ts";

const locales = Object.fromEntries(["en", "ru"].map((code) => [
  code, JSON.parse(readFileSync(new URL(`../locales/${code}.json`, import.meta.url), "utf8")) as Record<string, string>,
]));

function harness(locale = "en", stop = () => {}) {
  const calls: unknown[][] = [];
  const record = (name: string) => (...args: unknown[]) => { calls.push([name, ...args]); };
  const bubble = {
    setModel: record("model"),
    show: record("bubble"),
    hideAfter: record("linger"),
  };
  const robot = { setState: record("state"), setVoiceLevel: record("level") };
  const stamp = { show: record("stamp"), hideAfter: record("stamp-linger") };
  const waits: number[] = [];
  let release = () => {};
  const router = createMateAnswerRouter({
    robot, bubble, stamp,
    player: { push: record("push"), cancel: record("cancel") },
    t: (key) => locales[locale][key],
    stopOnboarding: () => { calls.push(["stop"]); stop(); },
    onPcm: record("pcm"),
    reportPlayback: record("report"),
    sleep: (ms) => {
      waits.push(ms);
      return new Promise<void>((resolve) => { release = resolve; });
    },
  });
  const send = (msg: AnswerMsg) => router.route(msg);
  return { calls, bubble, robot, router, send, waits, release: () => release() };
}

test("listening assembles heard text and click-to-stop in both locales", () => {
  for (const locale of ["en", "ru"]) {
    const h = harness(locale);
    h.send({ type: "state", generation: 1, state: "listening" });
    assert.deepEqual(h.calls, [["model", null], ["stop"], ["state", "listening"], ["bubble", null]]);
    h.calls.length = 0;
    h.send({ type: "heard", generation: 1, text: "" });
    h.send({ type: "line", generation: 1, key: "robot.click_stop" });
    h.send({ type: "heard", generation: 1, text: "<b>heard</b>" });
    h.send({ type: "heard", generation: 1, text: "" });
    assert.deepEqual(h.calls.filter(([kind]) => kind === "bubble"), [
      ["bubble", null],
      ["bubble", locales[locale]["robot.click_stop"]],
      ["bubble", `<b>heard</b>\n${locales[locale]["robot.click_stop"]}`],
      ["bubble", locales[locale]["robot.click_stop"]],
    ]);
  }
});

test("leaving listening clears heard text and voice level; a new recording resets click mode", () => {
  const h = harness();
  h.send({ type: "state", generation: 1, state: "listening" });
  h.send({ type: "line", generation: 1, key: "robot.click_stop" });
  h.send({ type: "level", generation: 1, level: 0.7 });
  assert.deepEqual(h.calls.at(-1), ["level", 0.7]);
  h.calls.length = 0;
  h.send({ type: "state", generation: 1, state: "thinking" });
  assert.deepEqual(h.calls, [["stop"], ["bubble", null], ["level", 0], ["state", "thinking"]]);
  h.calls.length = 0;
  h.send({ type: "heard", generation: 1, text: "late" });
  h.send({ type: "level", generation: 1, level: 0.9 });
  h.send({ type: "line", generation: 1, key: "robot.click_stop" });
  assert.deepEqual(h.calls, [["stop"], ["stop"], ["stop"]]);
  h.send({ type: "state", generation: 2, state: "listening" });
  h.send({ type: "heard", generation: 2, text: "new" });
  assert.deepEqual(h.calls.at(-1), ["bubble", "new"]);
});

test("stale and cancelled messages have no UI or onboarding effects", () => {
  const h = harness();
  h.send({ type: "model", generation: 3, model: "sam-3.1" });
  h.calls.length = 0;
  h.send({ type: "state", generation: 2, state: "listening" });
  h.send({ type: "line", generation: 2, key: "robot.network_error" });
  h.send({ type: "cancel", generation: 2 });
  assert.deepEqual(h.calls, []);
  h.send({ type: "cancel", generation: 3 });
  assert.deepEqual(h.calls, [["stop"], ["cancel", 3], ["state", "idle"]]);
  h.calls.length = 0;
  h.send({ type: "model", generation: 3, model: "sam-3.1" });
  h.send({ type: "item", generation: 3, item: { kind: "end" } });
  assert.deepEqual(h.calls, []);
});

test("older lesson visuals wait for recording to finish and retain only the latest bubble", () => {
  const h = harness();
  h.send({ type: "model", generation: 1, model: "sam-3.1" });
  h.send({ type: "state", generation: 2, state: "listening" });
  h.calls.length = 0;
  h.router.showBubble("first");
  h.router.showBubble("latest");
  h.router.present("state", () => h.router.setState("speaking"));
  assert.deepEqual(h.calls, []);
  h.send({ type: "heard", generation: 1, text: "old transcript" });
  assert.deepEqual(h.calls, []);
  h.send({ type: "state", generation: 2, state: "thinking" });
  assert.deepEqual(h.calls, [
    ["bubble", "latest"], ["level", 0], ["state", "speaking"], ["stop"], ["state", "thinking"],
  ]);
});

test("a replacement answer discards held presentation from the previous lesson", () => {
  const h = harness();
  h.send({ type: "model", generation: 1, model: "sam-3.1" });
  h.send({ type: "state", generation: 2, state: "listening" });
  h.router.showBubble("obsolete");
  h.calls.length = 0;
  h.send({ type: "model", generation: 3, model: "muse-spark-1.3" });
  h.send({ type: "state", generation: 3, state: "thinking" });
  assert.equal(h.calls.some((call) => call.includes("obsolete")), false);
  assert.deepEqual(h.calls.filter(([kind]) => kind === "model"), [
    ["model", null], ["model", "muse-spark-1.3"],
  ]);
});

test("lesson completion during recording reports done immediately but defers idle and bubble linger", () => {
  const h = harness();
  h.send({ type: "model", generation: 1, model: "sam-3.1" });
  h.send({ type: "state", generation: 2, state: "listening" });
  h.calls.length = 0;
  h.router.onDone(1);
  assert.deepEqual(h.calls, [["report", "done", 1]]);
  h.calls.length = 0;
  h.send({ type: "state", generation: 2, state: "thinking" });
  assert.deepEqual(h.calls, [
    ["level", 0], ["state", "idle"], ["linger", BUBBLE_LINGER_MS], ["stop"], ["state", "thinking"],
  ]);
});

test("cancelling an older lesson leaves the new recording listening and discards held text", () => {
  const h = harness();
  h.send({ type: "model", generation: 1, model: "sam-3.1" });
  h.send({ type: "state", generation: 2, state: "listening" });
  h.router.showBubble("discarded");
  h.calls.length = 0;
  h.send({ type: "cancel", generation: 1 });
  assert.deepEqual(h.calls, [["stop"], ["cancel", 1]]);
  h.send({ type: "heard", generation: 2, text: "still listening" });
  assert.deepEqual(h.calls.at(-1), ["bubble", "still listening"]);
  h.send({ type: "state", generation: 2, state: "thinking" });
  assert.equal(h.calls.some((call) => call.includes("discarded")), false);
});

test("locale lines use reading time; timeline completion lingers only for the active answer", async () => {
  for (const locale of ["en", "ru"]) {
    const h = harness(locale);
    h.send({ type: "line", generation: 1, key: "robot.network_error" });
    const text = locales[locale]["robot.network_error"];
    assert.deepEqual(h.calls.at(-1), ["bubble", text, bubbleReadMs(text)]);
    h.calls.length = 0;
    const held = h.router.onLine("CantFind");
    const cantFind = locales[locale]["robot.cant_find"];
    assert.deepEqual(h.calls, [["bubble", cantFind, bubbleReadMs(cantFind)]]);
    assert.deepEqual(h.waits, [CANT_FIND_HOLD_MS]);
    let settled = false;
    void Promise.resolve(held).then(() => { settled = true; });
    await Promise.resolve();
    assert.equal(settled, false);
    h.release();
    await held;
    h.calls.length = 0;
    h.router.onDone(0);
    assert.deepEqual(h.calls, []);
    h.router.onDone(1);
    assert.deepEqual(h.calls, [["state", "idle"], ["linger", BUBBLE_LINGER_MS], ["report", "done", 1]]);
    h.calls.length = 0;
    h.send({ type: "model", generation: 1, model: "sam-3.1" });
    assert.deepEqual(h.calls, [["stop"]]);
  }
});

test("timeline and PCM messages reach their sinks without losing their generation", () => {
  const h = harness();
  const pcm: AnswerMsg = {
    type: "pcm", generation: 4, sentence: 7, sampleRate: 24000, eos: true, samplesB64: "",
  };
  h.send({ type: "item", generation: 4, item: { kind: "end" } });
  h.send(pcm);
  assert.deepEqual(h.calls, [
    ["model", null], ["stop"], ["push", 4, { kind: "end" }],
    ["report", "trace", 4, "item end"], ["stop"], ["pcm", pcm],
  ]);
});

const stampCalls = (calls: unknown[][]) => calls.filter(([kind]) => String(kind).startsWith("stamp"));
const sayWith = (mood: string): AnswerMsg => ({
  type: "item", generation: 1,
  item: { kind: "say", value: { id: 1, text: "Text.", bound: [{ kind: "mood", value: mood }] } },
});

test("a safety check stamps its first mood's verdict, lingers with the answer, and goes with a new press", () => {
  const h = harness();
  h.send({ type: "state", generation: 1, state: "thinking" });
  h.router.markSafety(1);
  h.send({ type: "item", generation: 1, item: { kind: "lang", value: "en" } });
  assert.deepEqual(stampCalls(h.calls), []);
  h.send(sayWith("sad"));
  h.send(sayWith("calm"));
  assert.deepEqual(stampCalls(h.calls), [["stamp", "scam"]]);
  h.router.onDone(1);
  assert.deepEqual(stampCalls(h.calls).at(-1), ["stamp-linger", BUBBLE_LINGER_MS]);
  h.calls.length = 0;
  h.send({ type: "state", generation: 2, state: "listening" });
  assert.deepEqual(stampCalls(h.calls), [["stamp", null]]);
});

test("no stamp without the safety event, for another answer, after a newer press, or when ambiguous", () => {
  const h = harness();
  h.send({ type: "state", generation: 1, state: "thinking" });
  h.send(sayWith("sad"));
  for (const payload of [null, "1", -1, 1.5, NaN, Number.MAX_SAFE_INTEGER + 1, { generation: 1 }, 0]) {
    h.router.markSafety(payload);
  }
  h.send({ type: "state", generation: 2, state: "thinking" });
  h.router.markSafety(1);
  h.send({ type: "item", generation: 1, item: { kind: "say", value: { id: 2, text: "Old.", bound: [] } } });
  h.router.markSafety(3);
  h.send({ type: "item", generation: 2, item: { kind: "end" } });
  h.router.onDone(2);
  assert.deepEqual(stampCalls(h.calls), []);
  const ambiguous = harness();
  ambiguous.router.markSafety(1);
  ambiguous.send(sayWith("happy"));
  ambiguous.send(sayWith("sad"));
  ambiguous.router.onDone(1);
  assert.deepEqual(stampCalls(ambiguous.calls), []);
});

test("'looks OK' is withdrawn by a later warning; a cancel takes the stamp down", () => {
  const h = harness();
  h.router.markSafety(1);
  h.router.markSafety(1);
  h.send(sayWith("calm"));
  h.router.markSafety(1);
  h.send(sayWith("surprised"));
  assert.deepEqual(stampCalls(h.calls), [["stamp", "ok"], ["stamp", null]]);
  const cancelled = harness();
  cancelled.router.markSafety(1);
  cancelled.send(sayWith("surprised"));
  cancelled.send({ type: "cancel", generation: 1 });
  cancelled.send(sayWith("sad"));
  cancelled.router.onDone(1);
  assert.deepEqual(stampCalls(cancelled.calls), [["stamp", "careful"], ["stamp", null]]);
});

test("lesson-interrupt accepts nonnegative safe integers only, including generation zero", () => {
  const interrupted: number[] = [];
  for (const payload of [null, undefined, {}, "2", -1, 1.5, NaN, Infinity, Number.MAX_SAFE_INTEGER + 1, 0, 8]) {
    interruptLesson(payload, (generation) => interrupted.push(generation));
  }
  assert.deepEqual(interrupted, [0, 8]);
});

function windowHarness() {
  const h = harness();
  let sleeping = false;
  const events = createMateWindowEvents({
    bubble: h.bubble, robot: h.robot,
    isSleeping: () => sleeping,
    setSleeping: (value) => { sleeping = value; h.calls.push(["sleeping", value]); },
    closeMenu: () => { h.calls.push(["menu-close"]); },
    stopOnboarding: () => { h.calls.push(["stop"]); },
    hidingText: (minutes) => { h.calls.push(["hiding-text", minutes]); return locales.en["mate.hiding"]; },
    explainText: (control) => { h.calls.push(["control", control]); return locales.en["mate.explain_offer"]; },
  });
  return { ...h, events, live: awakeBubble(h.bubble, () => sleeping) };
}

test("mate-hiding closes the menu, stops onboarding, sleeps and protects its timed bubble", () => {
  const h = windowHarness();
  h.events.hide(60);
  assert.deepEqual(h.calls, [
    ["menu-close"], ["stop"], ["sleeping", true], ["state", "sleeping"],
    ["model", null], ["hiding-text", 60], ["bubble", locales.en["mate.hiding"], 3000],
  ]);
  h.calls.length = 0;
  h.live.setModel("sam-3.1");
  h.live.show("answer", 100);
  h.live.hideAfter(200);
  h.events.explain({ control: "button", ms: 100 });
  assert.deepEqual(h.calls, []);
  h.events.wake();
  assert.deepEqual(h.calls, [["sleeping", false], ["bubble", null], ["state", "idle"]]);
  h.calls.length = 0;
  h.live.setModel("sam-3.1");
  h.live.show("answer", 100);
  h.live.hideAfter(200);
  assert.deepEqual(h.calls, [["model", "sam-3.1"], ["bubble", "answer", 100], ["linger", 200]]);
});

test("explain-offer validates fields and forwards control and positive duration unchanged", () => {
  const h = windowHarness();
  for (const payload of [null, {}, { control: 2, ms: 1 }, { control: "x" }, { control: "x", ms: "1" },
    { control: "x", ms: 0 }, { control: "x", ms: -1 }, { control: "x", ms: NaN }]) {
    h.events.explain(payload);
  }
  assert.deepEqual(h.calls, []);
  // Empty controls and Infinity were accepted by the original handler too.
  for (const [control, ms] of [["<button>", 42], ["", Infinity]] as const) {
    h.events.explain({ control, ms });
    assert.deepEqual(h.calls.splice(0), [
      ["stop"], ["model", null], ["control", control], ["bubble", locales.en["mate.explain_offer"], ms],
    ]);
  }
});

test("guest-usage clamps the counter, marks exhaustion and hides absent trials", () => {
  for (const payload of [null, undefined, {}, { left: 2, limit: 0 }, { left: "2", limit: 10 }]) {
    assert.equal(guestTrialView(payload), null);
  }
  assert.deepEqual(guestTrialView({ left: 12, limit: 10 }), { left: "10", limit: "10", empty: false });
  assert.deepEqual(guestTrialView({ left: -2, limit: 10 }), { left: "0", limit: "10", empty: true });
  assert.deepEqual(guestTrialView({ left: 3, limit: 10 }), { left: "3", limit: "10", empty: false });
  for (const payload of [null, {}, { signedIn: false }, { signedIn: 1 }, { signedIn: "true" }]) {
    assert.equal(signedIn(payload), false);
  }
  assert.equal(signedIn({ signedIn: true }), true);
});

test("onboarding greets, shows examples in order, then hides; an accepted answer cancels its timer", () => {
  for (const cancel of [false, true]) {
    resetOnboardingSession();
    const pending = new Map<number, () => void>();
    const shown: (string | null)[] = [];
    let id = 0;
    const controller = startOnboarding({ showBubble: (text) => shown.push(text) }, {
      storage: null,
      translate: (key) => key === "onboarding.try_asking" ? "{question}" : key,
      clock: {
        setTimeout(callback, ms) {
          assert.ok(ms >= ONBOARDING_STEP_MS);
          pending.set(++id, callback);
          return id;
        },
        clearTimeout(handle) { pending.delete(handle as number); },
      },
    });
    assert.equal(shown[0], "robot.first_launch");
    if (cancel) {
      const h = harness("en", stopOnboarding);
      h.send({ type: "state", generation: 0, state: "listening" });
      assert.equal(controller.isActive(), false);
      assert.equal(pending.size, 0);
      assert.equal(shown.at(-1), null);
    } else {
      while (pending.size) {
        const [handle, advance] = pending.entries().next().value!;
        pending.delete(handle);
        advance();
      }
      assert.deepEqual(shown, [
        "robot.first_launch", "onboarding.example_1", "onboarding.example_2", "onboarding.example_3", null,
      ]);
      assert.equal(controller.isActive(), false);
    }
    stopOnboarding();
  }
});

test("every Mate locale key exists in both languages", () => {
  const source = readFileSync(new URL("./mate.ts", import.meta.url), "utf8")
    + readFileSync(new URL("./mate-logic.ts", import.meta.url), "utf8");
  const keys = new Set([...source.matchAll(/"(?:robot|mate|model|home)\.[a-z_]+"/g)].map(([key]) => JSON.parse(key)));
  assert.ok(keys.size >= 10);
  for (const key of keys) {
    for (const [locale, strings] of Object.entries(locales)) {
      assert.ok(strings[key], `${locale}: missing ${key}`);
    }
  }
});

test("the look-consent question stays up for the whole consent window", () => {
  for (const locale of ["en", "ru"]) {
    const h = harness(locale);
    h.send({ type: "state", generation: 1, state: "looking" });
    h.calls.length = 0;
    h.send({ type: "line", generation: 1, key: "robot.look_consent" });
    const text = locales[locale]["robot.look_consent"];
    assert.deepEqual(h.calls.filter(([kind]) => kind === "bubble"), [["bubble", text, LOOK_CONSENT_MS]]);
    assert.ok(LOOK_CONSENT_MS > bubbleReadMs(text));
    // A click on Mate answers on any laptop; F8 still works but is not offered.
    assert.ok(!text.includes("F8"), `${locale}: ${text}`);
    assert.ok(text.includes("Esc"), `${locale}: ${text}`);
    assert.match(text, locale === "en" ? /Click me/ : /Нажмите на меня/);
  }
});

test("look consent hides once its generation starts thinking or returns idle", () => {
  for (const state of ["thinking", "idle"] as const) {
    const h = harness();
    h.send({ type: "state", generation: 1, state: "looking" });
    h.send({ type: "line", generation: 1, key: "robot.look_consent" });
    h.calls.length = 0;
    h.send({ type: "state", generation: 2, state });
    h.send({ type: "state", generation: 1, state: "looking" });
    assert.deepEqual(h.calls.filter(([kind]) => kind === "bubble"), []);
    h.calls.length = 0;
    h.send({ type: "state", generation: 1, state });
    assert.deepEqual(h.calls.filter(([kind]) => kind === "bubble"), [["bubble", null]]);
    h.calls.length = 0;
    h.router.showBubble("Answer text.");
    h.send({ type: "state", generation: 1, state });
    assert.deepEqual(h.calls.filter(([kind]) => kind === "bubble"), [["bubble", "Answer text."]]);
  }
});

test("a lesson's look consent that closes during a recording is not shown afterwards", () => {
  const h = harness();
  h.send({ type: "model", generation: 1, model: "sam-3.1" });
  h.send({ type: "state", generation: 2, state: "listening" });
  h.send({ type: "line", generation: 1, key: "robot.look_consent" });
  h.send({ type: "state", generation: 1, state: "thinking" });
  h.calls.length = 0;
  h.send({ type: "state", generation: 2, state: "thinking" });
  assert.deepEqual(h.calls.filter(([kind]) => kind === "bubble"), [["bubble", null]]);
});
