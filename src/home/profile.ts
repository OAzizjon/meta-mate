// Home's "What should I call you?" panel (owner request, 2026-11-01): on the first open
// (`profileAsked` false) Mate asks for the person's name and, if they like, a few words about
// them; Save or Skip both mark it asked. The greeting's Edit link opens it again. The
// settings write itself is src/home.ts's (`HomeSettings.saveProfile`).
//
// Never focuses anything on its own: Home opens without the keyboard (invariant 1), and only
// an Edit click (the person is already in this window) puts the cursor in the name field.

import { PROFILE_ABOUT_MAX, PROFILE_NAME_MAX } from "../settings/model.ts";
import type { Profile, Settings } from "../settings/model.ts";
import { button, el } from "./dom.ts";
import type { Translate } from "./dom.ts";
import { homeIcon } from "./icons.ts";

/** The first-run panel shows once the person's settings are in (saving earlier would reset
 *  the others) and they have neither saved nor skipped it. */
export function shouldAskProfile(settings: Settings, canSave: boolean): boolean {
  return canSave && !settings.profileAsked;
}

/** "Hi, Anna!" with a name, the usual "Hello! I'm Mate." without. */
export function greetingText(name: string, t: Translate): string {
  const person = name.trim();
  // A function, so a "$&" in the name stays as typed.
  return person ? t("home.greeting_named").replaceAll("{person}", () => person) : t("home.greeting");
}

export interface ProfilePanel {
  readonly element: HTMLElement;
  /** Fills the fields from `profile` and shows the panel; `focus` puts the cursor in the name. */
  show(profile: Profile, focus: boolean): void;
  isShown(): boolean;
}

export interface ProfilePanelActions {
  /** Saves the typed profile (`null`: Skip, keep the saved one); resolves `false` on failure. */
  submit(profile: Profile | null): Promise<boolean>;
  /** After Save or Skip worked and the panel is gone. */
  done(): void;
}

export function createProfilePanel(t: Translate, actions: ProfilePanelActions): ProfilePanel {
  const panel = el("section", "home-profile comic-panel");
  panel.setAttribute("aria-labelledby", "home-profile-ask");
  panel.hidden = true;

  // Mate's line in a speech bubble, his bulb beside it.
  const head = el("div", "home-profile-head");
  const ask = el("h2", "home-bubble home-profile-ask", t("home.profile_ask"));
  ask.id = "home-profile-ask";
  head.append(homeIcon("mate", "home-profile-mate"), ask);

  const form = el("form", "home-profile-form");
  const nameLabel = el("label", "home-field-label", t("home.profile_name_label"));
  nameLabel.htmlFor = "home-profile-name";
  const nameInput = el("input", "home-field");
  nameInput.type = "text";
  nameInput.id = "home-profile-name";
  nameInput.maxLength = PROFILE_NAME_MAX;
  nameInput.autocomplete = "given-name";
  nameInput.spellcheck = false;

  const aboutLabel = el("label", "home-field-label", t("home.profile_about_label"));
  aboutLabel.htmlFor = "home-profile-about";
  const aboutInput = el("textarea", "home-field home-field-about");
  aboutInput.id = "home-profile-about";
  aboutInput.maxLength = PROFILE_ABOUT_MAX;
  aboutInput.rows = 3;
  aboutInput.placeholder = t("home.profile_about_placeholder");
  aboutInput.setAttribute("aria-describedby", "home-profile-hint");
  const hint = el("p", "home-profile-hint", t("home.profile_hint"));
  hint.id = "home-profile-hint";

  const failed = el("p", "home-try-status is-error");
  failed.setAttribute("role", "alert");
  failed.hidden = true;

  // Save is the form's submit, so Enter in the name field saves too.
  const actionsRow = el("div", "home-profile-actions");
  const saveBtn = el("button", "btn btn-primary", t("home.profile_save"));
  saveBtn.type = "submit";
  saveBtn.id = "home-profile-save";
  const skipBtn = button("btn btn-secondary", t("home.profile_skip"), "home-profile-skip");
  actionsRow.append(saveBtn, skipBtn);

  form.append(nameLabel, nameInput, aboutLabel, aboutInput, hint, failed, actionsRow);
  panel.append(head, form);

  // One write at a time: a double click saves once.
  let busy = false;
  const finish = (profile: Profile | null): void => {
    if (busy) {
      return;
    }
    busy = true;
    failed.hidden = true;
    void actions.submit(profile).catch(() => false).then((saved) => {
      busy = false;
      if (saved) {
        panel.hidden = true;
        actions.done();
      } else {
        failed.textContent = t("home.profile_save_failed");
        failed.hidden = false;
      }
    });
  };

  form.addEventListener("submit", (event) => {
    event.preventDefault();
    finish({ name: nameInput.value, about: aboutInput.value });
  });
  skipBtn.addEventListener("click", () => finish(null));

  return {
    element: panel,
    show(profile: Profile, focus: boolean): void {
      // Already open: keep what the person has typed.
      if (panel.hidden) {
        nameInput.value = profile.name;
        aboutInput.value = profile.about;
        failed.hidden = true;
        panel.hidden = false;
      }
      if (focus) {
        nameInput.focus();
      }
    },
    isShown: () => !panel.hidden,
  };
}
