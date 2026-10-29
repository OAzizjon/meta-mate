// The lesson card window's entry (card.html): the locale lookup, then the page itself.
import { getLocale, t } from "./i18n/index.ts";
import { followSettingsLocale } from "./i18n/settings.ts";
import { CARD_READY_EVENT, LESSON_CARD_EVENT, mountLessonCard, setCardI18n } from "./card.ts";
import type { CardController } from "./card.ts";

setCardI18n(t, getLocale);

if (typeof document !== "undefined") {
  const root = document.getElementById("card-app");
  if (root) {
    const isTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
    let receivedCard = false;
    let controller: CardController = mountLessonCard(root, { isTauri });
    void followSettingsLocale(() => {
      const initialCard = receivedCard ? controller.getCard() : undefined;
      controller.destroy();
      controller = mountLessonCard(root, { isTauri, initialCard });
    }).catch((error: unknown) => console.error("card: language:", error));

    if (isTauri) {
      void (async () => {
        try {
          const { emit, listen } = await import("@tauri-apps/api/event");
          await listen<unknown>(LESSON_CARD_EVENT, (event) => {
            if (controller.handleRaw(event.payload)) receivedCard = true;
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
