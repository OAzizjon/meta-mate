// The Settings window's Developer mode section ("For judges and developers"):
// a toggle with a hint and, when on, buttons for "Open Under the hood" and
// "Mate's surprise entrance". No i18n JSON or Tauri runtime imports so Node
// (`node --test`) runs this module directly.

import type { LocaleKey } from "../i18n/index.ts";
import { buildCheckbox, syncCheckbox } from "./controls.ts";
import type { CheckboxRefs } from "./controls.ts";
import type { SettingsInvoke } from "./model.ts";

export const DEV_STATUS_DURATION_MS = 4_000;
export const OPEN_UNDER_THE_HOOD_CMD = "open_under_the_hood" as const;
export const MATE_JUMP_CMD = "mate_jump" as const;
export type DevCommand = typeof OPEN_UNDER_THE_HOOD_CMD | typeof MATE_JUMP_CMD;

/** Whether the developer action buttons are hidden for the current `developerMode` setting. */
export function devButtonsHidden(developerMode: boolean): boolean {
  return !developerMode;
}

export interface DevSectionRefs {
  readonly section: HTMLElement;
  readonly titleEl: HTMLHeadingElement;
  readonly titleKey: LocaleKey;
  readonly toggle: CheckboxRefs;
  readonly buttonsRow: HTMLDivElement;
  readonly openBtn: HTMLButtonElement;
  readonly openKey: LocaleKey;
  readonly jumpBtn: HTMLButtonElement;
  readonly jumpKey: LocaleKey;
  readonly statusEl: HTMLParagraphElement;
  readonly failedKey: LocaleKey;
  readonly syncText: (translate?: (key: LocaleKey) => string) => void;
  readonly syncState: (developerMode: boolean) => void;
  readonly runCommand: (cmd: DevCommand) => Promise<boolean>;
  readonly destroy: () => void;
}

export function buildDevSection(
  onToggle: (developerMode: boolean) => void,
  invokeFn: SettingsInvoke | null = null,
  translate: (key: LocaleKey) => string = (key) => key,
  statusDurationMs: number = DEV_STATUS_DURATION_MS,
): DevSectionRefs {
  const section = document.createElement("section");
  section.className = "settings-group settings-dev";
  section.setAttribute("aria-labelledby", "settings-dev-title");

  // Under the For judges section's title (src/settings/sections-view.ts).
  const titleEl = document.createElement("h3");
  titleEl.id = "settings-dev-title";
  titleEl.className = "settings-account-title settings-dev-title";

  const toggle = buildCheckbox(
    "settings-dev-mode",
    "settings.dev_mode",
    (checked) => {
      syncState(checked);
      onToggle(checked);
    },
    "settings.dev_mode_hint",
  );

  const buttonsRow = document.createElement("div");
  buttonsRow.className = "settings-account-buttons settings-dev-buttons";

  const openBtn = document.createElement("button");
  openBtn.type = "button";
  openBtn.id = "settings-dev-open";
  openBtn.className = "btn btn-secondary settings-account-btn settings-dev-btn";

  const jumpBtn = document.createElement("button");
  jumpBtn.type = "button";
  jumpBtn.id = "settings-dev-jump";
  jumpBtn.className = "btn btn-secondary settings-account-btn settings-dev-btn";

  buttonsRow.append(openBtn, jumpBtn);

  const statusEl = document.createElement("p");
  statusEl.className = "settings-account-status settings-dev-status is-error";
  statusEl.setAttribute("role", "status");
  statusEl.setAttribute("aria-live", "polite");
  statusEl.hidden = true;

  section.append(titleEl, toggle.wrapper, buttonsRow, statusEl);

  let currentTranslate = translate;
  let failed = false;
  let destroyed = false;
  let timerId: ReturnType<typeof setTimeout> | null = null;

  const clearFailedTimer = (): void => {
    if (timerId !== null) {
      clearTimeout(timerId);
      timerId = null;
    }
  };

  const renderStatus = (): void => {
    statusEl.hidden = !failed;
    statusEl.textContent = failed ? currentTranslate("settings.dev_failed") : "";
  };

  const setFailed = (nextFailed: boolean): void => {
    clearFailedTimer();
    failed = nextFailed;
    renderStatus();
    if (nextFailed && !destroyed && statusDurationMs > 0) {
      timerId = setTimeout(() => {
        timerId = null;
        if (!destroyed) {
          failed = false;
          renderStatus();
        }
      }, statusDurationMs);
    }
  };

  const runCommand = async (cmd: DevCommand): Promise<boolean> => {
    if (!invokeFn) {
      if (!destroyed) setFailed(true);
      return false;
    }
    try {
      await invokeFn(cmd);
      if (!destroyed) setFailed(false);
      return true;
    } catch {
      if (!destroyed) setFailed(true);
      return false;
    }
  };

  openBtn.addEventListener("click", () => {
    void runCommand(OPEN_UNDER_THE_HOOD_CMD);
  });

  jumpBtn.addEventListener("click", () => {
    void runCommand(MATE_JUMP_CMD);
  });

  const syncState = (developerMode: boolean): void => {
    syncCheckbox(toggle, developerMode);
    const hidden = devButtonsHidden(developerMode);
    buttonsRow.hidden = hidden;
    openBtn.hidden = hidden;
    jumpBtn.hidden = hidden;
    if (hidden && failed) {
      setFailed(false);
    }
  };

  const syncText = (nextTranslate?: (key: LocaleKey) => string): void => {
    if (nextTranslate) {
      currentTranslate = nextTranslate;
    }
    titleEl.textContent = currentTranslate("settings.dev_title");
    toggle.titleEl.textContent = currentTranslate(toggle.labelKey);
    if (toggle.hintEl && toggle.hintKey) {
      toggle.hintEl.textContent = currentTranslate(toggle.hintKey);
    }
    openBtn.textContent = currentTranslate("settings.dev_open");
    jumpBtn.textContent = currentTranslate("settings.dev_jump");
    renderStatus();
  };

  syncText();
  syncState(false);

  return {
    section,
    titleEl,
    titleKey: "settings.dev_title",
    toggle,
    buttonsRow,
    openBtn,
    openKey: "settings.dev_open",
    jumpBtn,
    jumpKey: "settings.dev_jump",
    statusEl,
    failedKey: "settings.dev_failed",
    syncText,
    syncState,
    runCommand,
    destroy(): void {
      destroyed = true;
      clearFailedTimer();
      section.replaceChildren();
    },
  };
}
