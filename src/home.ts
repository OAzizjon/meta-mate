// Home window controller (WP89a): the friendly start page for older users.
// Greets them with Mate, shows how to talk to him (hold the configured talk key), offers three
// "Try it" questions (`invoke("home_try", { key })`), says what Mate never does and where
// the keepsake cards are saved, and opens Settings (`invoke("open_settings_window")`).
// The "Show this window when MetaMate starts" checkbox saves `showHomeAtStart` through
// `load_settings` / `save_settings` (src/settings/model.ts).
// Outside Tauri the questions report a failure and the checkbox keeps changes in memory.
// All user-facing strings come from `src/i18n` (invariant 7).

import type { LocaleKey } from "./i18n/index.ts";
import { pttKeycapLabel, withPttKey } from "./settings/ptt-key.ts";
import { createSettingsStore, subscribeSettingsChanged } from "./settings/model.ts";
import type { Settings, SettingsInvoke, SettingsSubscribe } from "./settings/model.ts";

// The locale lookup comes from the page entry (src/home-page.ts): importing src/i18n here
// would load the locale JSON, which Node's test runner cannot import.
let t = (key: LocaleKey): string => key;
let getLocale = (): string => "en";
export function setHomeI18n(translate: (key: LocaleKey) => string, locale: () => string): void {
  t = translate;
  getLocale = locale;
}

/** Asks one of the tray's "Try a question" questions about the person's last app window. */
export const HOME_TRY_COMMAND = "home_try";
/** Opens the Settings window, like the tray item. */
export const OPEN_SETTINGS_COMMAND = "open_settings_window";

/** The questions `home_try` accepts: their locale keys are the questions themselves. */
export const HOME_TRY_KEYS = [
  "tray.try_search",
  "tray.try_safe",
  "tray.try_bigger",
] as const satisfies readonly LocaleKey[];
export type HomeTryKey = (typeof HOME_TRY_KEYS)[number];

/** One line under each question on what Mate will do. */
export const HOME_TRY_HINTS: Readonly<Record<HomeTryKey, LocaleKey>> = {
  "tray.try_search": "home.try_search_hint",
  "tray.try_safe": "home.try_safe_hint",
  "tray.try_bigger": "home.try_bigger_hint",
};

/** The "What Mate never does" lines, in order. */
export const HOME_NEVER_KEYS = [
  "home.never_press",
  "home.never_look",
  "home.never_passwords",
] as const satisfies readonly LocaleKey[];

/** How long the line under the questions stays after a click. */
export const HOME_STATUS_MS = 30_000;

/** Minimal IPC invoke signature matching `@tauri-apps/api/core`'s `invoke`. */
export type HomeInvoke = SettingsInvoke;

export function isHomeTryKey(value: unknown): value is HomeTryKey {
  return typeof value === "string" && (HOME_TRY_KEYS as readonly string[]).includes(value);
}

/** `asked`: Rust took the question; `failed`: it did not; `busy`: an earlier one is still going. */
export type HomeTryOutcome = "asked" | "failed" | "busy";

export interface HomeTryRunner {
  /** Asks `key` via `invoke("home_try", { key })`; never throws. */
  run(key: unknown): Promise<HomeTryOutcome>;
  isBusy(): boolean;
}

/**
 * Creates the runner behind the "Try it" buttons. One question at a time: a second click
 * while the first is on its way asks nothing (a shaking hand may click twice). An unknown
 * key, or no Tauri (`invoke` null), fails without asking.
 */
export function createHomeTryRunner(invoke: HomeInvoke | null): HomeTryRunner {
  let busy = false;
  return {
    async run(key: unknown): Promise<HomeTryOutcome> {
      if (busy) {
        return "busy";
      }
      if (!invoke || !isHomeTryKey(key)) {
        return "failed";
      }
      busy = true;
      try {
        await invoke(HOME_TRY_COMMAND, { key });
        return "asked";
      } catch {
        return "failed";
      } finally {
        busy = false;
      }
    },
    isBusy(): boolean {
      return busy;
    },
  };
}

/** The line shown under the questions after a click (`null`: leave the line as it is). */
export function tryStatusKey(outcome: HomeTryOutcome): LocaleKey | null {
  switch (outcome) {
    case "asked":
      return "home.try_asked";
    case "failed":
      return "home.try_failed";
    case "busy":
      return null;
  }
}

/** Opens the Settings window via `invoke("open_settings_window")`; `false` when it could not. */
export async function openSettingsWindow(invoke: HomeInvoke | null): Promise<boolean> {
  if (!invoke) {
    return false;
  }
  try {
    await invoke(OPEN_SETTINGS_COMMAND);
    return true;
  } catch {
    return false;
  }
}

export interface HomeSettings {
  /** The current settings snapshot. */
  get(): Settings;
  /**
   * Whether the checkbox may save: the full settings have come in (from `load_settings` or a
   * `settings-changed` event), or there is no Tauri. Saving earlier would write the defaults
   * over the person's other settings.
   */
  canSave(): boolean;
  /** Saves `showHomeAtStart` over the current settings; does nothing until `canSave()`. */
  setShowAtStart(show: boolean): Promise<Settings>;
  /** Resolves once the first `load_settings` call has finished (or failed). */
  readonly ready: Promise<Settings>;
  /** Stops listening for `settings-changed`. */
  destroy(): void;
}

/**
 * Loads the settings once, follows `settings-changed` from other windows (so a save here
 * keeps what the Settings window changed meanwhile) and calls `onChange` on every update,
 * with `canSave()` at that moment.
 */
export function startHomeSettings(
  invoke: HomeInvoke | null,
  subscribe: SettingsSubscribe | null,
  onChange: (settings: Settings, canSave: boolean) => void,
): HomeSettings {
  let known = invoke === null;
  let destroyed = false;
  // Notes when `load_settings` answered: the store itself keeps the defaults when it fails.
  const tracked = (ipc: HomeInvoke): HomeInvoke => async (cmd, args) => {
    const result = await ipc(cmd, args);
    if (cmd === "load_settings") {
      known = true;
    }
    return result;
  };
  const store = createSettingsStore(invoke ? tracked(invoke) : null);

  const unsubscribe = subscribeSettingsChanged(store, subscribe, (updated) => {
    known = true;
    if (!destroyed) {
      onChange(updated, known);
    }
  });

  const ready = store.load().then((loaded) => {
    if (!destroyed) {
      onChange(loaded, known);
    }
    return loaded;
  });

  return {
    get: () => store.get(),
    canSave: () => known,
    async setShowAtStart(show: boolean): Promise<Settings> {
      if (!known) {
        return store.get();
      }
      try {
        return await store.update({ showHomeAtStart: show });
      } finally {
        if (!destroyed) {
          onChange(store.get(), known);
        }
      }
    },
    ready,
    destroy(): void {
      destroyed = true;
      unsubscribe();
    },
  };
}

/** What the Home page needs of Mate (src/robot's `RobotController` has it all). */
export interface HomeMate {
  setEyeTarget(cursor: { readonly x: number; readonly y: number } | null): void;
  setReduceMotion(reduce: boolean): void;
  destroy(): void;
}

export interface MountHomeOptions {
  readonly invoke?: HomeInvoke | null;
  readonly subscribe?: SettingsSubscribe | null;
  /** Mounts Mate into the header (src/home-page.ts passes src/robot; tests pass none). */
  readonly mountMate?: ((container: HTMLElement) => HomeMate) | null;
}

export interface HomeController {
  getSettings(): Settings;
  destroy(): void;
}

const REDUCED_MOTION_QUERY = "(prefers-reduced-motion: reduce)";

function el<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  className: string,
  text?: string,
): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  node.className = className;
  if (text !== undefined) {
    node.textContent = text;
  }
  return node;
}

/** A section titled by its own h2, with an optional line beside the title. */
function section(id: string, className: string, title: string, intro?: string): HTMLElement {
  const node = el("section", `home-section ${className}`);
  node.setAttribute("aria-labelledby", id);
  const heading = el("h2", "home-section-title", title);
  heading.id = id;
  if (intro === undefined) {
    node.append(heading);
  } else {
    const head = el("div", "home-section-head");
    head.append(heading, el("p", "home-section-intro", intro));
    node.append(head);
  }
  return node;
}

/**
 * Mounts the Home page inside `container`: the content scrolls, the checkbox, the Write to
 * Mate and Settings buttons and Quit stay in view at the bottom (home.html). The window's own
 * close button closes it.
 */
export function mountHome(container: HTMLElement, options: MountHomeOptions = {}): HomeController {
  const invoke = options.invoke ?? null;
  const runner = createHomeTryRunner(invoke);
  let destroyed = false;
  let statusTimer: ReturnType<typeof setTimeout> | undefined;

  if (typeof document !== "undefined") {
    document.documentElement.lang = getLocale();
  }

  const content = el("div", "home-content");

  // Header: Mate beside the greeting and how to talk to him.
  const hero = el("header", "home-hero");
  const mateBox = el("div", "home-mate");
  mateBox.setAttribute("aria-hidden", "true");

  const heroText = el("div", "home-hero-text");
  const title = el("h1", "home-title", t("home.greeting"));
  const intro = el("p", "home-intro", t("home.intro"));

  const talk = el("div", "home-talk");
  talk.setAttribute("role", "group");
  talk.setAttribute("aria-labelledby", "home-talk-title");
  const keyedDefault = (key: LocaleKey): string => withPttKey(t(key), "rightCtrl", t);
  const talkTitle = el("h2", "home-section-title", keyedDefault("home.talk_title"));
  talkTitle.id = "home-talk-title";
  const talkRow = el("div", "home-talk-row");
  const keycap = el("kbd", "home-keycap", pttKeycapLabel("rightCtrl", t));
  const talkSteps = el("ol", "home-talk-steps");
  talkSteps.append(
    el("li", "home-talk-step", keyedDefault("home.talk_hold")),
    el("li", "home-talk-step", keyedDefault("home.talk_release")),
  );
  talkRow.append(keycap, talkSteps);
  // One short line: where the key is, then the click alternative.
  const talkWhere = el("span", "home-talk-where", t("home.where_rightCtrl"));
  const talkClick = el("span", "home-talk-click", t("home.talk_click"));
  const talkMore = el("p", "home-talk-more");
  talkMore.append(talkWhere, " ", talkClick);
  talk.append(talkTitle, talkRow, talkMore);

  heroText.append(title, intro, talk);
  hero.append(mateBox, heroText);

  // Try it: three questions, each with what Mate will do and its own button.
  const trySection = section("home-try-title", "home-try", t("home.try_title"), t("home.try_intro"));
  const tryList = el("ul", "home-try-list");
  const tryStatus = el("p", "home-try-status");
  tryStatus.setAttribute("role", "status");
  tryStatus.setAttribute("aria-live", "polite");

  const tryButtons: HTMLButtonElement[] = [];
  const showStatus = (key: LocaleKey | null, failed: boolean): void => {
    if (key === null || destroyed) {
      return;
    }
    clearTimeout(statusTimer);
    tryStatus.textContent = t(key);
    tryStatus.classList.toggle("is-error", failed);
    tryStatus.scrollIntoView({ block: "nearest" });
    statusTimer = setTimeout(() => {
      tryStatus.textContent = "";
      tryStatus.classList.remove("is-error");
    }, HOME_STATUS_MS);
  };

  for (const key of HOME_TRY_KEYS) {
    const id = key.replace("tray.", "home-");
    const card = el("li", "home-try-card");
    const question = el("p", "home-try-question", t(key));
    question.id = `${id}-question`;
    const hint = el("p", "home-try-hint", t(HOME_TRY_HINTS[key]));
    hint.id = `${id}-hint`;
    // Three buttons say "Try it": each is described by its own question. Soft, so the footer's
    // "Write to Mate" stays the one main button.
    const button = el("button", "btn btn-soft home-try-btn", t("home.try_button"));
    button.type = "button";
    button.id = `${id}-btn`;
    button.setAttribute("aria-describedby", `${question.id} ${hint.id}`);
    button.addEventListener("click", () => {
      // Never `disabled` while busy: a keyboard user keeps their place on the button.
      for (const other of tryButtons) {
        other.classList.add("is-busy");
      }
      void runner.run(key).then((outcome) => {
        if (!runner.isBusy()) {
          for (const other of tryButtons) {
            other.classList.remove("is-busy");
          }
        }
        showStatus(tryStatusKey(outcome), outcome === "failed");
      });
    });
    tryButtons.push(button);
    card.append(question, hint, button);
    tryList.append(card);
  }
  trySection.append(tryList, tryStatus);

  // What Mate never does.
  const neverSection = section("home-never-title", "home-never", t("home.never_title"));
  const neverList = el("ul", "home-never-list");
  for (const key of HOME_NEVER_KEYS) {
    neverList.append(el("li", "home-never-item", t(key)));
  }
  neverSection.append(neverList);

  // Keepsake cards: what they are and where they are saved.
  const cardsSection = section("home-cards-title", "home-cards", t("home.cards_title"));
  cardsSection.append(
    el("p", "home-section-intro", t("home.cards_text")),
    el("p", "home-cards-folder", t("home.cards_folder")),
  );

  content.append(hero, trySection, neverSection, cardsSection);

  // Footer, kept in view: the checkbox as a small line, Write to Mate and Settings side by
  // side, Quit as a quiet text button under them.
  const footer = el("footer", "home-footer");
  const startOption = el("label", "home-start-option");
  startOption.htmlFor = "home-show-at-start";
  // Still a checkbox, drawn as an on/off switch (src/theme.css).
  const startInput = el("input", "home-start-input switch");
  startInput.type = "checkbox";
  startInput.id = "home-show-at-start";
  // Enabled once the person's settings are in: saving earlier would reset the others.
  startInput.disabled = true;
  const startText = el("span", "home-start-text", t("home.show_at_start"));
  startOption.append(startInput, startText);
  const saveAlert = el("p", "home-try-status is-error");
  saveAlert.setAttribute("role", "alert");
  saveAlert.hidden = true;

  const actions = el("div", "home-actions");
  const chatBtn = el("button", "btn btn-primary", t("home.write"));
  chatBtn.type = "button";
  chatBtn.id = "home-chat-btn";
  chatBtn.disabled = !invoke;
  chatBtn.addEventListener("click", () => {
    void invoke?.("open_chat").catch(() => console.error("home: could not open chat"));
  });
  const settingsBtn = el("button", "btn btn-secondary", t("tray.settings"));
  settingsBtn.type = "button";
  settingsBtn.id = "home-settings-btn";
  settingsBtn.addEventListener("click", () => {
    void openSettingsWindow(invoke).then((opened) => {
      if (!opened) {
        console.warn("[home] could not open the settings window");
      }
    });
  });
  actions.append(chatBtn, settingsBtn);
  const quitBtn = el("button", "home-quit", t("home.quit"));
  quitBtn.type = "button";
  quitBtn.id = "home-quit-btn";
  quitBtn.disabled = !invoke;
  quitBtn.addEventListener("click", () => {
    void invoke?.("quit_app").catch((error: unknown) => {
      console.error("[home] could not quit:", error);
    });
  });
  footer.append(startOption, saveAlert, actions, quitBtn);

  container.replaceChildren(content, footer);

  // Mate, idle and calm (his eyes follow the mouse in idle), once his box is in the page.
  const mate = options.mountMate ? options.mountMate(mateBox) : null;
  let eyeFrame = 0;
  let eyeTarget: { x: number; y: number } | null = null;
  const onPointerMove = (event: PointerEvent): void => {
    eyeTarget = { x: event.clientX, y: event.clientY };
    if (!eyeFrame) {
      eyeFrame = requestAnimationFrame(() => {
        eyeFrame = 0;
        mate?.setEyeTarget(eyeTarget);
      });
    }
  };
  const onPointerLeave = (): void => {
    eyeTarget = null;
    mate?.setEyeTarget(null);
  };
  if (mate) {
    window.addEventListener("pointermove", onPointerMove);
    document.documentElement.addEventListener("pointerleave", onPointerLeave);
  }

  // Reduce motion: the setting or the OS stops the keycap's press and Mate's bob.
  const motionQuery =
    typeof window !== "undefined" && typeof window.matchMedia === "function"
      ? window.matchMedia(REDUCED_MOTION_QUERY)
      : null;
  let reduceSetting = false;
  const syncMotion = (): void => {
    const reduced = reduceSetting || Boolean(motionQuery?.matches);
    container.classList.toggle("reduce-motion", reduced);
    document.documentElement.classList.toggle("reduce-motion", reduced);
    mate?.setReduceMotion(reduceSetting);
  };
  motionQuery?.addEventListener("change", syncMotion);

  const settings = startHomeSettings(invoke, options.subscribe ?? null, (updated, canSave) => {
    const keyed = (key: LocaleKey): string => withPttKey(t(key), updated.pttKey, t);
    keycap.textContent = pttKeycapLabel(updated.pttKey, t);
    talkTitle.textContent = keyed("home.talk_title");
    const steps = talkSteps.children;
    steps[0].textContent = keyed("home.talk_hold");
    steps[1].textContent = keyed("home.talk_release");
    talkClick.textContent = t("home.talk_click");
    talkWhere.textContent = t(`home.where_${updated.pttKey}` as LocaleKey);
    startInput.checked = updated.showHomeAtStart;
    startInput.disabled = !canSave;
    reduceSetting = updated.reduceMotion;
    syncMotion();
  });
  syncMotion();

  const setSaveFailed = (failed: boolean): void => {
    saveAlert.hidden = !failed;
    saveAlert.textContent = failed ? t("settings.save_failed") : "";
  };

  startInput.addEventListener("change", () => {
    void settings.setShowAtStart(startInput.checked).then(
      () => {
        if (!destroyed) setSaveFailed(false);
      },
      () => {
        if (!destroyed) setSaveFailed(true);
      },
    );
  });

  return {
    getSettings: () => settings.get(),
    destroy(): void {
      destroyed = true;
      clearTimeout(statusTimer);
      if (eyeFrame) {
        cancelAnimationFrame(eyeFrame);
      }
      window.removeEventListener("pointermove", onPointerMove);
      document.documentElement.removeEventListener("pointerleave", onPointerLeave);
      motionQuery?.removeEventListener("change", syncMotion);
      settings.destroy();
      mate?.destroy();
      container.replaceChildren();
    },
  };
}
