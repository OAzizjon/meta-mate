// Draws Mate's hints in his window (mate.html `#mate-hint`); src/hints/logic.ts decides which
// and when. One picture and two or three words each, animated unless motion is reduced
// (src/hints/hints.css). Never focusable and never in the way: `pointer-events: none`, so a
// click lands on Mate (his consent click included), and nothing shows while the beam's target
// is inside the window (src/safety-stamp.ts `stampClearOf`, as the stamp). Built with
// createElementNS and textContent only, never innerHTML.
import "./hints.css";
import { bubbleReadMs } from "../bubble-timing.ts";
import { el } from "../home/dom.ts";
import { svgEl } from "../robot/svg.ts";
import { stampClearOf } from "../safety-stamp.ts";
import { buildKeyboard } from "./keyboard.ts";
import { consentPhase, createHintPolicy, DRAG_SHOW_MS, TALK_SHOW_MS } from "./logic.ts";
import type { LocaleKey } from "../i18n/index.ts";
import type { StorageLike } from "../onboarding.ts";
import type { CssRect } from "../robot/types.ts";
import type { HintKind, HintScene, TimedHint } from "./logic.ts";

/** Whatever else may talk to the person on Mate's window: while one shows, no hint does. */
const BUSY_IDS = ["mate-bubble", "mate-onboarding", "mate-keycap", "mate-stamp", "mate-menu"];

export interface HintsOptions {
  readonly element: HTMLElement;
  readonly t: (key: LocaleKey) => string;
  /** The talk key's keycap label, and whether it is held (or pressed once) to talk. */
  readonly talkKey: () => { readonly label: string; readonly hold: boolean };
}

export interface Hints {
  /** The consent question's text while the bubble shows it; null when it goes. */
  consent(text: string | null): void;
  /** Answer `generation` started thinking. */
  asked(generation: number): void;
  /** An answer finished playing. */
  answered(): void;
  /** The beam's target in this window (CSS pixels), or null. */
  avoid(target: CssRect | null): void;
  /** Development: shows `kind` now; null hides it. */
  show(kind: TimedHint | null): void;
}

export function mountHints({ element, t, talkKey }: HintsOptions): Hints {
  const root = document.documentElement;
  const policy = createHintPolicy(defaultStorage());
  let showing: HintKind | null = null;
  let target: CssRect | null = null;
  let consentText: string | null = null;
  let consentAt = 0;
  let phaseTimer: ReturnType<typeof setTimeout> | undefined;
  let hideTimer: ReturnType<typeof setTimeout> | undefined;
  let dueTimer: ReturnType<typeof setTimeout> | undefined;

  const covered = (): boolean => !stampClearOf(target, window.innerWidth, window.innerHeight);
  const scene = (): HintScene => ({
    idle: root.dataset.mateState === "idle",
    busy: document.hidden || covered() || BUSY_IDS.some((id) => {
      const other = document.getElementById(id);
      return other !== null && !other.hidden;
    }),
  });
  const put = (kind: HintKind, nodes: Node[]): void => {
    showing = kind;
    element.dataset.kind = kind;
    element.replaceChildren(...nodes);
    element.hidden = false;
  };
  const clear = (): void => {
    showing = null;
    delete element.dataset.kind;
    element.replaceChildren();
    element.hidden = true;
  };
  const hideTimed = (): void => {
    clearTimeout(hideTimer);
    hideTimer = undefined;
    if (showing === "talk" || showing === "drag") clear();
  };
  const showTimed = (kind: TimedHint): void => {
    policy.shown(kind, Date.now());
    put(kind, kind === "talk" ? talkNodes(t, talkKey()) : dragNodes(t));
    hideTimer = setTimeout(() => {
      hideTimed();
      evaluate();
    }, kind === "talk" ? TALK_SHOW_MS : DRAG_SHOW_MS);
  };
  // Runs on every change of what the window shows; one timer waits for the next due hint.
  function evaluate(): void {
    clearTimeout(dueTimer);
    dueTimer = undefined;
    if (showing === "consent") return;
    const current = scene();
    if (showing !== null) {
      // Something else needs the window (a press, a bubble): the hint goes at once.
      if (!current.idle || current.busy) hideTimed();
      else return;
    }
    const kind = policy.next(current, Date.now());
    if (kind !== null) {
      showTimed(kind);
      return;
    }
    const due = policy.dueAt();
    if (due !== null) dueTimer = setTimeout(evaluate, Math.max(0, due - Date.now()));
  }
  const stepPhase = (): void => {
    clearTimeout(phaseTimer);
    if (consentText === null) return;
    const { phase, leftMs } = consentPhase(performance.now() - consentAt, bubbleReadMs(consentText));
    root.dataset.hintPhase = phase;
    element.hidden = phase !== "picture";
    phaseTimer = setTimeout(stepPhase, leftMs);
  };

  new MutationObserver(() => evaluate()).observe(root, {
    subtree: true,
    attributes: true,
    attributeFilter: ["hidden", "data-mate-state"],
  });
  document.addEventListener("visibilitychange", () => evaluate());
  evaluate();

  return {
    consent(text) {
      if (text === consentText) return;
      consentText = text;
      if (text === null) {
        clearTimeout(phaseTimer);
        delete root.dataset.hint;
        delete root.dataset.hintPhase;
        if (showing === "consent") clear();
        evaluate();
        return;
      }
      hideTimed();
      clearTimeout(dueTimer);
      root.dataset.hint = "consent";
      put("consent", consentNodes(t));
      consentAt = performance.now();
      stepPhase();
    },
    asked: (generation) => policy.asked(generation),
    answered() {
      policy.answered();
      evaluate();
    },
    avoid(next) {
      target = next;
      element.classList.toggle("is-covering", covered());
      evaluate();
    },
    show(kind) {
      if (showing === "consent") return;
      hideTimed();
      if (kind !== null) showTimed(kind);
    },
  };
}

function defaultStorage(): StorageLike | null {
  try {
    return window.localStorage;
  } catch {
    // Access to localStorage itself can throw SecurityError.
    return null;
  }
}

// Every word and arrow sits on its own solid pill or card: the window is transparent over any app.

/** "Fn + F8" over the keyboard, the desktop caption, and "or click me" pointing at Mate. */
function consentNodes(t: (key: LocaleKey) => string): Node[] {
  const card = el("div", "hint-card");
  const keys = el("div", "hint-keys");
  keys.append(
    el("kbd", "hint-cap hint-cap-fn", t("hints.key_fn")),
    icon("hint-plus", "0 0 16 16", [["path", "M8 2.5v11M2.5 8h11"]]),
    el("kbd", "hint-cap hint-cap-f8", t("hints.key_f8")),
  );
  card.append(keys, buildKeyboard(t("hints.key_fn"), t("hints.key_f8")),
    el("p", "hint-caption", t("hints.f8_alone")));
  const click = el("div", "hint-pill hint-click-consent");
  click.append(el("span", "hint-label", t("hints.click_me")), icon("hint-arrow", "0 0 28 20", [
    ["path", "M3 10H21", "hint-line"],
    ["path", "M15 4L22 10L15 16", "hint-line"],
  ]));
  return [card, click];
}

/** The talk key pressing down with "Hold & talk", a ring around Mate, and "or click me". */
function talkNodes(t: (key: LocaleKey) => string, key: { label: string; hold: boolean }): Node[] {
  const row = el("div", "hint-talk");
  row.append(
    el("kbd", "hint-cap hint-cap-talk", key.label),
    el("span", "hint-pill", t(key.hold ? "hints.talk_hold" : "hints.talk_press")),
  );
  const click = el("div", "hint-pill hint-click-talk", t("hints.click_me"));
  const mate = document.querySelector(".mate-svg")?.getBoundingClientRect();
  if (!mate || !(mate.width > 0)) return [row, click];
  // One layout read when the hint goes up: the ring hugs Mate wherever he stands.
  const size = Math.max(mate.width, mate.height) + 12;
  const centerY = mate.top + mate.height / 2;
  const ring = el("div", "hint-ring");
  ring.style.width = ring.style.height = `${size}px`;
  ring.style.left = `${mate.left + mate.width / 2 - size / 2}px`;
  ring.style.top = `${centerY - size / 2}px`;
  // Under the ring, and above the guest trial's pill at the bottom.
  click.style.top = `${Math.min(centerY + size / 2 + 2, window.innerHeight - 54)}px`;
  return [ring, row, click];
}

/** A hand with a curved arrow: "Drag me anywhere". */
function dragNodes(t: (key: LocaleKey) => string): Node[] {
  const pill = el("div", "hint-pill hint-drag");
  const hand = icon("hint-hand", "0 0 48 40", [
    ["path", "M6 34C14 40 30 40 38 28", "hint-line"],
    ["path", "M31.4 30.8L38 28L38 35.2", "hint-line"],
  ]);
  const grab = svgEl("g", { class: "hint-hand-move" });
  grab.append(svgEl("path", {
    class: "hint-hand-fill",
    d: "M8 26C5 22 3 18 4 15C5 13 7 13 8 15L9 17L9 7C9 5 12 5 12 7L12 13L12 4C12 2 15 2 15 4"
      + "L15 13L15 5C15 3 18 3 18 5L18 14L18 8C18 6 21 6 21 8L21 18C21 23 19 26 16 27Z",
  }));
  hand.append(grab);
  pill.append(hand, el("span", "hint-label", t("hints.drag")));
  return [pill];
}

/** A decorative icon: the words beside it say the same to a screen reader. */
function icon(
  className: string,
  viewBox: string,
  paths: ReadonlyArray<readonly ["path", string, string?]>,
): SVGSVGElement {
  const svg = svgEl("svg", { class: className, viewBox, "aria-hidden": "true", focusable: "false" });
  for (const [tag, d, pathClass] of paths) {
    svg.append(svgEl(tag, pathClass ? { d, class: pathClass } : { d }));
  }
  return svg;
}
