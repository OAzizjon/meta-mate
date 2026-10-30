// Highlight overlay window (docs/architecture.md section 2, WP5): listens for the `"highlight"`
// Tauri event from `src-tauri/src/highlight.rs`, validates the payload strictly, and draws the
// glowing target ring (inflated 6 px, rounded corners, 1.5 s pulse), the warm-white light beam
// polygon from `beamFrom` to the target's nearest edge, and the numeric `"2 / 3"` step badge.
// Never calls `focus()` (invariant 1).

import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { parseSettings, SETTINGS_CHANGED_EVENT } from "./settings/model.ts";
import {
  BADGE_HEIGHT,
  BADGE_WIDTH,
  BEAM_COLOR,
  BEAM_HALF_WIDTH,
  CLEAR_FADE_MS,
  RING_PAD,
  RING_RADIUS,
  computeBadgePosition,
  computeBeamPolygon,
  formatPolygonPoints,
  formatStepBadge,
  num,
  parseHighlightMsg,
  ringRect,
  shouldDim,
} from "./highlight/geometry.ts";
import type {
  CssRect,
  CssSize,
  HighlightMsg,
  HighlightTargetMsg,
} from "./highlight/geometry.ts";

export {
  BADGE_GAP,
  BADGE_HEIGHT,
  BADGE_WIDTH,
  BEAM_COLOR,
  BEAM_HALF_WIDTH,
  CLEAR_FADE_MS,
  MAX_STEP_TOTAL,
  RING_PAD,
  RING_RADIUS,
  VIEWPORT_MARGIN,
  badgePosition,
  beamPolygon,
  computeBadgePosition,
  computeBeamPolygon,
  formatPolygonPoints,
  formatStepBadge,
  inflateRect,
  isValidPoint,
  isValidRect,
  isValidStep,
  nearestRectEdge,
  parseHighlightMsg,
  ringRect,
} from "./highlight/geometry.ts";
export type {
  CssPoint,
  CssRect,
  CssSize,
  HighlightClearMsg,
  HighlightMsg,
  HighlightStep,
  HighlightTargetMsg,
  RectEdge,
  RectEdgeSide,
} from "./highlight/geometry.ts";

const SVG_NS = "http://www.w3.org/2000/svg";
const REDUCED_MOTION_QUERY = "(prefers-reduced-motion: reduce)";

export interface HighlightController {
  setSettings(raw: unknown): void;
  /** Applies a validated highlight message (`"target"` or `"clear"`). */
  apply(msg: HighlightMsg): void;
  /** Parses an untrusted payload and applies it if valid; returns `true` when accepted. */
  handleRaw(raw: unknown): boolean;
  /** Fades out the current highlight in 200 ms. */
  clear(): void;
  destroy(): void;
}

declare global {
  interface Window {
    __highlight?: HighlightController;
  }
}

function svgEl<K extends keyof SVGElementTagNameMap>(
  tag: K,
  attrs: Readonly<Record<string, string | number>> = {},
): SVGElementTagNameMap[K] {
  const el = document.createElementNS(SVG_NS, tag);
  for (const [name, value] of Object.entries(attrs)) {
    el.setAttribute(name, String(value));
  }
  return el;
}

function setAttr(el: Element, name: string, value: string | null): void {
  if (el.getAttribute(name) === value) return;
  if (value === null) el.removeAttribute(name);
  else el.setAttribute(name, value);
}

function ensureBadgeElement(existing: HTMLElement | null): HTMLElement {
  if (existing) return existing;
  const created = document.createElement("div");
  created.id = "highlight-badge";
  created.setAttribute("aria-hidden", "true");
  created.hidden = true;
  document.body.append(created);
  return created;
}

function viewportSize(): CssSize {
  return {
    width: window.innerWidth,
    height: window.innerHeight,
  };
}

function measuredBadgeSize(badge: HTMLElement): CssSize {
  const width = badge.offsetWidth > 0 ? badge.offsetWidth : BADGE_WIDTH;
  const height = badge.offsetHeight > 0 ? badge.offsetHeight : BADGE_HEIGHT;
  return { width, height };
}

export function mountHighlight(
  svgRoot: SVGSVGElement,
  badgeElement: HTMLElement | null = document.getElementById("highlight-badge"),
): HighlightController {
  const badge = ensureBadgeElement(badgeElement);

  const scene = svgEl("g", { class: "highlight-scene" });
  // In the existing highlight window: capture hides and verifies this whole window.
  const defs = svgEl("defs");
  const blur = svgEl("filter", {
    id: "spotlight-soft", x: "-50%", y: "-50%", width: "200%", height: "200%",
  });
  blur.append(svgEl("feGaussianBlur", { stdDeviation: 12 }));
  const mask = svgEl("mask", {
    id: "spotlight-hole", maskUnits: "userSpaceOnUse", x: 0, y: 0, width: "100%", height: "100%",
  });
  mask.append(svgEl("rect", { width: "100%", height: "100%", fill: "white" }));
  const hole = svgEl("rect", { fill: "black", rx: RING_RADIUS, filter: "url(#spotlight-soft)" });
  mask.append(hole);
  defs.append(blur, mask);
  const dim = svgEl("rect", {
    width: "100%", height: "100%", fill: "black", "fill-opacity": 0.35,
    mask: "url(#spotlight-hole)", display: "none",
  });
  const beam = svgEl("polygon", {
    class: "highlight-beam",
    fill: BEAM_COLOR,
    "fill-opacity": 0.38,
    display: "none",
  });
  const ringGroup = svgEl("g", { class: "highlight-ring" });
  const ringGlow = svgEl("rect", {
    class: "highlight-ring-glow",
    rx: RING_RADIUS,
    ry: RING_RADIUS,
    fill: BEAM_COLOR,
    "fill-opacity": 0.14,
    stroke: "#ffe08a",
    "stroke-width": 10,
    "stroke-opacity": 0.45,
  });
  const ringStroke = svgEl("rect", {
    class: "highlight-ring-stroke",
    rx: RING_RADIUS,
    ry: RING_RADIUS,
    fill: "none",
    stroke: "#ffd23f",
    "stroke-width": 3,
  });
  ringGroup.append(ringGlow, ringStroke);
  scene.append(dim, beam, ringGroup);
  svgRoot.replaceChildren(defs, scene);

  const motionQuery = window.matchMedia(REDUCED_MOTION_QUERY);
  let reduceSetting = false;
  const syncMotion = (): void => {
    svgRoot.classList.toggle("reduce-motion", reduceSetting || motionQuery.matches);
  };
  syncMotion();
  motionQuery.addEventListener("change", syncMotion);

  let activeTarget: HighlightTargetMsg | null = null;
  let clearTimer: ReturnType<typeof setTimeout> | undefined;

  const stopClearTimer = (): void => {
    if (clearTimer !== undefined) {
      clearTimeout(clearTimer);
      clearTimer = undefined;
    }
  };

  const positionBadge = (rect: CssRect): void => {
    const pos = computeBadgePosition(rect, viewportSize(), measuredBadgeSize(badge));
    badge.style.left = `${num(pos.x)}px`;
    badge.style.top = `${num(pos.y)}px`;
  };

  const onResize = (): void => {
    if (activeTarget) {
      const visible = activeTarget.dim && shouldDim(activeTarget.rect, viewportSize());
      setAttr(dim, "display", visible ? null : "none");
    }
    if (activeTarget?.step) {
      positionBadge(activeTarget.rect);
    }
  };
  window.addEventListener("resize", onResize);

  const drawTarget = (msg: HighlightTargetMsg): void => {
    stopClearTimer();
    activeTarget = msg;
    svgRoot.style.setProperty("--warmth", "0");
    svgRoot.style.setProperty("--warmth-period", "1.8s");
    svgRoot.classList.remove("on-target");
    setAttr(dim, "display", msg.dim && shouldDim(msg.rect, viewportSize()) ? null : "none");
    const opening = ringRect(msg.rect, RING_PAD + 16);
    setAttr(hole, "x", num(opening.left));
    setAttr(hole, "y", num(opening.top));
    setAttr(hole, "width", num(opening.right - opening.left));
    setAttr(hole, "height", num(opening.bottom - opening.top));

    const ring = ringRect(msg.rect, RING_PAD);
    const width = Math.max(0, ring.right - ring.left);
    const height = Math.max(0, ring.bottom - ring.top);
    for (const el of [ringGlow, ringStroke]) {
      setAttr(el, "x", num(ring.left));
      setAttr(el, "y", num(ring.top));
      setAttr(el, "width", num(width));
      setAttr(el, "height", num(height));
    }

    const polygon = msg.beamFrom
      ? computeBeamPolygon(msg.beamFrom, msg.rect, BEAM_HALF_WIDTH)
      : null;
    if (polygon) {
      setAttr(beam, "points", formatPolygonPoints(polygon));
      setAttr(beam, "display", null);
    } else {
      setAttr(beam, "points", null);
      setAttr(beam, "display", "none");
    }

    if (msg.step) {
      // Digits only (invariant 7): never parse as markup.
      badge.textContent = formatStepBadge(msg.step);
      badge.hidden = false;
      positionBadge(msg.rect);
      badge.classList.add("is-active");
    } else {
      badge.classList.remove("is-active");
      badge.hidden = true;
      badge.textContent = "";
    }

    scene.classList.add("is-active");
  };

  const clear = (): void => {
    stopClearTimer();
    activeTarget = null;
    scene.classList.remove("is-active");
    badge.classList.remove("is-active");
    clearTimer = setTimeout(() => {
      clearTimer = undefined;
      badge.hidden = true;
      badge.textContent = "";
      setAttr(beam, "points", null);
      setAttr(beam, "display", "none");
    }, CLEAR_FADE_MS);
  };

  const apply = (msg: HighlightMsg): void => {
    if (msg.type === "clear") {
      clear();
      return;
    }
    if (msg.type === "warmth") {
      if (!activeTarget) return;
      svgRoot.style.setProperty("--warmth", String(msg.level / 9));
      svgRoot.style.setProperty("--warmth-period", `${1.8 - msg.level / 9 * 1.2}s`);
      svgRoot.classList.toggle("on-target", msg.onTarget);
      return;
    }
    drawTarget(msg);
  };

  return {
    apply,
    setSettings(raw) {
      const settings = parseSettings(raw);
      reduceSetting = settings.reduceMotion;
      syncMotion();
      if (activeTarget) {
        activeTarget = { ...activeTarget, dim: settings.spotlight };
        onResize();
      }
    },
    handleRaw(raw) {
      const msg = parseHighlightMsg(raw);
      if (!msg) return false;
      apply(msg);
      return true;
    },
    clear,
    destroy() {
      stopClearTimer();
      motionQuery.removeEventListener("change", syncMotion);
      window.removeEventListener("resize", onResize);
      scene.remove();
      defs.remove();
      badge.hidden = true;
      badge.textContent = "";
      badge.classList.remove("is-active");
    },
  };
}

if (typeof document !== "undefined") {
  const svg = document.getElementById("highlight");
  if (svg instanceof SVGSVGElement) {
    const controller = mountHighlight(svg);
    if ("__TAURI_INTERNALS__" in window) {
      let settingsChanged = false;
      void import("@tauri-apps/api/core").then(async ({ invoke }) => {
        const settings = await invoke("load_settings");
        if (!settingsChanged) controller.setSettings(settings);
      }).catch((error: unknown) => console.error("[highlight] settings load failed", error));
      // This window only: with one highlight window per monitor, a global listener would
      // draw every monitor's ring at its own coordinates.
      void getCurrentWebviewWindow().listen<unknown>(SETTINGS_CHANGED_EVENT, (event) => {
        settingsChanged = true;
        controller.setSettings(event.payload);
      });
      void getCurrentWebviewWindow().listen<unknown>("highlight", (event) => {
        controller.handleRaw(event.payload);
      }).catch((error: unknown) => {
        console.error("[highlight] event listener failed", error);
      });
    }
    if (import.meta.env.DEV) {
      window.__highlight = controller;
    }
  }
}
