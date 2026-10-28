// The lesson card window's entry (card.html): the locale lookup, then the page itself.
import { getLocale, setLocale, t } from "./i18n/index.ts";
import { CARD_READY_EVENT, LESSON_CARD_EVENT, mountLessonCard, setCardI18n } from "./card.ts";
import type { CardController } from "./card.ts";

setCardI18n(t, getLocale);

if (typeof document !== "undefined") {
  const root = document.getElementById("card-app");
  if (root) {
    const isTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
    let controller: CardController = mountLessonCard(root, { isTauri });

    if (isTauri) {
      void (async () => {
        try {
          const { emit, listen } = await import("@tauri-apps/api/event");
          await listen<unknown>(LESSON_CARD_EVENT, (event) => {
            // The card comes in the app's language, not the system's: the page's own
            // labels are built at mount, so a new language mounts it again.
            const locale = (event.payload as { locale?: unknown } | null)?.locale;
            if ((locale === "en" || locale === "ru") && locale !== getLocale()) {
              setLocale(locale);
              controller.destroy();
              controller = mountLessonCard(root, { isTauri });
            }
            controller.handleRaw(event.payload);
          });
          // card.rs sends the card (a picture of megabytes) once the page listens.
          await emit(CARD_READY_EVENT);
        } catch (error) {
          console.error("[card] failed to listen for lesson-card event", error);
        }
      })();
    }

    const devMeta = import.meta as ImportMeta & {
      readonly env?: { readonly DEV?: boolean };
    };
    if (devMeta.env?.DEV) {
      (window as unknown as { __card?: () => CardController }).__card = () => controller;
    }
  }
}
