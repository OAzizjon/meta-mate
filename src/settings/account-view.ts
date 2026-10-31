// The Settings window's Account section (src/settings.ts): sign in with Google or GitHub,
// who is signed in, sign out, and the notice lines for screen readers. The state comes from
// the account store (src/settings/account.ts); every string from `src/i18n` (invariant 7).

import { t } from "../i18n/index.ts";
import type { LocaleKey } from "../i18n/index.ts";
import { accountNotice, syncProviderButtons } from "./account.ts";
import type { AccountState, AccountStore, AuthProvider } from "./account.ts";

export interface AccountSectionRefs {
  readonly section: HTMLElement;
  readonly syncText: () => void;
  readonly syncState: (state: AccountState) => void;
  readonly destroy: () => void;
}

export function buildAccountSection(accountStore: AccountStore): AccountSectionRefs {
  const section = document.createElement("section");
  section.className = "settings-group settings-account";
  section.setAttribute("aria-labelledby", "settings-account-title");

  const headerDiv = document.createElement("div");
  headerDiv.className = "settings-account-header";

  const titleEl = document.createElement("h2");
  titleEl.id = "settings-account-title";
  titleEl.className = "settings-account-title";

  const whyEl = document.createElement("p");
  whyEl.className = "settings-account-why";

  headerDiv.append(titleEl, whyEl);

  // Signed out view
  const signedOutView = document.createElement("div");
  signedOutView.className = "settings-account-signed-out";

  const buttonsRow = document.createElement("div");
  buttonsRow.className = "settings-account-buttons";

  const googleBtn = document.createElement("button");
  googleBtn.type = "button";
  googleBtn.id = "settings-auth-google";
  googleBtn.className = "btn btn-secondary settings-account-btn settings-account-btn-google";

  const githubBtn = document.createElement("button");
  githubBtn.type = "button";
  githubBtn.id = "settings-auth-github";
  githubBtn.className = "btn btn-secondary settings-account-btn settings-account-btn-github";

  buttonsRow.append(googleBtn, githubBtn);
  void syncProviderButtons(accountStore, [
    { provider: "google", button: googleBtn },
    { provider: "github", button: githubBtn },
  ]);
  signedOutView.append(buttonsRow);

  googleBtn.addEventListener("click", () => {
    void accountStore.signIn("google");
  });

  githubBtn.addEventListener("click", () => {
    void accountStore.signIn("github");
  });

  // Signed in view
  const signedInView = document.createElement("div");
  signedInView.className = "settings-account-signed-in";
  signedInView.style.display = "none";

  const userEl = document.createElement("span");
  userEl.className = "settings-account-user";

  const signOutBtn = document.createElement("button");
  signOutBtn.type = "button";
  signOutBtn.id = "settings-auth-signout";
  signOutBtn.className = "btn btn-secondary settings-account-sign-out";

  signOutBtn.addEventListener("click", () => {
    void accountStore.signOut();
  });

  signedInView.append(userEl, signOutBtn);

  // Accessible notice lines (`accountNotice`): waiting is read out politely, a failed
  // sign-in at once. Both stay in the DOM so screen readers hear every change.
  const statusEl = document.createElement("div");
  statusEl.className = "settings-account-status";
  statusEl.setAttribute("role", "status");
  statusEl.setAttribute("aria-live", "polite");

  const alertEl = document.createElement("div");
  alertEl.className = "settings-account-status is-error";
  alertEl.setAttribute("role", "alert");

  section.append(headerDiv, signedOutView, signedInView, statusEl, alertEl);

  const getProviderName = (provider?: AuthProvider): string => {
    if (provider === "github") {
      return t("auth.provider_github");
    }
    return t("auth.provider_google");
  };

  const syncState = (state: AccountState): void => {
    if (state.status === "signed_in") {
      signedOutView.style.display = "none";
      signedInView.style.display = "flex";
      userEl.textContent = t("auth.signed_in", {
        name: state.name,
        provider: getProviderName(state.provider),
      });
      signOutBtn.disabled = false;
    } else if (state.status === "signing_in") {
      signedOutView.style.display = "block";
      signedInView.style.display = "none";
      googleBtn.disabled = true;
      githubBtn.disabled = true;

      const isGoogle = state.provider === "google";
      googleBtn.classList.toggle("is-waiting", isGoogle);
      githubBtn.classList.toggle("is-waiting", !isGoogle);

      googleBtn.textContent = isGoogle ? t("auth.waiting") : t("auth.google");
      githubBtn.textContent = !isGoogle ? t("auth.waiting") : t("auth.github");
    } else {
      // signed_out
      signedOutView.style.display = "block";
      signedInView.style.display = "none";

      const canSign = accountStore.canSignIn();
      googleBtn.disabled = !canSign;
      githubBtn.disabled = !canSign;
      googleBtn.classList.remove("is-waiting");
      githubBtn.classList.remove("is-waiting");
      googleBtn.textContent = t("auth.google");
      githubBtn.textContent = t("auth.github");
    }

    const notice = accountNotice(state);
    statusEl.textContent = notice.status ? t(notice.status as LocaleKey) : "";
    alertEl.textContent = notice.alert ? t(notice.alert as LocaleKey) : "";
  };

  const syncText = (): void => {
    titleEl.textContent = t("auth.title");
    whyEl.textContent = t("auth.why");
    signOutBtn.textContent = t("auth.sign_out");
    syncState(accountStore.getState());
  };

  syncText();

  return {
    section,
    syncText,
    syncState,
    destroy(): void {
      section.replaceChildren();
    },
  };
}
