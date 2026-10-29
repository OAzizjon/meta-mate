// Shared drawing helpers for the Mate art pages (rendered to PNG by `mm art`).
// mate(opts) returns Mate (the classic light bulb, chosen 2026-10-24, v1.1 refresh 2026-10-29) as an SVG
// string; mateOrb() is the earlier round-robot look. The pixel helpers draw crisp pixel art on a canvas.

const INK = '#1F2A44';

function mateOrb({ eyes = 'open', arm = 'point', mouth = 'smile', glow = false, size = 520 } = {}) {
  const eyeShapes = {
    open: `<ellipse cx="89" cy="118" rx="11" ry="14" fill="#8FF5E9"/><ellipse cx="131" cy="118" rx="11" ry="14" fill="#8FF5E9"/>
           <circle cx="93" cy="112" r="4" fill="#fff"/><circle cx="135" cy="112" r="4" fill="#fff"/>`,
    wide: `<ellipse cx="89" cy="117" rx="13" ry="17" fill="#8FF5E9"/><ellipse cx="131" cy="117" rx="13" ry="17" fill="#8FF5E9"/>
           <circle cx="94" cy="110" r="5" fill="#fff"/><circle cx="136" cy="110" r="5" fill="#fff"/>`,
    happy: `<path d="M77 124 Q89 104 101 124" stroke="#8FF5E9" stroke-width="6" stroke-linecap="round" fill="none"/>
            <path d="M119 124 Q131 104 143 124" stroke="#8FF5E9" stroke-width="6" stroke-linecap="round" fill="none"/>`,
    squint: `<path d="M79 109 L97 118 L79 127" stroke="#8FF5E9" stroke-width="6" stroke-linecap="round" stroke-linejoin="round" fill="none"/>
             <path d="M141 109 L123 118 L141 127" stroke="#8FF5E9" stroke-width="6" stroke-linecap="round" stroke-linejoin="round" fill="none"/>`,
  };
  const mouths = {
    smile: '<path d="M99 138 Q110 147 121 138" stroke="#8FF5E9" stroke-width="4.5" stroke-linecap="round" fill="none"/>',
    grin: '<path d="M97 136 Q110 152 123 136 Z" fill="#8FF5E9"/>',
    flat: '<path d="M100 141 H120" stroke="#8FF5E9" stroke-width="4.5" stroke-linecap="round"/>',
  };
  const limb = (d, w) => `<path d="${d}" stroke="${INK}" stroke-width="${w + 9}" stroke-linecap="round" fill="none"/>
                          <path d="${d}" stroke="#F6F0E4" stroke-width="${w}" stroke-linecap="round" fill="none"/>`;
  const arms = {
    point: limb('M172 130 Q200 120 226 100', 8) + `<circle cx="229" cy="98" r="9" fill="#FFC94A" stroke="${INK}" stroke-width="4"/>`,
    stop: limb('M170 120 Q196 104 204 70', 8) + `<circle cx="205" cy="62" r="15" fill="#F6F0E4" stroke="${INK}" stroke-width="5"/>`,
    none: '',
  };
  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 260 240" width="${size}" height="${size * 240 / 260}">
  <defs>
    <radialGradient id="mBody" cx="38%" cy="32%" r="75%"><stop offset="0" stop-color="#FFFDF8"/><stop offset="1" stop-color="#E8DDCA"/></radialGradient>
    <linearGradient id="mScreen" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#2E5070"/><stop offset="1" stop-color="#1A2F42"/></linearGradient>
    <radialGradient id="mGlow"><stop offset="0" stop-color="#FFE08A" stop-opacity=".95"/><stop offset="1" stop-color="#FFE08A" stop-opacity="0"/></radialGradient>
  </defs>
  <ellipse cx="110" cy="224" rx="54" ry="8" fill="${INK}" opacity=".14"/>
  <path d="M110 54 V32" stroke="${INK}" stroke-width="6" stroke-linecap="round"/>
  <circle cx="110" cy="24" r="${glow ? 30 : 17}" fill="url(#mGlow)"/>
  <circle cx="110" cy="24" r="9" fill="#FFC94A" stroke="${INK}" stroke-width="4"/>
  <ellipse cx="40" cy="146" rx="12" ry="20" transform="rotate(28 40 146)" fill="url(#mBody)" stroke="${INK}" stroke-width="5"/>
  ${arms[arm]}
  <ellipse cx="110" cy="128" rx="74" ry="72" fill="url(#mBody)" stroke="${INK}" stroke-width="6"/>
  <rect x="56" y="86" width="108" height="72" rx="30" fill="url(#mScreen)" stroke="${INK}" stroke-width="5"/>
  ${eyeShapes[eyes]}
  ${mouths[mouth]}
  <ellipse cx="70" cy="170" rx="9" ry="6" fill="#FFB3A7" opacity=".85"/>
  <ellipse cx="150" cy="170" rx="9" ry="6" fill="#FFB3A7" opacity=".85"/>
  <circle cx="110" cy="184" r="7" fill="#7FD3C8" stroke="${INK}" stroke-width="3"/>
</svg>`;
}

const GLASS = 'M110 20 C160 20 186 56 180 96 C176 122 156 138 148 166 L72 166 C64 138 44 122 40 96 C34 56 60 20 110 20 Z';
const FILAMENT = 'M100 161 L98 148 L102 142 L106 148 L110 142 L114 148 L118 142 L122 148 L120 161';

// v1.1 brows, as src/robot/robot.css poses them; without `brows` they follow the eyes.
const BROWS_FOR_EYES = { open: 'rest', wide: 'up', happy: 'up', squint: 'raised' };

function bulbMate(variant = 'classic', { arm = 'none', eyes = 'open', mouth = 'smile', glow = false, size = 300, brows } = {}) {
  // Gradient ids are document-wide: give every drawing its own prefix (Codex review #261).
  const gid = `g${variant}${++bulbMate.seq}`;
  const screen = variant === 'screen';
  const classic = variant === 'classic';       // only B1 has the v1.1 refresh; B2 and B3 stay as compared
  const face = screen ? '#8FF5E9' : INK;       // B2 draws the face on a dark screen
  const glassStops = screen
    ? '<stop offset="0" stop-color="#FFFEF6"/><stop offset=".75" stop-color="#FFF4C9"/><stop offset="1" stop-color="#FFE59A"/>'
    : '<stop offset="0" stop-color="#FFFDF0"/><stop offset=".68" stop-color="#FFEFAE"/><stop offset="1" stop-color="#FFD866"/>';
  const metalStops = classic
    ? '<stop offset="0" stop-color="#4A7899"/><stop offset=".45" stop-color="#3B6688"/><stop offset="1" stop-color="#2E5070"/>'
    : '<stop offset="0" stop-color="#AEBACB"/><stop offset="1" stop-color="#8C9AAE"/>';
  const defs = `
    <radialGradient id="${gid}Glass" cx="42%" cy="32%" r="74%">${glassStops}</radialGradient>
    <radialGradient id="${gid}Aura"><stop offset="0" stop-color="#FFE08A" stop-opacity="${glow ? 0.8 : 0.45}"/><stop offset="1" stop-color="#FFE08A" stop-opacity="0"/></radialGradient>
    <radialGradient id="${gid}Haze"><stop offset="0" stop-color="#FFE08A" stop-opacity=".55"/><stop offset=".6" stop-color="#FFE08A" stop-opacity=".26"/><stop offset="1" stop-color="#FFE08A" stop-opacity="0"/></radialGradient>
    <radialGradient id="${gid}Core"><stop offset="0" stop-color="#FFF1B8" stop-opacity=".7"/><stop offset=".8" stop-color="#FFE6A0" stop-opacity=".38"/><stop offset="1" stop-color="#FFE08A" stop-opacity="0"/></radialGradient>
    <linearGradient id="${gid}Beam" x1="0" y1="0" x2="1" y2="0"><stop offset="0" stop-color="#FFE08A" stop-opacity=".95"/><stop offset="1" stop-color="#FFE08A" stop-opacity="0"/></linearGradient>
    <linearGradient id="${gid}Metal" x1="0" y1="0" x2="0" y2="1">${metalStops}</linearGradient>`;

  // Face coordinates: on the glass (B1, B3) or on the inner screen (B2).
  const ey = screen ? 96 : 96, ex1 = 89, ex2 = 131;
  const eyeSet = {
    open: `<ellipse cx="${ex1}" cy="${ey}" rx="10" ry="13" fill="${face}"/><ellipse cx="${ex2}" cy="${ey}" rx="10" ry="13" fill="${face}"/>
           <circle cx="${ex1 + 3.5}" cy="${ey - 5}" r="4" fill="#fff"/><circle cx="${ex2 + 3.5}" cy="${ey - 5}" r="4" fill="#fff"/>
           <circle cx="${ex1 - 3}" cy="${ey + 5}" r="1.8" fill="#fff" opacity=".8"/><circle cx="${ex2 - 3}" cy="${ey + 5}" r="1.8" fill="#fff" opacity=".8"/>`,
    wide: `<ellipse cx="${ex1}" cy="${ey - 1}" rx="12" ry="16" fill="${face}"/><ellipse cx="${ex2}" cy="${ey - 1}" rx="12" ry="16" fill="${face}"/>
           <circle cx="${ex1 + 4}" cy="${ey - 7}" r="5" fill="#fff"/><circle cx="${ex2 + 4}" cy="${ey - 7}" r="5" fill="#fff"/>`,
    happy: `<path d="M${ex1 - 11} ${ey + 5} Q${ex1} ${ey - 13} ${ex1 + 11} ${ey + 5}" stroke="${face}" stroke-width="6" stroke-linecap="round" fill="none"/>
            <path d="M${ex2 - 11} ${ey + 5} Q${ex2} ${ey - 13} ${ex2 + 11} ${ey + 5}" stroke="${face}" stroke-width="6" stroke-linecap="round" fill="none"/>`,
    squint: `<path d="M${ex1 - 9} ${ey - 8} L${ex1 + 8} ${ey} L${ex1 - 9} ${ey + 8}" stroke="${face}" stroke-width="6" stroke-linecap="round" stroke-linejoin="round" fill="none"/>
             <path d="M${ex2 + 9} ${ey - 8} L${ex2 - 8} ${ey} L${ex2 + 9} ${ey + 8}" stroke="${face}" stroke-width="6" stroke-linecap="round" stroke-linejoin="round" fill="none"/>`,
  };
  if (classic) {
    // v1.1: slightly larger eyes, each with a big and a tiny catch-light.
    Object.assign(eyeSet, {
      open: `<ellipse cx="${ex1}" cy="${ey}" rx="11" ry="14" fill="${face}"/><ellipse cx="${ex2}" cy="${ey}" rx="11" ry="14" fill="${face}"/>
             <circle cx="${ex1 + 3.5}" cy="${ey - 5.5}" r="4.2" fill="#fff"/><circle cx="${ex2 + 3.5}" cy="${ey - 5.5}" r="4.2" fill="#fff"/>
             <circle cx="${ex1 - 3.5}" cy="${ey + 5.5}" r="2" fill="#fff" opacity=".85"/><circle cx="${ex2 - 3.5}" cy="${ey + 5.5}" r="2" fill="#fff" opacity=".85"/>`,
      wide: `<ellipse cx="${ex1}" cy="${ey - 1}" rx="13" ry="17" fill="${face}"/><ellipse cx="${ex2}" cy="${ey - 1}" rx="13" ry="17" fill="${face}"/>
             <circle cx="${ex1 + 4.5}" cy="${ey - 8}" r="5.2" fill="#fff"/><circle cx="${ex2 + 4.5}" cy="${ey - 8}" r="5.2" fill="#fff"/>
             <circle cx="${ex1 - 4}" cy="${ey + 6}" r="2.2" fill="#fff" opacity=".85"/><circle cx="${ex2 - 4}" cy="${ey + 6}" r="2.2" fill="#fff" opacity=".85"/>`,
    });
  }
  // v1.1 brows: thin arcs whose ends sit at browY; the transforms are robot.css's, turned around
  // each arc's box centre (browY - 1.75) like its transform-box: fill-box.
  const browY = ey - 24, browCy = browY - 1.75;
  const browPoses = {
    rest: ['', ''],
    up: ['translate(0 -4)', 'translate(0 -4)'],
    high: ['translate(0 -7)', 'translate(0 -7)'],
    low: ['translate(0 2)', 'translate(0 2)'],
    raised: ['', `translate(0 -5) rotate(-6 ${ex2} ${browCy})`],
    tilted: [`translate(0 -1) rotate(-12 ${ex1} ${browCy})`, `translate(0 -1) rotate(12 ${ex2} ${browCy})`],
  };
  const brow = (cx, t) => `<path d="M${cx - 10} ${browY} Q${cx} ${browY - 7} ${cx + 10} ${browY}"${t ? ` transform="${t}"` : ''} stroke="${face}" stroke-width="4" stroke-linecap="round" fill="none"/>`;
  const [browL, browR] = browPoses[brows || BROWS_FOR_EYES[eyes]] || browPoses.rest;
  const browSet = classic ? brow(ex1, browL) + brow(ex2, browR) : '';
  const my = ey + 21;
  const mouthSet = {
    smile: `<path d="M98 ${my} Q110 ${my + 10} 122 ${my}" stroke="${face}" stroke-width="4.5" stroke-linecap="round" fill="none"/>`,
    grin: `<path d="M96 ${my - 2} Q110 ${my + 15} 124 ${my - 2} Z" fill="${face}"/>`,
    flat: `<path d="M101 ${my + 3} H119" stroke="${face}" stroke-width="4.5" stroke-linecap="round"/>`,
  };
  const cheeks = `<ellipse cx="70" cy="${my - 2}" rx="8" ry="5" fill="#FFB3A7" opacity=".9"/><ellipse cx="150" cy="${my - 2}" rx="8" ry="5" fill="#FFB3A7" opacity=".9"/>`;

  const limb = (d) => `<path d="${d}" stroke="${INK}" stroke-width="15" stroke-linecap="round" fill="none"/>
                       <path d="${d}" stroke="#FFF0B3" stroke-width="6" stroke-linecap="round" fill="none"/>`;
  const stub = (cx, cy, rot) => `<ellipse cx="${cx}" cy="${cy}" rx="10" ry="16" transform="rotate(${rot} ${cx} ${cy})" fill="#FFF0B3" stroke="${INK}" stroke-width="5"/>`;
  const beam = (x, y) => `<polygon points="${x},${y - 12} 262,${y - 46} 262,${y + 54} ${x},${y + 10}" fill="url(#${gid}Beam)"/>`;

  let armL = stub(40, 130, 30), armR = '';
  const halo = variant === 'halo';
  if (!halo) {
    if (arm === 'point') {
      armR = variant === 'classic'
        ? beam(222, 96) + limb('M170 120 Q194 110 208 100') +
          `<rect x="204" y="84" width="22" height="24" rx="6" transform="rotate(-20 215 96)" fill="#3B6688" stroke="${INK}" stroke-width="4"/>
           <circle cx="224" cy="92" r="4" fill="#FFF6D6"/>`
        : beam(230, 92) + limb('M170 120 Q198 110 222 94') +
          `<circle cx="228" cy="92" r="9" fill="#FFC94A" stroke="${INK}" stroke-width="4"/>`;
    } else if (arm === 'stop') {
      armR = limb('M168 118 Q192 102 198 76') + `<circle cx="199" cy="66" r="14" fill="#FFF0B3" stroke="${INK}" stroke-width="5"/>`;
    } else {
      armR = stub(180, 130, -30);
    }
  } else {
    armL = '';
  }

  // B3: a ring around the glass with a lens that casts the beam (the ring turns toward the target).
  const ringBack = `<path d="M15.5 120.7 A96 24 -10 0 1 204.5 87.3" stroke="${INK}" stroke-width="11" fill="none" opacity=".9"/>
                    <path d="M15.5 120.7 A96 24 -10 0 1 204.5 87.3" stroke="#7FD3C8" stroke-width="5" fill="none"/>`;
  const ringFront = `<path d="M15.5 120.7 A96 24 -10 0 0 204.5 87.3" stroke="${INK}" stroke-width="11" fill="none"/>
                     <path d="M15.5 120.7 A96 24 -10 0 0 204.5 87.3" stroke="#7FD3C8" stroke-width="5" fill="none"/>
                     <circle cx="200" cy="96" r="10" fill="#FFC94A" stroke="${INK}" stroke-width="4"/>`;
  const haloBeam = arm === 'point' ? `<polygon points="206,86 262,52 262,142 206,106" fill="url(#${gid}Beam)"/>` : '';
  const jets = halo ? `<ellipse cx="110" cy="214" rx="24" ry="7" fill="#8FF5E9" opacity=".55"/>
                       <path d="M98 198 Q102 210 98 216 M122 198 Q118 210 122 216" stroke="#7FD3C8" stroke-width="4" stroke-linecap="round" fill="none" opacity=".8"/>` : '';

  const base = `
    <rect x="80" y="166" width="60" height="15" rx="6" fill="url(#${gid}Metal)" stroke="${INK}" stroke-width="5"/>
    <rect x="84" y="179" width="52" height="14" rx="6" fill="url(#${gid}Metal)" stroke="${INK}" stroke-width="5"/>
    <path d="M96 192 Q110 208 124 192 Z" fill="${INK}"/>
    ${classic ? '<circle cx="128" cy="173" r="3" fill="#8FF5E9"/><circle cx="127" cy="172" r="1.1" fill="#fff" opacity=".9"/>' : ''}`;

  // v1.1 aura: a soft haze and a warm core that hugs the glass. The group opacity is the glow strength
  // and `glow` grows it by 1.12, as robot.css's listening state does.
  const auraScale = glow ? 1.12 : 1;
  const aura = classic
    ? `<g opacity="${glow ? 0.8 : 0.45}">
         <circle cx="110" cy="96" r="${(104 * auraScale).toFixed(1)}" fill="url(#${gid}Haze)"/>
         <circle cx="110" cy="96" r="${(86 * auraScale).toFixed(1)}" fill="url(#${gid}Core)"/>
       </g>`
    : `<circle cx="110" cy="96" r="${glow ? 112 : 100}" fill="url(#${gid}Aura)"/>`;

  // v1.1 glass: a dim filament that warms up with `glow` (listening, speaking) and a crisper highlight.
  const wire = 'fill="none" stroke-linecap="round" stroke-linejoin="round"';
  const filament = `<g opacity="${glow ? 0.9 : 0.45}">
      ${glow ? `<path d="${FILAMENT}" stroke="#FFC94A" stroke-width="8" opacity=".5" ${wire}/>` : ''}
      <path d="${FILAMENT}" stroke="#F28C28" stroke-width="3" ${wire}/>
    </g>`;
  const glass = `<path d="${GLASS}" fill="url(#${gid}Glass)" stroke="${INK}" stroke-width="6"/>
                 ${classic
                   ? `${filament}
                      <path d="M62 72 Q64 47 88 38" stroke="#fff" stroke-width="6" stroke-linecap="round" fill="none"/>
                      <circle cx="61" cy="85" r="3.2" fill="#fff"/>`
                   : '<path d="M62 70 Q66 46 88 38" stroke="#fff" stroke-width="7" stroke-linecap="round" fill="none" opacity=".9"/>'}`;
  const inner = screen
    ? `<path d="M100 40 q4 -8 8 0 t8 0 t8 0" stroke="#FFC94A" stroke-width="4" stroke-linecap="round" fill="none"/>
       <rect x="58" y="64" width="104" height="70" rx="28" fill="#22394F" stroke="${INK}" stroke-width="5"/>`
    : '';

  const shadowY = halo ? 228 : 226;
  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 260 240" width="${size}" height="${size * 240 / 260}">
  <defs>${defs}</defs>
  <ellipse cx="112" cy="${shadowY}" rx="50" ry="7" fill="${INK}" opacity=".13"/>
  ${aura}
  ${jets}${halo ? ringBack + haloBeam : ''}
  ${armL}${armR}
  ${base}
  ${glass}${inner}
  ${browSet}${eyeSet[eyes]}${mouthSet[mouth]}${screen ? '' : cheeks}
  ${halo ? ringFront : ''}
</svg>`;
}

bulbMate.seq = 0;

function mate(opts = {}) { return bulbMate('classic', opts); }

// --- pixel art ---------------------------------------------------------------

const PAL = {
  K: INK, Y: '#FFEFAE', y: '#FFFDF0', O: '#FFD866', N: '#2E5070', n: '#3B6688',
  H: '#FFFFFF', P: '#FFB3A7', G: '#8FF5E9', L: '#FFE9A8', E: '#FFC94A',
};

// Mate the bulb, 24 x 24. '.' = transparent.
const MATE_PX = [
  '........................',
  '........KKKKKKKK........',
  '......KKyyYYYYYYKK......',
  '.....KyyYYYYYYYYYYK.....',
  '....KyYYYYYYYYYYYYYK....',
  '...KyYYYYYYYYYYYYYYYK...',
  '...KYYYYYYYYYYYYYYYYK...',
  '..KYYYYYYYYYYYYYYYYYYK..',
  '..KYYYYKKYYYYYYKKYYYYK..',
  '..KYYYYKHYYYYYYKHYYYYK..',
  '..KYYYYKKYYYYYYKKYYYYK..',
  '..KYYPYYYYYYYYYYYYPYYK..',
  '..KYYYYYYKYYYYKYYYYYYK..',
  '...KYYYYYYKKKKYYYYYYK...',
  '...KOYYYYYYYYYYYYYYOK...',
  '....KOYYYYYYYYYYYYOK....',
  '.....KOOYYYYYYYYOOK.....',
  '......KOOYYYYYYOOK......',
  '.......KKKKKKKKKK.......',
  '.......KNNNNNNGNK.......',
  '........KnnnnnnK........',
  '........KNNNNNNK........',
  '..........KKKK..........',
  '........................',
];

function gridOf(rows) { return rows.map(r => r.split('')); }
function setCells(grid, cells) { for (const [x, y, c] of cells) { if (grid[y] && x >= 0 && x < grid[y].length) grid[y][x] = c; } }

function mateGrid(pose = 'idle') {
  const g = gridOf(MATE_PX);
  if (pose === 'listen') {
    // Taller eyes and a glow around the top of the glass.
    setCells(g, [[7, 7, 'K'], [8, 7, 'K'], [15, 7, 'K'], [16, 7, 'K'],
      [8, 0, 'L'], [9, 0, 'L'], [10, 0, 'L'], [13, 0, 'L'], [14, 0, 'L'], [15, 0, 'L'],
      [5, 1, 'L'], [6, 1, 'L'], [17, 1, 'L'], [18, 1, 'L'], [3, 3, 'L'], [20, 3, 'L']]);
  }
  if (pose === 'happy') {
    for (const x0 of [6, 14]) {
      setCells(g, [[x0 + 1, 8, 'K'], [x0 + 2, 8, 'K'], [x0, 9, 'K'], [x0 + 3, 9, 'K'],
        [x0 + 1, 9, 'Y'], [x0 + 2, 9, 'Y'], [x0 + 1, 10, 'Y'], [x0 + 2, 10, 'Y']]);
    }
    setCells(g, [[10, 12, 'K'], [11, 12, 'K'], [12, 12, 'K'], [13, 12, 'K'], [11, 13, 'P'], [12, 13, 'P']]);
  }
  return g;
}

// The little flashlight arm on the right of the 24-px sprite (cells relative to the sprite).
function armCells(len = 3, y = 12) {
  const cells = [];
  for (let x = 22; x < 22 + len; x++) cells.push([x, y - 1, 'K'], [x, y, 'Y'], [x, y + 1, 'K']);
  const t = 22 + len;
  for (let yy = y - 2; yy <= y + 2; yy++) for (let x = t; x <= t + 2; x++) cells.push([x, yy, 'K']);
  for (let yy = y - 1; yy <= y + 1; yy++) cells.push([t + 1, yy, 'n']);
  cells.push([t + 2, y, 'y']);
  return cells;
}

// 5 x 7 pixel font.
const FONT = {
  A: ['.###.', '#...#', '#...#', '#####', '#...#', '#...#', '#...#'], B: ['####.', '#...#', '#...#', '####.', '#...#', '#...#', '####.'],
  C: ['.###.', '#...#', '#....', '#....', '#....', '#...#', '.###.'], D: ['####.', '#...#', '#...#', '#...#', '#...#', '#...#', '####.'],
  E: ['#####', '#....', '#....', '####.', '#....', '#....', '#####'], F: ['#####', '#....', '#....', '####.', '#....', '#....', '#....'],
  G: ['.###.', '#...#', '#....', '#.###', '#...#', '#...#', '.####'], H: ['#...#', '#...#', '#...#', '#####', '#...#', '#...#', '#...#'],
  I: ['.###.', '..#..', '..#..', '..#..', '..#..', '..#..', '.###.'], J: ['..###', '...#.', '...#.', '...#.', '#..#.', '#..#.', '.##..'],
  K: ['#...#', '#..#.', '#.#..', '##...', '#.#..', '#..#.', '#...#'], L: ['#....', '#....', '#....', '#....', '#....', '#....', '#####'],
  M: ['#...#', '##.##', '#.#.#', '#.#.#', '#...#', '#...#', '#...#'], N: ['#...#', '#...#', '##..#', '#.#.#', '#..##', '#...#', '#...#'],
  O: ['.###.', '#...#', '#...#', '#...#', '#...#', '#...#', '.###.'], P: ['####.', '#...#', '#...#', '####.', '#....', '#....', '#....'],
  Q: ['.###.', '#...#', '#...#', '#...#', '#.#.#', '#..#.', '.##.#'], R: ['####.', '#...#', '#...#', '####.', '#.#..', '#..#.', '#...#'],
  S: ['.####', '#....', '#....', '.###.', '....#', '....#', '####.'], T: ['#####', '..#..', '..#..', '..#..', '..#..', '..#..', '..#..'],
  U: ['#...#', '#...#', '#...#', '#...#', '#...#', '#...#', '.###.'], V: ['#...#', '#...#', '#...#', '#...#', '#...#', '.#.#.', '..#..'],
  W: ['#...#', '#...#', '#...#', '#.#.#', '#.#.#', '#.#.#', '.#.#.'], X: ['#...#', '#...#', '.#.#.', '..#..', '.#.#.', '#...#', '#...#'],
  Y: ['#...#', '#...#', '.#.#.', '..#..', '..#..', '..#..', '..#..'], Z: ['#####', '....#', '...#.', '..#..', '.#...', '#....', '#####'],
  '!': ['..#..', '..#..', '..#..', '..#..', '..#..', '.....', '..#..'], '?': ['.###.', '#...#', '....#', '...#.', '..#..', '.....', '..#..'],
  ',': ['.....', '.....', '.....', '.....', '.....', '..#..', '.#...'], '.': ['.....', '.....', '.....', '.....', '.....', '.....', '..#..'],
  "'": ['..#..', '..#..', '.#...', '.....', '.....', '.....', '.....'], '-': ['.....', '.....', '.....', '#####', '.....', '.....', '.....'],
  ' ': ['.....', '.....', '.....', '.....', '.....', '.....', '.....'],
};

function pixelCanvas(id, w, h, scale) {
  const c = document.getElementById(id);
  c.width = w * scale; c.height = h * scale;
  const ctx = c.getContext('2d');
  const px = {
    w, h, scale, ctx,
    rect(x, y, rw, rh, color) { ctx.fillStyle = color; ctx.fillRect(x * scale, y * scale, rw * scale, rh * scale); },
    dot(x, y, color) { this.rect(x, y, 1, 1, color); },
    grid(g, x0, y0, mirror = false, palette = PAL) {
      g.forEach((row, y) => row.forEach((ch, x) => {
        if (ch !== '.' && palette[ch]) this.dot(x0 + (mirror ? row.length - 1 - x : x), y0 + y, palette[ch]);
      }));
    },
    text(str, x0, y0, color, size = 1) {
      let x = x0;
      for (const ch of str.toUpperCase()) {
        const glyph = FONT[ch] || FONT['?'];
        glyph.forEach((row, y) => [...row].forEach((b, i) => { if (b === '#') this.rect(x + i * size, y0 + y * size, size, size, color); }));
        x += 6 * size;
      }
      return x;
    },
    textWidth(str, size = 1) { return str.length * 6 * size - size; },
    tri(ax, ay, bx, by, cx, cy, color) {
      // Fill every art pixel whose centre lies inside the triangle.
      const s = (x1, y1, x2, y2, x3, y3) => (x1 - x3) * (y2 - y3) - (x2 - x3) * (y1 - y3);
      for (let y = Math.floor(Math.min(ay, by, cy)); y <= Math.ceil(Math.max(ay, by, cy)); y++) {
        for (let x = Math.floor(Math.min(ax, bx, cx)); x <= Math.ceil(Math.max(ax, bx, cx)); x++) {
          const px0 = x + 0.5, py0 = y + 0.5;
          const d1 = s(px0, py0, ax, ay, bx, by), d2 = s(px0, py0, bx, by, cx, cy), d3 = s(px0, py0, cx, cy, ax, ay);
          if (!((d1 < 0 || d2 < 0 || d3 < 0) && (d1 > 0 || d2 > 0 || d3 > 0))) this.dot(x, y, color);
        }
      }
    },
    line(x0, y0, x1, y1, color, thick = 1) {
      const dx = Math.abs(x1 - x0), dy = -Math.abs(y1 - y0), sx = x0 < x1 ? 1 : -1, sy = y0 < y1 ? 1 : -1;
      let err = dx + dy;
      for (;;) {
        this.rect(x0, y0, thick, thick, color);
        if (x0 === x1 && y0 === y1) break;
        const e2 = 2 * err;
        if (e2 >= dy) { err += dy; x0 += sx; }
        if (e2 <= dx) { err += dx; y0 += sy; }
      }
    },
  };
  return px;
}
