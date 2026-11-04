// Run with `npm test` (Node's own test runner; Node 24 strips the types).
// Unit tests for Mate's state vocabulary and state animator (`looking` sweep, reduce-motion,
// the v1.1 brows and filament, and the springs held still while the beam is on).
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

import type { BeamController } from "./beam.ts";
import { MOOD_FACES, THINKING_GAZE } from "./geometry.ts";
import type { BrowStyle } from "./geometry.ts";
import { NO_MOTION, createMotionController } from "./motion.ts";
import type { FrameClock, MotionController } from "./motion.ts";
import { LOOKING_SWEEP_MS, createStateAnimator } from "./states.ts";
import { buildMateSvg } from "./svg.ts";
import type { MateSvgRefs } from "./svg.ts";
import { ROBOT_MOODS, ROBOT_STATES, isRobotMood, isRobotState } from "./types.ts";
import type { RobotMood } from "./types.ts";

const ROBOT_CSS = readFileSync(new URL("./robot.css", import.meta.url), "utf8");

interface RecordedAnimation {
  readonly target: FakeSvgElement;
  readonly keyframes: Keyframe[];
  readonly options: KeyframeAnimationOptions;
  cancelled: boolean;
}

class FakeClassList {
  private readonly tokens = new Set<string>();

  add(...tokens: string[]): void {
    for (const token of tokens) this.tokens.add(token);
  }

  remove(...tokens: string[]): void {
    for (const token of tokens) this.tokens.delete(token);
  }

  contains(token: string): boolean {
    return this.tokens.has(token);
  }

  toggle(token: string, force?: boolean): boolean {
    const next = force === undefined ? !this.tokens.has(token) : force;
    if (next) this.tokens.add(token);
    else this.tokens.delete(token);
    return next;
  }
}

class FakeSvgElement {
  readonly tagName: string;
  readonly attrs = new Map<string, string>();
  readonly classList = new FakeClassList();
  readonly cssProperties = new Map<string, string>();
  readonly style: {
    opacity: string;
    setProperty(name: string, value: string): void;
    removeProperty(name: string): void;
  };
  readonly animations: RecordedAnimation[] = [];
  children: FakeSvgElement[] = [];
  parentElement: FakeSvgElement | null = null;
  textContent = "";

  constructor(tagName: string) {
    this.tagName = tagName;
    const properties = this.cssProperties;
    const styleObj = {
      opacity: "",
      setProperty(name: string, value: string): void { properties.set(name, value); },
      removeProperty(name: string): void {
        properties.delete(name);
        if (name === "opacity") styleObj.opacity = "";
      },
    };
    this.style = styleObj;
  }

  setAttribute(name: string, value: string): void {
    this.attrs.set(name, String(value));
    if (name === "class") {
      for (const token of String(value).split(/\s+/)) {
        if (token) this.classList.add(token);
      }
    }
  }

  getAttribute(name: string): string | null {
    return this.attrs.get(name) ?? null;
  }

  removeAttribute(name: string): void {
    this.attrs.delete(name);
  }

  append(...nodes: FakeSvgElement[]): void {
    for (const node of nodes) {
      node.parentElement = this;
      this.children.push(node);
    }
  }

  replaceChildren(...nodes: FakeSvgElement[]): void {
    for (const child of this.children) child.parentElement = null;
    this.children = [];
    this.append(...nodes);
  }

  querySelector(selector: string): FakeSvgElement | null {
    for (const child of this.children) {
      if (child.tagName === selector) return child;
      const nested = child.querySelector(selector);
      if (nested) return nested;
    }
    return null;
  }

  closest(selector: string): FakeSvgElement | null {
    if (selector.startsWith(".")) {
      const cls = selector.slice(1);
      let cur: FakeSvgElement | null = this;
      while (cur) {
        if (cur.classList.contains(cls)) return cur;
        cur = cur.parentElement;
      }
    }
    return null;
  }

  getBoundingClientRect(): { left: number; top: number; right: number; bottom: number; width: number; height: number } {
    return { left: 0, top: 0, right: 260, bottom: 240, width: 260, height: 240 };
  }

  animate(keyframes: Keyframe[], options: KeyframeAnimationOptions): Animation {
    const record: RecordedAnimation = {
      target: this,
      keyframes,
      options,
      cancelled: false,
    };
    this.animations.push(record);
    return {
      cancel() {
        record.cancelled = true;
      },
    } as unknown as Animation;
  }
}

function createHarness(motionFor?: (refs: MateSvgRefs) => MotionController) {
  const prevDocument = (globalThis as { document?: unknown }).document;
  (globalThis as { document?: unknown }).document = {
    createElementNS(_ns: string, tag: string) {
      return new FakeSvgElement(tag);
    },
  };
  const refs = buildMateSvg("test");
  const beamCalls: string[] = [];
  const beam: BeamController = {
    aimAt() {
      beamCalls.push("aimAt");
    },
    setPulse(waiting) {
      beamCalls.push(`pulse:${String(waiting)}`);
    },
    hide() {
      beamCalls.push("hide");
    },
  };
  const animator = createStateAnimator(refs, beam, motionFor?.(refs));
  return {
    refs: refs as unknown as {
      readonly svg: FakeSvgElement;
      readonly motion: FakeSvgElement;
      readonly body: FakeSvgElement;
      readonly cap: FakeSvgElement;
      readonly filament: FakeSvgElement;
      readonly eyes: FakeSvgElement;
      readonly mouth: FakeSvgElement;
      readonly layers: Record<string, FakeSvgElement>;
    },
    rawRefs: refs as MateSvgRefs,
    beamCalls,
    animator,
    cleanup() {
      animator.destroy();
      if (prevDocument === undefined) {
        delete (globalThis as { document?: unknown }).document;
      } else {
        (globalThis as { document?: unknown }).document = prevDocument;
      }
    },
  };
}

test("ROBOT_STATES and isRobotState accept 'looking' and reject unknown states", () => {
  assert.ok(ROBOT_STATES.includes("looking"));
  for (const state of ROBOT_STATES) {
    assert.equal(isRobotState(state), true, state);
  }
  for (const bad of ["Looking", "LOOKING", "staring", "idle ", "", null, undefined, 1, {}]) {
    assert.equal(isRobotState(bad), false);
  }
  for (const mood of ROBOT_MOODS) {
    assert.equal(isRobotMood(mood), true, mood);
  }
});

test("setting state to 'looking' sweeps the eyes left-right once (~600 ms) and holds, then transitions to 'thinking'", (t) => {
  const h = createHarness();
  t.after(() => h.cleanup());

  // Even if the mood was sleepy or thinking, 'looking' opens the eyes at neutral gaze.
  h.animator.setMood("sleepy");
  assert.equal(h.refs.eyes.getAttribute("data-shape"), "closed");
  h.animator.setMood("thinking");
  assert.equal(
    h.refs.eyes.getAttribute("transform"),
    `translate(${THINKING_GAZE.x} ${THINKING_GAZE.y})`,
  );

  h.animator.setState("looking");
  assert.equal(h.refs.svg.getAttribute("data-state"), "looking");
  assert.equal(h.refs.eyes.getAttribute("data-shape"), "open");
  assert.equal(h.refs.eyes.getAttribute("transform"), null);

  assert.equal(h.refs.eyes.animations.length, 1);
  const sweep = h.refs.eyes.animations[0];
  assert.equal(sweep.cancelled, false);
  assert.equal(sweep.options.duration, LOOKING_SWEEP_MS);
  assert.equal(sweep.options.duration, 600);
  assert.equal(sweep.options.fill, "forwards");
  assert.equal(sweep.keyframes.length, 4);
  assert.deepEqual(sweep.keyframes[0], { transform: "translateX(0)" });
  assert.match(String(sweep.keyframes[1].transform), /^translateX\(-\d/);
  assert.match(String(sweep.keyframes[2].transform), /^translateX\(\d/);
  assert.deepEqual(sweep.keyframes[3], { transform: "translateX(0)" });

  // Rust sends "thinking" right after copying the window: cancels the sweep and sets thinking gaze.
  h.animator.setState("thinking");
  assert.equal(h.refs.svg.getAttribute("data-state"), "thinking");
  assert.equal(sweep.cancelled, true);
  assert.equal(
    h.refs.eyes.getAttribute("transform"),
    `translate(${THINKING_GAZE.x} ${THINKING_GAZE.y})`,
  );
});

test("'looking' respects setReduceMotion(true) before and during the state", (t) => {
  const h = createHarness();
  t.after(() => h.cleanup());

  h.animator.setReduceMotion(true);
  assert.equal(h.refs.svg.classList.contains("reduce-motion"), true);

  h.animator.setState("looking");
  assert.equal(h.refs.svg.getAttribute("data-state"), "looking");
  assert.equal(h.refs.eyes.animations.length, 0);

  // Re-enabling motion and entering 'looking' starts the sweep; reducing motion mid-sweep cancels it.
  h.animator.setReduceMotion(false);
  h.animator.setState("looking");
  assert.equal(h.refs.eyes.animations.length, 1);
  assert.equal(h.refs.eyes.animations[0].cancelled, false);

  h.animator.setReduceMotion(true);
  assert.equal(h.refs.eyes.animations[0].cancelled, true);
});

test("'looking' respects the .reduce-motion class on the SVG or an ancestor container", (t) => {
  const h = createHarness();
  t.after(() => h.cleanup());

  // Direct .reduce-motion class on the SVG element.
  h.refs.svg.classList.add("reduce-motion");
  h.animator.setState("looking");
  assert.equal(h.refs.eyes.animations.length, 0);
  h.refs.svg.classList.remove("reduce-motion");

  // .reduce-motion class on an ancestor element (e.g. documentElement or #mate container).
  const root = new FakeSvgElement("div");
  root.classList.add("reduce-motion");
  root.append(h.refs.svg);

  h.animator.setState("looking");
  assert.equal(h.refs.eyes.animations.length, 0);
});

test("robot.css defines the 'looking' eye sweep and disables it under .reduce-motion", () => {
  assert.match(
    ROBOT_CSS,
    /\.mate-svg\[data-state="looking"\]\s+\.mate-eyes\s*\{[^}]*animation:\s*mate-look\s+0\.6s\s+ease-in-out\s+1\s+forwards/,
  );
  assert.match(ROBOT_CSS, /@keyframes\s+mate-look\s*\{/);
  assert.match(ROBOT_CSS, /\.mate-svg\.reduce-motion\s+\*\s*\{[^}]*animation:\s*none\s*!important/);
});

test("v1.1 layers: two aura circles, a dim filament in the glass, brows beside the eyes", (t) => {
  const h = createHarness();
  t.after(() => h.cleanup());

  assert.equal(h.refs.layers.aura.children.length, 2);
  const filament = h.refs.layers.glass.children.find((child) => child.classList.contains("mate-filament"));
  assert.ok(filament);
  assert.equal(filament.getAttribute("opacity"), "0.45");

  const [brows, eyes, mouth] = h.refs.layers.face.children;
  assert.equal(brows.classList.contains("mate-brows"), true);
  assert.equal(eyes, h.refs.eyes);
  assert.equal(mouth, h.refs.mouth);
  assert.deepEqual(
    brows.children.map((b) => b.getAttribute("class")),
    ["mate-brow mate-brow-l", "mate-brow mate-brow-r"],
  );
  // robot.css moves the brows; a transform attribute would be overridden by it.
  for (const b of brows.children) assert.equal(b.getAttribute("transform"), null);
});

test("moods move the brows: happy up, thinking one raised, sad tilted, surprised high", (t) => {
  const h = createHarness();
  t.after(() => h.cleanup());

  assert.equal(h.refs.svg.getAttribute("data-brows"), "rest");
  const expected: Record<RobotMood, BrowStyle> = {
    calm: "rest",
    happy: "up",
    thinking: "raised",
    surprised: "high",
    sad: "tilted",
    proud: "up",
    sleepy: "low",
  };
  for (const mood of ROBOT_MOODS) {
    assert.equal(MOOD_FACES[mood].brows, expected[mood], mood);
    h.animator.setMood(mood);
    assert.equal(h.refs.svg.getAttribute("data-brows"), expected[mood], mood);
  }
});

test("states lend the brows a pose, then the mood's brows return", (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const h = createHarness();
  t.after(() => h.cleanup());
  const brows = () => h.refs.svg.getAttribute("data-brows");

  h.animator.setState("listening");
  assert.equal(brows(), "up");
  h.animator.setState("thinking");
  assert.equal(brows(), "raised");
  h.animator.setState("cant_find");
  assert.equal(brows(), "tilted");
  h.animator.setState("sleeping");
  assert.equal(brows(), "low");
  h.animator.setState("idle");
  assert.equal(brows(), "rest");

  // oops raises one brow only as long as its squint (600 ms).
  h.animator.setMood("happy");
  h.animator.setState("oops");
  assert.equal(brows(), "raised");
  assert.equal(h.refs.eyes.getAttribute("data-shape"), "squint");
  t.mock.timers.tick(600);
  assert.equal(brows(), "up");
  assert.equal(h.refs.eyes.getAttribute("data-shape"), "happy");
});

test("robot.css poses every brow style, warms the filament in listening and speaking, and keeps reduce-motion", () => {
  const poses: readonly BrowStyle[] = ["up", "high", "low", "raised", "tilted"];
  for (const pose of poses) {
    assert.match(ROBOT_CSS, new RegExp(`\\.mate-svg\\[data-brows="${pose}"\\]\\s+\\.mate-brow(-[lr])?\\s*\\{[^}]*transform:`), pose);
  }
  for (const state of ["listening", "speaking"]) {
    assert.match(ROBOT_CSS, new RegExp(`\\.mate-svg\\[data-state="${state}"\\]\\s+\\.mate-filament\\s*[,{]`), state);
  }
  // The brows only transition transform, which the reduce-motion rule cuts back to opacity.
  assert.match(ROBOT_CSS, /\.mate-brow\s*\{[^}]*transition:\s*transform\b/);
  assert.match(ROBOT_CSS, /\.mate-svg\.reduce-motion\s+\*\s*\{[^}]*transition-property:\s*opacity\s*!important/);
});

test("speaking RMS bursts update mouth and aura once per frame and cancel on hide/state change", (t) => {
  const h = createHarness();
  t.after(() => h.cleanup());
  const originalRequest = globalThis.requestAnimationFrame;
  const originalCancel = globalThis.cancelAnimationFrame;
  const frames = new Map<number, FrameRequestCallback>();
  let id = 0;
  globalThis.requestAnimationFrame = (callback) => { frames.set(++id, callback); return id; };
  globalThis.cancelAnimationFrame = (frame) => { frames.delete(frame); };
  t.after(() => {
    globalThis.requestAnimationFrame = originalRequest;
    globalThis.cancelAnimationFrame = originalCancel;
  });
  h.animator.setState("speaking");
  const rest = h.refs.layers.aura.style.opacity;
  h.animator.setVoiceLevel(0.2);
  h.animator.setVoiceLevel(0.8);
  assert.equal(frames.size, 1);
  assert.equal(h.refs.layers.aura.style.opacity, rest);
  const flush = frames.get(id)!;
  frames.delete(id);
  flush(0);
  assert.equal(h.refs.layers.aura.style.opacity, "0.73");
  assert.equal(h.refs.mouth.getAttribute("data-shape"), "open");
  assert.equal(frames.size, 0);
  h.animator.setVoiceLevel(0.8001);
  assert.equal(frames.size, 0); // Same quantized level.
  h.animator.setVoiceLevel(0.4);
  h.animator.setHidden(true);
  assert.equal(frames.size, 0);
  h.animator.setVoiceLevel(0.5);
  assert.equal(frames.size, 0);
  h.animator.setHidden(false);
  assert.equal(h.refs.layers.aura.style.opacity, "0.63");
  h.animator.setVoiceLevel(0.7);
  h.animator.setState("idle");
  assert.equal(frames.size, 0);
  assert.equal(h.refs.layers.aura.style.opacity, "");
});
test("warmth brightens the waiting aura with reduced motion and becomes steady on target", (t) => {
  const h = createHarness();
  t.after(() => h.cleanup());
  h.animator.setState("waiting_for_click");
  h.animator.setReduceMotion(true);
  h.animator.setWarmth(0, false);
  assert.equal(h.refs.layers.aura.style.opacity, "0.45");
  assert.equal(h.refs.svg.cssProperties.get("--warmth-period"), "1.8s");
  h.animator.setWarmth(9, true);
  assert.equal(h.refs.layers.aura.style.opacity, "0.95");
  assert.ok(Math.abs(parseFloat(h.refs.svg.cssProperties.get("--warmth-period")!) - 0.6) < 1e-9);
  assert.equal(h.refs.svg.classList.contains("warmth-steady"), true);
  h.animator.setWarmth(null, false);
  assert.equal(h.refs.svg.classList.contains("has-warmth"), false);
  assert.equal(h.refs.layers.aura.style.opacity, "");
  h.animator.setState("idle");
  assert.equal(h.refs.layers.aura.style.opacity, "");
});

test("success bursts the six sparks outward once, and reduced motion keeps them still", (t) => {
  const h = createHarness();
  t.after(() => h.cleanup());
  h.animator.setState("success");
  const sparks = h.refs.layers.sparks.children;
  assert.equal(sparks.length, 6);
  for (const spark of sparks) {
    assert.equal(spark.animations.length, 1);
    assert.equal(spark.animations[0].options.fill, "both");
  }
  h.animator.setState("idle");
  for (const spark of sparks) assert.equal(spark.animations[0].cancelled, true);

  const still = createHarness();
  t.after(() => still.cleanup());
  still.animator.setReduceMotion(true);
  still.animator.setState("success");
  for (const spark of still.refs.layers.sparks.children) assert.equal(spark.animations.length, 0);
});

test("a new mood reacts once (a scam verdict flinches), a repeated mood does not", (t) => {
  const h = createHarness();
  t.after(() => h.cleanup());
  const before = h.refs.body.animations.length;
  h.animator.setMood("sad");
  assert.equal(h.refs.body.animations.length, before + 1);
  assert.ok("translate" in h.refs.body.animations[before].keyframes[1]);
  h.animator.setMood("sad");
  assert.equal(h.refs.body.animations.length, before + 1);
  // The flinch survives the "speaking" that follows the mood tag.
  h.animator.setState("speaking");
  assert.equal(h.refs.body.animations[before].cancelled, false);
  h.animator.setMood("calm");
  assert.equal(h.refs.body.animations.length, before + 1, "calm has no reaction");
});

test("Mate waves hello on his first idle and after waking, never with reduced motion", (t) => {
  const h = createHarness();
  t.after(() => h.cleanup());
  h.animator.setState("idle");
  const armL = h.refs.layers["arm-l"];
  assert.equal(armL.getAttribute("data-raised"), "true");
  assert.equal(armL.animations.length, 1);
  h.animator.setState("idle");
  assert.equal(armL.animations.length, 1, "only the first idle greets");
  h.animator.setState("listening");
  assert.equal(armL.getAttribute("data-raised"), null, "a new state lowers the hand");
  h.animator.setState("sleeping");
  h.animator.setState("idle");
  assert.equal(armL.animations.length, 2, "waking greets again");

  const still = createHarness();
  t.after(() => still.cleanup());
  still.animator.setReduceMotion(true);
  still.animator.setState("idle");
  assert.equal(still.refs.layers["arm-l"].animations.length, 0);
  still.animator.setMood("surprised");
  assert.equal(still.refs.body.animations.length, 0);
  still.animator.setReduceMotion(false);
  still.animator.setState("idle");
  assert.equal(still.refs.layers["arm-l"].animations.length, 1, "a suppressed greeting stays pending");
  still.animator.setState("idle");
  assert.equal(still.refs.layers["arm-l"].animations.length, 1, "the pending greeting plays only once");
});

/** Animation frames by hand: `frame()` moves the clock one frame on and runs what was due. */
function fakeFrames() {
  let now = 0;
  let id = 0;
  const pending = new Map<number, FrameRequestCallback>();
  const clock: FrameClock = {
    now: () => now,
    request(callback) {
      pending.set(++id, callback);
      return id;
    },
    cancel(frame) {
      pending.delete(frame);
    },
  };
  return {
    clock,
    pending,
    frame(ms = 16) {
      now += ms;
      const due = [...pending.values()];
      pending.clear();
      for (const callback of due) callback(now);
    },
  };
}

function createSpringHarness() {
  const frames = fakeFrames();
  let motion: MotionController = NO_MOTION;
  const h = createHarness((refs) => (motion = createMotionController(refs, frames.clock)));
  const springStyles = (): string[] =>
    [
      h.refs.motion.cssProperties.get("transform"),
      h.refs.cap.cssProperties.get("rotate"),
      h.refs.cap.cssProperties.get("translate"),
      h.refs.filament.cssProperties.get("rotate"),
      h.refs.layers["arm-l"].cssProperties.get("rotate"),
      h.refs.layers["arm-r"].cssProperties.get("rotate"),
    ].filter((value): value is string => value !== undefined);
  /** The window dragged right, one move per frame. */
  const drag = (from: number): void => {
    for (let i = 0; i < 6; i++) {
      motion.moved(from + i * 12, 40);
      frames.frame();
    }
  };
  return { ...h, frames, motion: () => motion, springStyles, drag };
}

test("one-shots in beam states never rotate, and oops keeps its vertical dip", (t) => {
  const h = createHarness();
  t.after(() => h.cleanup());
  h.animator.aimAt({ left: 400, top: 100, right: 500, bottom: 150 });

  function elements(root: FakeSvgElement): FakeSvgElement[] {
    return [root, ...root.children.flatMap(elements)];
  }
  for (const state of ["pointing", "waiting_for_click", "oops"] as const) {
    h.animator.setState(state);
    for (const mood of ROBOT_MOODS) h.animator.setMood(mood);
    for (const el of elements(h.refs.svg)) {
      for (const animation of el.animations.filter((a) => !a.cancelled)) {
        for (const frame of animation.keyframes) {
          assert.equal(frame.rotate, undefined, state);
          assert.doesNotMatch(String(frame.transform ?? ""), /rotate/i, state);
        }
      }
    }
  }
  const dip = h.refs.body.animations.find((a) => !a.cancelled);
  assert.ok(dip, "oops still reacts");
  assert.equal(dip.options.duration, 600);
  assert.deepEqual(dip.keyframes.map((frame) => frame.transform), [
    "translateY(0)", "translateY(3px)", "translateY(3px)", "translateY(0)",
  ]);
});

test("the springs never move or tilt the flashlight while the beam is on, and come back after", (t) => {
  const h = createSpringHarness();
  t.after(() => h.cleanup());
  h.animator.setState("idle");
  h.drag(0);
  assert.match(String(h.refs.motion.cssProperties.get("transform")), /^matrix\(/, "a drag leans him in idle");
  assert.equal(h.refs.filament.getAttribute("class"), "mate-filament");

  h.animator.aimAt({ left: 300, top: 40, right: 360, bottom: 80 });
  for (const state of ["pointing", "waiting_for_click", "oops"] as const) {
    h.animator.setState(state);
    assert.deepEqual(h.springStyles(), [], `${state}: the rest pose at once`);
    assert.equal(h.frames.pending.size, 0, `${state}: no frame left`);
    h.drag(100);
    h.motion().squash("jump");
    h.motion().release();
    h.animator.setMood("surprised");
    h.animator.setMood("calm");
    h.frames.frame();
    assert.deepEqual(h.springStyles(), [], `${state}: moves, kicks and moods are ignored`);
    assert.equal(h.frames.pending.size, 0, state);
  }
  assert.ok(h.beamCalls.includes("aimAt"));

  h.animator.setState("idle");
  h.drag(200);
  assert.match(String(h.refs.motion.cssProperties.get("transform")), /^matrix\(/, "back in idle he leans again");
});

test("with reduced motion the springs hold the rest pose, and the state still shows", (t) => {
  const h = createSpringHarness();
  t.after(() => h.cleanup());
  h.animator.setReduceMotion(true);
  h.animator.setState("idle");
  h.animator.setMood("surprised");
  h.drag(0);
  h.motion().release();
  assert.equal(h.frames.pending.size, 0);
  assert.deepEqual(h.springStyles(), []);
  assert.equal(h.refs.svg.getAttribute("data-state"), "idle");
  assert.equal(h.refs.svg.getAttribute("data-brows"), "high");

  // Reducing motion mid-drag puts him back at rest at once.
  h.animator.setReduceMotion(false);
  h.drag(100);
  assert.notDeepEqual(h.springStyles(), []);
  h.animator.setReduceMotion(true);
  assert.deepEqual(h.springStyles(), []);
  assert.equal(h.frames.pending.size, 0);
});

test("after a drag he wobbles to rest, then takes the spring styles off and asks for no frames", (t) => {
  const h = createSpringHarness();
  t.after(() => h.cleanup());
  h.animator.setState("idle");
  h.drag(0);
  h.motion().release();
  for (let i = 0; i < 400 && h.frames.pending.size > 0; i++) h.frames.frame();
  assert.equal(h.frames.pending.size, 0);
  assert.deepEqual(h.springStyles(), []);
});

test("robot.css floats idle Mate in layers whose periods never line up, and reduced motion drops the springs' poses", () => {
  const drift = ROBOT_CSS.match(/\.mate-svg\[data-state="idle"\]\s+\.mate-motion\s*\{[^}]*animation:\s*mate-drift\s+([\d.]+)s/);
  const body = ROBOT_CSS.match(
    /\.mate-svg\[data-state="idle"\]\s+\.mate-body\s*\{[^}]*mate-float\s+([\d.]+)s\s+linear[^}]*mate-sway\s+([\d.]+)s\s+ease-in-out\s+(-[\d.]+)s/,
  );
  assert.ok(drift && body);
  const ratio = Number(body[1]) / Number(drift[1]);
  for (let q = 1; q <= 5; q++) {
    assert.ok(Math.abs(ratio * q - Math.round(ratio * q)) > 0.05, `the float repeats after ${q} drifts`);
  }
  // The tilt follows the sideways swing: the same period, a little late.
  assert.equal(body[2], body[1]);
  const lateS = Number(body[1]) + Number(body[3]);
  assert.ok(lateS > 0.3 && lateS < 1.5, `late by ${lateS} s`);
  assert.match(ROBOT_CSS, new RegExp(`\\.mate-layer-shadow\\s*\\{[^}]*mate-float-shadow\\s+${drift[1]}s`));
  // Breath stays.
  assert.match(ROBOT_CSS, /\.mate-svg\[data-state="idle"\]\s+\.mate-layer-aura\s*\{[^}]*mate-breathe\s+4\.2s/);
  assert.match(ROBOT_CSS, /\.mate-svg\.reduce-motion\s+\.mate-motion\s*\{[^}]*transform:\s*none\s*!important/);
  assert.match(ROBOT_CSS, /\.mate-svg\.reduce-motion\s+:is\([^)]*\.mate-cap[^)]*\)\s*\{[^}]*rotate:\s*none\s*!important/);
});
