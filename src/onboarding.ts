// First-run onboarding in Mate's window (WP25).
// On the very first launch (a flag in localStorage, wrapped in try/catch; when storage fails,
// once per session in memory), Mate's bubble shows `robot.first_launch` and then, one at a time
// at a readable pace, three example questions (`onboarding.example_1..6`). Stops as soon as the first
// answer stream message arrives via `stopOnboarding()`. Pure scheduling logic is testable with
// a fake clock and without DOM or Tauri dependencies.

import type { LocaleKey } from "./i18n/index.ts";
import type { PttKey } from "./settings/model.ts";
import { pttKeycapLabel, withPttKey } from "./settings/ptt-key.ts";
import { bubbleReadMs } from "./bubble-timing.ts";

let pttKey: PttKey = "ctrl";
let refreshKey: (() => void) | null = null;
export function setOnboardingPttKey(key: PttKey): void {
  pttKey = key;
  refreshKey?.();
}

export const ONBOARDING_STORAGE_KEY = "metamate.onboarded";

// The locale lookup comes from outside (src/mate.ts passes i18n's `t`): importing src/i18n
// here would load the locale JSON, which Node's test runner cannot import.
let translator = (key: LocaleKey): string => key;
export function setOnboardingTranslator(translate: (key: LocaleKey) => string): void {
  translator = translate;
}
export const ONBOARDING_STEP_MS = 8000;
export const ONBOARDING_EXAMPLE_COUNT = 3;

export const ONBOARDING_EXAMPLE_KEYS = [
  "onboarding.example_1",
  "onboarding.example_2",
  "onboarding.example_3",
  "onboarding.example_4",
  "onboarding.example_5",
  "onboarding.example_6",
] as const satisfies readonly LocaleKey[];

export type OnboardingExampleKey = (typeof ONBOARDING_EXAMPLE_KEYS)[number];

/** Minimal storage contract matching `window.localStorage`. */
export interface StorageLike {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

/** Injectable clock contract for deterministic timer testing. */
export interface OnboardingClock {
  setTimeout(callback: () => void, ms: number): unknown;
  clearTimeout(handle: unknown): void;
}

/**
 * Pure view sink: `showBubble` drives `#mate-bubble`, `showExample` drives `#mate-onboarding`
 * (if separate), and `showKeycap` drives `#mate-keycap`.
 */
export interface OnboardingSink {
  /** Updates the main speech bubble (`null` hides it). */
  showBubble(text: string | null): void;
  /** Optional separate sink for example questions; defaults to `showBubble` when omitted. */
  showExample?(text: string | null): void;
  /** Optional sink for the first-launch keycap (`null` hides it). */
  showKeycap?(text: string | null): void;
}

export interface OnboardingOptions {
  /** Overrides `window.localStorage` (pass `null` or a throwing mock to test storage failure). */
  readonly storage?: StorageLike | null;
  /** Overrides the global timer functions. */
  readonly clock?: OnboardingClock;
  /** Overrides the minimum step interval (defaults to `ONBOARDING_STEP_MS = 6000`). */
  readonly stepMs?: number;
  /** Overrides the 3 example keys shown after `robot.first_launch` (or pass a picker). */
  readonly exampleKeys?: readonly OnboardingExampleKey[];
  /** Optional random number generator (`0 <= r < 1`) used when picking 3 examples from the 6. */
  readonly random?: () => number;
  /** Overrides translation lookup `t(key)` if needed in isolated tests. */
  readonly translate?: (key: LocaleKey) => string;
  /** Optional keycap DOM element override (`#mate-keycap`) for `mountOnboarding`. */
  readonly keycapElement?: HTMLElement | null;
}

export interface OnboardingController {
  /** Returns `true` while onboarding is actively displaying or waiting for its next timer. */
  isActive(): boolean;
  /** Immediately stops timers and hides any onboarding text still on screen. */
  stop(): void;
}

const DEFAULT_CLOCK: OnboardingClock = {
  setTimeout: (cb, ms) => globalThis.setTimeout(cb, ms),
  clearTimeout: (id) => globalThis.clearTimeout(id as ReturnType<typeof setTimeout>),
};

/** Session fallback flag when `localStorage` is unavailable or throws. */
let sessionShown = false;

/** Resets the in-memory session fallback (for unit tests). */
export function resetOnboardingSession(): void {
  sessionShown = false;
}

function defaultStorage(): StorageLike | null {
  try {
    if (typeof window !== "undefined" && window.localStorage) {
      return window.localStorage;
    }
  } catch {
    // Access to window.localStorage itself can throw SecurityError.
  }
  return null;
}

/**
 * Checks whether first-run onboarding should run and records the launch flag.
 * Wraps `localStorage` in `try/catch`; when storage is missing or throws, falls back to
 * showing onboarding once per session in memory.
 */
export function claimFirstRun(storage: StorageLike | null | undefined = defaultStorage()): boolean {
  if (storage) {
    try {
      const existing = storage.getItem(ONBOARDING_STORAGE_KEY);
      if (existing !== null) {
        sessionShown = true;
        return false;
      }
      storage.setItem(ONBOARDING_STORAGE_KEY, "1");
      sessionShown = true;
      return true;
    } catch {
      // Storage read or write failed (e.g. quota or disabled cookies); fall through to session flag.
    }
  }
  if (sessionShown) {
    return false;
  }
  sessionShown = true;
  return true;
}

/**
 * Picks `count` distinct example keys in order (defaults to the first 3 when `random` is omitted,
 * or shuffles with `random` when provided).
 */
export function pickExampleKeys(
  count: number = ONBOARDING_EXAMPLE_COUNT,
  random?: () => number,
): readonly OnboardingExampleKey[] {
  const clamped = Math.max(0, Math.min(count, ONBOARDING_EXAMPLE_KEYS.length));
  if (!random) {
    return ONBOARDING_EXAMPLE_KEYS.slice(0, clamped);
  }
  const pool: OnboardingExampleKey[] = [...ONBOARDING_EXAMPLE_KEYS];
  for (let i = 0; i < clamped; i++) {
    const r = random();
    const normalized = Number.isFinite(r) ? Math.max(0, Math.min(0.999999, r)) : 0;
    const j = i + Math.floor(normalized * (pool.length - i));
    const tmp = pool[i];
    pool[i] = pool[j];
    pool[j] = tmp;
  }
  return pool.slice(0, clamped);
}

let activeController: OnboardingController | null = null;

/**
 * Starts the pure onboarding schedule if this is the first launch.
 * Step 0 (t = 0): shows `robot.first_launch` in the bubble.
 * Steps 1..3: shows three wrapped example questions one at a time, keeping the keycap visible.
 * Each step lasts at least `stepMs` (default 6000 ms) and its text's reading time.
 * Step 4: hides the onboarding text and keycap after the third example.
 * Calling `stop()` (or `stopOnboarding()`) at any time cancels pending timers and hides the text.
 */
export function startOnboarding(
  sink: OnboardingSink,
  options: OnboardingOptions = {},
): OnboardingController {
  // If a previous controller was running, stop it cleanly first.
  activeController?.stop();

  const storage = options.storage !== undefined ? options.storage : defaultStorage();
  const clock = options.clock ?? DEFAULT_CLOCK;
  const stepMs = options.stepMs ?? ONBOARDING_STEP_MS;
  const translate = options.translate ?? ((key: LocaleKey) => translator(key));
  const examples =
    options.exampleKeys !== undefined
      ? options.exampleKeys.slice(0, ONBOARDING_EXAMPLE_COUNT)
      : pickExampleKeys(ONBOARDING_EXAMPLE_COUNT, options.random);

  if (!claimFirstRun(storage)) {
    const inert: OnboardingController = {
      isActive: () => false,
      stop: () => {},
    };
    activeController = inert;
    return inert;
  }

  let active = true;
  let timer: unknown = undefined;
  let stepIndex = 0;

  const showExample = sink.showExample
    ? (text: string | null) => sink.showExample!(text)
    : (text: string | null) => sink.showBubble(text);

  const clearTimer = (): void => {
    if (timer !== undefined) {
      clock.clearTimeout(timer);
      timer = undefined;
    }
  };

  const hideAll = (): void => {
    refreshKey = null;
    sink.showBubble(null);
    if (sink.showExample) {
      sink.showExample(null);
    }
    sink.showKeycap?.(null);
  };

  const advance = (): void => {
    timer = undefined;
    if (!active) return;

    if (stepIndex < examples.length) {
      const key = examples[stepIndex];
      stepIndex += 1;
      const text = translate("onboarding.try_asking").replaceAll("{question}", translate(key));
      if (sink.showExample) {
        // Keep the greeting or clear it if the example uses its own element;
        // showing the example in #mate-onboarding while hiding #mate-bubble avoids two bubbles at once,
        // or if showExample === showBubble it replaces the bubble text directly.
        sink.showBubble(null);
        showExample(text);
      } else {
        sink.showBubble(text);
      }
      // A sink may have stopped onboarding meanwhile: no timer after a stop.
      if (!active) return;
      timer = clock.setTimeout(advance, Math.max(stepMs, bubbleReadMs(text)));
    } else {
      active = false;
      hideAll();
    }
  };

  // The greeting mentions click-to-talk once; examples keep only the talk-key keycap.
  // Initial step: show `robot.first_launch` and the talk-key keycap immediately.
  if (sink.showExample) {
    sink.showExample(null);
  }
  refreshKey = () => {
    if (stepIndex === 0) {
      sink.showBubble(withPttKey(translate("robot.first_launch"), pttKey, translate));
    }
    sink.showKeycap?.(pttKeycapLabel(pttKey, translate));
  };
  refreshKey();
  const greeting = withPttKey(translate("robot.first_launch"), pttKey, translate);
  timer = clock.setTimeout(advance, Math.max(stepMs, bubbleReadMs(greeting)));

  const controller: OnboardingController = {
    isActive: () => active,
    stop() {
      if (!active) return;
      active = false;
      clearTimer();
      hideAll();
    },
  };

  activeController = controller;
  return controller;
}

function isElementLike(value: unknown): value is HTMLElement {
  return (
    typeof value === "object" &&
    value !== null &&
    ("textContent" in value || "hidden" in value)
  );
}

/**
 * Mounts first-run onboarding onto DOM elements in `mate.html` (`#mate-bubble`,
 * `#mate-onboarding`, and `#mate-keycap`). Uses `textContent` only (never `innerHTML`)
 * and toggles `hidden`.
 */
export function mountOnboarding(
  bubbleElement?: HTMLElement | null,
  onboardingElement?: HTMLElement | null,
  keycapOrOptions?: HTMLElement | OnboardingOptions | null,
  maybeOptions?: OnboardingOptions,
): OnboardingController {
  const hasExplicitKeycap =
    keycapOrOptions === null || isElementLike(keycapOrOptions);
  const options: OnboardingOptions = hasExplicitKeycap
    ? (maybeOptions ?? {})
    : (keycapOrOptions ?? {});
  const explicitKeycap = hasExplicitKeycap
    ? keycapOrOptions
    : options.keycapElement;

  const bubble =
    bubbleElement !== undefined
      ? bubbleElement
      : typeof document !== "undefined"
        ? document.getElementById("mate-bubble")
        : null;
  const onboarding =
    onboardingElement !== undefined
      ? onboardingElement
      : typeof document !== "undefined"
        ? document.getElementById("mate-onboarding")
        : null;
  const keycap =
    explicitKeycap !== undefined
      ? explicitKeycap
      : typeof document !== "undefined"
        ? document.getElementById("mate-keycap")
        : null;

  const writeEl = (el: HTMLElement | null, text: string | null): void => {
    if (!el) return;
    el.textContent = text ?? "";
    el.hidden = text === null;
  };

  // If both `#mate-bubble` and `#mate-onboarding` exist, `robot.first_launch` shows in the bubble
  // and the example questions show in `#mate-onboarding` (or if only one element is passed, both
  // use that element).
  const targetBubble = bubble ?? onboarding;
  const targetExample = onboarding && onboarding !== targetBubble ? onboarding : undefined;

  return startOnboarding(
    {
      showBubble: (text) => writeEl(targetBubble, text),
      showExample: targetExample ? (text) => writeEl(targetExample, text) : undefined,
      showKeycap: keycap ? (text) => writeEl(keycap, text) : undefined,
    },
    options,
  );
}

/**
 * Stops any running first-run onboarding immediately. Exported for `src/mate.ts` to call
 * as soon as the first answer stream message arrives.
 */
export function stopOnboarding(): void {
  activeController?.stop();
  activeController = null;
}
