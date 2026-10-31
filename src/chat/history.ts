// Chat history rendering policy, independent of the DOM and Tauri.
import type { Message } from "./model.ts";

export type InlineNode = { readonly kind: "text" | "key" | "url"; readonly text: string };
export type MessageBlock =
  | { readonly kind: "paragraph"; readonly children: readonly InlineNode[] }
  | { readonly kind: "list"; readonly items: { readonly number: number; readonly children: readonly InlineNode[] }[] };

/** Decorative chips only: preserve the text, never infer warnings or create links. */
export function parseMateInline(text: string): InlineNode[] {
  const pattern = /\b(?:https?:\/\/|www\.)[^\s<>«»"“”]+|"[^"\n]{1,80}"|«[^«»\n]{1,80}»|“[^“”\n]{1,80}”/giu;
  const nodes: InlineNode[] = [];
  let offset = 0;
  for (const match of text.matchAll(pattern)) {
    if (match.index > offset) nodes.push({ kind: "text", text: text.slice(offset, match.index) });
    const raw = match[0];
    if (/^(?:https?:\/\/|www\.)/iu.test(raw)) {
      let url = raw.replace(/[.,!;:?]+$/u, "");
      // Keep balanced parentheses in paths, but leave sentence punctuation outside the chip.
      while (url.endsWith(")") && url.split(")").length > url.split("(").length) url = url.slice(0, -1);
      nodes.push({ kind: "url", text: url });
      if (url.length < raw.length) nodes.push({ kind: "text", text: raw.slice(url.length) });
    } else {
      nodes.push({ kind: "key", text: raw });
    }
    offset = match.index + raw.length;
  }
  if (offset < text.length) nodes.push({ kind: "text", text: text.slice(offset) });
  return nodes;
}

/** Keep explicit numbering, including gaps and restarts, in semantic lists. */
export function parseMateBlocks(text: string): MessageBlock[] {
  const blocks: MessageBlock[] = [];
  for (const line of text.split(/\r?\n/u)) {
    const step = /^\s*([1-9]\d{0,2})[.)]\s+(\S.*)$/u.exec(line);
    if (step) {
      let last = blocks[blocks.length - 1];
      if (last?.kind !== "list") {
        last = { kind: "list", items: [] };
        blocks.push(last);
      }
      last.items.push({ number: Number(step[1]), children: parseMateInline(step[2]) });
    } else {
      blocks.push({ kind: "paragraph", children: parseMateInline(line) });
    }
  }
  return blocks;
}

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
