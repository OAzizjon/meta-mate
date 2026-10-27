// The flashlight beam in CSS pixels: a light cone from the lens to the target, plus a soft
// glow and a contour on the target (docs/robot.md "pointing"). It draws into an SVG without
// a viewBox that covers the page, so its user units are client CSS pixels.

import { BEAM_COLOR, BEAM_STOPS, CONTOUR_COLOR, computeBeamPolygon, rectCenter } from "./geometry";
import { num, setAttr, svgEl, uniqueId } from "./svg";
import type { CssPoint, CssRect } from "./types";

export interface BeamController {
  /** Shines from `nozzle` onto `target`; `halfWidth` is the cone's half width at the lens. */
  aimAt(nozzle: CssPoint, target: CssRect, halfWidth: number): void;
  /** The gentle 2 s pulse of "waiting for click". */
  setPulse(waiting: boolean): void;
  hide(): void;
}

/** The contour sits just outside the target so it never hides the button's own edge. */
const TARGET_PAD = 4;
const TARGET_RADIUS = 8;

export function createBeamController(svgRoot: SVGSVGElement): BeamController {
  const id = uniqueId("mateBeam");
  const gradient = svgEl("linearGradient", { id, gradientUnits: "userSpaceOnUse" });
  gradient.append(...BEAM_STOPS.map((stop) => svgEl("stop", stop)));
  const defs = svgEl("defs");
  defs.append(gradient);

  const cone = svgEl("polygon", { fill: `url(#${id})` });
  const glow = svgEl("rect", {
    rx: TARGET_RADIUS,
    fill: BEAM_COLOR,
    "fill-opacity": 0.22,
    stroke: BEAM_COLOR,
    "stroke-width": 10,
    "stroke-opacity": 0.45,
  });
  const contour = svgEl("rect", { rx: TARGET_RADIUS, fill: "none", stroke: CONTOUR_COLOR, "stroke-width": 3 });
  const group = svgEl("g", { class: "mate-beam" });
  group.append(cone, glow, contour);
  svgRoot.append(defs, group);

  return {
    aimAt(nozzle, target, halfWidth) {
      const points = computeBeamPolygon(nozzle, target, halfWidth);
      setAttr(cone, "points", points ? points.map((p) => `${num(p.x)},${num(p.y)}`).join(" ") : null);
      const center = rectCenter(target);
      setAttr(gradient, "x1", num(nozzle.x));
      setAttr(gradient, "y1", num(nozzle.y));
      setAttr(gradient, "x2", num(center.x));
      setAttr(gradient, "y2", num(center.y));
      const width = Math.max(0, target.right - target.left) + 2 * TARGET_PAD;
      const height = Math.max(0, target.bottom - target.top) + 2 * TARGET_PAD;
      for (const rect of [glow, contour]) {
        setAttr(rect, "x", num(target.left - TARGET_PAD));
        setAttr(rect, "y", num(target.top - TARGET_PAD));
        setAttr(rect, "width", num(width));
        setAttr(rect, "height", num(height));
      }
      group.classList.add("is-on");
    },
    setPulse(waiting) {
      group.classList.toggle("is-waiting", waiting);
    },
    hide() {
      group.classList.remove("is-on", "is-waiting");
    },
  };
}
