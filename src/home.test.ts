// Run with `npm test` (Node's own test runner; Node 24 strips the types).
// Unit tests for the Home window's logic (src/home.ts, WP89a): the "Try it" runner, the
// Settings button, the "Show this window when MetaMate starts" checkbox's settings, and the
// talk card and footer as built on a minimal fake document.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { inspect } from "node:util";

import {
  HOME_NEVER_KEYS,
  HOME_TRY_COMMAND,
  HOME_TRY_HINTS,
  HOME_TRY_KEYS,
  OPEN_SETTINGS_COMMAND,
  createHomeTryRunner,
  isHomeTryKey,
  mountHome,
  openSettingsWindow,
  setHomeI18n,
  startHomeSettings,
  tryStatusKey,
} from "./home.ts";
import type { HomeInvoke } from "./home.ts";
import { GUIDE_CARDS, isGuideShortcut } from "./home/guide.ts";
import { greetingText, shouldAskProfile } from "./home/profile.ts";
import { DEFAULT_SETTINGS, PTT_KEYS } from "./settings/model.ts";
import type { Settings } from "./settings/model.ts";

// The locale files read directly: src/i18n imports them as JSON modules, which Node's test
// runner cannot load.
type Strings = Record<string, string>;
const LOCALES: Record<string, Strings> = {
  en: JSON.parse(readFileSync(new URL("../locales/en.json", import.meta.url), "utf8")) as Strings,
  ru: JSON.parse(readFileSync(new URL("../locales/ru.json", import.meta.url), "utf8")) as Strings,
};
const HOME_SOURCE = readFileSync(new URL("./home.ts", import.meta.url), "utf8");
const HOME_PART_SOURCES = ["./home/profile.ts", "./home/guide.ts", "./home/lessons-view.ts"].map((path) =>
  readFileSync(new URL(path, import.meta.url), "utf8"),
);

interface Call {
  readonly cmd: string;
  readonly args?: Readonly<Record<string, unknown>>;
}

function deferred<T>(): { promise: Promise<T>; resolve: (value: T) => void; reject: (error: unknown) => void } {
  let resolve: (value: T) => void = () => {};
  let reject: (error: unknown) => void = () => {};
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

const SAVED: Settings = {
  ...DEFAULT_SETTINGS,
  model: "thorough",
  voiceSpeed: 1.2,
  robotSize: "XL",
  startWithWindows: true,
  showHomeAtStart: true,
};

test("the commands and questions match the Home window contract", () => {
  assert.equal(HOME_TRY_COMMAND, "home_try");
  assert.equal(OPEN_SETTINGS_COMMAND, "open_settings_window");
  assert.deepEqual([...HOME_TRY_KEYS], ["tray.try_search", "tray.try_safe", "tray.try_bigger"]);
  assert.deepEqual(Object.keys(HOME_TRY_HINTS).sort(), [...HOME_TRY_KEYS].sort());
});

test("every string the Home page shows is in both locales", () => {
  const keys = new Set<string>([
    ...HOME_TRY_KEYS,
    ...Object.values(HOME_TRY_HINTS),
    ...HOME_NEVER_KEYS,
    tryStatusKey("asked") ?? "",
    tryStatusKey("failed") ?? "",
    // The tray's "Open MetaMate" item (Rust).
    "tray.open_home",
  ]);
  for (const match of HOME_SOURCE.matchAll(/\bt\("([\w.]+)"\)/g)) {
    keys.add(match[1]);
  }
  assert.ok(keys.size > 20, `only ${keys.size} keys found`);
  for (const key of keys) {
    for (const [code, strings] of Object.entries(LOCALES)) {
      assert.ok(strings[key]?.trim(), `${code}: ${key}`);
    }
  }
  assert.ok(LOCALES.en["home.cards_folder"].includes("Pictures\\MetaMate"));
  assert.ok(LOCALES.ru["home.cards_folder"].includes("\\MetaMate"));
});

test("isHomeTryKey accepts only the three questions", () => {
  for (const key of HOME_TRY_KEYS) {
    assert.equal(isHomeTryKey(key), true, key);
  }
  for (const bad of ["tray.try_title", "home.try_button", "try_search", "", " tray.try_safe", null, undefined, 1, {}, [HOME_TRY_KEYS[0]]]) {
    assert.equal(isHomeTryKey(bad), false, inspect(bad));
  }
});

test("the try runner asks home_try with the key and reports asked or failed", async () => {
  const calls: Call[] = [];
  const runner = createHomeTryRunner(async (cmd, args) => {
    calls.push({ cmd, args });
    return null;
  });
  for (const key of HOME_TRY_KEYS) {
    assert.equal(await runner.run(key), "asked", key);
  }
  assert.deepEqual(
    calls,
    HOME_TRY_KEYS.map((key) => ({ cmd: "home_try", args: { key } })),
  );

  const failing = createHomeTryRunner(async () => {
    throw new Error("no app window remembered");
  });
  assert.equal(await failing.run("tray.try_safe"), "failed");
  assert.equal(failing.isBusy(), false);
});

test("the try runner never asks for an unknown key or outside Tauri", async () => {
  const calls: Call[] = [];
  const runner = createHomeTryRunner(async (cmd, args) => {
    calls.push({ cmd, args });
    return null;
  });
  for (const bad of ["tray.quit", "home_try", "", null, 7, { key: "tray.try_search" }]) {
    assert.equal(await runner.run(bad), "failed", inspect(bad));
  }
  assert.deepEqual(calls, []);

  assert.equal(await createHomeTryRunner(null).run("tray.try_search"), "failed");
});

test("the try runner asks one question at a time: a double click asks once", async () => {
  const pending = deferred<unknown>();
  const calls: Call[] = [];
  const runner = createHomeTryRunner((cmd, args) => {
    calls.push({ cmd, args });
    return pending.promise;
  });

  const first = runner.run("tray.try_search");
  assert.equal(runner.isBusy(), true);
  assert.equal(await runner.run("tray.try_bigger"), "busy");
  assert.equal(calls.length, 1);

  pending.resolve(null);
  assert.equal(await first, "asked");
  assert.equal(runner.isBusy(), false);

  const next = deferred<unknown>();
  const again = createHomeTryRunner(() => next.promise);
  const asked = again.run("tray.try_safe");
  next.reject(new Error("busy elsewhere"));
  assert.equal(await asked, "failed");
  assert.equal(again.isBusy(), false);
});

test("tryStatusKey says what happened and leaves the line alone while busy", () => {
  assert.equal(tryStatusKey("asked"), "home.try_asked");
  assert.equal(tryStatusKey("failed"), "home.try_failed");
  assert.equal(tryStatusKey("busy"), null);
});

test("openSettingsWindow invokes open_settings_window and never throws", async () => {
  const calls: Call[] = [];
  assert.equal(
    await openSettingsWindow(async (cmd, args) => {
      calls.push({ cmd, args });
      return null;
    }),
    true,
  );
  assert.deepEqual(calls, [{ cmd: "open_settings_window", args: undefined }]);

  assert.equal(
    await openSettingsWindow(async () => {
      throw new Error("no window");
    }),
    false,
  );
  assert.equal(await openSettingsWindow(null), false);
});

test("the checkbox saves showHomeAtStart over the loaded settings, keeping the others", async () => {
  const calls: Call[] = [];
  const invoke: HomeInvoke = async (cmd, args) => {
    calls.push({ cmd, args });
    return cmd === "load_settings" ? { ...SAVED } : null;
  };
  const changes: Array<readonly [Settings, boolean]> = [];
  const home = startHomeSettings(invoke, null, (settings, canSave) => {
    changes.push([settings, canSave]);
  });
  assert.equal(home.canSave(), false);

  assert.deepEqual(await home.ready, SAVED);
  assert.equal(home.canSave(), true);
  assert.deepEqual(changes, [[SAVED, true]]);

  const saved = await home.setShowAtStart(false);
  assert.deepEqual(saved, { ...SAVED, showHomeAtStart: false });
  assert.deepEqual(calls, [
    { cmd: "load_settings", args: undefined },
    { cmd: "save_settings", args: { settings: { ...SAVED, showHomeAtStart: false } } },
  ]);
  assert.deepEqual(changes.at(-1), [{ ...SAVED, showHomeAtStart: false }, true]);
  home.destroy();
});

test("the checkbox never saves before the settings are in: the defaults would reset the others", async () => {
  const load = deferred<unknown>();
  const calls: Call[] = [];
  const home = startHomeSettings(
    (cmd, args) => {
      calls.push({ cmd, args });
      return cmd === "load_settings" ? load.promise : Promise.resolve(null);
    },
    null,
    () => {},
  );

  assert.deepEqual(await home.setShowAtStart(false), DEFAULT_SETTINGS);
  assert.deepEqual(calls.map((call) => call.cmd), ["load_settings"]);

  load.resolve({ ...SAVED });
  await home.ready;
  assert.equal(home.canSave(), true);
  assert.equal(home.get().robotSize, "XL");
  home.destroy();
});

test("a failed load keeps the checkbox from saving until settings-changed brings them", async () => {
  const calls: Call[] = [];
  let emit: ((raw: unknown) => void) | null = null;
  let unsubscribed = false;
  const changes: Array<readonly [Settings, boolean]> = [];
  const home = startHomeSettings(
    async (cmd, args) => {
      calls.push({ cmd, args });
      if (cmd === "load_settings") {
        throw new Error("ipc down");
      }
      return null;
    },
    async (onChanged) => {
      emit = onChanged;
      return () => {
        unsubscribed = true;
      };
    },
    (settings, canSave) => {
      changes.push([settings, canSave]);
    },
  );

  assert.deepEqual(await home.ready, DEFAULT_SETTINGS);
  assert.equal(home.canSave(), false);
  assert.deepEqual(changes, [[DEFAULT_SETTINGS, false]]);
  await home.setShowAtStart(false);
  assert.deepEqual(calls.map((call) => call.cmd), ["load_settings"]);

  // The Settings window saved: every window gets the full settings.
  assert.notEqual(emit, null);
  emit!({ ...SAVED, sound: "fun" });
  assert.equal(home.canSave(), true);
  assert.deepEqual(changes.at(-1), [{ ...SAVED, sound: "fun" }, true]);

  await home.setShowAtStart(false);
  assert.deepEqual(calls.at(-1), {
    cmd: "save_settings",
    args: { settings: { ...SAVED, sound: "fun", showHomeAtStart: false } },
  });

  home.destroy();
  assert.equal(unsubscribed, true);
  const seen = changes.length;
  emit!({ ...SAVED, showHomeAtStart: false });
  assert.equal(changes.length, seen);
});

test("outside Tauri the checkbox works in memory", async () => {
  const changes: Settings[] = [];
  const home = startHomeSettings(null, null, (settings) => {
    changes.push(settings);
  });
  assert.equal(home.canSave(), true);
  assert.deepEqual(await home.ready, DEFAULT_SETTINGS);
  assert.equal(DEFAULT_SETTINGS.showHomeAtStart, true);

  const saved = await home.setShowAtStart(false);
  assert.equal(saved.showHomeAtStart, false);
  assert.equal(home.get().showHomeAtStart, false);
  assert.deepEqual(changes.map((settings) => settings.showHomeAtStart), [true, false]);
  home.destroy();
});

test("the Home intro and the line under the talk steps stay short in both languages", () => {
  for (const [code, strings] of Object.entries(LOCALES)) {
    assert.ok(strings["home.intro"].length <= 110, `${code}: home.intro`);
    for (const key of PTT_KEYS) {
      const line = `${strings[`home.where_${key}`]} ${strings["home.talk_click"]}`;
      assert.ok(line.length <= 100, `${code}: ${key}: ${line}`);
    }
  }
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

type FakeListener = (event: FakeEvent) => void;

interface FakeEvent {
  readonly target: unknown;
  readonly key?: string;
  defaultPrevented: boolean;
  preventDefault(): void;
}

/** The element `focus()` was last called on. */
let focused: FakeElement | null = null;

/** Just enough of an element for `mountHome`: text nodes are plain strings. */
class FakeElement {
  readonly tagName: string;
  readonly classList = new FakeClassList();
  readonly attributes = new Map<string, string>();
  readonly listeners = new Map<string, FakeListener[]>();
  childNodes: Array<FakeElement | string> = [];
  id = "";
  type = "";
  htmlFor = "";
  value = "";
  checked = false;
  disabled = false;
  hidden = false;
  /** A <dialog>'s state (no showModal here: the guide falls back to `open`). */
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

  /** Fires `type` at this element's listeners; returns the event they saw. */
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

function byClass(root: FakeElement, className: string): FakeElement {
  const found = descendants(root).find((node) => node.classList.contains(className));
  assert.ok(found, className);
  return found;
}

function byId(root: FakeElement, id: string): FakeElement {
  const found = descendants(root).find((node) => node.id === id);
  assert.ok(found, id);
  return found;
}

const tick = (): Promise<void> => new Promise((resolve) => setTimeout(resolve, 0));

/**
 * Mounts the Home page on a fake document with English text and the given saved settings
 * (`null`: `load_settings` never answers); `answer` replies to the other commands (it may
 * throw).
 */
async function mountFakeHome(
  saved: Settings | null,
  answer: (cmd: string, args?: Readonly<Record<string, unknown>>) => unknown = () => null,
): Promise<{
  readonly container: FakeElement;
  readonly calls: string[];
  readonly sent: Call[];
  readonly doc: FakeElement;
  readonly unmount: () => void;
}> {
  // The document's own listeners ("?" for the guide) live on `doc`.
  const doc = new FakeElement("#document");
  const globals = globalThis as { document?: unknown; window?: unknown };
  globals.document = {
    documentElement: new FakeElement("html"),
    createElement: (tag: string) => new FakeElement(tag),
    createElementNS: (_ns: string, tag: string) => new FakeElement(tag),
    addEventListener: (type: string, listener: FakeListener) => doc.addEventListener(type, listener),
    removeEventListener: (type: string, listener: FakeListener) => doc.removeEventListener(type, listener),
  };
  // No `matchMedia`: the OS motion setting is left out.
  globals.window = { addEventListener() {}, removeEventListener() {} };
  setHomeI18n((key) => LOCALES.en[key] ?? key, () => "en");
  focused = null;

  const calls: string[] = [];
  const sent: Call[] = [];
  const container = new FakeElement("main");
  const home = mountHome(container as unknown as HTMLElement, {
    invoke: async (cmd, args) => {
      calls.push(cmd);
      sent.push({ cmd, args });
      if (cmd === "load_settings") {
        // `null`: the settings never come in.
        return saved ? { ...saved } : new Promise(() => {});
      }
      return answer(cmd, args);
    },
  });
  await tick();
  return {
    container,
    calls,
    sent,
    doc,
    unmount(): void {
      home.destroy();
      delete globals.document;
      delete globals.window;
    },
  };
}

test("the talk card keeps the keycap and both steps, then one line on where the key is and clicking", async () => {
  const { container, unmount } = await mountFakeHome({ ...SAVED, pttKey: "f9" });
  try {
    const talk = byClass(container, "home-talk");
    assert.deepEqual(
      talk.children.map((node) => node.className),
      ["home-section-title", "home-talk-row", "home-talk-more"],
    );
    const [keycap, steps] = byClass(talk, "home-talk-row").children;
    assert.equal(keycap.tagName, "kbd");
    assert.equal(keycap.textContent, "F9");
    assert.equal(steps.children.length, 2);
    assert.equal(
      byClass(talk, "home-talk-more").textContent,
      `${LOCALES.en["home.where_f9"]} ${LOCALES.en["home.talk_click"]}`,
    );
  } finally {
    unmount();
  }
});

test("the footer: a start-up switch line, Write to Mate and Settings side by side, then a quiet Quit", async () => {
  const { container, calls, unmount } = await mountFakeHome(SAVED);
  try {
    const footer = container.children[1];
    assert.equal(footer.tagName, "footer");
    assert.deepEqual(
      footer.children.map((node) => node.className),
      ["home-start-option", "home-try-status is-error", "home-actions", "home-quit"],
    );
    const [startOption, saveAlert, actions, quit] = footer.children;
    assert.equal(saveAlert.hidden, true);

    // The switch line, enabled once the settings are in.
    const [startInput] = startOption.children;
    assert.equal(startOption.htmlFor, startInput.id);
    assert.equal(startInput.type, "checkbox");
    assert.equal(startInput.checked, SAVED.showHomeAtStart);
    assert.equal(startInput.disabled, false);

    // One primary and one secondary button; no Close button (the window's own close works).
    assert.deepEqual(actions.children.map((node) => node.id), ["home-chat-btn", "home-settings-btn"]);
    assert.ok(actions.children[0].classList.contains("btn-primary"));
    assert.ok(actions.children[1].classList.contains("btn-secondary"));
    assert.ok(!descendants(container).some((node) => node.id === "home-close-btn"));

    // Quit: still a button with its words, but not a big box.
    assert.equal(quit.tagName, "button");
    assert.equal(quit.type, "button");
    assert.equal(quit.id, "home-quit-btn");
    assert.equal(quit.textContent, LOCALES.en["home.quit"]);
    assert.ok(!quit.classList.contains("btn"));

    // Keyboard order: the switch, Write to Mate, Settings, Quit.
    assert.deepEqual(
      descendants(footer)
        .filter((node) => node.tagName === "input" || node.tagName === "button")
        .map((node) => node.id),
      ["home-show-at-start", "home-chat-btn", "home-settings-btn", "home-quit-btn"],
    );

    for (const button of [...actions.children, quit]) {
      button.dispatch("click");
    }
    await new Promise((resolve) => setTimeout(resolve, 0));
    assert.deepEqual(calls, ["cards_list", "load_settings", "open_chat", "open_settings_window", "quit_app"]);
  } finally {
    unmount();
  }
});

test("a failed startup save reports failure and keeps the active choice", async () => {
  const changes: Settings[] = [];
  const home = startHomeSettings(async (cmd) => {
    if (cmd === "load_settings") return { ...SAVED };
    throw new Error("disk full");
  }, null, (settings) => changes.push(settings));
  await home.ready;
  await assert.rejects(home.setShowAtStart(false));
  assert.equal(home.get().showHomeAtStart, false);
  assert.equal(changes.at(-1)?.showHomeAtStart, false);
  home.destroy();
});

test("every string the profile panel, the guide and My lessons show is in both locales", () => {
  const keys = new Set<string>(GUIDE_CARDS.flatMap((card) => [card.title, card.text, card.label, card.example]));
  for (const source of [HOME_SOURCE, ...HOME_PART_SOURCES]) {
    for (const match of source.matchAll(/"(home\.\w+)"/g)) {
      keys.add(match[1]);
    }
  }
  for (const key of ["home.greeting_named", "home.profile_add", "home.lessons_show_less", "home.lessons_folder_failed"]) {
    assert.ok(keys.has(key), key);
  }
  for (const key of keys) {
    for (const [code, strings] of Object.entries(LOCALES)) {
      assert.ok(strings[key]?.trim(), `${code}: ${key}`);
    }
  }
  for (const strings of Object.values(LOCALES)) {
    assert.ok(strings["home.greeting_named"].includes("{person}"));
  }
});

test("the guide has six to eight cards, each a picture, a title, a short line and one example", () => {
  assert.ok(GUIDE_CARDS.length >= 6 && GUIDE_CARDS.length <= 8, `${GUIDE_CARDS.length} cards`);
  assert.equal(new Set(GUIDE_CARDS.map((card) => card.icon)).size, GUIDE_CARDS.length);
  for (const card of GUIDE_CARDS) {
    assert.ok(["home.guide_say", "home.guide_type", "home.guide_try"].includes(card.label), card.title);
    for (const [code, strings] of Object.entries(LOCALES)) {
      assert.ok(strings[card.text].length <= 110, `${code}: ${card.text}`);
    }
  }
  // The talk card names the person's own key.
  assert.equal(GUIDE_CARDS[0].text, "home.guide_talk_text");
  for (const strings of Object.values(LOCALES)) {
    assert.ok(strings["home.guide_talk_text"].includes("{key}"));
  }
});

test("? opens the guide, but not with Ctrl, Alt or Windows, nor while typing in a field", () => {
  const press = (init: Record<string, unknown>) => ({ key: "?", ctrlKey: false, altKey: false, metaKey: false, target: null, ...init });
  for (const target of [null, { tagName: "BUTTON" }, { tagName: "INPUT", type: "checkbox" }, { tagName: "DIALOG" }]) {
    assert.equal(isGuideShortcut(press({ target })), true, inspect(target));
  }
  for (const target of [{ tagName: "INPUT", type: "text" }, { tagName: "INPUT", type: "search" }, { tagName: "TEXTAREA" }, { tagName: "DIV", isContentEditable: true }]) {
    assert.equal(isGuideShortcut(press({ target })), false, inspect(target));
  }
  for (const init of [{ key: "/" }, { key: "F1" }, { ctrlKey: true }, { altKey: true }, { metaKey: true }]) {
    assert.equal(isGuideShortcut(press(init)), false, inspect(init));
  }
});

test("the greeting uses the saved name as typed, and the panel waits for the settings", () => {
  const translate = (key: string): string => LOCALES.en[key] ?? key;
  assert.equal(greetingText("Anna", translate), "Hi, Anna!");
  assert.equal(greetingText("$& Bob", translate), "Hi, $& Bob!");
  assert.equal(greetingText("  ", translate), LOCALES.en["home.greeting"]);
  assert.equal(shouldAskProfile(DEFAULT_SETTINGS, true), true);
  assert.equal(shouldAskProfile(DEFAULT_SETTINGS, false), false);
  assert.equal(shouldAskProfile({ ...DEFAULT_SETTINGS, profileAsked: true }, true), false);
});

test("saveProfile marks the profile asked over the loaded settings; Skip keeps the saved profile", async () => {
  const calls: Call[] = [];
  const loaded: Settings = { ...SAVED, profile: { name: "Old", about: "Hi" } };
  const home = startHomeSettings(async (cmd, args) => {
    calls.push({ cmd, args });
    return cmd === "load_settings" ? { ...loaded } : null;
  }, null, () => {});
  await home.ready;

  const skipped = await home.saveProfile(null);
  assert.deepEqual(skipped, { ...loaded, profileAsked: true });
  assert.deepEqual(calls.at(-1), { cmd: "save_settings", args: { settings: skipped } });

  const saved = await home.saveProfile({ name: " Anna ", about: "I'm 68." });
  assert.deepEqual(saved, { ...loaded, profile: { name: "Anna", about: "I'm 68." }, profileAsked: true });
  assert.deepEqual(calls.at(-1), { cmd: "save_settings", args: { settings: saved } });
  home.destroy();
});

test("saveProfile never saves before the settings are in", async () => {
  const load = deferred<unknown>();
  const calls: string[] = [];
  const home = startHomeSettings((cmd) => {
    calls.push(cmd);
    return cmd === "load_settings" ? load.promise : Promise.resolve(null);
  }, null, () => {});
  await assert.rejects(home.saveProfile({ name: "Anna", about: "" }));
  assert.deepEqual(calls, ["load_settings"]);
  load.resolve({ ...SAVED });
  await home.ready;
  home.destroy();
});

test("the first open asks what to call the person, at the top; Save greets them by name", async () => {
  const { container, sent, unmount } = await mountFakeHome(SAVED);
  try {
    const panel = byClass(container, "home-profile");
    assert.equal(container.children[0].children[0], panel);
    assert.equal(panel.hidden, false);
    // Home opens without the keyboard: the panel never takes it on its own.
    assert.equal(focused, null);
    assert.equal(byId(panel, "home-profile-ask").textContent, LOCALES.en["home.profile_ask"]);
    const name = byId(panel, "home-profile-name");
    const about = byId(panel, "home-profile-about");
    assert.equal(name.maxLength, 40);
    assert.equal(about.maxLength, 300);
    assert.equal(about.placeholder, LOCALES.en["home.profile_about_placeholder"]);

    name.value = "  Anna ";
    about.value = "I'm 68.";
    assert.equal(byClass(panel, "home-profile-form").dispatch("submit").defaultPrevented, true);
    await tick();
    assert.deepEqual(
      sent.filter((call) => call.cmd === "save_settings").map((call) => call.args),
      [{ settings: { ...SAVED, profile: { name: "Anna", about: "I'm 68." }, profileAsked: true } }],
    );
    assert.equal(panel.hidden, true);
    assert.equal(byClass(container, "home-title").textContent, "Hi, Anna!");
    const edit = byId(container, "home-profile-edit");
    assert.equal(edit.hidden, false);
    assert.equal(edit.textContent, LOCALES.en["home.profile_edit"]);
    assert.equal(focused, edit);
  } finally {
    unmount();
  }
});

test("Skip marks the profile asked; a saved name keeps the panel shut until Edit", async () => {
  const first = await mountFakeHome(SAVED);
  try {
    byId(first.container, "home-profile-skip").dispatch("click");
    await tick();
    assert.deepEqual(
      first.sent.filter((call) => call.cmd === "save_settings").map((call) => call.args),
      [{ settings: { ...SAVED, profileAsked: true } }],
    );
    assert.equal(byClass(first.container, "home-profile").hidden, true);
    assert.equal(byClass(first.container, "home-title").textContent, LOCALES.en["home.greeting"]);
    assert.equal(byId(first.container, "home-profile-edit").textContent, LOCALES.en["home.profile_add"]);
  } finally {
    first.unmount();
  }

  const again = await mountFakeHome({ ...SAVED, profile: { name: "Anna", about: "Video calls." }, profileAsked: true });
  try {
    const panel = byClass(again.container, "home-profile");
    assert.equal(panel.hidden, true);
    assert.equal(byClass(again.container, "home-title").textContent, "Hi, Anna!");
    byId(again.container, "home-profile-edit").dispatch("click");
    assert.equal(panel.hidden, false);
    const name = byId(panel, "home-profile-name");
    assert.equal(name.value, "Anna");
    assert.equal(byId(panel, "home-profile-about").value, "Video calls.");
    assert.equal(focused, name);
    assert.deepEqual(again.calls.filter((cmd) => cmd === "save_settings"), []);
  } finally {
    again.unmount();
  }
});

test("a failed profile save keeps the panel open with a calm line", async () => {
  const { container, unmount } = await mountFakeHome(SAVED, (cmd) => {
    if (cmd === "save_settings") throw new Error("disk full");
    return null;
  });
  try {
    const panel = byClass(container, "home-profile");
    byId(panel, "home-profile-name").value = "Anna";
    byClass(panel, "home-profile-form").dispatch("submit");
    await tick();
    assert.equal(panel.hidden, false);
    const alert = descendants(panel).find((node) => node.attributes.get("role") === "alert");
    assert.ok(alert);
    assert.equal(alert.hidden, false);
    assert.equal(alert.textContent, LOCALES.en["home.profile_save_failed"]);
  } finally {
    unmount();
  }
});

test("before the settings are in, neither the panel nor Edit shows", async () => {
  const { container, calls, unmount } = await mountFakeHome(null);
  try {
    assert.equal(byClass(container, "home-profile").hidden, true);
    assert.equal(byId(container, "home-profile-edit").hidden, true);
    assert.equal(byClass(container, "home-title").textContent, LOCALES.en["home.greeting"]);
    assert.deepEqual(calls.filter((cmd) => cmd === "save_settings"), []);
  } finally {
    unmount();
  }
});

test("Guide and ? open the guide with the person's talk key; Close gives the keyboard back", async () => {
  const { container, doc, unmount } = await mountFakeHome({ ...SAVED, pttKey: "f9", profileAsked: true });
  try {
    const guideBtn = byId(container, "home-guide-btn");
    assert.equal(guideBtn.attributes.get("aria-keyshortcuts"), "?");
    const dialog = container.children[2];
    assert.equal(dialog.tagName, "dialog");
    assert.equal(dialog.open, false);

    guideBtn.dispatch("click");
    assert.equal(dialog.open, true);
    const cards = byClass(dialog, "home-guide-list").children;
    assert.equal(cards.length, GUIDE_CARDS.length);
    assert.equal(byClass(cards[0], "home-guide-key").textContent, LOCALES.en["settings.keycap_f9"]);
    assert.equal(
      byClass(cards[0], "home-guide-text").textContent,
      LOCALES.en["home.guide_talk_text"].replace("{key}", LOCALES.en["settings.key_f9"]),
    );
    // Close is the dialog's first button, so the keyboard lands on it.
    const close = byId(dialog, "home-guide-close");
    assert.equal(descendants(dialog).find((node) => node.tagName === "button"), close);
    close.dispatch("click");
    assert.equal(dialog.open, false);
    assert.equal(focused, guideBtn);

    // "?" while typing about oneself is just a question mark.
    const about = byId(container, "home-profile-about");
    assert.equal(doc.dispatch("keydown", { key: "?", target: about }).defaultPrevented, false);
    assert.equal(dialog.open, false);
    assert.equal(doc.dispatch("keydown", { key: "?", target: guideBtn }).defaultPrevented, true);
    assert.equal(dialog.open, true);
  } finally {
    unmount();
  }
});

test("My lessons lists the cards newest first; a card opens with card_show and its steps fold", async () => {
  const cards = [
    { id: "old", title: "Print a page", steps: ["Open the menu", "Click Print"], created: 1_793_000_000, thumb: null },
    {
      id: "new",
      title: "Attach a photo",
      steps: ["Click the clip", "Choose the photo", "Click Open", "Click Send"],
      created: 1_793_534_400,
      thumb: "data:image/png;base64,iVBORw0KGgo=",
    },
  ];
  const { container, sent, unmount } = await mountFakeHome({ ...SAVED, profileAsked: true }, (cmd) =>
    cmd === "cards_list" ? cards : null,
  );
  try {
    const section = byClass(container, "home-lessons");
    assert.equal(byClass(section, "home-lessons-empty").hidden, true);
    const items = byClass(section, "home-lessons-list").children;
    assert.deepEqual(items.map((item) => byClass(item, "home-lesson-title").textContent), ["Attach a photo", "Print a page"]);
    assert.equal(byClass(items[0], "home-lesson-img").src, cards[1].thumb);
    assert.ok(byClass(items[1], "home-lesson-thumb").classList.contains("is-placeholder"));
    assert.match(byClass(items[0], "home-lesson-date").textContent, /2026/);

    // The first two steps, then Show all; a two-step card needs no button.
    const steps = byClass(items[0], "home-lesson-steps").children;
    assert.deepEqual(steps.map((step) => step.hidden), [false, false, true, true]);
    const more = byClass(items[0], "home-lesson-more");
    assert.equal(more.attributes.get("aria-expanded"), "false");
    assert.equal(more.textContent, LOCALES.en["home.lessons_show_all"]);
    more.dispatch("click");
    assert.deepEqual(steps.map((step) => step.hidden), [false, false, false, false]);
    assert.equal(more.attributes.get("aria-expanded"), "true");
    assert.equal(more.textContent, LOCALES.en["home.lessons_show_less"]);
    assert.equal(byClass(items[1], "home-lesson-more").hidden, true);

    byClass(items[1], "home-lesson-open").dispatch("click");
    byId(section, "home-lessons-folder-btn").dispatch("click");
    await tick();
    assert.deepEqual(
      sent.filter((call) => call.cmd !== "load_settings"),
      [
        { cmd: "cards_list", args: undefined },
        { cmd: "card_show", args: { id: "old" } },
        { cmd: "cards_open_folder", args: undefined },
      ],
    );
  } finally {
    unmount();
  }
});

test("My lessons shows the friendly empty line when cards_list is missing or fails", async () => {
  const { container, unmount } = await mountFakeHome({ ...SAVED, profileAsked: true }, (cmd) => {
    if (cmd === "cards_list") throw new Error("command cards_list not found");
    return null;
  });
  try {
    const section = byClass(container, "home-lessons");
    const empty = byClass(section, "home-lessons-empty");
    assert.equal(empty.hidden, false);
    assert.equal(empty.textContent, LOCALES.en["home.lessons_empty"]);
    assert.equal(byClass(section, "home-lessons-list").hidden, true);
    assert.ok(!descendants(section).some((node) => node.classList.contains("is-error")));
  } finally {
    unmount();
  }
});
