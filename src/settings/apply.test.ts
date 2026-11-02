// Run with `npm test` (Node's own test runner; Node 24 strips the types).
// Unit tests for applying settings to Mate's WebView (src/settings/apply.ts, WP33).
import assert from "node:assert/strict";
import { test } from "node:test";

import {
  MATE_WINDOW_SIZE_PX,
  REDUCE_MOTION_CLASS,
  SETTINGS_CHANGED_EVENT,
  applySettingsToMate,
  doesRobotSizeFit,
  fitRobotSize,
  robotSizeDimensions,
  startMateSettings,
  toAudioSettings,
} from "./apply.ts";
import type { MateAudioSettings } from "./apply.ts";
import { DEFAULT_SETTINGS } from "./model.ts";
import type { RobotSize, Settings } from "./model.ts";

function createFakeRoot() {
  const classes = new Set<string>();
  return {
    classes,
    has(token: string): boolean {
      return classes.has(token);
    },
    classList: {
      toggle(token: string, force?: boolean): boolean {
        const next = force === undefined ? !classes.has(token) : force;
        if (next) classes.add(token);
        else classes.delete(token);
        return next;
      },
    },
  };
}

function createFakeTargets() {
  const docRoot = createFakeRoot();
  const containerRoot = createFakeRoot();
  const robotState: {
    reduceMotion: boolean;
    funMode: boolean;
    size: RobotSize;
    calls: string[];
  } = {
    reduceMotion: false,
    funMode: false,
    size: "M",
    calls: [],
  };
  const audioCalls: MateAudioSettings[] = [];

  return {
    docRoot,
    containerRoot,
    robotState,
    audioCalls,
    robot: {
      setReduceMotion(reduce: boolean) {
        robotState.reduceMotion = reduce;
        robotState.calls.push(`reduceMotion:${String(reduce)}`);
      },
      setFunMode(fun: boolean) {
        robotState.funMode = fun;
        robotState.calls.push(`funMode:${String(fun)}`);
      },
      setSize(size: RobotSize) {
        robotState.size = size;
        robotState.calls.push(`size:${size}`);
      },
    },
    audio: {
      setSettings(settings: MateAudioSettings) {
        audioCalls.push(settings);
      },
    },
  };
}

test("robotSizeDimensions and doesRobotSizeFit match Mate's 260x240 viewBox", () => {
  assert.deepEqual(robotSizeDimensions("S"), { width: 64, height: 59 });
  assert.deepEqual(robotSizeDimensions("M"), { width: 96, height: 89 });
  assert.deepEqual(robotSizeDimensions("L"), { width: 160, height: 148 });
  assert.deepEqual(robotSizeDimensions("XL"), { width: 220, height: 203 });

  const mateWindow = { width: MATE_WINDOW_SIZE_PX, height: MATE_WINDOW_SIZE_PX };
  for (const size of ["S", "M", "L", "XL"] as const) {
    assert.equal(doesRobotSizeFit(size, mateWindow), true, size);
    assert.equal(fitRobotSize(size, mateWindow), size, size);
    assert.equal(fitRobotSize(size, null), size, size);
    assert.equal(fitRobotSize(size, { width: 0, height: 0 }), size, size);
  }

  assert.equal(doesRobotSizeFit("S", { width: 0, height: 100 }), false);
  assert.equal(doesRobotSizeFit("S", { width: Number.NaN, height: 100 }), false);
});

test("fitRobotSize scales down only when the requested preset exceeds the window", () => {
  // 180x180 fits S, M, L (160x148), but not XL (220x203)
  assert.equal(fitRobotSize("XL", { width: 180, height: 180 }), "L");
  assert.equal(fitRobotSize("L", { width: 180, height: 180 }), "L");

  // 220x190 is wide enough for XL (220), but too short for XL's height (203)
  assert.equal(fitRobotSize("XL", { width: 220, height: 190 }), "L");

  // 120x120 fits S and M (96x89), but not L (160x148) or XL
  assert.equal(fitRobotSize("XL", { width: 120, height: 120 }), "M");
  assert.equal(fitRobotSize("L", { width: 120, height: 120 }), "M");
  assert.equal(fitRobotSize("M", { width: 120, height: 120 }), "M");

  // 70x70 fits only S (64x59)
  assert.equal(fitRobotSize("XL", { width: 70, height: 70 }), "S");
  assert.equal(fitRobotSize("M", { width: 70, height: 70 }), "S");
  assert.equal(fitRobotSize("S", { width: 70, height: 70 }), "S");

  // Smaller than S falls back to S
  assert.equal(fitRobotSize("XL", { width: 40, height: 40 }), "S");
});

test("applySettingsToMate parses untrusted input and applies reduceMotion, sound, and robotSize", () => {
  const t = createFakeTargets();

  const applied = applySettingsToMate(
    {
      model: "thorough",
      ptt: "toggle",
      sound: "fun",
      voiceSpeed: 1.13, // snaps to 1.15 via parseSettings
      robotSize: "L",
      reduceMotion: true,
      startWithWindows: false,
      extra: "ignored",
    },
    {
      roots: [t.docRoot, t.containerRoot],
      robot: t.robot,
      audio: t.audio,
      viewport: { width: 220, height: 220 },
    },
  );

  assert.equal(applied.settings.sound, "fun");
  assert.equal(applied.settings.voiceSpeed, 1.15);
  assert.equal(applied.effectiveRobotSize, "L");
  assert.equal(applied.rootReduceMotion, true);
  assert.equal(applied.funMode, true);
  assert.equal(t.docRoot.has(REDUCE_MOTION_CLASS), true);
  assert.equal(t.containerRoot.has(REDUCE_MOTION_CLASS), true);
  assert.equal(t.robotState.reduceMotion, true);
  assert.equal(t.robotState.funMode, true);
  assert.equal(t.robotState.size, "L");
  assert.deepEqual(t.audioCalls, [{ mode: "fun", voiceSpeed: 1.15, reduceMotion: true }]);

  // Switching back to calm and reduceMotion: false while OS prefers-reduced-motion is true
  const second = applySettingsToMate(
    {
      sound: "calm",
      robotSize: "XL",
      reduceMotion: false,
    },
    {
      roots: [t.docRoot, t.containerRoot],
      robot: t.robot,
      audio: t.audio,
      viewport: () => ({ width: 150, height: 150 }), // XL -> M
      prefersReducedMotion: () => true,
    },
  );

  assert.equal(second.effectiveRobotSize, "M");
  assert.equal(second.funMode, false);
  assert.equal(second.rootReduceMotion, true);
  assert.equal(t.docRoot.has(REDUCE_MOTION_CLASS), true);
  assert.equal(t.robotState.reduceMotion, false);
  assert.equal(t.robotState.funMode, false);
  assert.equal(t.robotState.size, "M");
  assert.deepEqual(t.audioCalls[1], {
    mode: "calm",
    voiceSpeed: DEFAULT_SETTINGS.voiceSpeed,
    reduceMotion: false,
  });
});

test("startMateSettings uses defaults outside Tauri and syncs late-attached audio", async () => {
  const t = createFakeTargets();
  const binding = startMateSettings({
    roots: [t.docRoot, t.containerRoot],
    robot: t.robot,
  });

  assert.deepEqual(binding.get(), DEFAULT_SETTINGS);
  assert.equal(t.docRoot.has(REDUCE_MOTION_CLASS), false);
  assert.equal(t.robotState.reduceMotion, false);
  assert.equal(t.robotState.funMode, false);
  assert.equal(t.robotState.size, "M");

  const loaded = await binding.ready;
  assert.deepEqual(loaded, DEFAULT_SETTINGS);

  binding.setAudio(t.audio);
  assert.deepEqual(t.audioCalls, [toAudioSettings(DEFAULT_SETTINGS)]);
  binding.destroy();
});

test("startMateSettings loads once via invoke('load_settings') and listens for settings-changed", async () => {
  assert.equal(SETTINGS_CHANGED_EVENT, "settings-changed");

  const t = createFakeTargets();
  const invokeCalls: string[] = [];
  let emitChanged: ((raw: unknown) => void) | null = null;
  let unsubscribed = false;

  const binding = startMateSettings(
    {
      root: t.docRoot,
      robot: t.robot,
      viewport: { width: 220, height: 220 },
    },
    {
      invoke: async (cmd) => {
        invokeCalls.push(cmd);
        return {
          model: "thorough",
          ptt: "toggle",
          sound: "fun",
          voiceSpeed: 1.2,
          robotSize: "XL",
          reduceMotion: true,
          startWithWindows: false,
          showHomeAtStart: true,
          developerMode: false,
          spotlight: true,
          theme: "light",
        } satisfies Settings;
      },
      subscribe: async (onChanged) => {
        emitChanged = onChanged;
        return () => {
          unsubscribed = true;
        };
      },
    },
  );

  const loaded = await binding.ready;
  assert.deepEqual(invokeCalls, ["load_settings"]);
  assert.equal(loaded.sound, "fun");
  assert.equal(loaded.robotSize, "XL");
  assert.equal(loaded.reduceMotion, true);
  assert.equal(t.docRoot.has(REDUCE_MOTION_CLASS), true);
  assert.equal(t.robotState.reduceMotion, true);
  assert.equal(t.robotState.funMode, true);
  assert.equal(t.robotState.size, "XL");

  // Audio engine starts after load_settings and receives the loaded settings immediately
  binding.setAudio(t.audio);
  assert.equal(t.audioCalls.length, 1);
  assert.deepEqual(t.audioCalls[0], {
    mode: "fun",
    voiceSpeed: 1.2,
    reduceMotion: true,
  });

  // Emit settings-changed event -> parsed with parseSettings and applied everywhere
  assert.notEqual(emitChanged, null);
  emitChanged!({
    sound: "calm",
    voiceSpeed: 0.92, // snaps to 0.9
    robotSize: "S",
    reduceMotion: false,
  });

  assert.equal(t.docRoot.has(REDUCE_MOTION_CLASS), false);
  assert.equal(t.robotState.reduceMotion, false);
  assert.equal(t.robotState.funMode, false);
  assert.equal(t.robotState.size, "S");
  assert.equal(t.audioCalls.length, 2);
  assert.deepEqual(t.audioCalls[1], {
    mode: "calm",
    voiceSpeed: 0.9,
    reduceMotion: false,
  });
  // No additional invoke calls when receiving settings-changed
  assert.deepEqual(invokeCalls, ["load_settings"]);

  binding.destroy();
  assert.equal(unsubscribed, true);

  // Events after destroy are ignored
  emitChanged!({ sound: "fun", robotSize: "L", reduceMotion: true });
  assert.equal(t.robotState.size, "S");
  assert.equal(t.audioCalls.length, 2);
});
