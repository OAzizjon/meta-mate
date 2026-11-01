// Pure localized key rendering shared by Home and onboarding.
import type { LocaleKey } from "../i18n/index.ts";
import type { PttKey } from "./model.ts";

/** The key's name inside a sentence: either Ctrl is plain "Ctrl"; only its Settings choice says which side. */
export function pttKeyLabel(key: PttKey, translate: (key: LocaleKey) => string): string {
  return key === "ctrl" ? pttKeycapLabel(key, translate) : translate(`settings.key_${key}` as LocaleKey);
}

export function pttKeycapLabel(key: PttKey, translate: (key: LocaleKey) => string): string {
  return translate(`settings.keycap_${key}` as LocaleKey);
}

export function withPttKey(text: string, key: PttKey, translate: (key: LocaleKey) => string): string {
  return text.replaceAll("{key}", pttKeyLabel(key, translate));
}
