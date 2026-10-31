// Home's comic icons, in Mate's bulb style: bold ink outlines, bulb-yellow fills. Built with
// createElementNS, no innerHTML (like src/bulb-badge.ts); the colours are `.home-icon .i-*`
// in src/home/comic.css, so both themes and contrast themes restyle them.

const SVG_NS = "http://www.w3.org/2000/svg";

/** `fill`: yellow with an ink rim; `paper`: the surface with an ink rim; `line`: ink only;
 *  `dot`: a round ink dot; `glow`: a soft yellow beam. */
type Stroke = "fill" | "paper" | "line" | "dot" | "glow";

const ICONS = {
  /** Mate himself: the card placeholder. */
  mate: [
    ["fill", "M22 44c0-7-8-10-8-20a18 18 0 0 1 36 0c0 10-8 13-8 20z"],
    ["dot", "M26 22h.01"],
    ["dot", "M38 22h.01"],
    ["line", "M27 30q5 5 10 0"],
    ["line", "M23 50h18"],
    ["line", "M26 56h12"],
  ],
  talk: [
    ["paper", "M10 30h28a4 4 0 0 1 4 4v14a4 4 0 0 1-4 4H10a4 4 0 0 1-4-4V34a4 4 0 0 1 4-4z"],
    ["line", "M14 41h20"],
    ["line", "M47 22a10 10 0 0 1 0 14"],
    ["line", "M53 15a20 20 0 0 1 0 28"],
  ],
  beam: [
    ["glow", "M20 16L58 42L40 56z"],
    ["fill", "M12 22c0-3-5-5-5-11a9 9 0 0 1 18 0c0 6-5 8-5 11z"],
    ["line", "M13 27h6"],
    ["paper", "M34 42h20a3 3 0 0 1 3 3v8a3 3 0 0 1-3 3H34a3 3 0 0 1-3-3v-8a3 3 0 0 1 3-3z"],
  ],
  steps: [
    ["line", "M6 56h14V44h14V32h14V20h10"],
    ["fill", "M41 4a7 7 0 1 1 0 14a7 7 0 1 1 0-14z"],
    ["line", "M38 11l2.5 2.5L45 8"],
  ],
  safe: [
    ["fill", "M32 6l20 7v15c0 14-9 22-20 28C21 50 12 42 12 28V13z"],
    ["line", "M23 31l6 6 12-13"],
  ],
  lessons: [
    ["line", "M17 11h33a3 3 0 0 1 3 3v31"],
    ["paper", "M11 17h33a3 3 0 0 1 3 3v33a3 3 0 0 1-3 3H11a3 3 0 0 1-3-3V20a3 3 0 0 1 3-3z"],
    ["line", "M13 47l9-10 7 7 5-5 8 8"],
    ["dot", "M35 28h.01"],
  ],
  open: [
    ["paper", "M8 10h48a3 3 0 0 1 3 3v36a3 3 0 0 1-3 3H8a3 3 0 0 1-3-3V13a3 3 0 0 1 3-3z"],
    ["line", "M5 20h54"],
    ["dot", "M11 15h.01"],
    ["dot", "M17 15h.01"],
    ["fill", "M21 36a11 11 0 1 0 22 0a11 11 0 1 0-22 0z"],
    ["line", "M32 25c-5 4-5 18 0 22c5-4 5-18 0-22"],
    ["line", "M21 36h22"],
  ],
  chat: [
    ["paper", "M10 10h44a4 4 0 0 1 4 4v24a4 4 0 0 1-4 4H28L16 54V42h-6a4 4 0 0 1-4-4V14a4 4 0 0 1 4-4z"],
    ["line", "M15 21h30"],
    ["line", "M15 30h20"],
    ["line", "M41 26v8"],
  ],
  move: [
    ["fill", "M26 38c0-4-6-6-6-13a12 12 0 0 1 24 0c0 7-6 9-6 13z"],
    ["line", "M27 44h10"],
    ["line", "M29 50h6"],
    ["line", "M4 28h11M8 24l-4 4 4 4"],
    ["line", "M60 28H49M56 24l4 4-4 4"],
  ],
} satisfies Record<string, ReadonlyArray<readonly [Stroke, string]>>;

export type HomeIconName = keyof typeof ICONS;

/** A decorative icon (hidden from screen readers: the text beside it says the same). */
export function homeIcon(name: HomeIconName, className = ""): HTMLSpanElement {
  const box = document.createElement("span");
  box.className = `home-icon ${className}`.trim();
  box.setAttribute("aria-hidden", "true");
  const svg = document.createElementNS(SVG_NS, "svg");
  svg.setAttribute("viewBox", "0 0 64 64");
  svg.setAttribute("focusable", "false");
  for (const [stroke, d] of ICONS[name]) {
    const path = document.createElementNS(SVG_NS, "path");
    path.setAttribute("class", `i-${stroke}`);
    path.setAttribute("d", d);
    svg.append(path);
  }
  box.append(svg);
  return box;
}
