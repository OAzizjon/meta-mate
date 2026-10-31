// Pure chat validation and keyboard behavior; no DOM or Tauri dependencies.
export const MAX_TEXT = 1000;
export const HISTORY_LIMIT = 200;

export const CHAT_MOODS = ["calm", "happy", "surprised", "sad"] as const;
export type ChatMood = (typeof CHAT_MOODS)[number];

export function isChatMood(value: unknown): value is ChatMood {
  return typeof value === "string" && (CHAT_MOODS as readonly string[]).includes(value);
}

export interface Message {
  readonly role: "person" | "mate";
  readonly text: string;
  readonly timestamp: number;
  readonly mood?: ChatMood;
}

export function canSend(text: string): boolean {
  return text.trim().length > 0 && Array.from(text).length <= MAX_TEXT;
}

export function sendsOnKey(key: string, shiftKey: boolean, isComposing: boolean): boolean {
  return key === "Enter" && !shiftKey && !isComposing;
}

/** Treat the history file and IPC payload as data, including any markup in messages. */
export function parseHistory(raw: unknown): Message[] {
  if (!Array.isArray(raw)) return [];
  return raw.filter((entry): entry is Message => {
    if (typeof entry !== "object" || entry === null) return false;
    const { role, text, timestamp } = entry as Record<string, unknown>;
    return (role === "person" || role === "mate") && typeof text === "string"
      && text.trim().length > 0 && typeof timestamp === "number"
      && Number.isSafeInteger(timestamp) && timestamp >= 0 && timestamp <= 8_640_000_000_000_000;
  }).slice(-HISTORY_LIMIT).map(({ role, text, timestamp, mood }) => (
    role === "mate" && isChatMood(mood)
      ? { role, text, timestamp, mood }
      : { role, text, timestamp }
  ));
}
