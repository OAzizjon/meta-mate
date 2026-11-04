import assert from "node:assert/strict";
import { test } from "node:test";
import { applyTheme, followWindowSettings } from "./theme.ts";
import { createSettingsStore, isTheme, parseSettings, THEMES } from "./model.ts";
import type { Settings } from "./model.ts";
import { startMateSettings } from "./apply.ts";

test("theme validates independently and survives loading and saving", async () => {
  for (const theme of THEMES) {
    assert.equal(isTheme(theme), true);
    assert.equal(parseSettings({ theme, sound: "fun" }).theme, theme);
  }
  for (const theme of [undefined, null, "future", "Dark", 0, {}, []]) {
    assert.equal(isTheme(theme), false);
    const settings = parseSettings({ theme, sound: "fun" });
    assert.equal(settings.theme, "light");
    assert.equal(settings.sound, "fun");
  }
  assert.equal(parseSettings({ sound: "fun" }).theme, "light");
  const calls: unknown[] = [];
  const store = createSettingsStore(async (cmd, args) => {
    if (cmd === "load_settings") return { theme: "dark", sound: "fun" };
    calls.push({ cmd, args });
  });
  assert.equal((await store.load()).theme, "dark");
  const saved = await store.update({ theme: "light" });
  assert.equal(saved.sound, "fun");
  assert.deepEqual(calls, [{ cmd: "save_settings", args: { settings: saved } }]);
});

test("applyTheme sets an explicit default and switches both ways", () => {
  const root = { dataset: { theme: "dark" } };
  applyTheme(undefined, root);
  assert.equal(root.dataset.theme, "light");
  applyTheme({ theme: "dark" }, root);
  assert.equal(root.dataset.theme, "dark");
  applyTheme({ theme: "light" }, root);
  assert.equal(root.dataset.theme, "light");
  applyTheme({ theme: "unknown" }, root);
  assert.equal(root.dataset.theme, "light");
});

test("window subscription applies the stored theme and live validated changes", async () => {
  const root = { dataset: { theme: "" } };
  let emit = (_raw: unknown): void => {};
  const refreshed: Settings[] = [];
  await followWindowSettings(root, async (cmd) => {
    assert.equal(cmd, "load_settings");
    assert.equal(root.dataset.theme, "");
    return { theme: "dark", language: "ru" };
  }, async (onChanged) => {
    emit = onChanged;
    return () => {};
  }, (settings) => refreshed.push(settings));
  assert.equal(root.dataset.theme, "dark");
  assert.equal(refreshed[0].language, "ru");
  emit({ theme: "light", language: "en" });
  assert.equal(root.dataset.theme, "light");
  assert.equal(refreshed.at(-1)?.language, "en");
  emit({ theme: "unknown" });
  assert.equal(root.dataset.theme, "light");
});

test("a failed initial load releases the default palette", async () => {
  const root = { dataset: {} as { theme?: string } };
  await followWindowSettings(root, async () => {
    assert.equal(root.dataset.theme, undefined);
    throw new Error("unavailable");
  }, async () => () => {}, () => {});
  assert.equal(root.dataset.theme, "light");
});

test("a failed subscription still loads the saved palette", async () => {
  const root = { dataset: {} as { theme?: string } };
  await followWindowSettings(root, async () => ({ theme: "dark" }), async () => {
    throw new Error("unavailable");
  }, () => {});
  assert.equal(root.dataset.theme, "dark");
});

test("a change during loading wins over the stale initial window theme", async () => {
  const root = { dataset: { theme: "" } };
  let emit = (_raw: unknown): void => {};
  const refreshed: Settings[] = [];
  await followWindowSettings(root, async () => {
    emit({ theme: "dark" });
    return { theme: "light" };
  }, async (onChanged) => {
    emit = onChanged;
    return () => {};
  }, (settings) => refreshed.push(settings));
  assert.equal(root.dataset.theme, "dark");
  assert.deepEqual(refreshed.map((settings) => settings.theme), ["dark"]);
});

test("Mate's existing settings callback applies startup and external theme changes", async () => {
  const root = { dataset: { theme: "" } };
  let emit = (_raw: unknown): void => {};
  const binding = startMateSettings({}, {
    invoke: async () => ({ theme: "dark" }),
    subscribe: async (onChanged) => {
      emit = onChanged;
      return () => {};
    },
    onChange: (settings) => { applyTheme(settings, root); },
  });
  assert.equal(root.dataset.theme, "light");
  await binding.ready;
  assert.equal(root.dataset.theme, "dark");
  emit({ theme: "light" });
  assert.equal(root.dataset.theme, "light");
  binding.destroy();
  emit({ theme: "dark" });
  assert.equal(root.dataset.theme, "light");
});
