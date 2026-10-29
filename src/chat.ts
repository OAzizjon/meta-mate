// Chat is a normal, deliberately opened window. DOM focus never activates its HWND.
import { Channel, invoke } from "@tauri-apps/api/core";
import { getLocale, initI18n, t } from "./i18n/index.ts";
import { canSend, parseHistory, sendsOnKey } from "./chat/model.ts";
import { followSettingsLocale } from "./i18n/settings.ts";
import type { Message } from "./chat/model.ts";

const root = document.getElementById("chat-app")!;
const header = document.createElement("header");
const title = document.createElement("h1");
const clear = document.createElement("button");
clear.type = "button";
header.append(title, clear);
const log = document.createElement("ol");
log.id = "messages";
log.setAttribute("role", "log");
log.setAttribute("aria-live", "polite");
log.setAttribute("aria-relevant", "additions text");
const status = document.createElement("p");
status.id = "status";
status.setAttribute("role", "status");
const form = document.createElement("form");
const label = document.createElement("label");
label.htmlFor = "chat-input";
const input = document.createElement("textarea");
input.id = "chat-input";
input.rows = 3;
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
const update = (): void => { send.disabled = !ready || sending || !canSend(input.value); };

function render(raw: unknown): void {
  const next = parseHistory(raw);
  const atBottom = log.scrollHeight - log.scrollTop - log.clientHeight < 80;
  // Keep existing nodes and the reader's scroll position while a sentence arrives.
  const prefix = messages.length <= next.length && messages.every((old, index) => {
    const current = next[index];
    return current.role === old.role && current.timestamp === old.timestamp;
  });
  if (!prefix) log.replaceChildren();
  next.forEach((message, index) => {
    let bubble = log.children[index] as HTMLLIElement | undefined;
    if (!bubble) {
      bubble = document.createElement("li");
      bubble.className = `message ${message.role}`;
      const speaker = document.createElement("strong");
      speaker.textContent = t(message.role === "person" ? "chat.person" : "chat.mate");
      const text = document.createElement("p");
      const time = document.createElement("time");
      const date = new Date(message.timestamp);
      time.dateTime = date.toISOString();
      time.textContent = date.toLocaleTimeString(getLocale(), { hour: "2-digit", minute: "2-digit" });
      bubble.append(speaker, text, time);
      log.append(bubble);
    }
    const text = bubble.querySelector("p")!;
    if (text.textContent !== message.text) text.textContent = message.text;
  });
  messages = next;
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
  void invoke("chat_send", { text }).then(() => {
    if (input.value === text) input.value = "";
  }).catch(() => { status.textContent = t("chat.send_failed"); }).finally(() => {
    sending = false;
    update();
    // Keep typing after Send, but never pull focus back from another window or Clear.
    if (document.hasFocus() && (document.activeElement === send || document.activeElement === input)) input.focus();
  });
});
clear.addEventListener("click", () => {
  clear.disabled = true;
  void invoke("chat_clear").then(() => { status.textContent = ""; }).catch(() => {
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
}

async function start(): Promise<void> {
  const channel = new Channel<unknown>();
  channel.onmessage = render;
  // The channel sends its snapshot under the history lock, before any later updates.
  const locale = await invoke<string>("chat_subscribe", { channel });
  initI18n({ locale });
  localize();
  const current = messages;
  messages = [];
  log.replaceChildren();
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
  log.replaceChildren();
  render(current);
}).catch((error: unknown) => console.error("chat: language:", error));
void start().then(() => { clear.disabled = false; }).catch(() => {
  status.textContent = t("chat.load_failed");
});
