// Run with `npm test` (Node's own test runner; Node 24 strips the types).
// Unit tests for the pure settings model (src/settings/model.ts, WP24).
import assert from "node:assert/strict";
import { test } from "node:test";
import { inspect } from "node:util";

import {
  DEFAULT_SETTINGS,
  MAX_VOICE_SPEED,
  MIN_VOICE_SPEED,
  MODEL_CHOICES,
  PTT_MODES,
  ROBOT_SIZES,
  SETTINGS_KEYS,
  SOUND_MODES,
  createSettingsStore,
  defaultSettings,
  formatVoiceSpeed,
  parseSettings,
  updateSettings,
} from "./model.ts";
import type { Settings } from "./model.ts";

test("defaultSettings returns a fresh copy of the expected defaults", () => {
  const defaults = defaultSettings();
  assert.deepEqual(defaults, {
    model: "quick",
    ptt: "hold",
    sound: "calm",
    voiceSpeed: 1,
    robotSize: "M",
    reduceMotion: false,
    startWithWindows: false,
  });
  assert.deepEqual(defaults, DEFAULT_SETTINGS);
  assert.notEqual(defaults, DEFAULT_SETTINGS);
});

test("parseSettings accepts every valid enum combination and boundary voiceSpeed", () => {
  for (const model of MODEL_CHOICES) {
    for (const ptt of PTT_MODES) {
      for (const sound of SOUND_MODES) {
        for (const robotSize of ROBOT_SIZES) {
          for (const voiceSpeed of [MIN_VOICE_SPEED, 0.95, 1, 1.15, MAX_VOICE_SPEED]) {
            const raw: Settings = {
              model,
              ptt,
              sound,
              voiceSpeed,
              robotSize,
              reduceMotion: model === "thorough",
              startWithWindows: ptt === "toggle",
            };
            const parsed = parseSettings(raw);
            assert.deepEqual(parsed, raw, inspect(raw));
            assert.notEqual(parsed, raw);
          }
        }
      }
    }
  }
});

test("parseSettings drops unknown fields and preserves valid ones", () => {
  const raw = {
    model: "thorough",
    ptt: "toggle",
    sound: "fun",
    voiceSpeed: 1.25,
    robotSize: "XL",
    reduceMotion: true,
    startWithWindows: true,
    unknownField: "dropped",
    extraNumber: 99,
    nested: { a: 1 },
  };
  const parsed = parseSettings(raw);
  assert.deepEqual(Object.keys(parsed), [...SETTINGS_KEYS]);
  assert.deepEqual(parsed, {
    model: "thorough",
    ptt: "toggle",
    sound: "fun",
    voiceSpeed: 1.25,
    robotSize: "XL",
    reduceMotion: true,
    startWithWindows: true,
  });
  assert.equal("unknownField" in parsed, false);
});

test("parseSettings replaces out-of-range and wrong-type fields with defaults per field", () => {
  assert.deepEqual(
    parseSettings({
      model: "thorough",
      ptt: "invalid",
      sound: "fun",
      voiceSpeed: 1.4,
      robotSize: "XXL",
      reduceMotion: true,
      startWithWindows: "true",
    }),
    {
      model: "thorough",
      ptt: DEFAULT_SETTINGS.ptt,
      sound: "fun",
      voiceSpeed: DEFAULT_SETTINGS.voiceSpeed,
      robotSize: DEFAULT_SETTINGS.robotSize,
      reduceMotion: true,
      startWithWindows: DEFAULT_SETTINGS.startWithWindows,
    },
  );

  const badSpeeds: unknown[] = [
    0.7999,
    1.3001,
    0,
    -1,
    100,
    Number.NaN,
    Number.POSITIVE_INFINITY,
    Number.NEGATIVE_INFINITY,
    "1.0",
    null,
    undefined,
    true,
    [1],
  ];
  for (const voiceSpeed of badSpeeds) {
    const parsed = parseSettings({ model: "thorough", voiceSpeed });
    assert.equal(parsed.model, "thorough", inspect(voiceSpeed));
    assert.equal(parsed.voiceSpeed, DEFAULT_SETTINGS.voiceSpeed, inspect(voiceSpeed));
  }

  const badEnums = [
    { model: "QUICK", ptt: "HOLD", sound: "CALM", robotSize: "m" },
    { model: "spark", ptt: "press", sound: "loud", robotSize: "XS" },
    { model: 1, ptt: false, sound: null, robotSize: ["M"] },
    { reduceMotion: 1, startWithWindows: 0 },
  ];
  for (const raw of badEnums) {
    assert.deepEqual(parseSettings(raw), DEFAULT_SETTINGS, inspect(raw));
  }
});

test("parseSettings ignores inherited properties and never throws on non-objects or throwing getters", () => {
  for (const nonObject of [null, undefined, false, true, 0, 42, "settings", [], [1, 2]]) {
    assert.deepEqual(parseSettings(nonObject), DEFAULT_SETTINGS, inspect(nonObject));
  }

  const inherited = Object.create({
    model: "thorough",
    ptt: "toggle",
    sound: "fun",
    voiceSpeed: 1.2,
    robotSize: "L",
    reduceMotion: true,
    startWithWindows: true,
  });
  assert.deepEqual(parseSettings(inherited), DEFAULT_SETTINGS);

  const boom = (): never => {
    throw new Error("boom");
  };
  assert.deepEqual(parseSettings(new Proxy({}, { get: boom, getOwnPropertyDescriptor: boom })), DEFAULT_SETTINGS);

  const partialBoom: Record<string, unknown> = {
    model: "thorough",
    robotSize: "L",
  };
  Object.defineProperty(partialBoom, "voiceSpeed", {
    enumerable: true,
    get: boom,
  });
  assert.deepEqual(parseSettings(partialBoom), {
    ...DEFAULT_SETTINGS,
    model: "thorough",
    robotSize: "L",
  });
});

test("updateSettings and formatVoiceSpeed stay within invariants", () => {
  const updated = updateSettings(DEFAULT_SETTINGS, {
    model: "thorough",
    voiceSpeed: 1.15,
    reduceMotion: true,
  });
  assert.equal(updated.model, "thorough");
  assert.equal(updated.voiceSpeed, 1.15);
  assert.equal(updated.reduceMotion, true);
  assert.equal(updated.ptt, "hold");

  assert.equal(formatVoiceSpeed(0.8), "0.8\u00d7");
  assert.equal(formatVoiceSpeed(1), "1.0\u00d7");
  assert.equal(formatVoiceSpeed(1.25), "1.25\u00d7");
  assert.equal(formatVoiceSpeed(99), "1.0\u00d7");
  assert.doesNotMatch(formatVoiceSpeed(1.05), /[A-Za-z\u0400-\u04FF]/);
});

test("createSettingsStore loads and saves through invoke and falls back to memory outside Tauri", async () => {
  const memoryStore = createSettingsStore(null);
  assert.deepEqual(await memoryStore.load(), DEFAULT_SETTINGS);
  await memoryStore.update({ model: "thorough", robotSize: "XL" });
  assert.equal(memoryStore.get().model, "thorough");
  assert.equal(memoryStore.get().robotSize, "XL");

  const calls: Array<{ cmd: string; args?: Readonly<Record<string, unknown>> }> = [];
  const tauriStore = createSettingsStore(async (cmd, args) => {
    calls.push({ cmd, args });
    if (cmd === "load_settings") {
      return {
        model: "thorough",
        ptt: "toggle",
        sound: "fun",
        voiceSpeed: 2.5, // out of range -> default 1
        robotSize: "L",
        reduceMotion: true,
        startWithWindows: false,
        extra: "ignored",
      };
    }
    return null;
  });

  const loaded = await tauriStore.load();
  assert.equal(calls.length, 1);
  assert.equal(calls[0].cmd, "load_settings");
  assert.deepEqual(loaded, {
    model: "thorough",
    ptt: "toggle",
    sound: "fun",
    voiceSpeed: 1,
    robotSize: "L",
    reduceMotion: true,
    startWithWindows: false,
  });

  const saved = await tauriStore.update({ voiceSpeed: 1.2, startWithWindows: true });
  assert.equal(calls.length, 2);
  assert.equal(calls[1].cmd, "save_settings");
  assert.deepEqual(calls[1].args, { settings: saved });
  assert.equal(saved.voiceSpeed, 1.2);
  assert.equal(saved.startWithWindows, true);
});
