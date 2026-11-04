// "Under the hood" debug window controller (WP14, D78).
// Listens for the `"debug"` Tauri event carrying `DebugMsg` payloads,
// updates the in-memory generation store, lights the active model chip in the
// pipeline strip, and renders generation cards with model badges, timeline chips,
// muted text, PCM arrivals, timings against key-up, and look stats.
// Message text is never logged to the console.

import { listen } from "@tauri-apps/api/event";
import { followSettingsLocale } from "./i18n/settings.ts";
import { DebugStore, renderDebugViewHtml, renderPipelineStripHtml } from "./debug/view.ts";
import type { DebugAnswerModel, DebugMsg, GenerationRecord } from "./debug/view.ts";

void followSettingsLocale(() => {});

export interface DebugController {
  handleRaw(raw: unknown): boolean;
  addMsg(msg: DebugMsg): void;
  getGenerations(): readonly GenerationRecord[];
  getActiveModel(): DebugAnswerModel | null;
  clear(): void;
  destroy(): void;
}

declare global {
  interface Window {
    __debug?: DebugController;
  }
}

export function mountDebug(
  container: HTMLElement,
  pipelineEl: HTMLElement | null = typeof document !== "undefined"
    ? document.getElementById("debug-pipeline")
    : null,
): DebugController {
  const store = new DebugStore();

  const render = (): void => {
    if (pipelineEl) {
      pipelineEl.innerHTML = renderPipelineStripHtml(store.getActiveModel());
    }
    container.innerHTML = renderDebugViewHtml(store.getGenerations());
  };

  render();

  return {
    handleRaw(raw: unknown): boolean {
      const ok = store.handleRaw(raw);
      if (ok) {
        render();
      } else {
        // Diagnostic warning only: payload text never goes to the log.
        console.warn("[debug] dropped malformed debug message");
      }
      return ok;
    },
    addMsg(msg: DebugMsg): void {
      store.addMsg(msg);
      render();
    },
    getGenerations(): readonly GenerationRecord[] {
      return store.getGenerations();
    },
    getActiveModel(): DebugAnswerModel | null {
      return store.getActiveModel();
    },
    clear(): void {
      store.clear();
      render();
    },
    destroy(): void {
      store.clear();
      if (pipelineEl) {
        pipelineEl.innerHTML = renderPipelineStripHtml(null);
      }
      container.replaceChildren();
    },
  };
}

if (typeof document !== "undefined") {
  const root = document.getElementById("debug-app");
  if (root) {
    const controller = mountDebug(root);
    if ("__TAURI_INTERNALS__" in window) {
      void listen<unknown>("debug", (event) => {
        controller.handleRaw(event.payload);
      }).catch((error: unknown) => {
        console.error("[debug] event listener failed", error);
      });
    }
    if (import.meta.env.DEV) {
      window.__debug = controller;
    }
  }
}
