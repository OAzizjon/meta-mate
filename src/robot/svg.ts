// Builds Mate's layered SVG from geometry.ts with createElementNS only (no innerHTML), and
// swaps the face and arm layers in place.

import {
  ARM_L,
  ARM_R,
  AURA_GRADIENT,
  CHEEKS,
  EYES,
  FLASHLIGHT,
  GLASS_GRADIENT,
  METAL_GRADIENT,
  MIRROR_TRANSFORM,
  MOUTHS,
  SHADOW,
  SHOULDER,
  SLEEP_Z,
  STATUS_LIGHT,
  STRAIGHT_AIM,
  VIEW_H,
  VIEW_W,
  auraNodes,
  baseNodes,
  glassNodes,
  openMouthRy,
} from "./geometry";
import type { ArmAim, ArmPose, EyeShape, GradientSpec, MouthShape, SvgAttrs, SvgNode, ViewPoint } from "./geometry";

const SVG_NS = "http://www.w3.org/2000/svg";

/** docs/robot.md's layers (the beam is beam.ts) plus the sleeping "z". */
export const LAYERS = [
  "shadow",
  "aura",
  "arm-l",
  "arm-r",
  "flashlight",
  "base",
  "status-light",
  "glass",
  "face",
  "cheeks",
  "z",
] as const;
export type LayerName = (typeof LAYERS)[number];

export interface MateSvgRefs {
  readonly svg: SVGSVGElement;
  /** Everything but the shadow: hover, lean, jump and tilt move this group. */
  readonly body: SVGGElement;
  readonly layers: Readonly<Record<LayerName, SVGGElement>>;
  /** Mirrors both arms when the target is on the left. */
  readonly arms: SVGGElement;
  /** Turns the right arm and the flashlight around the shoulder. */
  readonly aim: SVGGElement;
  readonly eyes: SVGGElement;
  readonly mouth: SVGGElement;
}

let seq = 0;

/** Gradient ids are document-wide: every drawing gets its own prefix (as in mate-lib.js). */
export function uniqueId(prefix: string): string {
  seq += 1;
  return `${prefix}${seq}`;
}

export function svgEl<K extends keyof SVGElementTagNameMap>(tag: K, attrs: SvgAttrs = {}): SVGElementTagNameMap[K] {
  const el = document.createElementNS(SVG_NS, tag);
  for (const [name, value] of Object.entries(attrs)) el.setAttribute(name, String(value));
  return el;
}

/** Sets (or with null removes) an attribute only when it changes. */
export function setAttr(el: Element, name: string, value: string | null): void {
  if (el.getAttribute(name) === value) return;
  if (value === null) el.removeAttribute(name);
  else el.setAttribute(name, value);
}

/** Short attribute numbers: rounded to 0.01. */
export function num(value: number): string {
  return String(Math.round(value * 100) / 100);
}

function build(node: SvgNode): SVGElement {
  const el = svgEl(node.tag, node.attrs);
  if (node.text !== undefined) el.textContent = node.text;
  if (node.children) el.append(...node.children.map(build));
  return el;
}

function gradient(spec: GradientSpec, id: string): SVGElement {
  const el = svgEl(spec.tag, { ...spec.attrs, id });
  el.append(...spec.stops.map((stop) => svgEl("stop", stop)));
  return el;
}

function layer(name: LayerName, nodes: readonly SvgNode[] = []): SVGGElement {
  const g = svgEl("g", { class: `mate-layer-${name}` });
  g.append(...nodes.map(build));
  return g;
}

export function buildMateSvg(idPrefix: string = uniqueId("mate")): MateSvgRefs {
  const ids = { glass: `${idPrefix}Glass`, aura: `${idPrefix}Aura`, metal: `${idPrefix}Metal` };
  const svg = svgEl("svg", {
    viewBox: `0 0 ${VIEW_W} ${VIEW_H}`,
    class: "mate-svg",
    "shape-rendering": "geometricPrecision",
    "aria-hidden": "true",
  });
  const defs = svgEl("defs");
  defs.append(gradient(GLASS_GRADIENT, ids.glass), gradient(AURA_GRADIENT, ids.aura), gradient(METAL_GRADIENT, ids.metal));

  const layers: Record<LayerName, SVGGElement> = {
    shadow: layer("shadow", SHADOW),
    aura: layer("aura", auraNodes(`url(#${ids.aura})`)),
    "arm-l": layer("arm-l"),
    "arm-r": layer("arm-r"),
    flashlight: layer("flashlight", FLASHLIGHT),
    base: layer("base", baseNodes(`url(#${ids.metal})`)),
    "status-light": layer("status-light", STATUS_LIGHT),
    glass: layer("glass", glassNodes(`url(#${ids.glass})`)),
    face: layer("face"),
    cheeks: layer("cheeks", CHEEKS),
    z: layer("z", SLEEP_Z),
  };
  const eyes = svgEl("g", { class: "mate-eyes" });
  const mouth = svgEl("g", { class: "mate-mouth" });
  layers.face.append(eyes, mouth);
  const aim = svgEl("g", { class: "mate-aim" });
  aim.append(layers["arm-r"], layers.flashlight);
  const arms = svgEl("g", { class: "mate-arms" });
  arms.append(layers["arm-l"], aim);

  // The art's paint order: shadow, aura, arms, base, glass, face, cheeks.
  const body = svgEl("g", { class: "mate-body" });
  body.append(layers.aura, arms, layers.base, layers["status-light"], layers.glass, layers.face, layers.cheeks, layers.z);
  svg.append(defs, layers.shadow, body);

  const refs: MateSvgRefs = { svg, body, layers, arms, aim, eyes, mouth };
  setEyes(refs, "open");
  setMouth(refs, "smile");
  setArmPose(refs, "rest");
  return refs;
}

function swap(group: SVGGElement, key: string, nodes: readonly SvgNode[]): void {
  if (group.getAttribute("data-shape") === key) return;
  group.replaceChildren(...nodes.map(build));
  group.setAttribute("data-shape", key);
}

export function setEyes(refs: MateSvgRefs, shape: EyeShape): void {
  swap(refs.eyes, shape, EYES[shape]);
}

export function setMouth(refs: MateSvgRefs, shape: MouthShape): void {
  swap(refs.mouth, shape, MOUTHS[shape]);
}

/** The speaking mouth, opened by a voice level 0..1. */
export function setMouthOpen(refs: MateSvgRefs, level: number): void {
  setMouth(refs, "open");
  const ellipse = refs.mouth.querySelector("ellipse");
  if (ellipse) setAttr(ellipse, "ry", num(openMouthRy(level)));
}

/** Shifts the eyes by `offset` viewBox units (looking around). */
export function setGaze(refs: MateSvgRefs, offset: ViewPoint): void {
  const still = Math.abs(offset.x) < 0.01 && Math.abs(offset.y) < 0.01;
  setAttr(refs.eyes, "transform", still ? null : `translate(${num(offset.x)} ${num(offset.y)})`);
}

/** Rest, point (flashlight up, turned by `aim`) or shrug (both palms up). */
export function setArmPose(refs: MateSvgRefs, pose: ArmPose, aim: ArmAim = STRAIGHT_AIM): void {
  const armR = refs.layers["arm-r"];
  if (armR.getAttribute("data-pose") !== pose) {
    refs.layers["arm-l"].replaceChildren(...ARM_L[pose].map(build));
    armR.replaceChildren(...ARM_R[pose].map(build));
    armR.setAttribute("data-pose", pose);
    setAttr(refs.layers.flashlight, "display", pose === "point" ? null : "none");
  }
  const pointing = pose === "point";
  // Transform attributes, never CSS: robot.css animates the layers inside these groups.
  setAttr(refs.arms, "transform", pointing && aim.mirrored ? MIRROR_TRANSFORM : null);
  const turned = pointing && aim.angleDeg !== 0;
  setAttr(refs.aim, "transform", turned ? `rotate(${num(aim.angleDeg)} ${SHOULDER.x} ${SHOULDER.y})` : null);
}
