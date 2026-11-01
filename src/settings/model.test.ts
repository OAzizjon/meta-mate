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
  PTT_KEYS,
  ROBOT_SIZES,
  SETTINGS_CHANGED_EVENT,
  SETTINGS_KEYS,
  SOUND_MODES,
  createSettingsStore,
  defaultSettings,
  formatVoiceSpeed,
  parseSettings,
  snapVoiceSpeed,
  subscribeSettingsChanged,
  updateSettings,
  voiceSpeedPace,
} from "./model.ts";
import type { Settings } from "./model.ts";

test("the Rust settings JSON preserves the profile and profileAsked", () => {
  // Keep this JSON literal in sync with profile_serialization_matches_the_window_contract.
  const raw = JSON.parse(`{
            "profile": {"name": "Demo", "about": "Learning the app"}, "profileAsked": true,
            "language": "auto", "theme": "light", "model": "quick",
            "ptt": "hold", "pttKey": "ctrl", "sound": "calm",
            "voiceSpeed": 1.0, "robotSize": "M", "reduceMotion": false,
            "spotlight": true, "startWithWindows": false,
            "showHomeAtStart": true
        }`);
  const settings = parseSettings(raw);
  assert.deepEqual(settings.profile, { name: "Demo", about: "Learning the app" });
  assert.equal(settings.profileAsked, true);
  assert.deepEqual(settings, raw);
});

test("profiles default, sanitize Unicode data and survive unrelated saves", async () => {
  assert.deepEqual(parseSettings({ sound: "fun" }).profile, { name: "", about: "" });
  assert.equal(parseSettings({}).profileAsked, false);
  const settings = parseSettings({
    profile: { name: "  <Анна>  Мария\u0000  ", about: "я ".repeat(400) },
    profileAsked: true,
  });
  assert.equal(settings.profile.name, "Анна Мария");
  assert.ok(Array.from(settings.profile.about).length <= 300);
  assert.equal(Array.from(parseSettings({ profile: { name: "😀".repeat(50) } }).profile.name).length, 40);
  assert.equal(settings.profileAsked, true);
  assert.deepEqual(updateSettings(settings, { sound: "fun" }).profile, settings.profile);
  const store = createSettingsStore();
  await store.save(settings);
  store.get().profile.name = "Changed";
  const loaded = await store.load();
  loaded.profile.name = "Changed";
  assert.equal(store.get().profile.name, "Анна Мария");
});

test("talk keys parse independently and older settings retain their preferences", () => {
  for (const pttKey of PTT_KEYS) {
    assert.equal(parseSettings({ pttKey, sound: "fun" }).pttKey, pttKey);
    assert.equal(updateSettings(DEFAULT_SETTINGS, { pttKey }).pttKey, pttKey);
  }
  // "rightCtrl" is retired (D75): a saved one becomes either Ctrl.
  for (const pttKey of [undefined, null, "future", "rightCtrl", "RightCtrl", 0, {}]) {
    const settings = parseSettings({ pttKey, sound: "fun" });
    assert.equal(settings.pttKey, "ctrl");
    assert.equal(settings.sound, "fun");
  }
});

test("defaultSettings returns a fresh copy of the expected defaults", () => {
  const defaults = defaultSettings();
  assert.deepEqual(defaults, {
    profile: { name: "", about: "" },
    profileAsked: false,
    language: "auto",
    theme: "light",
    model: "quick",
    ptt: "hold",
    pttKey: "ctrl",
    sound: "calm",
    voiceSpeed: 1,
    robotSize: "M",
    reduceMotion: false,
    startWithWindows: false,
    showHomeAtStart: true,
    spotlight: true,
  });
  assert.deepEqual(defaults, DEFAULT_SETTINGS);
  assert.notEqual(defaults, DEFAULT_SETTINGS);
});

test("showHomeAtStart defaults for older settings and preserves an explicit choice", () => {
  assert.equal(parseSettings({ model: "thorough" }).showHomeAtStart, true);
  for (const showHomeAtStart of [true, false]) {
    assert.equal(parseSettings({ showHomeAtStart }).showHomeAtStart, showHomeAtStart);
  }
  for (const showHomeAtStart of [null, "false", 0]) {
    assert.equal(parseSettings({ showHomeAtStart }).showHomeAtStart, true);
  }
  const updated = updateSettings(DEFAULT_SETTINGS, { showHomeAtStart: false });
  assert.equal(updateSettings(updated, { sound: "fun" }).showHomeAtStart, false);
});

test("parseSettings accepts every valid enum combination and boundary voiceSpeed", () => {
  for (const model of MODEL_CHOICES) {
    for (const ptt of PTT_MODES) {
      for (const sound of SOUND_MODES) {
        for (const robotSize of ROBOT_SIZES) {
          for (const voiceSpeed of [MIN_VOICE_SPEED, 0.95, 1, 1.15, MAX_VOICE_SPEED]) {
            const raw: Settings = {
              profile: { name: "", about: "" },
              profileAsked: false,
              language: "auto",
              theme: "light",
              model,
              ptt,
              pttKey: "ctrl",
              sound,
              voiceSpeed,
              robotSize,
              reduceMotion: model === "thorough",
              startWithWindows: ptt === "toggle",
              showHomeAtStart: model === "quick",
              spotlight: true,
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
    pttKey: "ctrl",
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
    profile: { name: "", about: "" },
    profileAsked: false,
    language: "auto",
    theme: "light",
    model: "thorough",
    ptt: "toggle",
    pttKey: "ctrl",
    sound: "fun",
    voiceSpeed: 1.25,
    robotSize: "XL",
    reduceMotion: true,
    startWithWindows: true,
    showHomeAtStart: true,
    spotlight: true,
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
      language: "auto",
      profile: { name: "", about: "" },
      profileAsked: false,
      theme: "light",
      model: "thorough",
      ptt: DEFAULT_SETTINGS.ptt,
      pttKey: "ctrl",
      sound: "fun",
      voiceSpeed: DEFAULT_SETTINGS.voiceSpeed,
      robotSize: DEFAULT_SETTINGS.robotSize,
      reduceMotion: true,
      startWithWindows: DEFAULT_SETTINGS.startWithWindows,
      showHomeAtStart: DEFAULT_SETTINGS.showHomeAtStart,
      spotlight: true,
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
    pttKey: "ctrl",
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
    language: "auto",
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

test("parseSettings snaps voiceSpeed to the 0.05 step (rounded to 2 decimals) and resets out-of-range values", () => {
  const cases: ReadonlyArray<readonly [unknown, number, string]> = [
    [1.03, 1.05, "1.05\u00d7"],
    [0.82, 0.8, "0.8\u00d7"],
    [1.299, 1.3, "1.3\u00d7"],
    [0.875, 0.9, "0.9\u00d7"],
    [0.79, DEFAULT_SETTINGS.voiceSpeed, "1.0\u00d7"],
    [1.31, DEFAULT_SETTINGS.voiceSpeed, "1.0\u00d7"],
    [-0.5, DEFAULT_SETTINGS.voiceSpeed, "1.0\u00d7"],
    [Number.NaN, DEFAULT_SETTINGS.voiceSpeed, "1.0\u00d7"],
  ];

  for (const [input, expectedSpeed, expectedLabel] of cases) {
    const parsed = parseSettings({ voiceSpeed: input });
    assert.equal(parsed.voiceSpeed, expectedSpeed, inspect(input));
    assert.equal(snapVoiceSpeed(input), expectedSpeed, inspect(input));
    assert.equal(formatVoiceSpeed(parsed.voiceSpeed), expectedLabel, inspect(input));
  }
});

test("voiceSpeedPace says slower, usual or faster for the snapped speed", () => {
  assert.equal(voiceSpeedPace(MIN_VOICE_SPEED), "slower");
  assert.equal(voiceSpeedPace(0.95), "slower");
  assert.equal(voiceSpeedPace(1), "usual");
  assert.equal(voiceSpeedPace(1.02), "usual");
  assert.equal(voiceSpeedPace(1.05), "faster");
  assert.equal(voiceSpeedPace(MAX_VOICE_SPEED), "faster");
  // Out of range resets to the usual speed, as formatVoiceSpeed shows it.
  for (const speed of [0.5, 2, Number.NaN]) {
    assert.equal(voiceSpeedPace(speed), "usual", inspect(speed));
  }
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
    pttKey: "ctrl",
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
    profile: { name: "", about: "" },
    profileAsked: false,
    language: "auto",
    theme: "light",
    model: "thorough",
    ptt: "toggle",
    pttKey: "ctrl",
    sound: "fun",
    voiceSpeed: 1,
    robotSize: "L",
    reduceMotion: true,
    startWithWindows: false,
    showHomeAtStart: true,
    spotlight: true,
  });

  const saved = await tauriStore.update({ voiceSpeed: 1.2, startWithWindows: true });
  assert.equal(calls.length, 2);
  assert.equal(calls[1].cmd, "save_settings");
  assert.deepEqual(calls[1].args, { settings: saved });
  assert.equal(saved.voiceSpeed, 1.2);
  assert.equal(saved.startWithWindows, true);
});

test("subscribeSettingsChanged refreshes store state without echoing save_settings", async () => {
  assert.equal(SETTINGS_CHANGED_EVENT, "settings-changed");

  const calls: Array<{ cmd: string; args?: Readonly<Record<string, unknown>> }> = [];
  const store = createSettingsStore(async (cmd, args) => {
    calls.push({ cmd, args });
    return DEFAULT_SETTINGS;
  });

  let emitChanged: ((raw: unknown) => void) | null = null;
  let unsubscribed = false;
  const refreshed: Settings[] = [];

  const stop = subscribeSettingsChanged(
    store,
    async (onChanged) => {
      emitChanged = onChanged;
      return () => {
        unsubscribed = true;
      };
    },
    (updated) => {
      refreshed.push(updated);
    },
  );

  await Promise.resolve();
  assert.notEqual(emitChanged, null);

  emitChanged!({
    language: "auto",
    theme: "light",
    model: "thorough",
    ptt: "toggle",
    pttKey: "ctrl",
    sound: "fun",
    voiceSpeed: 1.03,
    robotSize: "XL",
    reduceMotion: true,
    startWithWindows: true,
  });

  assert.equal(calls.length, 0);
  assert.equal(refreshed.length, 1);
  assert.deepEqual(refreshed[0], {
    profile: { name: "", about: "" },
    profileAsked: false,
    language: "auto",
    theme: "light",
    model: "thorough",
    ptt: "toggle",
    pttKey: "ctrl",
    sound: "fun",
    voiceSpeed: 1.05,
    robotSize: "XL",
    reduceMotion: true,
    startWithWindows: true,
    showHomeAtStart: true,
    spotlight: true,
  });
  assert.deepEqual(store.get(), refreshed[0]);

  stop();
  assert.equal(unsubscribed, true);

  emitChanged!(DEFAULT_SETTINGS);
  assert.equal(refreshed.length, 1);
  assert.equal(store.get().model, "thorough");
});

test("language choices survive saves and invalid or older settings keep other preferences", async () => {
  for (const language of ["auto", "en", "ru"] as const) {
    const store = createSettingsStore(null);
    await store.update({ language, sound: "fun" });
    assert.equal((await store.update({ voiceSpeed: 1.2 })).language, language);
    assert.equal(parseSettings({ language }).language, language);
  }
  for (const language of [undefined, null, "de", "EN", 0, {}]) {
    const parsed = parseSettings({ language, sound: "fun" });
    assert.equal(parsed.language, "auto");
    assert.equal(parsed.sound, "fun");
  }
});
test("external language changes refresh the store without saving back", async () => {
  let emit: ((raw: unknown) => void) | undefined;
  const calls: string[] = [];
  const store = createSettingsStore(async (command) => { calls.push(command); return {}; });
  const languages: string[] = [];
  const stop = subscribeSettingsChanged(store, async (onChanged) => {
    emit = onChanged;
    return () => {};
  }, (settings) => languages.push(settings.language));
  await Promise.resolve();
  for (const language of ["ru", "en", "auto"]) {
    emit!({ ...DEFAULT_SETTINGS, language, model: "thorough" });
    assert.equal(store.get().language, language);
    assert.equal(store.get().model, "thorough");
  }
  assert.deepEqual(languages, ["ru", "en", "auto"]);
  assert.deepEqual(calls, []);
  stop();
});

test("spotlight migrates old settings and persists either choice", () => {
  assert.equal(parseSettings({ sound: "fun" }).spotlight, true);
  for (const spotlight of [true, false]) {
    const settings = updateSettings(DEFAULT_SETTINGS, { spotlight });
    assert.equal(parseSettings(JSON.parse(JSON.stringify(settings))).spotlight, spotlight);
  }
  for (const spotlight of [null, 0, "false"]) assert.equal(parseSettings({ spotlight }).spotlight, true);
});
