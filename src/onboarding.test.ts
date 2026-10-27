// Run with `npm test` (Node's own test runner; Node 24 strips the types).
// Tests for first-run onboarding (src/onboarding.ts) with a fake clock and storage mocks.
import assert from "node:assert/strict";
import { beforeEach, test } from "node:test";

import { readFileSync } from "node:fs";

// The locale files read directly: src/i18n imports them as JSON modules, which Node's test
// runner cannot load.
type Strings = Record<string, string>;
const LOCALES: Record<string, Strings> = {
  en: JSON.parse(readFileSync(new URL("../locales/en.json", import.meta.url), "utf8")) as Strings,
  ru: JSON.parse(readFileSync(new URL("../locales/ru.json", import.meta.url), "utf8")) as Strings,
};
let current = "en";
function initI18n(_options: { locale: string; robotName: string }): void {
  current = _options.locale;
}
function setLocale(locale: string): void {
  current = locale;
}
function t(key: string): string {
  return (LOCALES[current]?.[key] ?? key).replaceAll("{name}", "Mate");
}
import {
  ONBOARDING_EXAMPLE_COUNT,
  ONBOARDING_EXAMPLE_KEYS,
  ONBOARDING_STEP_MS,
  ONBOARDING_STORAGE_KEY,
  claimFirstRun,
  mountOnboarding,
  pickExampleKeys,
  resetOnboardingSession,
  startOnboarding,
  stopOnboarding,
} from "./onboarding.ts";
import type { OnboardingClock, StorageLike } from "./onboarding.ts";
import { setOnboardingTranslator } from "./onboarding.ts";

setOnboardingTranslator((key) => t(key));

interface ScheduledTask {
  readonly id: number;
  readonly runAt: number;
  readonly callback: () => void;
}

/** Deterministic fake clock for testing 6-second step scheduling. */
function createFakeClock(): OnboardingClock & {
  now(): number;
  pendingCount(): number;
  advance(ms: number): void;
} {
  let currentTime = 0;
  let nextId = 1;
  const tasks = new Map<number, ScheduledTask>();

  return {
    now: () => currentTime,
    pendingCount: () => tasks.size,
    setTimeout(callback: () => void, ms: number): number {
      const id = nextId++;
      tasks.set(id, { id, runAt: currentTime + ms, callback });
      return id;
    },
    clearTimeout(handle: unknown): void {
      if (typeof handle === "number") {
        tasks.delete(handle);
      }
    },
    advance(ms: number): void {
      const target = currentTime + ms;
      while (true) {
        let earliest: ScheduledTask | undefined;
        for (const task of tasks.values()) {
          if (task.runAt <= target && (!earliest || task.runAt < earliest.runAt)) {
            earliest = task;
          }
        }
        if (!earliest) break;
        tasks.delete(earliest.id);
        currentTime = earliest.runAt;
        earliest.callback();
      }
      currentTime = target;
    },
  };
}

function createMemoryStorage(initial: Record<string, string> = {}): StorageLike & {
  readonly data: Map<string, string>;
} {
  const data = new Map<string, string>(Object.entries(initial));
  return {
    data,
    getItem(key: string): string | null {
      return data.get(key) ?? null;
    },
    setItem(key: string, value: string): void {
      data.set(key, value);
    },
  };
}

function createThrowingStorage(mode: "both" | "set_only" = "both"): StorageLike {
  return {
    getItem(): string | null {
      if (mode === "both") {
        throw new Error("SecurityError: storage disabled");
      }
      return null;
    },
    setItem(): void {
      throw new Error("QuotaExceededError: storage full");
    },
  };
}

beforeEach(() => {
  stopOnboarding();
  resetOnboardingSession();
  initI18n({ locale: "en", robotName: "Mate" });
});

test("onboarding.example_1..6 exist in both English and Russian locales", () => {
  assert.equal(ONBOARDING_EXAMPLE_KEYS.length, 6);

  setLocale("en");
  const enExamples = ONBOARDING_EXAMPLE_KEYS.map((key) => t(key));
  assert.deepEqual(enExamples, [
    "Where is the volume control?",
    "How do I make the text bigger?",
    "Where is the zoom slider?",
    "How do I open a new tab?",
    "Where is the Wi-Fi switch?",
    "How do I print this page?",
  ]);

  setLocale("ru");
  for (const key of ONBOARDING_EXAMPLE_KEYS) {
    const ruText = t(key);
    assert.ok(ruText.length > 0, `empty Russian translation for ${key}`);
    assert.match(ruText, /[\u0400-\u04ff]/, `expected Cyrillic in ${key}: ${ruText}`);
    assert.ok(ruText.endsWith("?"), `expected question mark in ${key}: ${ruText}`);
  }
});

test("first launch shows robot.first_launch then 3 examples every 6 s, then hides", () => {
  const clock = createFakeClock();
  const storage = createMemoryStorage();
  const shown: (string | null)[] = [];

  const controller = startOnboarding(
    { showBubble: (text) => shown.push(text) },
    { clock, storage },
  );

  assert.equal(controller.isActive(), true);
  assert.equal(storage.getItem(ONBOARDING_STORAGE_KEY), "1");
  assert.deepEqual(shown, [t("robot.first_launch")]);

  // Before 6 s, still on first_launch
  clock.advance(ONBOARDING_STEP_MS - 1);
  assert.deepEqual(shown, [t("robot.first_launch")]);

  // At 6 s -> example 1
  clock.advance(1);
  assert.deepEqual(shown, [t("robot.first_launch"), t("onboarding.example_1")]);

  // At 12 s -> example 2
  clock.advance(ONBOARDING_STEP_MS);
  assert.deepEqual(shown, [
    t("robot.first_launch"),
    t("onboarding.example_1"),
    t("onboarding.example_2"),
  ]);

  // At 18 s -> example 3
  clock.advance(ONBOARDING_STEP_MS);
  assert.deepEqual(shown, [
    t("robot.first_launch"),
    t("onboarding.example_1"),
    t("onboarding.example_2"),
    t("onboarding.example_3"),
  ]);
  assert.equal(controller.isActive(), true);

  // At 24 s -> finished and hidden
  clock.advance(ONBOARDING_STEP_MS);
  assert.equal(shown[shown.length - 1], null);
  assert.equal(controller.isActive(), false);
  assert.equal(clock.pendingCount(), 0);
});

test("second launch with persisted localStorage flag does not show onboarding", () => {
  const clock = createFakeClock();
  const storage = createMemoryStorage({ [ONBOARDING_STORAGE_KEY]: "1" });
  const shown: (string | null)[] = [];

  const controller = startOnboarding(
    { showBubble: (text) => shown.push(text) },
    { clock, storage },
  );

  assert.equal(controller.isActive(), false);
  assert.deepEqual(shown, []);
  assert.equal(clock.pendingCount(), 0);
});

test("when localStorage throws, onboarding runs once per session and not a second time", () => {
  const clock = createFakeClock();
  const brokenStorage = createThrowingStorage("both");
  const shownFirst: (string | null)[] = [];

  const first = startOnboarding(
    { showBubble: (text) => shownFirst.push(text) },
    { clock, storage: brokenStorage },
  );
  assert.equal(first.isActive(), true);
  assert.deepEqual(shownFirst, [t("robot.first_launch")]);

  first.stop();

  const shownSecond: (string | null)[] = [];
  const second = startOnboarding(
    { showBubble: (text) => shownSecond.push(text) },
    { clock, storage: brokenStorage },
  );
  assert.equal(second.isActive(), false);
  assert.deepEqual(shownSecond, []);
});

test("when localStorage.setItem throws, onboarding still runs once per session", () => {
  const clock = createFakeClock();
  const quotaStorage = createThrowingStorage("set_only");

  assert.equal(claimFirstRun(quotaStorage), true);
  assert.equal(claimFirstRun(quotaStorage), false);

  const shown: (string | null)[] = [];
  const controller = startOnboarding(
    { showBubble: (text) => shown.push(text) },
    { clock, storage: quotaStorage },
  );
  assert.equal(controller.isActive(), false);
  assert.deepEqual(shown, []);
});

test("stopOnboarding() cancels timers immediately when the first answer stream message arrives", () => {
  const clock = createFakeClock();
  const storage = createMemoryStorage();
  const shown: (string | null)[] = [];

  const controller = startOnboarding(
    { showBubble: (text) => shown.push(text) },
    { clock, storage },
  );

  // Advance to first example (t = 6 s)
  clock.advance(ONBOARDING_STEP_MS);
  assert.deepEqual(shown, [t("robot.first_launch"), t("onboarding.example_1")]);
  assert.equal(clock.pendingCount(), 1);

  // First answer stream message arrives -> mate.ts calls stopOnboarding()
  stopOnboarding();
  assert.equal(controller.isActive(), false);
  assert.equal(clock.pendingCount(), 0);
  assert.equal(shown[shown.length - 1], null);

  // Advancing the clock further produces no more examples
  const countAfterStop = shown.length;
  clock.advance(ONBOARDING_STEP_MS * 10);
  assert.equal(shown.length, countAfterStop);
});

test("pickExampleKeys selects 3 distinct keys from onboarding.example_1..6", () => {
  const defaultThree = pickExampleKeys();
  assert.equal(defaultThree.length, ONBOARDING_EXAMPLE_COUNT);
  assert.deepEqual(defaultThree, [
    "onboarding.example_1",
    "onboarding.example_2",
    "onboarding.example_3",
  ]);

  // Reverse-ish deterministic random
  let seed = 0.85;
  const picked = pickExampleKeys(3, () => {
    seed = (seed + 0.37) % 1;
    return seed;
  });
  assert.equal(picked.length, 3);
  assert.equal(new Set(picked).size, 3);
  for (const key of picked) {
    assert.ok((ONBOARDING_EXAMPLE_KEYS as readonly string[]).includes(key));
  }
});

test("mountOnboarding updates DOM elements using textContent and hidden attribute", () => {
  const clock = createFakeClock();
  const storage = createMemoryStorage();

  const fakeBubble = { textContent: "", hidden: true } as unknown as HTMLElement;
  const fakeOnboarding = { textContent: "", hidden: true } as unknown as HTMLElement;

  const controller = mountOnboarding(fakeBubble, fakeOnboarding, {
    clock,
    storage,
    exampleKeys: ["onboarding.example_4", "onboarding.example_5", "onboarding.example_6"],
  });

  // t = 0: greeting in bubble, onboarding container stays hidden
  assert.equal(controller.isActive(), true);
  assert.equal(fakeBubble.hidden, false);
  assert.equal(fakeBubble.textContent, t("robot.first_launch"));
  assert.equal(fakeOnboarding.hidden, true);

  // t = 6 s: bubble hides, onboarding container shows example_4
  clock.advance(ONBOARDING_STEP_MS);
  assert.equal(fakeBubble.hidden, true);
  assert.equal(fakeOnboarding.hidden, false);
  assert.equal(fakeOnboarding.textContent, t("onboarding.example_4"));

  // t = 12 s: example_5
  clock.advance(ONBOARDING_STEP_MS);
  assert.equal(fakeOnboarding.textContent, t("onboarding.example_5"));

  // stopOnboarding() hides both
  stopOnboarding();
  assert.equal(fakeBubble.hidden, true);
  assert.equal(fakeOnboarding.hidden, true);
  assert.equal(fakeOnboarding.textContent, "");
});
