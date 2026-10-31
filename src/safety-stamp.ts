// "Is this safe?" (crates/mm-core/src/api/prompts.rs SAFETY_CHECK_PROMPT): the comic stamp Mate's
// window shows with the verdict for the length of the answer (src/mate.ts). Pure, so
// `node --test` runs it directly.
import type { LocaleKey } from "./i18n/index.ts";
import type { Bound, TimelineItem } from "./ipc/timeline.ts";
import type { CssRect } from "./robot/types.ts";

/** Rust → Mate's window: this generation is a safety check (src-tauri/src/voice.rs `SAFETY_EVENT`). */
export const SAFETY_CHECK_EVENT = "safety-check";

export type SafetyVerdict = "scam" | "careful";

/**
 * The prompt's warning moods: sad looks like a scam, surprised be careful. Calm ("looks fine")
 * gets no stamp on purpose: a green stamp would show before the rest of the answer is heard and
 * reads as the certainty the prompt forbids ("never say it is surely safe"), to the people least
 * able to discount it (Meta Muse Wonder review, D73).
 */
const VERDICT_MOODS: ReadonlyMap<string, SafetyVerdict> = new Map<string, SafetyVerdict>([
  ["sad", "scam"],
  ["surprised", "careful"],
]);

export const STAMP_KEYS: Readonly<Record<SafetyVerdict, LocaleKey>> = {
  scam: "mate.stamp_scam",
  careful: "mate.stamp_careful",
};

/** A safety answer's stamp so far: undefined until its first item after `<lang>`, then a verdict or none (null, for good). */
export type StampState = SafetyVerdict | null | undefined;

/**
 * The stamp after the next timeline item of a safety answer. The verdict is the one mood its
 * first sentence starts with ("After <lang CODE>, start with exactly one mood"); anything else
 * first (calm included) leaves no stamp for the whole answer: no stamp beats a wrong one.
 */
export function nextStamp(state: StampState, item: TimelineItem): StampState {
  if (state === null) return null;
  if (state === undefined) {
    if (item.kind === "lang") return undefined;
    const moods = boundsOf(item).flatMap((bound) => (bound.kind === "mood" ? [bound.value] : []));
    if (item.kind !== "say" || moods.length !== 1) return null;
    state = VERDICT_MOODS.get(moods[0]) ?? null;
  }
  return state;
}

/**
 * Whether the stamp may show while the beam is on `target` (Mate's window, CSS pixels; null: no
 * beam). Mate stands beside his target, but placement.rs's last resort, a corner, may overlap
 * it: any part of the target inside the window hides the stamp.
 */
export function stampClearOf(target: CssRect | null, width: number, height: number): boolean {
  if (target === null) return true;
  // NaN compares false everywhere: an unreadable target hides the stamp.
  return target.right <= 0 || target.bottom <= 0 || target.left >= width || target.top >= height;
}

function boundsOf(item: TimelineItem): readonly Bound[] {
  if (item.kind === "say") return item.value.bound;
  return item.kind === "bound" ? [item.value] : [];
}
