import assert from "node:assert/strict";
import test from "node:test";
import {
  createCuePolicy,
  createCuePolicyState,
  CUE_COOLDOWN_MS,
  cueGainForMode,
  cueLevelForMode,
  decideCue,
  type CueDecision,
} from "./cue-policy.ts";
import { LEVEL_GAIN_TABLE, MAX_CUE_PAN, panFromViewportX } from "./mixer.ts";

test("maps each Mate event to its sound cue", () => {
  let t = 1_000;
  const played: Array<{ name: string; opts: unknown }> = [];
  const policy = createCuePolicy({
    now: () => t,
    isSpeaking: () => false,
    getSettings: () => ({ volume: 1, voiceSpeed: 1, mode: "fun", reduceMotion: false }),
    getViewportWidth: () => 1000,
    playCue: (name, opts) => {
      played.push({ name, opts });
    },
  });

  // 1. Listening starts -> "listen"
  assert.equal(
    policy.trigger({ type: "answer-state", generation: 1, state: "listening" })?.cue,
    "listen",
  );

  // 2. Looking (screenshot taken) -> "whoosh" softly
  t += CUE_COOLDOWN_MS;
  const look = policy.trigger({ type: "answer-state", generation: 1, state: "looking" });
  assert.equal(look?.cue, "whoosh");
  assert.equal(look?.options.level, "soft");

  // 3. Thinking -> "think" (non-looping one-shot)
  t += CUE_COOLDOWN_MS;
  const think = policy.trigger({ type: "answer-state", generation: 1, state: "thinking" });
  assert.equal(think?.cue, "think");
  assert.equal(think?.options.loop, false);

  // 4. Beam lands on target / <point> resolves -> "ping"
  t += CUE_COOLDOWN_MS;
  const ping = policy.trigger({
    type: "beam-target",
    rect: { left: 700, top: 100, right: 900, bottom: 140 },
  });
  assert.equal(ping?.cue, "ping");

  // 5. Lesson step done right -> "success"
  t += CUE_COOLDOWN_MS;
  assert.equal(
    policy.trigger({ type: "answer-line", generation: 1, key: "robot.right_click" })?.cue,
    "success",
  );

  // 6. Click away from the beam -> "oops"
  t += CUE_COOLDOWN_MS;
  assert.equal(
    policy.trigger({ type: "answer-line", generation: 1, key: "robot.wrong_place" })?.cue,
    "oops",
  );

  // 7. Lesson done -> "done"
  t += CUE_COOLDOWN_MS;
  assert.equal(
    policy.trigger({ type: "answer-line", generation: 1, key: "robot.done" })?.cue,
    "done",
  );

  // 8. Mate goes to sleep -> "sleep"
  t += CUE_COOLDOWN_MS;
  assert.equal(policy.trigger({ type: "sleep" })?.cue, "sleep");

  // 9. Mate wakes -> "wake"
  t += CUE_COOLDOWN_MS;
  assert.equal(policy.trigger({ type: "wake" })?.cue, "wake");

  // 10. Mate's menu opens -> "pop"
  t += CUE_COOLDOWN_MS;
  assert.equal(policy.trigger({ type: "menu-open" })?.cue, "pop");

  assert.deepEqual(
    played.map((p) => p.name),
    ["listen", "whoosh", "think", "ping", "success", "oops", "done", "sleep", "wake", "pop"],
  );
});

test("speech suppression blocks all cues while a sentence is playing except listen", () => {
  let t = 1_000;
  const policy = createCuePolicy({
    now: () => t,
    isSpeaking: () => true,
    getSettings: () => ({ volume: 1, voiceSpeed: 1, mode: "fun" }),
    getViewportWidth: () => 800,
  });

  // Blocked while speaking:
  assert.equal(policy.decide({ type: "answer-state", generation: 1, state: "looking" }), null);
  assert.equal(policy.decide({ type: "answer-state", generation: 1, state: "thinking" }), null);
  assert.equal(
    policy.decide({ type: "beam-target", rect: { left: 10, top: 10, right: 50, bottom: 50 } }),
    null,
  );
  assert.equal(
    policy.decide({ type: "answer-line", generation: 1, key: "robot.right_click" }),
    null,
  );
  assert.equal(
    policy.decide({ type: "answer-line", generation: 1, key: "robot.wrong_place" }),
    null,
  );
  assert.equal(policy.decide({ type: "answer-line", generation: 1, key: "robot.done" }), null);
  assert.equal(policy.decide({ type: "wake" }), null);
  assert.equal(policy.decide({ type: "sleep" }), null);
  assert.equal(policy.decide({ type: "menu-open" }), null);

  // "listen" is allowed immediately even over active speech:
  const listenDecision = policy.decide({
    type: "answer-state",
    generation: 2,
    state: "listening",
  });
  assert.equal(listenDecision?.cue, "listen");
});

test("rate-limits cues to at most one per 400 ms", () => {
  let state = createCuePolicyState({ now: 1_000, mode: "fun", volume: 1 });

  const first = decideCue({ type: "wake" }, state);
  assert.equal(first.decision?.cue, "wake");
  state = first.state;

  // At +100 ms and +399 ms, cues are suppressed and do not extend the cooldown:
  const early1 = decideCue({ type: "menu-open" }, { ...state, now: 1_100 });
  assert.equal(early1.decision, null);
  state = early1.state;

  const early2 = decideCue(
    { type: "answer-state", generation: 1, state: "listening" },
    { ...state, now: 1_399 },
  );
  assert.equal(early2.decision, null);
  state = early2.state;

  // Exactly at +400 ms from the first played cue, the next cue plays:
  const afterCooldown = decideCue({ type: "menu-open" }, { ...state, now: 1_400 });
  assert.equal(afterCooldown.decision?.cue, "pop");
});

test("plays think at most once per answer generation and resets on a new answer", () => {
  let t = 1_000;
  const policy = createCuePolicy({
    now: () => t,
    getSettings: () => ({ volume: 1, voiceSpeed: 1, mode: "fun" }),
  });

  // First "thinking" for generation 1 plays "think":
  const first = policy.decide({ type: "answer-state", generation: 1, state: "thinking" });
  assert.equal(first?.cue, "think");
  assert.equal(first?.options.loop, false);

  // Generation 1 transitions to "looking" -> plays "whoosh":
  t += CUE_COOLDOWN_MS;
  assert.equal(
    policy.decide({ type: "answer-state", generation: 1, state: "looking" })?.cue,
    "whoosh",
  );

  // Generation 1 returns to "thinking" after screenshot -> must NOT repeat "think":
  t += CUE_COOLDOWN_MS;
  assert.equal(
    policy.decide({ type: "answer-state", generation: 1, state: "thinking" }),
    null,
  );
  t += CUE_COOLDOWN_MS;
  assert.equal(policy.decide({ type: "thinking", generation: 1 }), null);

  // Generation 2 (new answer) -> plays "think" once:
  t += CUE_COOLDOWN_MS;
  assert.equal(
    policy.decide({ type: "answer-state", generation: 2, state: "thinking" })?.cue,
    "think",
  );
  t += CUE_COOLDOWN_MS;
  assert.equal(
    policy.decide({ type: "answer-state", generation: 2, state: "thinking" }),
    null,
  );
});

test("plays done only after a lesson and never after an ordinary chat answer", () => {
  let t = 1_000;
  const policy = createCuePolicy({
    now: () => t,
    getSettings: () => ({ volume: 1, voiceSpeed: 1, mode: "fun" }),
  });

  // Generation 1: ordinary chat answer with a sentence and <point>, no lesson step/wait
  policy.decide({
    type: "answer-item",
    generation: 1,
    item: {
      kind: "say",
      value: {
        id: 1,
        text: "Click Save.",
        bound: [{ kind: "point", value: { kind: "id", value: "btn_save" } }],
      },
    },
  });
  t += CUE_COOLDOWN_MS;
  assert.equal(policy.decide({ type: "answer-done", generation: 1 }), null);

  // Generation 2: lesson answer containing a <step> bound item and a wait item
  policy.decide({
    type: "answer-item",
    generation: 2,
    item: {
      kind: "say",
      value: {
        id: 1,
        text: "Step 1: click File.",
        bound: [
          {
            kind: "step",
            value: { number: 1, total: 2, target: { kind: "id", value: "menu_file" } },
          },
        ],
      },
    },
  });
  policy.decide({ type: "answer-item", generation: 2, item: { kind: "wait" } });
  t += CUE_COOLDOWN_MS;
  const lessonDone = policy.decide({ type: "answer-done", generation: 2 });
  assert.equal(lessonDone?.cue, "done");

  // Duplicate done for the same lesson generation does not repeat:
  t += CUE_COOLDOWN_MS;
  assert.equal(policy.decide({ type: "answer-done", generation: 2 }), null);

  // Generation 3+: lessons that end with a refusal must not play "done":
  let gen = 3;
  for (const key of [
    "robot.cant_find",
    "robot.private_window",
    "robot.protected_window",
    "robot.window_closed",
    "robot.sign_in_first",
    "robot.guest_limit",
    "robot.press_cancelled",
    "robot.press_failed",
  ] as const) {
    policy.decide({ type: "answer-item", generation: gen, item: { kind: "wait" } });
    policy.decide({ type: "answer-line", generation: gen, key });
    t += CUE_COOLDOWN_MS;
    assert.equal(policy.decide({ type: "answer-done", generation: gen }), null, key);
    gen += 1;
  }
});

test("respects calm vs fun sound mode, zero volume, and ignores reduced motion", () => {
  const calmState = createCuePolicyState({
    now: 1_000,
    mode: "calm",
    volume: 0.8,
    reduceMotion: false,
  });
  const funState = createCuePolicyState({
    now: 1_000,
    mode: "fun",
    volume: 0.8,
    reduceMotion: false,
  });

  const calmWake = decideCue({ type: "wake" }, calmState).decision as CueDecision;
  const funWake = decideCue({ type: "wake" }, funState).decision as CueDecision;
  assert.equal(calmWake.cue, "wake");
  assert.equal(funWake.cue, "wake");
  assert.equal(calmWake.options.level, cueLevelForMode("calm"));
  assert.equal(funWake.options.level, cueLevelForMode("fun"));
  assert.equal(calmWake.options.level, "soft");
  assert.equal(funWake.options.level, "normal");
  assert.ok((calmWake.options.gain ?? 0) < (funWake.options.gain ?? 0));
  assert.equal(calmWake.options.gain, LEVEL_GAIN_TABLE.soft);
  assert.equal(funWake.options.gain, LEVEL_GAIN_TABLE.normal);

  // "whoosh" (looking) is soft in fun mode and even quieter in calm mode:
  const calmLook = decideCue({ type: "looking" }, calmState).decision as CueDecision;
  const funLook = decideCue({ type: "looking" }, funState).decision as CueDecision;
  assert.equal(calmLook.options.level, "soft");
  assert.equal(funLook.options.level, "soft");
  assert.equal(funLook.options.gain, cueGainForMode("soft", "fun"));
  assert.equal(calmLook.options.gain, cueGainForMode("soft", "calm"));
  assert.ok((calmLook.options.gain ?? 0) < (funLook.options.gain ?? 0));

  // Volume of zero suppresses all cues (including "listen"):
  const zeroVolState = createCuePolicyState({ now: 2_000, mode: "fun", volume: 0 });
  assert.equal(decideCue({ type: "listening" }, zeroVolState).decision, null);
  assert.equal(decideCue({ type: "wake" }, zeroVolState).decision, null);

  // Reduced motion does not affect sound cue output:
  const motionOn = decideCue(
    { type: "menu-open" },
    createCuePolicyState({ now: 3_000, mode: "fun", volume: 1, reduceMotion: false }),
  ).decision;
  const motionReduced = decideCue(
    { type: "menu-open" },
    createCuePolicyState({ now: 3_000, mode: "fun", volume: 1, reduceMotion: true }),
  ).decision;
  assert.deepEqual(motionReduced, motionOn);
});

test("pans ping to the target rect via panFromViewportX and ignores null beam clear", () => {
  let t = 1_000;
  const policy = createCuePolicy({
    now: () => t,
    getSettings: () => ({ volume: 1, voiceSpeed: 1, mode: "fun" }),
    getViewportWidth: () => 1000,
  });

  // Clearing the beam (null rect) does not play a cue:
  assert.equal(policy.decide({ type: "beam-target", rect: null }), null);

  // Far-left target (center X = 50 in 1000px viewport -> negative pan):
  const leftPing = policy.decide({
    type: "beam-target",
    rect: { left: 0, top: 20, right: 100, bottom: 60 },
  });
  assert.equal(leftPing?.cue, "ping");
  assert.equal(leftPing?.options.pan, panFromViewportX(50, 1000));
  assert.ok((leftPing?.options.pan ?? 0) < 0);
  assert.ok((leftPing?.options.pan ?? 0) >= -MAX_CUE_PAN);

  // Center target (center X = 500 in 1000px viewport -> 0 pan):
  t += CUE_COOLDOWN_MS;
  const centerPing = policy.decide({
    type: "beam-target",
    rect: { left: 450, top: 20, right: 550, bottom: 60 },
  });
  assert.equal(centerPing?.cue, "ping");
  assert.equal(centerPing?.options.pan, 0);

  // Far-right target (center X = 950 in 1000px viewport -> positive pan):
  t += CUE_COOLDOWN_MS;
  const rightPing = policy.decide({
    type: "beam-target",
    rect: { left: 900, top: 20, right: 1000, bottom: 60 },
  });
  assert.equal(rightPing?.cue, "ping");
  assert.equal(rightPing?.options.pan, panFromViewportX(950, 1000));
  assert.ok((rightPing?.options.pan ?? 0) > 0);
  assert.ok((rightPing?.options.pan ?? 0) <= MAX_CUE_PAN);
});
