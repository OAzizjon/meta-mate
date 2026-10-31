// Chat history rendering policy, independent of the DOM and Tauri.
import type { Message } from "./model.ts";

export type AwaitingReply = "sent" | "asked" | null;

/** Streaming text may change while the same message bubbles remain in place. */
export function canReuseMessages(previous: readonly Message[], next: readonly Message[]): boolean {
  return previous.length <= next.length && previous.every((old, index) => {
    const current = next[index];
    return current.role === old.role && current.timestamp === old.timestamp;
  });
}

/** Wait for the question to appear before a reply or cleared history can end the wait. */
export function nextAwaitingReply(awaiting: AwaitingReply, next: readonly Message[]): AwaitingReply {
  const last = next[next.length - 1]?.role;
  if (awaiting === "sent" && last === "person") return "asked";
  // A Mate reply ends the wait; so does an empty history once the question was in it (cleared,
  // even when the cleared file could not be written).
  if (awaiting === "asked" && (last === "mate" || last === undefined)) return null;
  return awaiting;
}
