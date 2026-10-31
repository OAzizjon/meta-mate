// "My lessons" on Home (owner request, 2026-11-01): the picture cards Mate made after a
// lesson, newest first, each with its picture (or Mate's bulb), title, day and steps (the
// first two, then "Show all"). A card opens in the card window (`card_show`); "Open the
// folder" shows them in Explorer. No cards, no command or a failure all show the friendly
// empty line, never an error. The list logic is src/home/lessons.ts.

import type { LocaleKey } from "../i18n/index.ts";
import {
  CARDS_OPEN_FOLDER_COMMAND,
  CARD_SHOW_COMMAND,
  formatLessonDate,
  loadLessonCards,
  shownLessonSteps,
} from "./lessons.ts";
import type { LessonCard, LessonsInvoke } from "./lessons.ts";
import { button, el } from "./dom.ts";
import type { Translate } from "./dom.ts";
import { homeIcon } from "./icons.ts";

export interface LessonsSection {
  readonly element: HTMLElement;
  /** Asks `cards_list` again; a newer refresh wins over an older one still on its way. */
  refresh(): Promise<void>;
  destroy(): void;
}

export interface LessonsOptions {
  readonly invoke: LessonsInvoke | null;
  readonly t: Translate;
  readonly locale: () => string;
}

export function createLessonsSection({ invoke, t, locale }: LessonsOptions): LessonsSection {
  let destroyed = false;
  let generation = 0;
  // Which cards show all their steps: kept across refreshes.
  const expanded = new Set<string>();

  const section = el("section", "home-section home-cards home-lessons");
  section.setAttribute("aria-labelledby", "home-cards-title");
  const head = el("div", "home-section-head");
  const heading = el("h2", "home-section-title", t("home.cards_title"));
  heading.id = "home-cards-title";
  head.append(heading, el("p", "home-section-intro", t("home.cards_text")));

  const list = el("ul", "home-lessons-list");
  list.hidden = true;
  const empty = el("div", "home-lessons-empty");
  empty.hidden = true;
  empty.append(homeIcon("mate", "home-lessons-empty-mate"), el("p", "home-bubble", t("home.lessons_empty")));

  const status = el("p", "home-try-status");
  status.setAttribute("role", "status");
  status.setAttribute("aria-live", "polite");
  const say = (key: LocaleKey | null): void => {
    status.textContent = key === null ? "" : t(key);
    status.classList.toggle("is-error", key !== null);
  };

  const folder = el("div", "home-lessons-folder");
  const folderBtn = button("btn btn-secondary", t("home.lessons_folder"), "home-lessons-folder-btn");
  folderBtn.disabled = !invoke;
  folderBtn.addEventListener("click", () => {
    say(null);
    void invoke?.(CARDS_OPEN_FOLDER_COMMAND).catch(() => {
      if (!destroyed) say("home.lessons_folder_failed");
    });
  });
  folder.append(folderBtn, el("p", "home-cards-folder", t("home.cards_folder")));

  section.append(head, list, empty, folder, status);

  const openCard = (card: LessonCard): void => {
    say(null);
    if (!invoke) {
      say("home.lessons_open_failed");
      return;
    }
    void invoke(CARD_SHOW_COMMAND, { id: card.id }).catch(() => {
      if (!destroyed) say("home.lessons_open_failed");
    });
  };

  const lessonItem = (card: LessonCard, index: number): HTMLLIElement => {
    const item = el("li", "home-lesson comic-panel");
    // The picture, title and day are one button: the card's one action is opening it.
    const open = el("button", "home-lesson-open");
    open.type = "button";
    open.id = `home-lesson-${index}-open`;
    const thumb = el("span", "home-lesson-thumb");
    if (card.thumb) {
      const img = el("img", "home-lesson-img");
      img.alt = "";
      img.decoding = "async";
      img.src = card.thumb;
      thumb.append(img);
    } else {
      thumb.classList.add("is-placeholder");
      thumb.append(homeIcon("mate"));
    }
    const text = el("span", "home-lesson-text");
    text.append(el("span", "home-lesson-title", card.title || t("home.lessons_untitled")));
    const date = formatLessonDate(card.created, locale());
    if (date) {
      text.append(el("span", "home-lesson-date", date));
    }
    text.append(el("span", "home-lesson-cta", t("home.lessons_open")));
    open.append(thumb, text);
    open.addEventListener("click", () => openCard(card));
    item.append(open);

    if (card.steps.length > 0) {
      const steps = el("ol", "home-lesson-steps");
      steps.id = `home-lesson-${index}-steps`;
      const lines = card.steps.map((step) => el("li", "home-lesson-step", step));
      steps.append(...lines);
      item.append(steps);
      const more = button("home-link home-lesson-more", "", `home-lesson-${index}-more`);
      more.setAttribute("aria-controls", steps.id);
      const sync = (): void => {
        const { shown, canExpand } = shownLessonSteps(lines.length, expanded.has(card.id));
        lines.forEach((line, n) => {
          line.hidden = n >= shown;
        });
        more.hidden = !canExpand;
        more.setAttribute("aria-expanded", String(shown === lines.length));
        more.textContent = t(shown === lines.length ? "home.lessons_show_less" : "home.lessons_show_all");
      };
      more.addEventListener("click", () => {
        if (!expanded.delete(card.id)) {
          expanded.add(card.id);
        }
        sync();
      });
      sync();
      item.append(more);
    }
    return item;
  };

  // Rebuilt only when the cards changed: a refresh on coming back to the window must not
  // take the keyboard off the card it is on.
  let shownCards: string | null = null;
  const render = (cards: readonly LessonCard[]): void => {
    const signature = JSON.stringify(
      cards.map((card) => [card.id, card.title, card.created, card.steps, card.thumb?.length ?? 0]),
    );
    if (signature === shownCards) {
      return;
    }
    shownCards = signature;
    list.replaceChildren(...cards.map(lessonItem));
    list.hidden = cards.length === 0;
    empty.hidden = cards.length > 0;
  };

  return {
    element: section,
    async refresh(): Promise<void> {
      const mine = ++generation;
      section.setAttribute("aria-busy", "true");
      const cards = await loadLessonCards(invoke);
      if (destroyed || mine !== generation) {
        return;
      }
      section.setAttribute("aria-busy", "false");
      render(cards);
    },
    destroy(): void {
      destroyed = true;
    },
  };
}
