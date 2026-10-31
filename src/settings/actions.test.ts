// Run with npm test (Node's test runner strips the TypeScript types).
import assert from "node:assert/strict";
import { test } from "node:test";
import { createSettingsActions, settingsReduceMotion } from "./actions.ts";
import { createSettingsStore, defaultSettings } from "./model.ts";
import type { Settings } from "./model.ts";

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((res, rej) => { resolve = res; reject = rej; });
  return { promise, resolve, reject };
}

test("settings reduce motion when either the user or system requests it", () => {
  for (const [setting, system, expected] of [
    [false, false, false], [false, true, true],
    [true, false, true], [true, true, true],
    [false, undefined, false], [true, undefined, true],
  ] as const) {
    assert.equal(settingsReduceMotion(setting, system), expected);
  }
});

test("patches refresh with validated settings and preserve unrelated preferences", async () => {
  const store = createSettingsStore();
  await store.update({ language: "ru", pttKey: "f10", spotlight: false });
  const refreshed: Settings[] = [];
  const actions = createSettingsActions(store, (settings) => refreshed.push(settings), () => true);
  const updated = await actions.applyPatch({ voiceSpeed: 1.03, robotSize: "XL" });
  assert.deepEqual(updated, {
    ...defaultSettings(), language: "ru", pttKey: "f10", spotlight: false,
    voiceSpeed: 1.05, robotSize: "XL",
  });
  assert.deepEqual(refreshed, [updated]);
  assert.deepEqual(store.get(), updated);
});

test("patch refresh waits for save completion and returns the store result", async () => {
  const saved = deferred<unknown>();
  const commands: string[] = [];
  const store = createSettingsStore((command) => { commands.push(command); return saved.promise; });
  const refreshed: Settings[] = [];
  const actions = createSettingsActions(store, (settings) => refreshed.push(settings), () => true);
  const pending = actions.applyPatch({ model: "thorough" });
  assert.deepEqual(commands, ["save_settings"]);
  assert.deepEqual(refreshed, []);
  saved.resolve(null);
  const updated = await pending;
  assert.equal(updated.model, "thorough");
  assert.equal(refreshed.length, 1);
  assert.equal(refreshed[0], updated);
});

test("failed IPC saves still refresh the validated in-memory change", async () => {
  const store = createSettingsStore(async () => { throw new Error("offline"); });
  const refreshed: Settings[] = [];
  const actions = createSettingsActions(store, (settings) => refreshed.push(settings), () => true);
  const updated = await actions.applyPatch({ sound: "fun" });
  assert.equal(updated.sound, "fun");
  assert.deepEqual(refreshed, [updated]);
});

test("loads and reloads refresh with the latest store result", async () => {
  let language: Settings["language"] = "en";
  const commands: string[] = [];
  const store = createSettingsStore(async (command) => {
    commands.push(command);
    return { ...defaultSettings(), language };
  });
  const refreshed: Settings[] = [];
  const actions = createSettingsActions(store, (settings) => refreshed.push(settings), () => true);
  const first = await actions.load();
  language = "ru";
  const second = await actions.load();
  assert.deepEqual(commands, ["load_settings", "load_settings"]);
  assert.deepEqual(refreshed.map((settings) => settings.language), ["en", "ru"]);
  assert.equal(refreshed[0], first);
  assert.equal(refreshed[1], second);
});

test("destroying the controller while loading suppresses refresh but returns settings", async () => {
  const loaded = deferred<unknown>();
  const store = createSettingsStore(() => loaded.promise);
  let active = true;
  const refreshed: Settings[] = [];
  const actions = createSettingsActions(store, (settings) => refreshed.push(settings), () => active);
  const pending = actions.load();
  active = false;
  loaded.resolve({ ...defaultSettings(), reduceMotion: true });
  const result = await pending;
  assert.equal(result.reduceMotion, true);
  assert.deepEqual(refreshed, []);
});

test("pending patches still refresh after destroy, preserving the existing controller behavior", async () => {
  const saved = deferred<unknown>();
  const store = createSettingsStore(() => saved.promise);
  let active = true;
  const refreshed: Settings[] = [];
  const actions = createSettingsActions(store, (settings) => refreshed.push(settings), () => active);
  const pending = actions.applyPatch({ reduceMotion: true });
  active = false;
  saved.resolve(null);
  assert.deepEqual(refreshed, [await pending]);
});

test("a late initial load refreshes with local edits instead of overwriting them", async () => {
  const loaded = deferred<unknown>();
  const store = createSettingsStore(async (command) => command === "load_settings" ? loaded.promise : null);
  const refreshed: Settings[] = [];
  const actions = createSettingsActions(store, (settings) => refreshed.push(settings), () => true);
  const pending = actions.load();
  const edited = await actions.applyPatch({ language: "ru", model: "thorough" });
  loaded.resolve({ ...defaultSettings(), language: "en" });
  assert.deepEqual(await pending, edited);
  assert.deepEqual(refreshed, [edited, edited]);
});

test("failed IPC loads refresh from the current in-memory settings", async () => {
  const store = createSettingsStore(async () => { throw new Error("offline"); });
  await store.update({ ptt: "toggle" });
  const refreshed: Settings[] = [];
  const actions = createSettingsActions(store, (settings) => refreshed.push(settings), () => true);
  const loaded = await actions.load();
  assert.equal(loaded.ptt, "toggle");
  assert.deepEqual(refreshed, [loaded]);
});

test("a rejecting custom store does not refresh and propagates its error", async () => {
  const failure = new Error("custom store failed");
  const failed = deferred<Settings>();
  const store = { ...createSettingsStore(), load: () => failed.promise, update: () => failed.promise };
  const refreshed: Settings[] = [];
  const actions = createSettingsActions(store, (settings) => refreshed.push(settings), () => true);
  const loading = assert.rejects(actions.load(), (error) => error === failure);
  const updating = assert.rejects(actions.applyPatch({}), (error) => error === failure);
  failed.reject(failure);
  await Promise.all([loading, updating]);
  assert.deepEqual(refreshed, []);
});
