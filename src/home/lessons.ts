// "My lessons" on Home: the picture cards Mate makes after a lesson, as `cards_list` returns
// them. Pure (no DOM, no locale JSON) so Node's test runner loads it; the section itself is
// src/home/lessons-view.ts.

/** Lists the saved cards, newest first. */
export const CARDS_LIST_COMMAND = "cards_list";
/** Opens one card in the card window: `{ id }`. */
export const CARD_SHOW_COMMAND = "card_show";
/** Opens the cards' folder in Explorer. */
export const CARDS_OPEN_FOLDER_COMMAND = "cards_open_folder";

/** Steps shown before "Show all". */
export const LESSON_STEPS_SHOWN = 2;

/** Only a PNG data URL is drawn: never a file or web address. */
const THUMB_PREFIX = "data:image/png;base64,";

export interface LessonCard {
  readonly id: string;
  /** May be empty: the page shows a stand-in title. */
  readonly title: string;
  readonly steps: readonly string[];
  /** Unix seconds; 0 when the card did not say. */
  readonly created: number;
  /** A `data:image/png` URL, or null for the bulb placeholder. */
  readonly thumb: string | null;
}

/** Minimal IPC invoke signature matching `@tauri-apps/api/core`'s `invoke`. */
export type LessonsInvoke = (
  cmd: string,
  args?: Readonly<Record<string, unknown>>,
) => Promise<unknown>;

function isRecord(value: unknown): value is Readonly<Record<string, unknown>> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

/** One card from the payload, or null when it has no usable id. Never throws. */
export function parseLessonCard(raw: unknown): LessonCard | null {
  try {
    if (!isRecord(raw)) {
      return null;
    }
    const { id, title, steps, created, thumb } = raw;
    if (typeof id !== "string" || !id.trim()) {
      return null;
    }
    const lines = Array.isArray(steps) ? steps : [];
    return {
      id,
      title: typeof title === "string" ? title.trim() : "",
      steps: lines
        .map((step: unknown) => (typeof step === "string" ? step.trim() : ""))
        .filter((step) => step !== ""),
      created: typeof created === "number" && Number.isFinite(created) && created > 0 ? created : 0,
      thumb: typeof thumb === "string" && thumb.startsWith(THUMB_PREFIX) ? thumb : null,
    };
  } catch {
    return null;
  }
}

/**
 * The `cards_list` payload as cards, newest first (cards made at the same second keep their
 * order); entries without an id and repeated ids are dropped. Anything but an array is no
 * cards. Never throws.
 */
export function parseLessonCards(raw: unknown): LessonCard[] {
  if (!Array.isArray(raw)) {
    return [];
  }
  const seen = new Set<string>();
  const cards: LessonCard[] = [];
  for (const item of raw) {
    const card = parseLessonCard(item);
    if (card && !seen.has(card.id)) {
      seen.add(card.id);
      cards.push(card);
    }
  }
  return cards.sort((a, b) => b.created - a.created);
}

/** Asks `cards_list`; no Tauri, a missing command or a failure is no cards. Never throws. */
export async function loadLessonCards(invoke: LessonsInvoke | null): Promise<LessonCard[]> {
  if (!invoke) {
    return [];
  }
  try {
    return parseLessonCards(await invoke(CARDS_LIST_COMMAND));
  } catch {
    return [];
  }
}

/** The card's day in the page language ("November 1, 2026"); "" when it has none. */
export function formatLessonDate(created: number, locale: string, timeZone?: string): string {
  if (!(created > 0) || !Number.isFinite(created)) {
    return "";
  }
  try {
    return new Intl.DateTimeFormat(locale, { day: "numeric", month: "long", year: "numeric", timeZone })
      .format(new Date(created * 1000));
  } catch {
    return "";
  }
}

/**
 * How many steps show: the first `LESSON_STEPS_SHOWN` while collapsed, all once expanded.
 * `canExpand` says whether "Show all" is needed at all.
 */
export function shownLessonSteps(
  total: number,
  expanded: boolean,
): { readonly shown: number; readonly canExpand: boolean } {
  const count = Math.max(0, Math.floor(total));
  const canExpand = count > LESSON_STEPS_SHOWN;
  return { shown: canExpand && !expanded ? LESSON_STEPS_SHOWN : count, canExpand };
}
