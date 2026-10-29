// How long Mate's speech bubble (src/mate.ts) stays on screen (WP85): long enough for an older
// person to read it at an easy pace, longer texts longer. Pure, so `node --test` runs it directly.

/** A bubble that hides by itself never goes sooner than this. */
export const BUBBLE_MIN_MS = 6000;
/** Time to notice the bubble and find the start of its text. */
export const BUBBLE_NOTICE_MS = 1500;
/** About 11 characters a second: a slow, easy reading pace in English and Russian alike. */
export const BUBBLE_MS_PER_CHAR = 90;
/** Even a long text goes in the end (the next push-to-talk hides it anyway). */
export const BUBBLE_MAX_MS = 20000;
/** Once an answer has ended, its last words stay at least this long. */
export const BUBBLE_LINGER_MS = 3000;

/** How long `text` stays on screen so it can be read at an easy pace. */
export function bubbleReadMs(text: string): number {
  // Code points, not UTF-16 units: an emoji is one character to read.
  const chars = [...text.trim()].length;
  const ms = BUBBLE_NOTICE_MS + chars * BUBBLE_MS_PER_CHAR;
  return Math.min(BUBBLE_MAX_MS, Math.max(BUBBLE_MIN_MS, ms));
}

/**
 * How much longer a bubble that has shown `text` for `shownMs` stays after the answer ended:
 * whatever is left of its reading time, and never less than `minMs`.
 */
export function bubbleLingerMs(
  text: string,
  shownMs: number,
  minMs: number = BUBBLE_LINGER_MS,
): number {
  const shown = Number.isFinite(shownMs) && shownMs > 0 ? shownMs : 0;
  return Math.max(minMs, bubbleReadMs(text) - shown);
}
