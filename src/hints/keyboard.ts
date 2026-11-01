// The consent hint's picture (src/hints/view.ts): a mini laptop keyboard, 200 x 90 user units,
// with Fn (bottom-left corner) and F8 (top row) lit in Mate's yellow. Plain blocks for every
// other key, so the two lit ones are all the eye finds. Built with createElementNS only (no
// innerHTML); colours and the press loop are `.hint-kb-*` in src/hints/hints.css, so themes,
// contrast themes and reduced motion restyle it.
import { svgEl } from "../robot/svg.ts";

export const KEYBOARD_VIEW_W = 200;
export const KEYBOARD_VIEW_H = 90;

const LEFT = 5;
const RIGHT = 195;
const GAP = 1.6;
const TOP_ROW = { y: 5, h: 9 };
/** Esc, F1 ... F12: F8 is the ninth. */
const TOP_KEYS = 13;
const F8_INDEX = 8;
const LETTER_ROWS = [
  { y: 17, first: 10 },
  { y: 29.5, first: 14 },
  { y: 42, first: 20 },
];
const LETTER_H = 10.5;
const LETTER_W = 12;
const BOTTOM_ROW = { y: 54.5, h: 12 };
/** Fn, Ctrl, Win, Alt, the space bar, Alt, Ctrl, then the arrows take what is left. */
const BOTTOM_WIDTHS = [17, 14, 14, 14, 66, 14, 14];

/** The keyboard, `fn` and `f8` the labels its lit keys carry (locale strings). */
export function buildKeyboard(fn: string, f8: string): SVGSVGElement {
  const svg = svgEl("svg", {
    class: "hint-kb",
    viewBox: `0 0 ${KEYBOARD_VIEW_W} ${KEYBOARD_VIEW_H}`,
    "aria-hidden": "true",
    focusable: "false",
  });
  svg.append(svgEl("rect", {
    class: "hint-kb-body", x: 1.2, y: 1.2, width: 197.6, height: 87.6, rx: 7,
  }));
  const topPitch = (RIGHT - LEFT + GAP) / TOP_KEYS;
  for (let i = 0; i < TOP_KEYS; i++) {
    const x = LEFT + i * topPitch;
    const w = topPitch - GAP;
    svg.append(i === F8_INDEX
      ? hotKey("hint-kb-f8", x, TOP_ROW.y, w, TOP_ROW.h, f8)
      : key(x, TOP_ROW.y, w, TOP_ROW.h));
  }
  for (const row of LETTER_ROWS) {
    svg.append(key(LEFT, row.y, row.first, LETTER_H));
    let x = LEFT + row.first + GAP;
    while (x + LETTER_W + GAP + LETTER_W <= RIGHT) {
      svg.append(key(x, row.y, LETTER_W, LETTER_H));
      x += LETTER_W + GAP;
    }
    // The row's last key (Backspace, Enter, Shift) stretches to the edge.
    svg.append(key(x, row.y, RIGHT - x, LETTER_H));
  }
  let x = LEFT;
  BOTTOM_WIDTHS.forEach((w, i) => {
    svg.append(i === 0
      ? hotKey("hint-kb-fn", x, BOTTOM_ROW.y, w, BOTTOM_ROW.h, fn)
      : key(x, BOTTOM_ROW.y, w, BOTTOM_ROW.h));
    x += w + GAP;
  });
  svg.append(key(x, BOTTOM_ROW.y, RIGHT - x, BOTTOM_ROW.h));
  // The touchpad says "laptop" at a glance.
  svg.append(svgEl("rect", { class: "hint-kb-pad", x: 72, y: 71, width: 56, height: 14, rx: 3 }));
  return svg;
}

function key(x: number, y: number, width: number, height: number): SVGRectElement {
  return svgEl("rect", { class: "hint-kb-key", x, y, width, height, rx: 1.8 });
}

/** A lit key over its own dark edge: pressing moves it down onto the edge. */
function hotKey(className: string, x: number, y: number, width: number, height: number, label: string): SVGGElement {
  const group = svgEl("g", { class: `hint-kb-hot ${className}` });
  group.append(svgEl("rect", { class: "hint-kb-edge", x, y: y + 1.6, width, height, rx: 1.8 }));
  const cap = svgEl("g", { class: "hint-kb-cap" });
  cap.append(svgEl("rect", { class: "hint-kb-top", x, y, width, height, rx: 1.8 }));
  const text = svgEl("text", {
    class: "hint-kb-label",
    x: x + width / 2,
    y: y + height / 2,
    "text-anchor": "middle",
    "dominant-baseline": "central",
  });
  // textContent only: never parsed as markup.
  text.textContent = label;
  cap.append(text);
  group.append(cap);
  return group;
}
