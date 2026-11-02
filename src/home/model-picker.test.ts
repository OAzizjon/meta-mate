// Unit tests for the Home model picker (src/home/model-picker.ts) and Settings third model
// radio (src/settings/choices-view.ts).
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

import { mountHome, setHomeI18n } from "../home.ts";
import { buildModelGroup } from "../settings/choices-view.ts";
import { DEFAULT_SETTINGS, MODEL_CHOICES } from "../settings/model.ts";
import type { ModelChoice, Settings } from "../settings/model.ts";
import {
  HOME_MODEL_CARDS,
  HOME_MODEL_CHOICES,
  HOME_MODEL_SAVED_MS,
  HOME_MODEL_SPECS,
  arrowModelChoice,
  isSelectKey,
  modelSavedText,
  mountModelPicker,
  nextModelChoice,
  prevModelChoice,
  stepModelChoice,
} from "./model-picker.ts";

type Strings = Record<string, string>;
const LOCALES: Record<string, Strings> = {
  en: JSON.parse(readFileSync(new URL("../../locales/en.json", import.meta.url), "utf8")) as Strings,
  ru: JSON.parse(readFileSync(new URL("../../locales/ru.json", import.meta.url), "utf8")) as Strings,
};

const REQUIRED_KEYS = [
  "home.model_title",
  "home.model_intro",
  "home.model_quick",
  "home.model_quick_hint",
  "home.model_thorough",
  "home.model_thorough_hint",
  "home.model_open",
  "home.model_open_hint",
  "home.model_saved",
  "home.model_failed",
  "settings.model_open",
  "settings.model_open_hint",
] as const;

test("choice order and specs cover quick, thorough and open in order", () => {
  assert.deepEqual([...MODEL_CHOICES], ["quick", "thorough", "open"]);
  assert.deepEqual([...HOME_MODEL_CHOICES], ["quick", "thorough", "open"]);
  assert.deepEqual(
    HOME_MODEL_CARDS.map((card) => card.value),
    ["quick", "thorough", "open"],
  );
  for (const choice of MODEL_CHOICES) {
    assert.equal(HOME_MODEL_SPECS[choice].value, choice);
  }
  assert.equal(HOME_MODEL_SAVED_MS, 4_000);
});

test("arrow keys step forward and backward across the three choices and wrap around", () => {
  assert.equal(nextModelChoice("quick"), "thorough");
  assert.equal(nextModelChoice("thorough"), "open");
  assert.equal(nextModelChoice("open"), "quick");

  assert.equal(prevModelChoice("quick"), "open");
  assert.equal(prevModelChoice("open"), "thorough");
  assert.equal(prevModelChoice("thorough"), "quick");

  assert.equal(stepModelChoice("quick", 1), "thorough");
  assert.equal(stepModelChoice("quick", -1), "open");

  assert.equal(arrowModelChoice("quick", "ArrowRight"), "thorough");
  assert.equal(arrowModelChoice("thorough", "ArrowDown"), "open");
  assert.equal(arrowModelChoice("open", "ArrowRight"), "quick");
  assert.equal(arrowModelChoice("quick", "ArrowLeft"), "open");
  assert.equal(arrowModelChoice("open", "ArrowUp"), "thorough");
  assert.equal(arrowModelChoice("quick", "Enter"), null);
  assert.equal(arrowModelChoice("quick", " "), null);
  assert.equal(arrowModelChoice("quick", "Tab"), null);

  assert.equal(isSelectKey("Enter"), true);
  assert.equal(isSelectKey(" "), true);
  assert.equal(isSelectKey("ArrowRight"), false);
});

test("modelSavedText formats the confirmation line with the chosen model name", () => {
  const en = (key: string): string => LOCALES.en[key] ?? key;
  const ru = (key: string): string => LOCALES.ru[key] ?? key;

  assert.equal(modelSavedText("quick", en), "Mate now thinks with Muse Spark 1.3.");
  assert.equal(modelSavedText("thorough", en), "Mate now thinks with Muse Wonder 1.0.");
  assert.equal(modelSavedText("open", en), "Mate now thinks with Muse Glimmer 30B.");

  assert.ok(modelSavedText("thorough", ru).includes("Muse Wonder 1.0"));
  assert.ok(modelSavedText("open", ru).includes("Muse Glimmer 30B"));

  // Safe replacement when a translated string contains `$&`.
  assert.equal(
    modelSavedText("quick", (key) => (key === "home.model_saved" ? "Model: {model}" : "$& Spark")),
    "Model: $& Spark",
  );
});

test("all model picker and settings locale keys exist in both English and Russian", () => {
  for (const key of REQUIRED_KEYS) {
    for (const [code, strings] of Object.entries(LOCALES)) {
      assert.ok(strings[key]?.trim(), `${code}: ${key}`);
    }
  }
  assert.equal(LOCALES.en["home.model_title"], "Mate's brain");
  assert.equal(LOCALES.en["home.model_saved"], "Mate now thinks with {model}.");
  assert.ok(LOCALES.ru["home.model_saved"].includes("{model}"));
  assert.ok(LOCALES.en["home.model_open_hint"].includes("Muse Spark"));
  assert.ok(LOCALES.ru["home.model_open_hint"].includes("Muse Spark"));
});

class FakeClassList {
  readonly tokens = new Set<string>();
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
    const next = force ?? !this.tokens.has(token);
    if (next) this.tokens.add(token);
    else this.tokens.delete(token);
    return next;
  }
}

interface FakeEvent {
  readonly target: unknown;
  readonly key?: string;
  defaultPrevented: boolean;
  preventDefault(): void;
}

type FakeListener = (event: FakeEvent) => void;

let focused: FakeElement | null = null;

class FakeElement {
  readonly tagName: string;
  readonly classList = new FakeClassList();
  readonly attributes = new Map<string, string>();
  readonly listeners = new Map<string, FakeListener[]>();
  childNodes: Array<FakeElement | string> = [];
  id = "";
  type = "";
  name = "";
  htmlFor = "";
  value = "";
  checked = false;
  disabled = false;
  hidden = false;
  open = false;

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

  get children(): FakeElement[] {
    return this.childNodes.filter((node): node is FakeElement => node instanceof FakeElement);
  }

  get textContent(): string {
    return this.childNodes.map((node) => (typeof node === "string" ? node : node.textContent)).join("");
  }

  set textContent(value: string) {
    this.childNodes = [value];
  }

  append(...nodes: Array<FakeElement | string>): void {
    this.childNodes.push(...nodes);
  }

  appendChild(node: FakeElement): FakeElement {
    this.childNodes.push(node);
    return node;
  }

  replaceChildren(...nodes: Array<FakeElement | string>): void {
    this.childNodes = [...nodes];
  }

  setAttribute(name: string, value: string): void {
    this.attributes.set(name, value);
  }

  addEventListener(type: string, listener: FakeListener): void {
    this.listeners.set(type, [...(this.listeners.get(type) ?? []), listener]);
  }

  removeEventListener(type: string, listener: FakeListener): void {
    this.listeners.set(type, (this.listeners.get(type) ?? []).filter((other) => other !== listener));
  }

  focus(): void {
    focused = this;
  }

  dispatch(type: string, init: { key?: string; target?: unknown } = {}): FakeEvent {
    const event: FakeEvent = {
      target: init.target ?? this,
      key: init.key,
      defaultPrevented: false,
      preventDefault() {
        this.defaultPrevented = true;
      },
    };
    for (const listener of this.listeners.get(type) ?? []) listener(event);
    return event;
  }
}

function descendants(node: FakeElement): FakeElement[] {
  return node.children.flatMap((child) => [child, ...descendants(child)]);
}

function byId(root: FakeElement, id: string): FakeElement {
  const found = descendants(root).find((node) => node.id === id);
  assert.ok(found, id);
  return found;
}

function withFakeDom(): () => void {
  const doc = new FakeElement("#document");
  const globals = globalThis as { document?: unknown; window?: unknown };
  globals.document = {
    documentElement: new FakeElement("html"),
    createElement: (tag: string) => new FakeElement(tag),
    createElementNS: (_ns: string, tag: string) => new FakeElement(tag),
    addEventListener: (type: string, listener: FakeListener) => doc.addEventListener(type, listener),
    removeEventListener: (type: string, listener: FakeListener) => doc.removeEventListener(type, listener),
  };
  globals.window = { addEventListener() {}, removeEventListener() {} };
  focused = null;
  return () => {
    delete globals.document;
    delete globals.window;
  };
}

const tick = (ms = 0): Promise<void> => new Promise((resolve) => setTimeout(resolve, ms));

test("mountModelPicker renders three comic radio cards, moves on arrows, selects on Enter/Space/click and shows confirmation", async () => {
  const cleanup = withFakeDom();
  try {
    const selected: ModelChoice[] = [];
    let shouldFail = false;
    const picker = mountModelPicker({
      t: (key) => LOCALES.en[key] ?? key,
      initialModel: "quick",
      savedMs: 30,
      async onSelect(model) {
        if (shouldFail) {
          throw new Error("save failed");
        }
        selected.push(model);
      },
    });

    const root = picker.element as unknown as FakeElement;
    assert.equal(byId(root, "home-model-title").textContent, "Mate's brain");
    const group = descendants(root).find((node) => node.attributes.get("role") === "radiogroup");
    assert.ok(group);
    assert.equal(group.children.length, 3);

    const quick = byId(root, "home-model-quick");
    const thorough = byId(root, "home-model-thorough");
    const open = byId(root, "home-model-open");

    for (const card of [quick, thorough, open]) {
      assert.equal(card.attributes.get("role"), "radio");
      assert.ok(descendants(card).some((node) => node.tagName === "svg"));
    }

    assert.equal(quick.attributes.get("aria-checked"), "true");
    assert.equal(thorough.attributes.get("aria-checked"), "false");
    assert.equal(open.attributes.get("aria-checked"), "false");

    // Arrow keys move focus and roving tabindex without saving yet.
    const moveRight = quick.dispatch("keydown", { key: "ArrowRight" });
    assert.equal(moveRight.defaultPrevented, true);
    assert.equal(focused, thorough);
    assert.equal(thorough.attributes.get("tabindex"), "0");
    assert.equal(quick.attributes.get("tabindex"), "-1");
    assert.deepEqual(selected, []);

    // Enter selects the focused choice and shows the 4 s confirmation line.
    const enter = thorough.dispatch("keydown", { key: "Enter" });
    assert.equal(enter.defaultPrevented, true);
    await tick();
    assert.deepEqual(selected, ["thorough"]);
    assert.equal(thorough.attributes.get("aria-checked"), "true");
    assert.equal(quick.attributes.get("aria-checked"), "false");

    const status = byId(root, "home-model-status");
    assert.equal(status.hidden, false);
    assert.equal(status.textContent, "Mate now thinks with Muse Wonder 1.0.");

    // Space on open selects open.
    const space = open.dispatch("keydown", { key: " " });
    assert.equal(space.defaultPrevented, true);
    await tick();
    assert.deepEqual(selected, ["thorough", "open"]);
    assert.equal(open.attributes.get("aria-checked"), "true");
    assert.equal(status.textContent, "Mate now thinks with Muse Glimmer 30B.");

    // Confirmation hides after savedMs.
    await tick(45);
    assert.equal(status.hidden, true);
    assert.equal(status.textContent, "");

    // A failed save shows home.model_failed with is-error.
    shouldFail = true;
    quick.dispatch("click");
    await tick();
    assert.equal(status.hidden, false);
    assert.equal(status.classList.contains("is-error"), true);
    assert.equal(status.textContent, LOCALES.en["home.model_failed"]);

    picker.destroy();
  } finally {
    cleanup();
  }
});

test("Home places Mate's brain after the greeting and before Try it, saves via settings and syncs with Settings", async () => {
  const cleanup = withFakeDom();
  setHomeI18n((key) => LOCALES.en[key] ?? key, () => "en");
  let emitSettingsChanged: ((raw: unknown) => void) | null = null;
  const savedPayloads: Settings[] = [];
  const container = new FakeElement("main");
  const home = mountHome(container as unknown as HTMLElement, {
    invoke: async (cmd, args) => {
      if (cmd === "load_settings") {
        return { ...DEFAULT_SETTINGS, model: "thorough", profileAsked: true };
      }
      if (cmd === "save_settings") {
        savedPayloads.push((args as { settings: Settings }).settings);
      }
      return null;
    },
    subscribe: async (onChanged) => {
      emitSettingsChanged = onChanged;
      return () => {};
    },
  });

  try {
    await tick();
    const content = container.children[0];
    const classes = content.children.map((node) => node.className);
    const heroIdx = classes.findIndex((c) => c.includes("home-hero"));
    const modelIdx = classes.findIndex((c) => c.includes("home-model"));
    const tryIdx = classes.findIndex((c) => c.includes("home-try"));
    assert.ok(heroIdx >= 0 && modelIdx === heroIdx + 1 && tryIdx === modelIdx + 1, classes.join(" | "));

    assert.equal(byId(container, "home-model-thorough").attributes.get("aria-checked"), "true");

    // Selecting "open" in Home saves immediately via save_settings.
    byId(container, "home-model-open").dispatch("click");
    await tick();
    assert.equal(savedPayloads.length, 1);
    assert.equal(savedPayloads[0].model, "open");
    assert.equal(byId(container, "home-model-open").attributes.get("aria-checked"), "true");
    assert.equal(byId(container, "home-model-status").textContent, "Mate now thinks with Muse Glimmer 30B.");

    // External settings-changed from Settings updates the selected radio in Home.
    assert.notEqual(emitSettingsChanged, null);
    emitSettingsChanged!({ ...DEFAULT_SETTINGS, model: "quick", profileAsked: true });
    assert.equal(byId(container, "home-model-quick").attributes.get("aria-checked"), "true");
    assert.equal(byId(container, "home-model-open").attributes.get("aria-checked"), "false");
  } finally {
    home.destroy();
    cleanup();
  }
});

test("Settings buildModelGroup includes the third radio choice open with its label and hint keys", () => {
  const cleanup = withFakeDom();
  try {
    const picked: ModelChoice[] = [];
    const group = buildModelGroup((model) => picked.push(model));
    assert.deepEqual(
      group.items.map((item) => item.value),
      ["quick", "thorough", "open"],
    );
    const openItem = group.items[2];
    assert.equal(openItem.input.id, "settings-model-open");
    assert.equal(openItem.input.value, "open");
    assert.equal(openItem.labelKey, "settings.model_open");
    assert.equal(openItem.hintKey, "settings.model_open_hint");

    openItem.input.checked = true;
    (openItem.input as unknown as FakeElement).dispatch("change");
    assert.deepEqual(picked, ["open"]);
  } finally {
    cleanup();
  }
});
