// Run with `npm test` (Node's own test runner; Node 24 strips the types).
// Unit tests for the Settings window's Updates section (src/settings/update-view.ts).
import assert from "node:assert/strict";
import { test } from "node:test";

import { interpolate } from "../i18n/interpolate.ts";
import type { LocaleCode, LocaleKey, TemplateVars } from "../i18n/index.ts";
import {
  UPDATE_INSTALL_CMD,
  UPDATE_CHECK_CMD,
  buildUpdateSection,
  isValidSemver,
  parseUpdateStatus,
  resolveUpdateNote,
} from "./update-view.ts";

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
  textContent = "";
  hidden = false;
  disabled = false;

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

const TEMPLATES: Readonly<Record<string, string>> = {
  "settings.update_title": "Updates",
  "settings.update_current": "MetaMate {version}",
  "settings.update_check": "Check for updates",
  "settings.update_install": "Update to {version}",
  "settings.update_latest": "You have the latest version.",
  "settings.update_failed": "Could not check or install the update. Please try again.",
};

function fakeTranslate(key: LocaleKey, vars?: TemplateVars): string {
  const template = TEMPLATES[key] ?? key;
  return interpolate(template, "Mate", vars);
}

test("parseUpdateStatus and isValidSemver validate wire shapes strictly", () => {
  assert.equal(isValidSemver("0.2.0"), true);
  assert.equal(isValidSemver("01.2.0"), false);
  assert.equal(isValidSemver("0.2"), false);
  assert.equal(isValidSemver("0.2.0-beta"), false);

  assert.deepEqual(
    parseUpdateStatus({ current: "0.2.0", available: null, notes: null }),
    { current: "0.2.0", available: null, notes: null },
  );
  assert.deepEqual(
    parseUpdateStatus({
      current: "0.2.0",
      available: "0.3.0",
      notes: { en: "Bug fixes", ru: "Исправления" },
    }),
    {
      current: "0.2.0",
      available: "0.3.0",
      notes: { en: "Bug fixes", ru: "Исправления" },
    },
  );

  for (const invalid of [
    null,
    undefined,
    "0.2.0",
    [],
    {},
    { current: "bad", available: null, notes: null },
    { current: "0.2.0", available: "bad", notes: null },
    { current: "0.2.0", available: "0.3.0", notes: { en: "a".repeat(201) } },
    { current: "0.2.0", available: "0.3.0", notes: { de: "Hallo" } },
    { current: "0.2.0", available: null, notes: null, extra: 1 },
  ]) {
    assert.equal(parseUpdateStatus(invalid), null);
  }
});

test("resolveUpdateNote selects active locale with fallback", () => {
  const notes = { en: "Short line", ru: "Короткая строка" };
  assert.equal(resolveUpdateNote(notes, "en"), "Short line");
  assert.equal(resolveUpdateNote(notes, "ru"), "Короткая строка");
  assert.equal(resolveUpdateNote({ en: "English only" }, "ru"), "English only");
  assert.equal(resolveUpdateNote(null, "en"), "");
});

test("buildUpdateSection checks for updates and shows latest or switches to install", async () => {
  const calls: string[] = [];
  let nextReply: unknown = { current: "0.2.0", available: null, notes: null };
  let locale: LocaleCode = "en";

  const view = buildUpdateSection(
    async (cmd) => {
      calls.push(cmd);
      return nextReply;
    },
    fakeTranslate,
    () => locale,
  );

  assert.equal(view.titleEl.textContent, "Updates");
  assert.equal(view.currentEl.textContent, "MetaMate 0.2.0");
  assert.equal(view.actionBtn.textContent, "Check for updates");
  assert.equal(view.statusEl.hidden, true);

  // Clicking "Check for updates" when up to date shows settings.update_latest.
  fake(view.actionBtn).dispatch("click");
  await Promise.resolve();
  assert.deepEqual(calls, [UPDATE_CHECK_CMD]);
  assert.equal(view.statusEl.hidden, false);
  assert.equal(view.statusEl.textContent, "You have the latest version.");
  assert.equal(fake(view.statusEl).classList.contains("is-error"), false);

  // When a newer version is returned, the button switches to "Update to {version}".
  nextReply = {
    current: "0.2.0",
    available: "0.3.0",
    notes: { en: "Faster startup", ru: "Быстрый запуск" },
  };
  await view.runAction();
  assert.equal(view.actionBtn.textContent, "Update to 0.3.0");
  assert.equal(view.notesEl.hidden, false);
  assert.equal(view.notesEl.textContent, "Faster startup");
  assert.equal(view.statusEl.hidden, true);

  // Switching locale updates the note text.
  locale = "ru";
  view.syncText(fakeTranslate, "ru");
  assert.equal(view.notesEl.textContent, "Быстрый запуск");

  // Clicking "Update to 0.3.0" invokes update_install.
  await view.runAction();
  assert.equal(calls.at(-1), UPDATE_INSTALL_CMD);
  view.destroy();
});

test("buildUpdateSection shows settings.update_failed on status or install error", async () => {
  const view = buildUpdateSection(
    async () => {
      throw new Error("network error");
    },
    fakeTranslate,
  );

  const ok = await view.runAction();
  assert.equal(ok, false);
  assert.equal(view.statusEl.hidden, false);
  assert.equal(view.statusEl.textContent, "Could not check or install the update. Please try again.");
  assert.equal(fake(view.statusEl).classList.contains("is-error"), true);
  view.destroy();
});
