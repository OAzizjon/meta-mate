// Highlight overlay window (docs/architecture.md section 2, WP5): listens for the `"highlight"`
// Tauri event from `src-tauri/src/highlight.rs`, validates the payload strictly, and draws the
// glowing target ring (inflated 6 px, rounded corners, 1.5 s pulse), the warm-white light beam
// polygon from `beamFrom` to the target's nearest edge, the numeric `"2 / 3"` step badge, and the
// X-ray scan while the model thinks (outlines sweeping top to bottom, hidden fields as dark tiles
// with a lock). Never calls `focus()` (invariant 1).

import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { parseSettings, SETTINGS_CHANGED_EVENT } from "./settings/model.ts";
import { applyTheme } from "./settings/theme.ts";
import { setLocale, t } from "./i18n/index.ts";
import {
  BADGE_HEIGHT,
  BADGE_WIDTH,
  BEAM_COLOR,
  BEAM_HALF_WIDTH,
  BURST_EVENT,
  BURST_FADE_MS,
  BURST_HOLD_MS,
  BURST_POP_MS,
  CLEAR_FADE_MS,
  RING_PAD,
  RING_RADIUS,
  SCAN_FADE_MS,
  SCAN_LIFETIME_MS,
  burstTiltAngle,
  computeBadgePosition,
  computeBeamPolygon,
  computeBurstGeometry,
  formatPolygonPoints,
  formatStepBadge,
  lockCenter,
  missPath,
  nextBurstWord,
  num,
  parseHighlightMsg,
  ringRect,
  scanDelay,
  shouldDim,
} from "./highlight/geometry.ts";
import type {
  CssRect,
  CssSize,
  HighlightMsg,
  HighlightMissMsg,
  HighlightScanMsg,
  HighlightTargetMsg,
} from "./highlight/geometry.ts";

export {
  BADGE_GAP,
  BADGE_HEIGHT,
  BADGE_WIDTH,
  BEAM_COLOR,
  BEAM_HALF_WIDTH,
  CLEAR_FADE_MS,
  LOCK_MIN_SIDE,
  MAX_SCAN_RECTS,
  MAX_STEP_TOTAL,
  RING_PAD,
  RING_RADIUS,
  SCAN_FADE_MS,
  SCAN_LIFETIME_MS,
  SCAN_SWEEP_MS,
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
  lockCenter,
  missPath,
  nearestRectEdge,
  parseHighlightMsg,
  ringRect,
  scanDelay,
  BURST_EVENT,
  BURST_FADE_MS,
  BURST_HOLD_MS,
  BURST_INNER_RADIUS,
  BURST_MARGIN,
  BURST_OUTER_RADIUS,
  BURST_POP_MS,
  BURST_SPIKES,
  BURST_WORDS_EN,
  BURST_WORDS_RU,
  burstTiltAngle,
  clampBurstCenter,
  computeBurstGeometry,
  computeBurstPolygon,
  nextBurstWord,
} from "./highlight/geometry.ts";
export type {
  BurstGeometry,
  CssPoint,
  CssRect,
  CssSize,
  HighlightBurstMsg,
  HighlightClearMsg,
  HighlightMissMsg,
  HighlightMsg,
  HighlightScanMsg,
  HighlightStep,
  HighlightTargetMsg,
  RectEdge,
  RectEdgeSide,
} from "./highlight/geometry.ts";

const SVG_NS = "http://www.w3.org/2000/svg";
const REDUCED_MOTION_QUERY = "(prefers-reduced-motion: reduce)";
// A padlock about 10 x 13 CSS px around its center: the shackle's legs stand on the body.
const LOCK_SHACKLE = "M -3 -1 V -3.5 A 3 3 0 0 1 3 -3.5 V -1";
const LOCK_BODY = { x: -5, y: -1, width: 10, height: 7, rx: 1.5 } as const;

export interface HighlightController {
  setSettings(raw: unknown): void;
  /** Applies a validated highlight message. */
  apply(msg: HighlightMsg): void;
  /** Parses an untrusted payload and applies it if valid; returns `true` when accepted. */
  handleRaw(raw: unknown): boolean;
  /** Shows a comic onomatopoeia burst at `rect`. */
  burst(rect: CssRect): void;
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
  // The beam is light, not paint: faint at Mate, brightest where it lands, soft at the edges.
  const beamLight = svgEl("linearGradient", { id: "beam-light", gradientUnits: "userSpaceOnUse" });
  beamLight.append(
    svgEl("stop", { offset: 0, "stop-color": "#ffcd3c", "stop-opacity": 0.1 }),
    svgEl("stop", { offset: 0.6, "stop-color": "#ffd75a", "stop-opacity": 0.3 }),
    svgEl("stop", { offset: 1, "stop-color": "#ffe27d", "stop-opacity": 0.48 }),
  );
  const beamSoft = svgEl("filter", {
    id: "beam-soft", x: "-20%", y: "-20%", width: "140%", height: "140%",
  });
  beamSoft.append(svgEl("feGaussianBlur", { stdDeviation: 2 }));
  defs.append(blur, mask, beamLight, beamSoft);
  const dim = svgEl("rect", {
    class: "highlight-dim",
    width: "100%", height: "100%", fill: "#141a2c", "fill-opacity": 0.4,
    mask: "url(#spotlight-hole)", display: "none",
  });
  const beam = svgEl("polygon", {
    class: "highlight-beam",
    fill: "url(#beam-light)",
    filter: "url(#beam-soft)",
    display: "none",
  });
  const ringGroup = svgEl("g", { class: "highlight-ring" });
  const ringGlow = svgEl("rect", {
    class: "highlight-ring-glow",
    rx: RING_RADIUS,
    ry: RING_RADIUS,
    fill: "none",
    stroke: "#ffe58a",
    "stroke-width": 12,
    "stroke-opacity": 0.45,
  });
  const ringStroke = svgEl("rect", {
    class: "highlight-ring-stroke",
    rx: RING_RADIUS,
    ry: RING_RADIUS,
    fill: "none",
    stroke: "#ffcd3c",
    "stroke-width": 4,
  });
  ringGroup.append(ringGlow, ringStroke);
  const missGroup = svgEl("g", { class: "highlight-miss" });
  const missMask = svgEl("mask", { id: "miss-reveal", maskUnits: "userSpaceOnUse" });
  defs.append(missMask);
  scene.append(dim, beam, ringGroup, missGroup);
  // After the scene: the Spotlight dim never covers the scan or burst.
  const scanGroup = svgEl("g", { class: "highlight-scan" });
  const burstGroup = svgEl("g", { class: "highlight-burst-group" });
  svgRoot.replaceChildren(defs, scene, scanGroup, burstGroup);

  const motionQuery = window.matchMedia(REDUCED_MOTION_QUERY);
  let reduceSetting = false;
  let motionAnimations: Animation[] = [];
  const syncMotion = (): void => {
    svgRoot.classList.toggle("reduce-motion", reduceSetting || motionQuery.matches);
    if (reduceSetting || motionQuery.matches) {
      // Cancel drawing and ripple expansion immediately if the preference changes.
      for (const animation of motionAnimations) animation.cancel();
      motionAnimations = [];
    }
  };
  syncMotion();
  motionQuery.addEventListener("change", syncMotion);

  let activeTarget: HighlightTargetMsg | null = null;
  let clearTimer: ReturnType<typeof setTimeout> | undefined;
  let missTimer: ReturnType<typeof setTimeout> | undefined;
  let missRect: CssRect | null = null;
  // The lesson step the wrong click belonged to: the next step clears it, even on the same control.
  let missStep: number | null = null;
  const stopMiss = (): void => {
    if (missTimer !== undefined) clearTimeout(missTimer);
    missTimer = undefined;
    missRect = null;
    missStep = null;
    for (const animation of motionAnimations) animation.cancel();
    motionAnimations = [];
    missGroup.getAnimations().forEach((animation) => animation.cancel());
    missGroup.replaceChildren();
    missMask.replaceChildren();
  };
  const drawMiss = (msg: HighlightMissMsg): void => {
    stopMiss();
    // Aim at the padded ring; clicks already inside its glow need no trail.
    const d = missPath(msg.from, ringRect(msg.rect));
    if (!d) return;
    missRect = msg.rect;
    missStep = activeTarget?.step?.number ?? null;
    const reveal = svgEl("path", {
      d, pathLength: 1, fill: "none", stroke: "white", "stroke-width": 12,
    });
    missMask.append(reveal);
    const trail = svgEl("path", {
      d, fill: "none", stroke: BEAM_COLOR, "stroke-width": 3,
      "stroke-linecap": "round", "stroke-dasharray": "1 9", mask: "url(#miss-reveal)",
    });
    const ripple = svgEl("circle", {
      cx: msg.from.x, cy: msg.from.y, r: 16, fill: "none",
      stroke: BEAM_COLOR, "stroke-width": 2, "stroke-opacity": 0.65,
    });
    missGroup.append(ripple, trail);
    missGroup.animate([
      { opacity: 0 }, { opacity: 1, offset: 0.15 },
      { opacity: 1, offset: 0.7 }, { opacity: 0 },
    ], { duration: 1600, fill: "forwards" });
    if (!(reduceSetting || motionQuery.matches)) {
      motionAnimations = [
        reveal.animate([
          { strokeDasharray: "1", strokeDashoffset: "1" },
          { strokeDasharray: "1", strokeDashoffset: "0" },
        ], { duration: 700, delay: 100, fill: "both", easing: "ease-out" }),
        ripple.animate([{ r: "4px", opacity: 0.8 }, { r: "24px", opacity: 0 }], {
          duration: 700, fill: "forwards", easing: "ease-out",
        }),
      ];
    }
    missTimer = setTimeout(stopMiss, 1600);
  };

  const stopClearTimer = (): void => {
    if (clearTimer !== undefined) {
      clearTimeout(clearTimer);
      clearTimer = undefined;
    }
  };

  let scanTimer: ReturnType<typeof setTimeout> | undefined;
  const stopScanTimer = (): void => {
    if (scanTimer !== undefined) clearTimeout(scanTimer);
    scanTimer = undefined;
  };
  // Its elements go once the fade is over: nothing of the scan stays to animate.
  const fadeScan = (): void => {
    stopScanTimer();
    if (!scanGroup.hasChildNodes()) return;
    scanGroup.classList.remove("is-active");
    scanTimer = setTimeout(() => {
      scanTimer = undefined;
      scanGroup.replaceChildren();
    }, SCAN_FADE_MS);
  };
  const drawScan = (msg: HighlightScanMsg): void => {
    // Nothing of this look on this monitor: the last scan here fades.
    if (msg.rects.length + msg.locked.length === 0) {
      fadeScan();
      return;
    }
    stopScanTimer();
    scanGroup.replaceChildren();
    const height = viewportSize().height;
    const box = (rect: CssRect): Record<string, string> => ({
      x: num(rect.left),
      y: num(rect.top),
      width: num(rect.right - rect.left),
      height: num(rect.bottom - rect.top),
    });
    // Each appears when the sweep passes its top (no sweep with reduced motion: CSS).
    const sweep = (el: SVGElement, rect: CssRect): void => {
      el.style.animationDelay = `${scanDelay(rect, height)}ms`;
      scanGroup.append(el);
    };
    for (const rect of msg.rects) {
      sweep(svgEl("rect", { class: "highlight-scan-outline", rx: 3, ry: 3, ...box(rect) }), rect);
    }
    for (const rect of msg.locked) {
      // A dark tile and a lock, never words (invariant 7) or what the field holds.
      const tile = svgEl("g", { class: "highlight-scan-locked" });
      tile.append(svgEl("rect", { class: "highlight-scan-tile", rx: 3, ry: 3, ...box(rect) }));
      const center = lockCenter(rect);
      if (center) {
        const lock = svgEl("g", {
          class: "highlight-scan-lock",
          transform: `translate(${num(center.x)} ${num(center.y)})`,
        });
        lock.append(svgEl("path", { d: LOCK_SHACKLE }), svgEl("rect", LOCK_BODY));
        tile.append(lock);
      }
      sweep(tile, rect);
    }
    scanGroup.classList.add("is-active");
    scanTimer = setTimeout(fadeScan, SCAN_LIFETIME_MS - SCAN_FADE_MS);
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
    // The chosen element keeps its ring; the rest of what Mate read fades.
    fadeScan();
    if (missRect && ((msg.step?.number ?? null) !== missStep ||
      (Object.keys(missRect) as (keyof CssRect)[]).some((key) => missRect?.[key] !== msg.rect[key]))) {
      stopMiss();
    }
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
    if (polygon && msg.beamFrom) {
      // The gradient runs from Mate's flashlight to the middle of the target.
      setAttr(beamLight, "x1", num(msg.beamFrom.x));
      setAttr(beamLight, "y1", num(msg.beamFrom.y));
      // A zero-length gradient has no direction: keep it at least a pixel long.
      const centerX = (msg.rect.left + msg.rect.right) / 2;
      const centerY = (msg.rect.top + msg.rect.bottom) / 2;
      const short = Math.hypot(centerX - msg.beamFrom.x, centerY - msg.beamFrom.y) < 1;
      setAttr(beamLight, "x2", num(short ? msg.beamFrom.x + 1 : centerX));
      setAttr(beamLight, "y2", num(centerY));
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

    if (burstEl && !burstEl.classList.contains("is-fading")) {
      // Never cover the next target for more than 1 s: transition immediately to fade-out.
      if (burstTimer) {
        clearTimeout(burstTimer);
        burstTimer = undefined;
      }
      burstEl.classList.remove("is-popping");
      burstEl.classList.add("is-fading");
      burstFadeTimer = setTimeout(() => {
        stopBurst();
      }, BURST_FADE_MS);
    }

    scene.classList.add("is-active");
  };

  let burstTurn = 0;
  let burstTimer: ReturnType<typeof setTimeout> | undefined;
  let burstFadeTimer: ReturnType<typeof setTimeout> | undefined;
  let burstEl: SVGElement | null = null;

  const stopBurst = (): void => {
    if (burstTimer) {
      clearTimeout(burstTimer);
      burstTimer = undefined;
    }
    if (burstFadeTimer) {
      clearTimeout(burstFadeTimer);
      burstFadeTimer = undefined;
    }
    if (burstEl) {
      burstEl.remove();
      burstEl = null;
    }
  };

  const showBurst = (rect: CssRect): void => {
    stopBurst();
    const words = [
      t("highlight.burst_click"),
      t("highlight.burst_nice"),
      t("highlight.burst_done"),
    ];
    const word = nextBurstWord(burstTurn, words);
    const tilt = burstTiltAngle(burstTurn);
    burstTurn += 1;

    const geo = computeBurstGeometry(rect, viewportSize());
    burstEl = svgEl("g", {
      class: "highlight-burst is-popping",
    });
    setAttr(
      burstEl,
      "style",
      `--burst-cx: ${num(geo.center.x)}px; --burst-cy: ${num(geo.center.y)}px;`,
    );

    const star = svgEl("polygon", {
      class: "highlight-burst-star",
      points: formatPolygonPoints(geo.points),
    });

    const text = svgEl("text", {
      class: "highlight-burst-text",
      x: num(geo.center.x),
      y: num(geo.center.y),
      transform: `rotate(${tilt} ${num(geo.center.x)} ${num(geo.center.y)})`,
    });
    text.textContent = word;

    burstEl.append(star, text);
    burstGroup.append(burstEl);

    // Pops in with an overshoot scale (200 ms), holds 600 ms, fades out (250 ms).
    burstTimer = setTimeout(() => {
      burstTimer = undefined;
      if (!burstEl) return;
      burstEl.classList.remove("is-popping");
      burstEl.classList.add("is-fading");
      burstFadeTimer = setTimeout(() => {
        stopBurst();
      }, BURST_FADE_MS);
    }, BURST_POP_MS + BURST_HOLD_MS);
  };

  const clear = (): void => {
    stopClearTimer();
    stopMiss();
    stopBurst();
    fadeScan();
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
    if (msg.type === "miss") {
      drawMiss(msg);
      return;
    }
    if (msg.type === "scan") {
      drawScan(msg);
      return;
    }
    if (msg.type === "burst") {
      showBurst(msg.rect);
      return;
    }
    drawTarget(msg);
  };

  return {
    apply,
    setSettings(raw) {
      const settings = parseSettings(raw);
      setLocale(settings.language);
      if (typeof document !== "undefined") {
        applyTheme(settings.theme, document.documentElement);
      }
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
    burst: showBurst,
    clear,
    destroy() {
      stopClearTimer();
      stopMiss();
      stopScanTimer();
      stopBurst();
      motionQuery.removeEventListener("change", syncMotion);
      window.removeEventListener("resize", onResize);
      scene.remove();
      scanGroup.remove();
      burstGroup.remove();
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
      void getCurrentWebviewWindow().listen<unknown>(BURST_EVENT, (event) => {
        controller.handleRaw(event.payload);
      }).catch((error: unknown) => {
        console.error("[highlight] burst event listener failed", error);
      });
    }
    if (import.meta.env.DEV) {
      window.__highlight = controller;
    }
  }
}
