// Placeholder for spike S1: a plain bulb so the window is visible. Mate's real SVG
// (assets/art/src/mate-lib.js, docs/robot.md) replaces it in src/robot.
const root = document.getElementById("mate");
if (root) {
  root.innerHTML = `
    <svg viewBox="0 0 100 100" width="80%" height="80%" role="img">
      <circle cx="50" cy="42" r="30" fill="#ffe680" stroke="#1e2a4a" stroke-width="3" />
      <rect x="38" y="70" width="24" height="16" rx="3" fill="#1e2a4a" />
      <circle cx="50" cy="78" r="3" fill="#5cf2c2" />
    </svg>`;
}
