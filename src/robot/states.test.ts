// Run with `npm test` (Node's own test runner; Node 24 strips the types).
// Unit tests for Mate's state vocabulary and state animator (`looking` sweep and reduce-motion).
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

import type { BeamController } from "./beam.ts";
import { THINKING_GAZE } from "./geometry.ts";
import { LOOKING_SWEEP_MS, createStateAnimator } from "./states.ts";
import { buildMateSvg } from "./svg.ts";
import type { MateSvgRefs } from "./svg.ts";
import { ROBOT_MOODS, ROBOT_STATES, isRobotMood, isRobotState } from "./types.ts";

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
  readonly style: {
    opacity: string;
    removeProperty(name: string): void;
  };
  readonly animations: RecordedAnimation[] = [];
  children: FakeSvgElement[] = [];
  parentElement: FakeSvgElement | null = null;
  textContent = "";

  constructor(tagName: string) {
    this.tagName = tagName;
    const styleObj = {
      opacity: "",
      removeProperty(name: string): void {
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

function createHarness() {
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
  const animator = createStateAnimator(refs, beam);
  return {
    refs: refs as unknown as {
      readonly svg: FakeSvgElement;
      readonly body: FakeSvgElement;
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
