// Placeholder for spike S1: one outline in CSS pixels. The beam contour from the teacher
// (docs/robot.md) replaces it.
const svg = document.getElementById("highlight");
if (svg) {
  svg.innerHTML = `<rect x="8" y="8" width="160" height="48" rx="10" fill="none"
    stroke="#ffd23f" stroke-width="4" />`;
}
