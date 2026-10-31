// Home's guide (owner request, 2026-11-01): what Mate can do as a strip of comic cards, one
// picture, a bold title, a short line and one example each. Opened by the header's Guide
// button or the "?" key; Esc or Close closes it. A modal <dialog>: the keyboard stays inside
// while it is open and goes back to the opener after.

import type { LocaleKey } from "../i18n/index.ts";
import type { PttKey } from "../settings/model.ts";
import { pttKeycapLabel, withPttKey } from "../settings/ptt-key.ts";
import { button, el } from "./dom.ts";
import type { Translate } from "./dom.ts";
import { homeIcon } from "./icons.ts";
import type { HomeIconName } from "./icons.ts";

export interface GuideCard {
  readonly icon: HomeIconName;
  readonly title: LocaleKey;
  /** May hold `{key}`: the person's talk key. */
  readonly text: LocaleKey;
  /** "Say:", "Type:" or "Try:" before the example. */
  readonly label: LocaleKey;
  readonly example: LocaleKey;
}

type GuideTopic = "talk" | "beam" | "teach" | "safe" | "lessons" | "open" | "chat" | "move";

/** A card's keys are `home.guide_<topic>_title`, `_text` and `_example` (tsc checks them). */
function entry(topic: GuideTopic, icon: HomeIconName, label: LocaleKey): GuideCard {
  return {
    icon,
    title: `home.guide_${topic}_title` as const,
    text: `home.guide_${topic}_text` as const,
    label,
    example: `home.guide_${topic}_example` as const,
  };
}

export const GUIDE_CARDS: readonly GuideCard[] = [
  entry("talk", "talk", "home.guide_say"),
  entry("beam", "beam", "home.guide_say"),
  entry("teach", "steps", "home.guide_say"),
  entry("safe", "safe", "home.guide_say"),
  entry("lessons", "lessons", "home.guide_say"),
  entry("open", "open", "home.guide_say"),
  entry("chat", "chat", "home.guide_type"),
  entry("move", "move", "home.guide_try"),
];

/** The key that opens the guide (and `aria-keyshortcuts` on its button). */
export const GUIDE_SHORTCUT = "?";

/** Inputs where "?" is typed, not a shortcut. */
const KEY_INPUT_TYPES = new Set(["checkbox", "radio", "button", "submit", "reset", "range", "color", "file"]);

/** What `isGuideShortcut` reads of a `KeyboardEvent`. */
export interface ShortcutEvent {
  readonly key: string;
  readonly ctrlKey: boolean;
  readonly altKey: boolean;
  readonly metaKey: boolean;
  readonly target: unknown;
}

/** "?" alone, and not while the person types in a text field. */
export function isGuideShortcut(event: ShortcutEvent): boolean {
  if (event.key !== GUIDE_SHORTCUT || event.ctrlKey || event.altKey || event.metaKey) {
    return false;
  }
  const target = event.target as { tagName?: unknown; type?: unknown; isContentEditable?: unknown } | null;
  if (!target || typeof target !== "object") {
    return true;
  }
  const tag = typeof target.tagName === "string" ? target.tagName.toLowerCase() : "";
  if (tag === "textarea" || tag === "select" || target.isContentEditable === true) {
    return false;
  }
  return !(tag === "input" && !KEY_INPUT_TYPES.has(String(target.type).toLowerCase()));
}

export interface HomeGuide {
  readonly element: HTMLDialogElement;
  /** Opens the guide with the current talk key; focus returns to `opener` on close. */
  open(opener: HTMLElement | null): void;
  isOpen(): boolean;
}

function guideCard(card: GuideCard, index: number, t: Translate, pttKey: PttKey): HTMLLIElement {
  const item = el("li", "home-guide-card comic-panel");
  const art = el("div", "home-guide-art");
  art.append(el("span", "home-guide-number", String(index + 1)), homeIcon(card.icon));
  if (card.icon === "talk") {
    art.append(el("kbd", "home-keycap home-guide-key", pttKeycapLabel(pttKey, t)));
  }
  const say = el("p", "home-guide-say");
  say.append(el("span", "home-guide-say-label", t(card.label)), " ", el("q", "home-guide-example", t(card.example)));
  item.append(
    art,
    el("h3", "home-guide-card-title", t(card.title)),
    el("p", "home-guide-text", withPttKey(t(card.text), pttKey, t)),
    say,
  );
  return item;
}

export function createGuide(t: Translate, pttKey: () => PttKey): HomeGuide {
  const dialog = el("dialog", "home-guide");
  dialog.setAttribute("aria-labelledby", "home-guide-title");
  const head = el("div", "home-guide-head");
  const heading = el("div", "home-guide-heading");
  const title = el("h2", "home-guide-title", t("home.guide_title"));
  title.id = "home-guide-title";
  heading.append(title, el("p", "home-guide-intro", t("home.guide_intro")));
  // First in the dialog: showModal() puts the keyboard here.
  const closeBtn = button("btn btn-secondary home-guide-close", t("home.guide_close"), "home-guide-close");
  head.append(heading, closeBtn);
  const list = el("ol", "home-guide-list");
  dialog.append(head, list);

  // The keyboard goes back to where it was, however the guide closed.
  let opener: HTMLElement | null = null;
  const onClosed = (): void => {
    const back = opener;
    opener = null;
    back?.focus();
  };
  // Esc closes a modal dialog by itself and fires "close" too.
  dialog.addEventListener("close", onClosed);
  const close = (): void => {
    if (!dialog.open) {
      return;
    }
    if (typeof dialog.close === "function") {
      dialog.close();
    } else {
      dialog.open = false;
      onClosed();
    }
  };
  closeBtn.addEventListener("click", close);

  return {
    element: dialog,
    open(from: HTMLElement | null): void {
      if (dialog.open) {
        return;
      }
      opener = from;
      const key = pttKey();
      list.replaceChildren(...GUIDE_CARDS.map((card, index) => guideCard(card, index, t, key)));
      if (typeof dialog.showModal === "function") {
        dialog.showModal();
      } else {
        dialog.open = true;
      }
    },
    isOpen: () => dialog.open === true,
  };
}
