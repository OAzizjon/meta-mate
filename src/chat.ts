// Chat is a normal, deliberately opened window. DOM focus never activates its HWND.
import { Channel, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { bulbBadge } from "./bulb-badge.ts";
import { getLocale, t } from "./i18n/index.ts";
import { canSend, parseHistory, sendsOnKey } from "./chat/model.ts";
import { followSettingsLocale } from "./i18n/settings.ts";
import type { Message } from "./chat/model.ts";
import { canReuseMessages, mateAnswerGrew, nextAwaitingReply, parseMateBlocks } from "./chat/history.ts";
import type { AwaitingReply } from "./chat/history.ts";

// A cancelled answer never arrives: the thinking bubble gives up after this long.
const THINKING_LIMIT_MS = 120_000;
const COPIED_MS = 1_500;
// The empty chat's questions; each one is sent at once.
const SUGGESTIONS = [
  "chat.suggest_abilities", "chat.suggest_safe", "chat.suggest_text_size", "chat.suggest_youtube",
] as const;
const MODEL_LABELS = {
  "muse-spark-1.3": "chat.model_spark",
  "muse-wonder-1.0": "chat.model_wonder",
  "muse-glimmer-30b": "chat.model_glimmer",
} as const;

const root = document.getElementById("chat-app")!;
const header = document.createElement("header");
const brand = document.createElement("div");
brand.className = "brand";
const title = document.createElement("h1");
brand.append(bulbBadge(), title);
const clear = document.createElement("button");
clear.type = "button";
clear.className = "btn btn-secondary";
header.append(brand, clear);
const log = document.createElement("ol");
log.id = "messages";
log.setAttribute("role", "log");
log.setAttribute("aria-live", "polite");
log.setAttribute("aria-relevant", "additions text");
const emptyState = document.createElement("li");
emptyState.className = "empty-state";
const greeting = document.createElement("p");
greeting.className = "greeting";
const intro = document.createElement("p");
const suggestions = document.createElement("div");
suggestions.className = "suggestions";
suggestions.setAttribute("role", "group");
const chips = SUGGESTIONS.map((key) => {
  const chip = document.createElement("button");
  chip.type = "button";
  chip.className = "btn suggestion";
  chip.addEventListener("click", () => {
    // The chip hides with the empty chat: DOM focus only, in the window just clicked.
    if (document.hasFocus()) input.focus();
    ask(t(key), false);
  });
  return chip;
});
suggestions.append(...chips);
emptyState.append(greeting, intro, suggestions);
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
// Floats over the bottom of the log while the person reads above a new answer.
const latest = document.createElement("button");
latest.id = "latest";
latest.type = "button";
latest.className = "btn btn-primary";
latest.hidden = true;
const logWrap = document.createElement("div");
logWrap.className = "log-wrap";
logWrap.append(log, latest);
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
send.className = "btn btn-primary";
// Talk is a click on Mate; Stop while a recording listens (chat.rs "chat-talk").
const talk = document.createElement("button");
talk.id = "talk";
talk.type = "button";
talk.className = "btn btn-secondary";
actions.append(hint, talk, send);
form.append(label, input, actions);
root.append(header, logWrap, status, form);

let messages: Message[] = [];
let ready = false;
let sending = false;
let listening = false;
// From Send: "sent" until the question shows in the history, then "asked" until Mate writes.
let awaiting: AwaitingReply = null;
let thinkingTimer = 0;
// Each Mate answer's text as written, for its Copy button: never the rendered chips.
const answerTexts = new WeakMap<HTMLButtonElement, string>();
const update = (): void => {
  send.disabled = !ready || sending || !canSend(input.value);
  talk.disabled = !ready;
  for (const chip of chips) chip.disabled = !ready || sending;
};
const showTalk = (): void => {
  talk.textContent = t(listening ? "chat.stop_talking" : "chat.talk");
};
const nearBottom = (): boolean => log.scrollHeight - log.scrollTop - log.clientHeight < 80;

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

/** The model chip and Copy under one Mate answer. */
function answerTools(): HTMLDivElement {
  const tools = document.createElement("div");
  tools.className = "message-tools";
  const chip = document.createElement("span");
  chip.className = "model-chip";
  chip.hidden = true;
  const copy = document.createElement("button");
  copy.type = "button";
  copy.className = "btn btn-quiet copy";
  copy.textContent = t("chat.copy");
  let copied = 0;
  copy.addEventListener("click", () => {
    const text = answerTexts.get(copy);
    if (text === undefined) return;
    void navigator.clipboard.writeText(text).then(() => {
      copy.textContent = t("chat.copied");
      copy.classList.add("is-copied");
      window.clearTimeout(copied);
      copied = window.setTimeout(() => {
        copy.textContent = t("chat.copy");
        copy.classList.remove("is-copied");
      }, COPIED_MS);
    }).catch((error: unknown) => console.error("chat: copy:", error));
  });
  tools.append(chip, copy);
  return tools;
}

function render(raw: unknown): void {
  const next = parseHistory(raw);
  const atBottom = nearBottom();
  // Keep existing nodes and the reader's scroll position while a sentence arrives.
  const prefix = canReuseMessages(messages, next);
  // A rebuilt history (opening, language) stands still; a new answer slides in.
  const arriving = prefix && messages.length > 0;
  const grew = mateAnswerGrew(messages, next);
  if (!prefix) log.replaceChildren(emptyState);
  emptyState.hidden = next.length > 0 || awaiting !== null;
  next.forEach((message, index) => {
    const previous = prefix ? messages[index] : undefined;
    let item = log.children[index + 1] as HTMLLIElement | undefined;
    if (!item || item === thinking) {
      item = document.createElement("li");
      item.className = `message ${message.role}`;
      const from = document.createElement("span");
      from.className = "from";
      from.textContent = t(message.role === "person" ? "chat.from_you" : "chat.from_mate");
      const bubble = document.createElement("div");
      bubble.className = "bubble";
      const text = document.createElement("div");
      text.className = "message-body";
      const moodLabel = document.createElement("span");
      moodLabel.className = "mood-label";
      moodLabel.hidden = true;
      const time = document.createElement("time");
      const date = new Date(message.timestamp);
      time.dateTime = date.toISOString();
      time.textContent = date.toLocaleTimeString(getLocale(), { hour: "2-digit", minute: "2-digit" });
      bubble.append(moodLabel, text, time);
      item.append(from, bubble);
      if (message.role === "mate") {
        item.append(answerTools());
        if (arriving) item.classList.add("arrive");
      }
      // Insert above the thinking bubble without moving it, so the log does not announce it again.
      log.insertBefore(item, thinking.parentNode === log ? thinking : null);
    }
    const body = item.querySelector<HTMLDivElement>(".message-body")!;
    if (!previous || previous.text !== message.text) {
      if (message.role === "person") body.textContent = message.text;
      else {
        const copy = item.querySelector<HTMLButtonElement>(".copy");
        if (copy) answerTexts.set(copy, message.text);
        const blocks = parseMateBlocks(message.text).map((block) => {
          const element = document.createElement(block.kind === "list" ? "ol" : "p");
          const rows = block.kind === "list" ? block.items : [{ children: block.children }];
          for (const row of rows) {
            const parent = block.kind === "list" ? document.createElement("li") : element;
            if ("number" in row && typeof row.number === "number") (parent as HTMLLIElement).value = row.number;
            for (const node of row.children) {
              if (node.kind === "text") parent.append(document.createTextNode(node.text));
              else {
                const chip = document.createElement(node.kind === "key" ? "kbd" : "code");
                chip.className = `chat-${node.kind}`;
                chip.textContent = node.text;
                parent.append(chip);
              }
            }
            if (parent !== element) element.append(parent);
          }
          return element;
        });
        body.replaceChildren(...blocks);
      }
    }
    if (!previous || previous.mood !== message.mood) {
      const bubble = item.querySelector<HTMLDivElement>(".bubble")!;
      const moodLabel = item.querySelector<HTMLSpanElement>(".mood-label")!;
      const mood = message.role === "mate" ? message.mood : undefined;
      bubble.className = mood ? `bubble mood-${mood}` : "bubble";
      const labels = {
        calm: "chat.mood_calm", happy: "chat.mood_happy",
        surprised: "chat.mood_surprised", sad: "chat.mood_sad",
      } as const;
      moodLabel.hidden = !mood;
      moodLabel.textContent = mood ? t(labels[mood]) : "";
    }
    if (!previous || previous.model !== message.model) {
      const chip = item.querySelector<HTMLSpanElement>(".model-chip");
      if (chip) {
        chip.hidden = !message.model;
        chip.textContent = message.model ? t(MODEL_LABELS[message.model]) : "";
      }
    }
  });
  messages = next;
  const nextAwaiting = nextAwaitingReply(awaiting, next);
  if (awaiting !== null && nextAwaiting === null) stopThinking();
  else awaiting = nextAwaiting;
  if (awaiting !== null && thinking.parentNode !== log) log.append(thinking);
  if (atBottom) log.scrollTop = log.scrollHeight;
  // Scrolled up to read: offer the new answer instead of pulling the page away.
  if (atBottom || !prefix) latest.hidden = true;
  else if (grew) latest.hidden = false;
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
  ask(input.value, true);
});

/** Send a typed question or a suggestion; a suggestion leaves the typed text alone. */
function ask(text: string, typed: boolean): void {
  if (!ready || sending || !canSend(text)) return;
  if (typed && document.hasFocus() && document.activeElement === send) input.focus();
  sending = true;
  update();
  status.textContent = "";
  startThinking();
  void invoke("chat_send", { text }).then(() => {
    if (typed && input.value === text) input.value = "";
  }).catch(() => {
    stopThinking();
    status.textContent = t("chat.send_failed");
  }).finally(() => {
    sending = false;
    update();
    // Keep typing after Send, but never pull focus back from another window or Clear.
    if (document.hasFocus() && (document.activeElement === send || document.activeElement === input)) input.focus();
  });
}

talk.addEventListener("click", () => {
  status.textContent = "";
  void invoke("chat_talk").catch(() => {
    status.textContent = t("chat.send_failed");
  });
});

log.addEventListener("scroll", () => {
  if (nearBottom()) latest.hidden = true;
});
latest.addEventListener("click", () => {
  latest.hidden = true;
  const still = window.matchMedia("(prefers-reduced-motion: reduce)").matches
    || document.documentElement.classList.contains("reduce-motion");
  log.scrollTo({ top: log.scrollHeight, behavior: still ? "auto" : "smooth" });
  // The button hid under the pointer: DOM focus back to the box, in this window only.
  if (document.hasFocus()) input.focus();
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
  showTalk();
  log.setAttribute("aria-label", t("chat.messages"));
  greeting.textContent = t("chat.greeting");
  intro.textContent = t("chat.empty");
  suggestions.setAttribute("aria-label", t("chat.suggestions"));
  chips.forEach((chip, index) => { chip.textContent = t(SUGGESTIONS[index]); });
  latest.textContent = t("chat.latest");
  thinkingText.textContent = t("chat.thinking");
}

async function start(): Promise<void> {
  const channel = new Channel<unknown>();
  channel.onmessage = render;
  // Before subscribing: the subscription reports a recording already listening.
  await listen<unknown>("chat-talk", (event) => {
    listening = event.payload === true;
    showTalk();
  });
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
