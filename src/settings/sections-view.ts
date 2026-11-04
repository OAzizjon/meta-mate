// The Settings window's sections (src/settings.ts): the long list in six groups, each under a
// comic caption with a small picture in a bulb-yellow coin (settings.html). Builds the DOM
// only, with createElementNS and no innerHTML; src/settings.ts writes the localized titles
// (invariant 7).

import type { LocaleKey } from "../i18n/index.ts";

const SVG_NS = "http://www.w3.org/2000/svg";

export type SettingsSectionId = "mate" | "voice" | "look" | "privacy" | "updates" | "judges";

/** `paper`: paper inside the ink line; `line`: the ink line only. 24 x 24 user units. */
type Stroke = "paper" | "line";

const ICONS: Readonly<Record<SettingsSectionId, ReadonlyArray<readonly [Stroke, string]>>> = {
  // Mate's bulb, as in src/bulb-badge.ts.
  mate: [
    ["paper", "M8.5 15.5C8.5 13.5 6 12.5 6 9.5a6 6 0 0 1 12 0c0 3-2.5 4-2.5 6z"],
    ["line", "M9 18.5h6M10 21.5h4"],
  ],
  // A microphone.
  voice: [
    ["paper", "M9 5.5a3 3 0 0 1 6 0v5a3 3 0 0 1-6 0z"],
    ["line", "M5.5 10.5a6.5 6.5 0 0 0 13 0M12 17v4M8.5 21h7"],
  ],
  // An eye.
  look: [
    ["paper", "M2.5 12S6 5.5 12 5.5 21.5 12 21.5 12 18 18.5 12 18.5 2.5 12 2.5 12z"],
    ["line", "M12 9a3 3 0 1 0 0 6a3 3 0 1 0 0-6z"],
  ],
  // A padlock.
  privacy: [
    ["paper", "M7 10.5h10a1.5 1.5 0 0 1 1.5 1.5v7a1.5 1.5 0 0 1-1.5 1.5H7A1.5 1.5 0 0 1 5.5 19v-7A1.5 1.5 0 0 1 7 10.5z"],
    ["line", "M8.5 10.5V8a3.5 3.5 0 0 1 7 0v2.5M12 14.5v2"],
  ],
  // Two arrows going round.
  updates: [
    ["line", "M19 12a7 7 0 0 1-12.2 4.7M5 12a7 7 0 0 1 12.2-4.7M17.5 3.5v4h-4M6.5 20.5v-4h4"],
  ],
  // A star.
  judges: [
    ["paper", "M12 3l2.7 5.6 6.1.9-4.4 4.3 1 6.1L12 17l-5.4 2.9 1-6.1-4.4-4.3 6.1-.9z"],
  ],
};

export interface SettingsSectionRefs {
  readonly section: HTMLElement;
  readonly titleText: HTMLSpanElement;
  readonly titleKey: LocaleKey;
}

/** A section titled `titleKey` holding `cards` in order. */
export function buildSettingsSection(
  id: SettingsSectionId,
  titleKey: LocaleKey,
  cards: readonly HTMLElement[],
): SettingsSectionRefs {
  const section = document.createElement("section");
  section.className = `settings-section settings-section-${id}`;
  const titleId = `settings-section-${id}-title`;
  section.setAttribute("aria-labelledby", titleId);

  const title = document.createElement("h2");
  title.className = "settings-section-title";
  title.id = titleId;

  const icon = document.createElement("span");
  icon.className = "settings-section-icon";
  icon.setAttribute("aria-hidden", "true");
  const svg = document.createElementNS(SVG_NS, "svg");
  svg.setAttribute("viewBox", "0 0 24 24");
  svg.setAttribute("focusable", "false");
  for (const [stroke, d] of ICONS[id]) {
    const path = document.createElementNS(SVG_NS, "path");
    path.setAttribute("class", `i-${stroke}`);
    path.setAttribute("d", d);
    svg.append(path);
  }
  icon.append(svg);

  const titleText = document.createElement("span");
  titleText.className = "settings-section-text";
  title.append(icon, titleText);

  section.append(title, ...cards);
  return { section, titleText, titleKey };
}

export interface PrivacyNoteRefs {
  readonly card: HTMLDivElement;
  readonly lines: ReadonlyArray<{ readonly el: HTMLLIElement; readonly key: LocaleKey }>;
}

/** What Mate sees, as two calm lines above the account (invariant 3). */
export function buildPrivacyNote(): PrivacyNoteRefs {
  const card = document.createElement("div");
  card.className = "settings-group settings-privacy";
  const list = document.createElement("ul");
  list.className = "settings-privacy-list";
  const keys: readonly LocaleKey[] = ["settings.privacy_look", "settings.privacy_passwords"];
  const lines = keys.map((key) => {
    const el = document.createElement("li");
    el.className = "settings-privacy-line";
    list.append(el);
    return { el, key };
  });
  card.append(list);
  return { card, lines };
}
