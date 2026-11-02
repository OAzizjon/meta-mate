// The Settings window's Updates section (src/settings.ts): shows "MetaMate {version}",
// any localized release note for an available update, and a "Check for updates" /
// "Update to {version}" button backed by `update_status` and `update_install`.
// No i18n JSON or Tauri runtime imports so Node (`node --test`) runs this module directly.

import type { LocaleCode, LocaleKey, TemplateVars } from "../i18n/index.ts";
import type { SettingsInvoke } from "./model.ts";

export const UPDATE_STATUS_CMD = "update_status" as const;
export const UPDATE_CHECK_CMD = "update_check" as const;
export const UPDATE_INSTALL_CMD = "update_install" as const;
export const DEFAULT_CURRENT_VERSION = "0.2.0";

export interface UpdateNotes {
  readonly en?: string;
  readonly ru?: string;
}

export interface UpdateStatus {
  readonly current: string;
  readonly available: string | null;
  readonly notes: UpdateNotes | null;
}

export type UpdateNotice = "none" | "latest" | "failed";

const SEMVER_RE = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/;

function isPlainObject(value: unknown): value is Readonly<Record<string, unknown>> {
  return (
    typeof value === "object" &&
    value !== null &&
    !Array.isArray(value) &&
    Object.getPrototypeOf(value) === Object.prototype
  );
}

export function isValidSemver(version: unknown): version is string {
  return typeof version === "string" && SEMVER_RE.test(version);
}

function parseNotes(raw: unknown): UpdateNotes | null | undefined {
  if (raw === null || raw === undefined) return null;
  if (!isPlainObject(raw)) return undefined;
  for (const key of Object.keys(raw)) {
    if (key !== "en" && key !== "ru") return undefined;
  }
  const en = raw.en;
  const ru = raw.ru;
  if (en !== undefined && en !== null && (typeof en !== "string" || Array.from(en).length > 200)) {
    return undefined;
  }
  if (ru !== undefined && ru !== null && (typeof ru !== "string" || Array.from(ru).length > 200)) {
    return undefined;
  }
  const out: { en?: string; ru?: string } = {};
  if (typeof en === "string") out.en = en;
  if (typeof ru === "string") out.ru = ru;
  return out;
}

/** Validates the `update_status` command payload from Rust. */
export function parseUpdateStatus(raw: unknown): UpdateStatus | null {
  try {
    if (!isPlainObject(raw)) return null;
    for (const key of Object.keys(raw)) {
      if (key !== "current" && key !== "available" && key !== "notes") return null;
    }
    const { current, available } = raw;
    if (!isValidSemver(current)) return null;
    if (available !== null && available !== undefined && !isValidSemver(available)) {
      return null;
    }
    const notes = parseNotes(raw.notes);
    if (notes === undefined) return null;
    return {
      current,
      available: available ?? null,
      notes,
    };
  } catch {
    return null;
  }
}

/** Picks the release note for `locale`, falling back to the other language when absent. */
export function resolveUpdateNote(notes: UpdateNotes | null, locale: LocaleCode): string {
  if (!notes) return "";
  const primary = locale === "ru" ? notes.ru : notes.en;
  const fallback = locale === "ru" ? notes.en : notes.ru;
  return (primary ?? fallback ?? "").trim();
}

export interface UpdateSectionRefs {
  readonly section: HTMLElement;
  readonly titleEl: HTMLHeadingElement;
  readonly currentEl: HTMLParagraphElement;
  readonly notesEl: HTMLParagraphElement;
  readonly actionBtn: HTMLButtonElement;
  readonly statusEl: HTMLParagraphElement;
  readonly getStatus: () => UpdateStatus;
  readonly getNotice: () => UpdateNotice;
  readonly load: () => Promise<UpdateStatus | null>;
  readonly runAction: () => Promise<boolean>;
  readonly syncText: (
    translate?: (key: LocaleKey, vars?: TemplateVars) => string,
    locale?: LocaleCode,
  ) => void;
  readonly destroy: () => void;
}

export function buildUpdateSection(
  invoke: SettingsInvoke | null = null,
  translate: (key: LocaleKey, vars?: TemplateVars) => string = (key) => key,
  getLocaleFn: () => LocaleCode = () => "en",
): UpdateSectionRefs {
  const section = document.createElement("section");
  section.className = "settings-group settings-update";
  section.setAttribute("aria-labelledby", "settings-update-title");

  const titleEl = document.createElement("h2");
  titleEl.id = "settings-update-title";
  titleEl.className = "settings-account-title settings-update-title";

  const currentEl = document.createElement("p");
  currentEl.id = "settings-update-current";
  currentEl.className = "settings-account-why settings-update-current";

  const notesEl = document.createElement("p");
  notesEl.id = "settings-update-notes";
  notesEl.className = "settings-account-why settings-update-notes";
  notesEl.hidden = true;

  const buttonsRow = document.createElement("div");
  buttonsRow.className = "settings-account-buttons settings-update-buttons";

  const actionBtn = document.createElement("button");
  actionBtn.type = "button";
  actionBtn.id = "settings-update-action";
  actionBtn.className = "btn btn-secondary settings-account-btn settings-update-btn";
  buttonsRow.append(actionBtn);

  const statusEl = document.createElement("p");
  statusEl.id = "settings-update-status";
  statusEl.className = "settings-account-status settings-update-status";
  statusEl.setAttribute("role", "status");
  statusEl.setAttribute("aria-live", "polite");
  statusEl.hidden = true;

  section.append(titleEl, currentEl, notesEl, buttonsRow, statusEl);

  let currentTranslate = translate;
  let currentLocale: LocaleCode = getLocaleFn();
  let status: UpdateStatus = {
    current: DEFAULT_CURRENT_VERSION,
    available: null,
    notes: null,
  };
  let notice: UpdateNotice = "none";
  let busy = false;
  let destroyed = false;

  const render = (): void => {
    titleEl.textContent = currentTranslate("settings.update_title");
    currentEl.textContent = currentTranslate("settings.update_current", {
      version: status.current,
    });
    const noteText =
      status.available !== null ? resolveUpdateNote(status.notes, currentLocale) : "";
    notesEl.hidden = noteText.length === 0;
    notesEl.textContent = noteText;

    if (status.available !== null) {
      actionBtn.textContent = currentTranslate("settings.update_install", {
        version: status.available,
      });
    } else {
      actionBtn.textContent = currentTranslate("settings.update_check");
    }
    actionBtn.disabled = busy;

    if (notice === "latest") {
      statusEl.hidden = false;
      statusEl.classList.remove("is-error");
      statusEl.textContent = currentTranslate("settings.update_latest");
    } else if (notice === "failed") {
      statusEl.hidden = false;
      statusEl.classList.add("is-error");
      statusEl.textContent = currentTranslate("settings.update_failed");
    } else {
      statusEl.hidden = true;
      statusEl.classList.remove("is-error");
      statusEl.textContent = "";
    }
  };

  const load = async (): Promise<UpdateStatus | null> => {
    if (!invoke) return null;
    try {
      const raw = await invoke(UPDATE_STATUS_CMD);
      const parsed = parseUpdateStatus(raw);
      if (!parsed || destroyed) return null;
      status = parsed;
      render();
      return parsed;
    } catch {
      return null;
    }
  };

  const runAction = async (): Promise<boolean> => {
    if (busy || destroyed) return false;
    if (!invoke) {
      notice = "failed";
      render();
      return false;
    }
    busy = true;
    notice = "none";
    render();
    try {
      if (status.available !== null) {
        await invoke(UPDATE_INSTALL_CMD);
        if (!destroyed) {
          busy = false;
          render();
        }
        return true;
      }
      const raw = await invoke(UPDATE_CHECK_CMD);
      const parsed = parseUpdateStatus(raw);
      if (!parsed) {
        if (!destroyed) {
          busy = false;
          notice = "failed";
          render();
        }
        return false;
      }
      if (!destroyed) {
        status = parsed;
        busy = false;
        notice = parsed.available === null ? "latest" : "none";
        render();
      }
      return true;
    } catch {
      if (!destroyed) {
        busy = false;
        notice = "failed";
        render();
      }
      return false;
    }
  };

  actionBtn.addEventListener("click", () => {
    void runAction();
  });

  const syncText = (
    nextTranslate?: (key: LocaleKey, vars?: TemplateVars) => string,
    nextLocale?: LocaleCode,
  ): void => {
    if (nextTranslate) {
      currentTranslate = nextTranslate;
    }
    currentLocale = nextLocale ?? getLocaleFn();
    render();
  };

  syncText();

  return {
    section,
    titleEl,
    currentEl,
    notesEl,
    actionBtn,
    statusEl,
    getStatus: () => status,
    getNotice: () => notice,
    load,
    runAction,
    syncText,
    destroy(): void {
      destroyed = true;
      section.replaceChildren();
    },
  };
}
