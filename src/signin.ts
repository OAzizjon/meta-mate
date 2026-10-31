// The sign-in window's entry (signin.html, src-tauri/src/signin.rs): signing in is required
// before Mate answers. Mate, one sentence and two big buttons; the account store
// (src/settings/account.ts) does the rest, and Rust closes the window once signed in.
import { getAppName, t } from "./i18n/index.ts";
import { followSettingsLocale } from "./i18n/settings.ts";
import type { LocaleKey } from "./i18n/index.ts";
import { guestUsageOf } from "./mate-menu.ts";
import { mountRobot } from "./robot/index.ts";
import { AUTH_CHANGED_EVENT, accountNotice, createAccountStore, syncProviderButtons } from "./settings/account.ts";
import type {
  AccountInvoke,
  AccountState,
  AccountSubscribe,
  AuthProvider,
} from "./settings/account.ts";

const isTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

const tauriInvoke: AccountInvoke = async (cmd, args) => {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke(cmd, args);
};

const tauriSubscribe: AccountSubscribe = async (onChanged) => {
  const { listen } = await import("@tauri-apps/api/event");
  return listen<unknown>(AUTH_CHANGED_EVENT, (event) => {
    onChanged(event.payload);
  });
};

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

function mount(root: HTMLElement): void {
  document.title = getAppName();
  const store = createAccountStore(isTauri ? tauriInvoke : null, isTauri ? tauriSubscribe : null);

  const hero = el("section", "signin-hero");
  const mateBox = el("div", "signin-mate");
  mateBox.setAttribute("aria-hidden", "true");
  const words = el("div", "signin-words");
  const title = el("h1", "signin-title", t("signin.title"));
  const intro = el("p", "signin-intro", t("signin.intro"));
  words.append(title, intro);
  hero.append(mateBox, words);

  const buttons = el("div", "signin-buttons");
  const providers: readonly [AuthProvider, LocaleKey][] = [
    ["google", "auth.google"],
    ["github", "auth.github"],
  ];
  const providerButtons = providers.map(([provider, key]) => {
    const button = el("button", "btn btn-secondary signin-btn", t(key));
    button.type = "button";
    button.addEventListener("click", () => {
      void store.signIn(provider);
    });
    buttons.append(button);
    return { provider, key, button };
  });
  void syncProviderButtons(store, providerButtons);

  const browser = el("p", "signin-browser", t("signin.browser"));

  // The guest trial: up to 12 messages an hour without an account.
  const guest = el("section", "signin-guest");
  guest.hidden = true;
  const guestLine = el("p", "signin-guest-line");
  guestLine.setAttribute("role", "status");
  const guestButton = el("button", "btn btn-quiet signin-btn signin-btn-quiet", t("signin.guest_try"));
  guestButton.type = "button";
  guest.append(guestLine, guestButton);
  let guestUsage: unknown = null;
  const showGuest = (raw: unknown): void => {
    guestUsage = raw;
    const usage = guestUsageOf(raw);
    guest.hidden = usage === null;
    if (!usage) return;
    guestLine.textContent =
      usage.left > 0
        ? t("signin.guest_left", { left: String(usage.left), limit: String(usage.limit) })
        : t("signin.guest_none");
    guestButton.disabled = usage.left === 0;
  };
  guestButton.addEventListener("click", () => {
    if (!isTauri) return;
    void tauriInvoke("try_as_guest").catch(() => showGuest({ left: 0, limit: 12 }));
  });
  if (isTauri) {
    void tauriInvoke("guest_usage").then(showGuest, () => undefined);
    void import("@tauri-apps/api/event").then(({ listen }) =>
      listen<unknown>("guest-usage", (event) => showGuest(event.payload)),
    );
  }
  // Waiting is read out politely, a failure at once.
  const status = el("p", "signin-status");
  status.setAttribute("role", "status");
  status.setAttribute("aria-live", "polite");
  const alert = el("p", "signin-status is-error");
  alert.setAttribute("role", "alert");

  root.append(hero, buttons, browser, guest, status, alert);

  const robot = mountRobot(mateBox);
  robot.setMood("calm");
  robot.setState("idle");
  robot.setSize("M");

  const sync = (state: AccountState): void => {
    const waitingFor = state.status === "signing_in" ? state.provider : null;
    for (const { provider, key, button } of providerButtons) {
      button.disabled = state.status !== "signed_out" || !store.canSignIn();
      button.classList.toggle("is-waiting", waitingFor === provider);
      button.textContent = waitingFor === provider ? t("auth.waiting") : t(key);
    }
    const notice = accountNotice(state);
    status.textContent =
      state.status === "signed_in"
        ? t("signin.done")
        : notice.status
          ? t(notice.status as LocaleKey)
          : "";
    alert.textContent = notice.alert ? t(notice.alert as LocaleKey) : "";
  };
  store.subscribe(sync);
  sync(store.getState());
  void store.load().then(sync, () => undefined);
  void followSettingsLocale(() => {
    title.textContent = t("signin.title");
    intro.textContent = t("signin.intro");
    browser.textContent = t("signin.browser");
    guestButton.textContent = t("signin.guest_try");
    showGuest(guestUsage);
    sync(store.getState());
  }).catch((error: unknown) => console.error("signin: language:", error));
}

if (typeof document !== "undefined") {
  const root = document.getElementById("signin-app");
  if (root) {
    mount(root);
  }
}
