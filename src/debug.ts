// Debug overlay / window controller (WP14): development builds only.
// Listens for the `"debug"` Tauri event carrying `DebugMsg` payloads,
// updates the in-memory generation store, and renders timeline chips, muted text,
// PCM arrivals, horizontal bar chart timings against key-up, and look stats.
// Message text is never logged to the console.

import { listen } from "@tauri-apps/api/event";
import { DebugStore, renderDebugViewHtml } from "./debug/view.ts";
import type { DebugMsg, GenerationRecord } from "./debug/view.ts";

export interface DebugController {
  handleRaw(raw: unknown): boolean;
  addMsg(msg: DebugMsg): void;
  getGenerations(): readonly GenerationRecord[];
  clear(): void;
  destroy(): void;
}

declare global {
  interface Window {
    __debug?: DebugController;
  }
}

export function mountDebug(container: HTMLElement): DebugController {
  const store = new DebugStore();

  const render = (): void => {
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
    clear(): void {
      store.clear();
      render();
    },
    destroy(): void {
      store.clear();
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
