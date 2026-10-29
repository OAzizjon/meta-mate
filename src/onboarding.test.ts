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
  return (LOCALES[current]?.[key] ?? key).replaceAll("{name}", "Mate").replaceAll("{key}", LOCALES[current]["settings.key_rightCtrl"]);
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
import { setOnboardingTranslator, setOnboardingPttKey } from "./onboarding.ts";
import { bubbleReadMs } from "./bubble-timing.ts";

function exampleText(key: string): string {
  return t("onboarding.try_asking").replaceAll("{question}", t(key));
}

function stepDuration(text: string): number {
  return Math.max(ONBOARDING_STEP_MS, bubbleReadMs(text));
}

setOnboardingTranslator((key) => t(key));

test("onboarding mentions the click alternative only in the greeting in both languages", () => {
  for (const locale of ["en", "ru"]) {
    resetOnboardingSession();
    setLocale(locale);
    const clock = createFakeClock();
    const shown: string[] = [];
    const controller = startOnboarding(
      { showBubble: (text) => { if (text) shown.push(text); } },
      { clock, storage: createMemoryStorage() },
    );
    const click = t("home.talk_click");
    assert.ok(shown[0].includes(click));
    clock.advance(stepDuration(t("robot.first_launch")));
    assert.equal(shown.filter((text) => text.includes(click)).length, 1);
    assert.equal(shown.at(-1), exampleText("onboarding.example_1"));
    controller.stop();
  }
  setLocale("en");
});

test("the greeting and keycap follow live key changes without restarting examples", () => {
  resetOnboardingSession();
  const clock = createFakeClock();
  const shown: (string | null)[] = [];
  const caps: (string | null)[] = [];
  setOnboardingPttKey("f9");
  const controller = startOnboarding(
    { showBubble: (text) => shown.push(text), showKeycap: (text) => caps.push(text) },
    { clock, storage: createMemoryStorage(), translate: (key) => LOCALES.en[key] },
  );
  assert.equal(caps.at(-1), "F9");
  assert.ok(shown.at(-1)?.includes("F9"));
  setOnboardingPttKey("insert");
  assert.equal(caps.at(-1), "Insert");
  clock.advance(stepDuration(LOCALES.en["robot.first_launch"].replaceAll("{key}", "F9")));
  const count = shown.length;
  setOnboardingPttKey("f10");
  assert.equal(caps.at(-1), "F10");
  assert.equal(shown.length, count);
  controller.stop();
  setOnboardingPttKey("rightCtrl");
});

interface ScheduledTask {
  readonly id: number;
  readonly runAt: number;
  readonly callback: () => void;
}

/** Deterministic fake clock for testing readable step scheduling. */
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

test("each step lasts long enough to read, then hides after three wrapped examples", () => {
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

  // The greeting stays for its full reading time.
  clock.advance(stepDuration(t("robot.first_launch")) - 1);
  assert.deepEqual(shown, [t("robot.first_launch")]);

  clock.advance(1);
  assert.deepEqual(shown, [t("robot.first_launch"), exampleText("onboarding.example_1")]);

  clock.advance(stepDuration(exampleText("onboarding.example_1")));
  assert.deepEqual(shown, [
    t("robot.first_launch"),
    exampleText("onboarding.example_1"),
    exampleText("onboarding.example_2"),
  ]);

  clock.advance(stepDuration(exampleText("onboarding.example_2")));
  assert.deepEqual(shown, [
    t("robot.first_launch"),
    exampleText("onboarding.example_1"),
    exampleText("onboarding.example_2"),
    exampleText("onboarding.example_3"),
  ]);
  assert.equal(controller.isActive(), true);

  clock.advance(stepDuration(exampleText("onboarding.example_3")));
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

  clock.advance(stepDuration(t("robot.first_launch")));
  assert.deepEqual(shown, [t("robot.first_launch"), exampleText("onboarding.example_1")]);
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
  const fakeKeycap = { textContent: "", hidden: true } as unknown as HTMLElement;

  const controller = mountOnboarding(fakeBubble, fakeOnboarding, fakeKeycap, {
    clock,
    storage,
    exampleKeys: ["onboarding.example_4", "onboarding.example_5", "onboarding.example_6"],
  });

  // t = 0: greeting in bubble, keycap shown, onboarding container stays hidden
  assert.equal(controller.isActive(), true);
  assert.equal(fakeBubble.hidden, false);
  assert.equal(fakeBubble.textContent, t("robot.first_launch"));
  assert.equal(fakeKeycap.hidden, false);
  assert.equal(fakeKeycap.textContent, "Ctrl");
  assert.equal(fakeOnboarding.hidden, true);

  // Examples replace the greeting; the printed key label stays visible.
  clock.advance(stepDuration(t("robot.first_launch")));
  assert.equal(fakeBubble.hidden, true);
  assert.equal(fakeKeycap.hidden, false);
  assert.equal(fakeKeycap.textContent, "Ctrl");
  assert.equal(fakeOnboarding.hidden, false);
  assert.equal(fakeOnboarding.textContent, exampleText("onboarding.example_4"));

  clock.advance(stepDuration(exampleText("onboarding.example_4")));
  assert.equal(fakeOnboarding.textContent, exampleText("onboarding.example_5"));

  // stopOnboarding() hides all
  stopOnboarding();
  assert.equal(fakeBubble.hidden, true);
  assert.equal(fakeKeycap.hidden, true);
  assert.equal(fakeOnboarding.hidden, true);
  assert.equal(fakeOnboarding.textContent, "");
});

for (const locale of ["en", "ru"]) {
  for (const stepMs of [1000, 25_000]) {
    test(`${locale}: bubble examples and keycap respect reading time and stepMs=${stepMs}`, () => {
      setLocale(locale);
      const clock = createFakeClock();
      const bubble = { textContent: "", hidden: true } as unknown as HTMLElement;
      const keycap = { textContent: "", hidden: true } as unknown as HTMLElement;
      const controller = mountOnboarding(bubble, null, keycap, {
        clock,
        storage: createMemoryStorage(),
        stepMs,
        exampleKeys: ["onboarding.example_1"],
      });
      const greeting = t("robot.first_launch");
      assert.equal(bubble.textContent, greeting);
      const greetingMs = Math.max(stepMs, bubbleReadMs(greeting));
      clock.advance(greetingMs - 1);
      assert.equal(bubble.textContent, greeting);
      clock.advance(1);
      const example = exampleText("onboarding.example_1");
      assert.equal(bubble.textContent, example);
      assert.equal(bubble.hidden, false);
      assert.equal(keycap.textContent, "Ctrl");
      assert.equal(keycap.hidden, false);
      clock.advance(Math.max(stepMs, bubbleReadMs(example)) - 1);
      assert.equal(bubble.textContent, example);
      assert.equal(controller.isActive(), true);
      clock.advance(1);
      assert.equal(bubble.hidden, true);
      assert.equal(keycap.hidden, true);
      assert.equal(controller.isActive(), false);
      assert.equal(clock.pendingCount(), 0);
    });
  }
}

test("keycap stays through examples and hides on completion or stopOnboarding()", () => {
  const clock = createFakeClock();
  const storage = createMemoryStorage();
  const keycaps: (string | null)[] = [];

  setLocale("ru");
  startOnboarding(
    {
      showBubble: () => {},
      showKeycap: (text) => keycaps.push(text),
    },
    { clock, storage },
  );

  assert.deepEqual(keycaps, ["Ctrl"]);

  // Still shown just before the greeting finishes.
  clock.advance(stepDuration(t("robot.first_launch")) - 1);
  assert.deepEqual(keycaps, ["Ctrl"]);

  // Stays visible when the examples begin, then hides when all steps finish.
  clock.advance(1);
  assert.deepEqual(keycaps, ["Ctrl"]);
  clock.advance(100_000);
  assert.deepEqual(keycaps, ["Ctrl", null]);

  // Stopping early during robot.first_launch also hides the keycap immediately
  resetOnboardingSession();
  const fakeBubble = { textContent: "", hidden: true } as unknown as HTMLElement;
  const fakeOnboarding = { textContent: "", hidden: true } as unknown as HTMLElement;
  const fakeKeycap = { textContent: "", hidden: true } as unknown as HTMLElement;

  mountOnboarding(fakeBubble, fakeOnboarding, fakeKeycap, {
    clock,
    storage: createMemoryStorage(),
  });
  assert.equal(fakeKeycap.hidden, false);
  assert.equal(fakeKeycap.textContent, "Ctrl");

  clock.advance(2000);
  stopOnboarding();
  assert.equal(fakeKeycap.hidden, true);
  assert.equal(fakeKeycap.textContent, "");
});
