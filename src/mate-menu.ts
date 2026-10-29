// Page controls in Mate's unfocusable WebView; never use a native popup or focus().
import type { LocaleKey } from "./i18n/index.ts";

export const MATE_MENU_ITEMS = [
  { key: "tray.chat", command: "open_chat" },
  { key: "mate.menu_home", command: "open_home" },
  { key: "tray.settings", command: "open_settings_window" },
  null,
  { key: "tray.hide_hour", command: "hide_mate" },
  null,
  { key: "home.quit", command: "quit_app" },
] as const;

/** The guest trial's count from Rust (`GuestUsage`), or null when there is none. */
export function guestUsageOf(raw: unknown): { left: number; limit: number } | null {
  if (typeof raw !== "object" || raw === null) return null;
  const { left, limit } = raw as Record<string, unknown>;
  if (typeof left !== "number" || typeof limit !== "number" || limit <= 0) return null;
  return { left: Math.max(0, Math.min(left, limit)), limit };
}

export function hideReturnTime(minutes: number, now: number, locale: string): string {
  return new Date(now + minutes * 60_000).toLocaleTimeString(locale, {
    hour: "2-digit",
    minute: "2-digit",
  });
}

export function mountMateMenu(
  invoke: (command: string, args?: Record<string, unknown>) => Promise<unknown>,
  translate: (key: LocaleKey) => string,
): { open(): void; close(): void } {
  const menu = document.createElement("div");
  menu.id = "mate-menu";
  menu.hidden = true;
  document.body.append(menu);
  let wanted = false;
  let leaveTimer: ReturnType<typeof setTimeout> | undefined;
  // Serialize resize/restore: leaving during an unfinished open must still shrink back.
  let queue = Promise.resolve();
  const resize = (open: boolean): void => {
    queue = queue.then(async () => {
      await invoke("mate_menu_size", { open });
      menu.hidden = !wanted;
    }).catch((error: unknown) => {
      menu.hidden = true;
      wanted = false;
      console.error("mate: menu size:", error);
      // A failed open may have grown the window already: always ask for the old size back.
      if (open) {
        void invoke("mate_menu_size", { open: false }).catch(() => undefined);
      }
    });
  };
  const close = (): void => {
    clearTimeout(leaveTimer);
    if (!wanted) return;
    wanted = false;
    menu.hidden = true;
    resize(false);
  };
  const leave = (): void => {
    clearTimeout(leaveTimer);
    if (wanted) leaveTimer = setTimeout(close, 1500);
  };
  menu.addEventListener("pointerleave", leave);
  document.documentElement.addEventListener("pointerleave", leave);
  menu.addEventListener("pointerenter", () => clearTimeout(leaveTimer));
  // Prevent even DOM focus; controls work entirely through pointer clicks.
  menu.addEventListener("pointerdown", (event) => event.preventDefault());
  document.addEventListener("pointerdown", (event) => {
    if (event.target instanceof Node && !menu.contains(event.target)) close();
  }, true);
  return {
    close,
    open() {
      clearTimeout(leaveTimer);
      if (wanted) return;
      wanted = true;
      menu.replaceChildren();
      for (const item of MATE_MENU_ITEMS) {
        if (item === null) {
          menu.append(document.createElement("hr"));
          continue;
        }
        const button = document.createElement("button");
        button.type = "button";
        button.tabIndex = -1;
        button.textContent = translate(item.key);
        button.addEventListener("click", () => {
          // Dispatch immediately, so the hide deadline starts at the click.
          void invoke(item.command).catch((error: unknown) => {
            console.error("mate: menu action:", error);
          });
          close();
        });
        menu.append(button);
      }
      resize(true);
    },
  };
}
