// Run with `npm test` (Node's own test runner; Node 24 strips the types).
// Unit tests for the Home window's logic (src/home.ts, WP89a): the "Try it" runner, the
// Settings button and the "Show this window when MetaMate starts" checkbox's settings.
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
  openSettingsWindow,
  startHomeSettings,
  tryStatusKey,
} from "./home.ts";
import type { HomeInvoke } from "./home.ts";
import { DEFAULT_SETTINGS } from "./settings/model.ts";
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
