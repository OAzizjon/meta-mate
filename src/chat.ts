// Chat is a normal, deliberately opened window. DOM focus never activates its HWND.
import { Channel, invoke } from "@tauri-apps/api/core";
import { getLocale, t } from "./i18n/index.ts";
import { canSend, parseHistory, sendsOnKey } from "./chat/model.ts";
import { followSettingsLocale } from "./i18n/settings.ts";
import type { Message } from "./chat/model.ts";

const SVG_NS = "http://www.w3.org/2000/svg";
// A cancelled answer never arrives: the thinking bubble gives up after this long.
const THINKING_LIMIT_MS = 120_000;

/** Mate's light bulb as a small decorative header badge (createElementNS, no innerHTML). */
function bulbBadge(): HTMLSpanElement {
  const badge = document.createElement("span");
  badge.className = "badge";
  badge.setAttribute("aria-hidden", "true");
  const svg = document.createElementNS(SVG_NS, "svg");
  svg.setAttribute("viewBox", "0 0 24 24");
  // Glass, then the two lines of the base.
  for (const d of ["M8.5 15.5C8.5 13.5 6 12.5 6 9.5a6 6 0 0 1 12 0c0 3-2.5 4-2.5 6z", "M9 18.5h6", "M10 21.5h4"]) {
    const path = document.createElementNS(SVG_NS, "path");
    path.setAttribute("d", d);
    svg.append(path);
  }
  badge.append(svg);
  return badge;
}

const root = document.getElementById("chat-app")!;
const header = document.createElement("header");
const brand = document.createElement("div");
brand.className = "brand";
const title = document.createElement("h1");
brand.append(bulbBadge(), title);
const clear = document.createElement("button");
clear.type = "button";
header.append(brand, clear);
const log = document.createElement("ol");
log.id = "messages";
log.setAttribute("role", "log");
log.setAttribute("aria-live", "polite");
log.setAttribute("aria-relevant", "additions text");
const emptyState = document.createElement("li");
emptyState.className = "empty-state";
log.append(emptyState);
// Shown after the last message while an answer is awaited; never counted as a message.
const thinking = document.createElement("li");
thinking.className = "message mate thinking";
thinking.setAttribute("role", "status");
const thinkingBubble = document.createElement("div");
thinkingBubble.className = "bubble";
const thinkingText = document.createElement("span");
const dots = document.createElement("span");
dots.className = "dots";
dots.setAttribute("aria-hidden", "true");
dots.append(document.createElement("span"), document.createElement("span"), document.createElement("span"));
thinkingBubble.append(thinkingText, dots);
thinking.append(thinkingBubble);
const status = document.createElement("p");
status.id = "status";
status.setAttribute("role", "status");
const form = document.createElement("form");
const label = document.createElement("label");
label.htmlFor = "chat-input";
const input = document.createElement("textarea");
input.id = "chat-input";
input.rows = 2;
input.setAttribute("aria-describedby", "chat-hint");
const actions = document.createElement("div");
actions.className = "compose-actions";
const hint = document.createElement("span");
hint.id = "chat-hint";
const send = document.createElement("button");
send.id = "send";
send.type = "submit";
actions.append(hint, send);
form.append(label, input, actions);
root.append(header, log, status, form);

let messages: Message[] = [];
let ready = false;
let sending = false;
// From Send: "sent" until the question shows in the history, then "asked" until Mate writes.
let awaiting: "sent" | "asked" | null = null;
let thinkingTimer = 0;
const update = (): void => { send.disabled = !ready || sending || !canSend(input.value); };

function startThinking(): void {
  awaiting = "sent";
  window.clearTimeout(thinkingTimer);
  thinkingTimer = window.setTimeout(stopThinking, THINKING_LIMIT_MS);
  emptyState.hidden = true;
  if (thinking.parentNode !== log) log.append(thinking);
  log.scrollTop = log.scrollHeight;
}

function stopThinking(): void {
  awaiting = null;
  window.clearTimeout(thinkingTimer);
  thinking.remove();
  emptyState.hidden = messages.length > 0;
}

function render(raw: unknown): void {
  const next = parseHistory(raw);
  const atBottom = log.scrollHeight - log.scrollTop - log.clientHeight < 80;
  // Keep existing nodes and the reader's scroll position while a sentence arrives.
  const prefix = messages.length <= next.length && messages.every((old, index) => {
    const current = next[index];
    return current.role === old.role && current.timestamp === old.timestamp;
  });
  if (!prefix) log.replaceChildren(emptyState);
  emptyState.hidden = next.length > 0 || awaiting !== null;
  next.forEach((message, index) => {
    let item = log.children[index + 1] as HTMLLIElement | undefined;
    if (!item || item === thinking) {
      item = document.createElement("li");
      item.className = `message ${message.role}`;
      const from = document.createElement("span");
      from.className = "from";
      from.textContent = t(message.role === "person" ? "chat.from_you" : "chat.from_mate");
      const bubble = document.createElement("div");
      bubble.className = "bubble";
      const text = document.createElement("p");
      const time = document.createElement("time");
      const date = new Date(message.timestamp);
      time.dateTime = date.toISOString();
      time.textContent = date.toLocaleTimeString(getLocale(), { hour: "2-digit", minute: "2-digit" });
      bubble.append(text, time);
      item.append(from, bubble);
      // Insert above the thinking bubble without moving it, so the log does not announce it again.
      log.insertBefore(item, thinking.parentNode === log ? thinking : null);
    }
    const text = item.querySelector("p")!;
    if (text.textContent !== message.text) text.textContent = message.text;
  });
  messages = next;
  const last = next[next.length - 1]?.role;
  if (awaiting === "sent" && last === "person") awaiting = "asked";
  // A Mate reply ends the wait; so does an empty history once the question was in it (cleared,
  // even when the cleared file could not be written).
  else if (awaiting === "asked" && (last === "mate" || last === undefined)) stopThinking();
  if (awaiting !== null && thinking.parentNode !== log) log.append(thinking);
  if (atBottom) log.scrollTop = log.scrollHeight;
}

input.addEventListener("input", update);
input.addEventListener("keydown", (event) => {
  if (sendsOnKey(event.key, event.shiftKey, event.isComposing)) {
    event.preventDefault();
    form.requestSubmit();
  }
});
form.addEventListener("submit", (event) => {
  event.preventDefault();
  if (!ready || sending || !canSend(input.value)) return;
  const text = input.value;
  if (document.hasFocus() && document.activeElement === send) input.focus();
  sending = true;
  update();
  status.textContent = "";
  startThinking();
  void invoke("chat_send", { text }).then(() => {
    if (input.value === text) input.value = "";
  }).catch(() => {
    stopThinking();
    status.textContent = t("chat.send_failed");
  }).finally(() => {
    sending = false;
    update();
    // Keep typing after Send, but never pull focus back from another window or Clear.
    if (document.hasFocus() && (document.activeElement === send || document.activeElement === input)) input.focus();
  });
});
clear.addEventListener("click", () => {
  clear.disabled = true;
  // Clearing cancels the answer being written, so nothing is awaited any more.
  void invoke("chat_clear").then(() => {
    status.textContent = "";
    stopThinking();
  }).catch(() => {
    status.textContent = t("chat.save_failed");
  }).finally(() => { clear.disabled = false; });
});

function localize(): void {
  document.documentElement.lang = getLocale();
  document.title = title.textContent = t("chat.title");
  clear.textContent = t("chat.clear");
  label.textContent = t("chat.input");
  hint.textContent = t("chat.hint");
  send.textContent = t("chat.send");
  log.setAttribute("aria-label", t("chat.messages"));
  emptyState.textContent = t("chat.empty");
  thinkingText.textContent = t("chat.thinking");
}

async function start(): Promise<void> {
  const channel = new Channel<unknown>();
  channel.onmessage = render;
  // The channel sends its snapshot under the history lock, before any later updates.
  await invoke("chat_subscribe", { channel });
  localize();
  const current = messages;
  messages = [];
  log.replaceChildren(emptyState);
  render(current);
  ready = true;
  update();
  // This is DOM focus only, and only while the person is already in this window.
  if (document.hasFocus()) input.focus();
  window.addEventListener("focus", () => {
    if (document.activeElement === document.body) input.focus();
  });
}

localize();
update();
clear.disabled = true;
void followSettingsLocale(() => {
  localize();
  // Speaker names and times are written once per bubble: rebuild them in the new language.
  const current = messages;
  messages = [];
  log.replaceChildren(emptyState);
  render(current);
}).catch((error: unknown) => console.error("chat: language:", error));
void start().then(() => { clear.disabled = false; }).catch(() => {
  status.textContent = t("chat.load_failed");
});
