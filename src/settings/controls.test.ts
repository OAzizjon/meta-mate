// Run with `npm test` (Node's own test runner; Node 24 strips the types).
// The Settings window's radio groups (src/settings/controls.ts, src/settings/choices-view.ts)
// on a minimal fake document, since Node has no DOM: short choices are segments on one row,
// the model choice keeps its cards, and the radio semantics stay the same.
import assert from "node:assert/strict";
import { test } from "node:test";

import {
  buildLanguageGroup,
  buildModelGroup,
  buildPttGroup,
  buildPttKeyGroup,
  buildSizeGroup,
  buildSoundGroup,
} from "./choices-view.ts";
import { buildCheckbox, syncCheckbox, syncRadioSelection } from "./controls.ts";
import type { RadioGroupRefs } from "./controls.ts";

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

/** Just enough of an element for the builders. */
class FakeElement {
  readonly tagName: string;
  readonly classList = new FakeClassList();
  readonly attributes = new Map<string, string>();
  readonly listeners = new Map<string, Array<() => void>>();
  readonly children: FakeElement[] = [];
  parentElement: FakeElement | null = null;
  id = "";
  type = "";
  name = "";
  value = "";
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

/** The group's parts as the fake elements they are. */
function parts(group: RadioGroupRefs<string>) {
  const [legend, list, hints] = fake(group.fieldset).children;
  return {
    fieldset: fake(group.fieldset),
    legend,
    list,
    hints,
    items: group.items.map((item) => ({
      wrapper: fake(item.wrapper),
      input: fake(item.input),
      hint: item.hintEl ? fake(item.hintEl) : undefined,
    })),
  };
}

const noop = (): void => {};

test("short choices are segments on one row; only the model choice keeps a card per option", () => {
  const segmented: Array<readonly [string, RadioGroupRefs<string>, number]> = [
    ["language", buildLanguageGroup(noop), 3],
    ["pttKey", buildPttKeyGroup(noop), 6],
    ["ptt", buildPttGroup(noop), 2],
    ["sound", buildSoundGroup(noop), 2],
    ["robotSize", buildSizeGroup(noop), 4],
  ];
  for (const [name, group, count] of segmented) {
    const { fieldset, legend, list, items } = parts(group);
    assert.equal(group.layout, "segments", name);
    // Still a radio group: a fieldset named by its legend, one name, each radio in its label.
    assert.equal(fieldset.tagName, "fieldset", name);
    assert.equal(legend.tagName, "legend", name);
    assert.equal(list.className, "settings-segments", name);
    assert.equal(items.length, count, name);
    for (const { wrapper, input } of items) {
      assert.equal(wrapper.tagName, "label", name);
      assert.equal(wrapper.className, "settings-segment", name);
      assert.equal(wrapper.htmlFor, input.id, name);
      assert.equal(input.type, "radio", name);
      assert.equal(input.name, name);
      assert.equal(input.parentElement, wrapper, name);
    }
  }

  const model = parts(buildModelGroup(noop));
  assert.equal(model.list.className, "settings-options");
  assert.equal(model.hints, undefined);
  for (const { wrapper, hint } of model.items) {
    assert.equal(wrapper.className, "settings-radio-option");
    // The description stays inside its card.
    assert.equal(hint?.parentElement?.parentElement, wrapper);
  }
});

test("a segment's hint sits under the row, describes its radio, and shows for the chosen one", () => {
  const ptt = buildPttGroup(noop);
  const { fieldset, list, hints, items } = parts(ptt);
  assert.equal(hints?.className, "settings-segment-hints");
  assert.equal(fieldset.children.length, 3);
  assert.equal(fieldset.children[1], list);
  assert.equal(fieldset.children[2], hints);
  for (const { input, hint } of items) {
    assert.equal(hint?.parentElement, hints);
    assert.equal(input.getAttribute("aria-describedby"), hint?.id);
  }

  syncRadioSelection(ptt, "toggle");
  assert.deepEqual(items.map(({ input }) => input.checked), [false, true]);
  assert.deepEqual(items.map(({ wrapper }) => wrapper.classList.contains("is-selected")), [false, true]);
  assert.deepEqual(items.map(({ hint }) => hint?.hidden), [true, false]);

  // Segments without hints get no hint line.
  assert.equal(parts(buildSoundGroup(noop)).hints, undefined);

  // Cards keep every description in view.
  const model = buildModelGroup(noop);
  syncRadioSelection(model, "quick");
  assert.deepEqual(parts(model).items.map(({ hint }) => hint?.hidden), [false, false, false]);
});

test("choosing a segment marks it and shows its hint at once, then passes the value on", () => {
  const chosen: string[] = [];
  const ptt = buildPttGroup((value) => {
    chosen.push(value);
  });
  syncRadioSelection(ptt, "hold");
  const [hold, toggle] = parts(ptt).items;

  // The browser checks the radio (and unchecks the other), then fires `change`.
  hold.input.checked = false;
  toggle.input.checked = true;
  toggle.input.dispatch("change");

  assert.deepEqual(chosen, ["toggle"]);
  assert.equal(toggle.wrapper.classList.contains("is-selected"), true);
  assert.equal(hold.wrapper.classList.contains("is-selected"), false);
  assert.deepEqual([hold.hint?.hidden, toggle.hint?.hidden], [true, false]);

  // A `change` on a radio that is not checked chooses nothing.
  hold.input.dispatch("change");
  assert.deepEqual(chosen, ["toggle"]);
});

test("model cards name radios by the title and describe them by the hint", () => {
  const model = buildModelGroup(noop);
  for (const { input, titleEl, hintEl } of model.items) {
    assert.ok(titleEl.id);
    assert.equal(input.getAttribute("aria-labelledby"), titleEl.id);
    assert.equal(input.getAttribute("aria-describedby"), hintEl?.id);
    assert.notEqual(titleEl.id, hintEl?.id);
  }
});

test("buildCheckbox wires an optional hint and syncCheckbox updates checked and selection class", () => {
  const plain = buildCheckbox("settings-reduce-motion", "settings.reduce_motion", noop);
  assert.equal(plain.hintEl, undefined);
  assert.equal(plain.hintKey, undefined);
  assert.equal(fake(plain.input).getAttribute("aria-describedby"), null);

  const toggled: boolean[] = [];
  const withHint = buildCheckbox(
    "settings-dev-mode",
    "settings.dev_mode",
    (checked) => toggled.push(checked),
    "settings.dev_mode_hint",
  );
  assert.equal(withHint.hintKey, "settings.dev_mode_hint");
  assert.equal(withHint.hintEl?.id, "settings-dev-mode-hint");
  assert.equal(fake(withHint.input).getAttribute("aria-describedby"), "settings-dev-mode-hint");

  syncCheckbox(withHint, true);
  assert.equal(withHint.input.checked, true);
  assert.equal(fake(withHint.wrapper).classList.contains("is-selected"), true);

  syncCheckbox(withHint, false);
  assert.equal(withHint.input.checked, false);
  assert.equal(fake(withHint.wrapper).classList.contains("is-selected"), false);

  fake(withHint.input).checked = true;
  fake(withHint.input).dispatch("change");
  assert.deepEqual(toggled, [true]);
});
