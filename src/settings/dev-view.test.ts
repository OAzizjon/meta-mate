// Run with `npm test` (Node's own test runner; Node 24 strips the types).
// Unit tests for the Settings window's Developer mode section (src/settings/dev-view.ts).
import assert from "node:assert/strict";
import { test } from "node:test";

import {
  DEV_STATUS_DURATION_MS,
  MATE_JUMP_CMD,
  OPEN_UNDER_THE_HOOD_CMD,
  buildDevSection,
  devButtonsHidden,
} from "./dev-view.ts";

class FakeClassList {
  readonly tokens = new Set<string>();

  add(...tokens: string[]): void {
    for (const token of tokens) this.tokens.add(token);
  }

  contains(token: string): boolean {
    return this.tokens.has(token);
  }

  toggle(token: string, force?: boolean): boolean {
    const next = force ?? !this.tokens.has(token);
    if (next) this.tokens.add(token);
    else this.tokens.delete(token);
    return next;
  }
}

class FakeElement {
  readonly tagName: string;
  readonly classList = new FakeClassList();
  readonly attributes = new Map<string, string>();
  readonly listeners = new Map<string, Array<() => void>>();
  readonly children: FakeElement[] = [];
  parentElement: FakeElement | null = null;
  id = "";
  type = "";
  htmlFor = "";
  textContent = "";
  checked = false;
  hidden = false;

  constructor(tagName: string) {
    this.tagName = tagName;
  }

  get className(): string {
    return [...this.classList.tokens].join(" ");
  }

  set className(value: string) {
    this.classList.tokens.clear();
    this.classList.add(...value.split(/\s+/).filter(Boolean));
  }

  append(...nodes: FakeElement[]): void {
    for (const node of nodes) {
      node.parentElement = this;
      this.children.push(node);
    }
  }

  replaceChildren(...nodes: FakeElement[]): void {
    this.children.length = 0;
    this.append(...nodes);
  }

  setAttribute(name: string, value: string): void {
    this.attributes.set(name, value);
  }

  getAttribute(name: string): string | null {
    return this.attributes.get(name) ?? null;
  }

  addEventListener(type: string, listener: () => void): void {
    this.listeners.set(type, [...(this.listeners.get(type) ?? []), listener]);
  }

  dispatch(type: string): void {
    for (const listener of this.listeners.get(type) ?? []) listener();
  }
}

(globalThis as { document?: unknown }).document = {
  createElement: (tag: string) => new FakeElement(tag),
};

const fake = (node: unknown): FakeElement => node as FakeElement;

test("devButtonsHidden hides buttons when developerMode is false and shows them when true", () => {
  assert.equal(devButtonsHidden(false), true);
  assert.equal(devButtonsHidden(true), false);
});

test("buildDevSection renders toggle with hint and hides action buttons until developerMode is on", () => {
  const toggled: boolean[] = [];
  const dev = buildDevSection(
    (checked) => toggled.push(checked),
    null,
    (key) => `t:${key}`,
  );

  assert.equal(dev.titleEl.textContent, "t:settings.dev_title");
  assert.equal(dev.toggle.titleEl.textContent, "t:settings.dev_mode");
  assert.equal(dev.toggle.hintEl?.textContent, "t:settings.dev_mode_hint");
  assert.equal(fake(dev.toggle.input).getAttribute("aria-describedby"), "settings-dev-mode-hint");
  assert.equal(dev.openBtn.textContent, "t:settings.dev_open");
  assert.equal(dev.jumpBtn.textContent, "t:settings.dev_jump");

  // Default off: buttons hidden, status hidden.
  assert.equal(dev.toggle.input.checked, false);
  assert.equal(dev.buttonsRow.hidden, true);
  assert.equal(dev.openBtn.hidden, true);
  assert.equal(dev.jumpBtn.hidden, true);
  assert.equal(dev.statusEl.hidden, true);
  assert.equal(dev.statusEl.textContent, "");

  // Toggle on via user change event: buttons appear at once and callback fires.
  fake(dev.toggle.input).checked = true;
  fake(dev.toggle.input).dispatch("change");
  assert.deepEqual(toggled, [true]);
  assert.equal(fake(dev.toggle.wrapper).classList.contains("is-selected"), true);
  assert.equal(dev.buttonsRow.hidden, false);
  assert.equal(dev.openBtn.hidden, false);
  assert.equal(dev.jumpBtn.hidden, false);

  // Sync off from store: buttons hide again.
  dev.syncState(false);
  assert.equal(dev.toggle.input.checked, false);
  assert.equal(fake(dev.toggle.wrapper).classList.contains("is-selected"), false);
  assert.equal(dev.buttonsRow.hidden, true);
  assert.equal(dev.openBtn.hidden, true);
  assert.equal(dev.jumpBtn.hidden, true);
  dev.destroy();
});

test("developer buttons invoke open_under_the_hood and mate_jump", async () => {
  const calls: string[] = [];
  const dev = buildDevSection(
    () => {},
    async (cmd) => {
      calls.push(cmd);
      return null;
    },
    (key) => `t:${key}`,
  );
  dev.syncState(true);

  fake(dev.openBtn).dispatch("click");
  fake(dev.jumpBtn).dispatch("click");
  await Promise.resolve();

  assert.deepEqual(calls, [OPEN_UNDER_THE_HOOD_CMD, MATE_JUMP_CMD]);
  assert.equal(dev.statusEl.hidden, true);
  assert.equal(dev.statusEl.textContent, "");
  dev.destroy();
});

test("failed developer button shows settings.dev_failed for 4 s and never throws", async (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });

  const dev = buildDevSection(
    () => {},
    async (cmd) => {
      throw new Error(`${cmd} failed`);
    },
    (key) => `t:${key}`,
  );
  dev.syncState(true);

  const ok = await dev.runCommand(OPEN_UNDER_THE_HOOD_CMD);
  assert.equal(ok, false);
  assert.equal(dev.statusEl.hidden, false);
  assert.equal(dev.statusEl.textContent, "t:settings.dev_failed");

  // Still visible just before 4 s.
  t.mock.timers.tick(DEV_STATUS_DURATION_MS - 1);
  assert.equal(dev.statusEl.hidden, false);
  assert.equal(dev.statusEl.textContent, "t:settings.dev_failed");

  // Cleared at 4 s.
  t.mock.timers.tick(1);
  assert.equal(dev.statusEl.hidden, true);
  assert.equal(dev.statusEl.textContent, "");

  // Second button failure also shows status and clears when developerMode is turned off.
  fake(dev.jumpBtn).dispatch("click");
  await Promise.resolve();
  assert.equal(dev.statusEl.hidden, false);
  assert.equal(dev.statusEl.textContent, "t:settings.dev_failed");

  dev.syncState(false);
  assert.equal(dev.statusEl.hidden, true);
  assert.equal(dev.statusEl.textContent, "");
  dev.destroy();
});
