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

/** Just enough of an element for `mountHome`: text nodes are plain strings. */
class FakeElement {
  readonly tagName: string;
  readonly classList = new FakeClassList();
  readonly attributes = new Map<string, string>();
  readonly listeners = new Map<string, Array<() => void>>();
  childNodes: Array<FakeElement | string> = [];
  id = "";
  type = "";
  htmlFor = "";
  checked = false;
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

  addEventListener(type: string, listener: () => void): void {
    this.listeners.set(type, [...(this.listeners.get(type) ?? []), listener]);
  }

  removeEventListener(type: string, listener: () => void): void {
    this.listeners.set(type, (this.listeners.get(type) ?? []).filter((other) => other !== listener));
  }

  dispatch(type: string): void {
    for (const listener of this.listeners.get(type) ?? []) listener();
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

/** Mounts the Home page on a fake document with English text and the given saved settings. */
async function mountFakeHome(saved: Settings): Promise<{
  readonly container: FakeElement;
  readonly calls: string[];
  readonly unmount: () => void;
}> {
  const globals = globalThis as { document?: unknown; window?: unknown };
  globals.document = {
    documentElement: new FakeElement("html"),
    createElement: (tag: string) => new FakeElement(tag),
  };
  // No `matchMedia`: the OS motion setting is left out.
  globals.window = { addEventListener() {}, removeEventListener() {} };
  setHomeI18n((key) => LOCALES.en[key] ?? key, () => "en");

  const calls: string[] = [];
  const container = new FakeElement("main");
  const home = mountHome(container as unknown as HTMLElement, {
    invoke: async (cmd) => {
      calls.push(cmd);
      return cmd === "load_settings" ? { ...saved } : null;
    },
  });
  await new Promise((resolve) => setTimeout(resolve, 0));
  return {
    container,
    calls,
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
      ["home-start-option", "home-actions", "home-quit"],
    );
    const [startOption, actions, quit] = footer.children;

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
    assert.deepEqual(calls, ["load_settings", "open_chat", "open_settings_window", "quit_app"]);
  } finally {
    unmount();
  }
});
