// Localization loader, language resolution, and typed translation helpers (invariant 7).

import en from "../../locales/en.json";
import ru from "../../locales/ru.json";
import { interpolate, type TemplateVars } from "./interpolate.ts";

export { interpolate, type TemplateVars };

export type LocaleCode = "en" | "ru";
export type LocaleKey = keyof typeof en;

/**
 * Product and robot name defaults matching `crates/mm-core/src/constants.rs` (invariant 6).
 * Only exported as constants and default init config; `interpolate` always uses the runtime
 * robot name configured via `initI18n`.
 */
export const APP_NAME = "MetaMate";
export const ROBOT_NAME = "Mate";

/**
 * Teacher line identifiers matching `crates/mm-core/src/teacher/line.rs`.
 */
export type TeacherLineKey =
  | "WrongPlace"
  | "RightClick"
  | "CantFind"
  | "StepN"
  | "Done"
  | "NetworkError"
  | "TooManyTurns"
  | "WontPress";

export interface StepVars {
  readonly n: number;
  readonly m: number;
  readonly instruction: string;
}

export interface I18nInitOptions {
  /** Explicit language setting (`"en"`, `"ru"`, `"auto"`, or omitted). */
  readonly locale?: string | null;
  /** Robot name injected for `{name}` placeholders (passed in at init). */
  readonly robotName?: string;
  /** Product name injected when needed. */
  readonly appName?: string;
  /** Optional override for `navigator.language` in headless tests. */
  readonly navigatorLanguage?: string | null;
  /** Optional override for dev-mode detection in tests. */
  readonly isDev?: boolean;
}

type LocaleTable = Readonly<Record<string, string>>;

const CATALOGS: Readonly<Record<LocaleCode, LocaleTable>> = {
  en,
  ru,
};

const LINE_KEY_MAP: Readonly<Record<TeacherLineKey, LocaleKey>> = {
  WrongPlace: "robot.wrong_place",
  RightClick: "robot.right_click",
  CantFind: "robot.cant_find",
  StepN: "robot.step",
  Done: "robot.done",
  NetworkError: "robot.network_error",
  TooManyTurns: "robot.too_many_turns",
  WontPress: "robot.wont_press",
};

function detectDevMode(): boolean {
  try {
    const meta = import.meta as ImportMeta & {
      readonly env?: { readonly DEV?: boolean };
    };
    if (typeof meta.env?.DEV === "boolean") {
      return meta.env.DEV;
    }
  } catch {
    // Ignore when running outside Vite.
  }
  return false;
}

function readNavigatorLanguage(): string | undefined {
  if (typeof navigator !== "undefined" && typeof navigator.language === "string") {
    return navigator.language;
  }
  return undefined;
}

function parseLocaleTag(tag: string | null | undefined): LocaleCode | undefined {
  if (!tag) {
    return undefined;
  }
  const normalized = tag.trim().toLowerCase();
  if (!normalized || normalized === "auto" || normalized === "automatic") {
    return undefined;
  }
  const primary = normalized.split(/[-_]/)[0];
  if (primary === "ru") {
    return "ru";
  }
  if (primary === "en") {
    return "en";
  }
  return undefined;
}

/**
 * Picks the active language:
 * 1. Explicit setting (`"en"` or `"ru"`; `"auto"`/empty falls through)
 * 2. `navigator.language` (or `navigatorLanguage` override)
 * 3. `"en"` fallback
 */
export function resolveLocale(
  explicit?: string | null,
  navigatorLanguage: string | null | undefined = readNavigatorLanguage(),
): LocaleCode {
  return (
    parseLocaleTag(explicit) ??
    parseLocaleTag(navigatorLanguage) ??
    "en"
  );
}

let currentLocale: LocaleCode = resolveLocale();
let currentRobotName: string = ROBOT_NAME;
let currentAppName: string = APP_NAME;
let devMode: boolean = detectDevMode();
const loggedMissingKeys = new Set<string>();

/**
 * Initializes the i18n state with an optional explicit locale setting and the
 * robot name used for `{name}` substitutions.
 */
export function initI18n(options: I18nInitOptions = {}): LocaleCode {
  if (options.robotName !== undefined && options.robotName.length > 0) {
    currentRobotName = options.robotName;
  }
  if (options.appName !== undefined && options.appName.length > 0) {
    currentAppName = options.appName;
  }
  if (options.isDev !== undefined) {
    devMode = options.isDev;
  }
  const navLang =
    options.navigatorLanguage !== undefined
      ? options.navigatorLanguage
      : readNavigatorLanguage();
  currentLocale = resolveLocale(options.locale, navLang);
  return currentLocale;
}

export function getLocale(): LocaleCode {
  return currentLocale;
}

export function setLocale(locale: LocaleCode | "auto" | null | undefined): LocaleCode {
  currentLocale = resolveLocale(locale);
  return currentLocale;
}

export function getRobotName(): string {
  return currentRobotName;
}

export function getAppName(): string {
  return currentAppName;
}

/** Clears the once-per-key dev warning cache (for unit tests). */
export function resetMissingKeyWarnings(): void {
  loggedMissingKeys.clear();
}

function logMissingOnce(locale: LocaleCode, key: string): void {
  if (!devMode) {
    return;
  }
  const cacheKey = `${locale}:${key}`;
  if (loggedMissingKeys.has(cacheKey)) {
    return;
  }
  loggedMissingKeys.add(cacheKey);
  console.warn(`[i18n] Missing translation for key "${key}" in locale "${locale}", falling back to "en".`);
}

/**
 * Looks up `key` in the active locale catalog (falling back to English and
 * logging once in dev if missing) and substitutes `{placeholder}` tokens.
 * `{name}` defaults to the robot name passed in at `initI18n`.
 */
export function t(key: LocaleKey, vars?: TemplateVars): string {
  const stringKey = String(key);
  const activeCatalog = CATALOGS[currentLocale];
  let template: string | undefined = Object.prototype.hasOwnProperty.call(
    activeCatalog,
    stringKey,
  )
    ? activeCatalog[stringKey]
    : undefined;

  if (template === undefined) {
    logMissingOnce(currentLocale, stringKey);
    const enCatalog = CATALOGS.en;
    template = Object.prototype.hasOwnProperty.call(enCatalog, stringKey)
      ? enCatalog[stringKey]
      : stringKey;
  }

  return interpolate(template, currentRobotName, vars);
}

/**
 * Translates a `TeacherLineKey` (`crates/mm-core/src/teacher/line.rs`) into the
 * localized user-facing string, substituting `{n}`, `{m}`, and `{instruction}` for `StepN`.
 */
export function tLine(line: TeacherLineKey, step?: StepVars): string {
  const key = LINE_KEY_MAP[line];
  if (line === "StepN" && step !== undefined) {
    return t(key, {
      n: step.n,
      m: step.m,
      instruction: step.instruction,
    });
  }
  return t(key);
}
