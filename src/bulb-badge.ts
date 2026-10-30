// Mate's light bulb as a small decorative badge for window headers (chat.html, settings.html).
// Built with createElementNS, no innerHTML; its look is `.bulb-badge` in src/theme.css.

const SVG_NS = "http://www.w3.org/2000/svg";

export function bulbBadge(): HTMLSpanElement {
  const badge = document.createElement("span");
  badge.className = "bulb-badge";
  badge.setAttribute("aria-hidden", "true");
  const svg = document.createElementNS(SVG_NS, "svg");
  svg.setAttribute("viewBox", "0 0 24 24");
  // Glass, then the two lines of the base.
  for (const d of ["M8.5 15.5C8.5 13.5 6 12.5 6 9.5a6 6 0 0 1 12 0c0 3-2.5 4-2.5 6z", "M9 18.5h6", "M10 21.5h4"]) {
    const path = document.createElementNS(SVG_NS, "path");
    path.setAttribute("d", d);
    svg.append(path);
  }
  badge.append(svg);
  return badge;
}
