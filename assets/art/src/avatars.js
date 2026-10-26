// Candidate looks for Mate (2026-10-24), drawn as SVG strings for the avatar-options sheet.
// Each function takes { pose: 'idle' | 'point', size } and returns an <svg> with viewBox 0 0 260 240.
// Uses INK from mate-lib.js.

function svgWrap(size, body, defs = '') {
  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 260 240" width="${size}" height="${size * 240 / 260}">
  <defs>${defs}</defs>
  <ellipse cx="112" cy="226" rx="52" ry="8" fill="${INK}" opacity=".14"/>
  ${body}
</svg>`;
}

// A. Owl: round owl robot, camera-lens eyes, ear tufts; a wing points.
function owl({ pose = 'idle', size = 300 } = {}) {
  const defs = `
    <radialGradient id="oBody" cx="40%" cy="30%" r="80%"><stop offset="0" stop-color="#F6E4C4"/><stop offset="1" stop-color="#D7B585"/></radialGradient>
    <radialGradient id="oLens" cx="40%" cy="35%" r="70%"><stop offset="0" stop-color="#A6F8EE"/><stop offset=".55" stop-color="#2E8B80"/><stop offset="1" stop-color="#174744"/></radialGradient>`;
  const wingL = `<path d="M54 138 Q28 160 42 198 Q58 182 68 158 Z" fill="#C79F6E" stroke="${INK}" stroke-width="5" stroke-linejoin="round"/>`;
  const wingR = pose === 'point'
    ? `<path d="M166 132 Q204 116 234 92 Q230 112 218 122 Q230 122 236 118 Q218 146 174 162 Z" fill="#C79F6E" stroke="${INK}" stroke-width="5" stroke-linejoin="round"/>
       <circle cx="240" cy="90" r="8" fill="#FFC94A" stroke="${INK}" stroke-width="4"/>`
    : `<path d="M176 138 Q202 160 188 198 Q172 182 162 158 Z" fill="#C79F6E" stroke="${INK}" stroke-width="5" stroke-linejoin="round"/>`;
  const body = `
    ${wingL}${wingR}
    <path d="M64 74 L56 30 L94 58 Z" fill="url(#oBody)" stroke="${INK}" stroke-width="5" stroke-linejoin="round"/>
    <path d="M160 74 L168 30 L130 58 Z" fill="url(#oBody)" stroke="${INK}" stroke-width="5" stroke-linejoin="round"/>
    <ellipse cx="112" cy="134" rx="70" ry="80" fill="url(#oBody)" stroke="${INK}" stroke-width="6"/>
    <ellipse cx="112" cy="172" rx="40" ry="36" fill="#FBF3E4"/>
    <path d="M96 164 l8 7 l8 -7 M112 164 l8 7 l8 -7 M104 182 l8 7 l8 -7" stroke="#D7B585" stroke-width="3.5" stroke-linecap="round" stroke-linejoin="round" fill="none"/>
    <circle cx="87" cy="114" r="30" fill="#FFFDF6" stroke="${INK}" stroke-width="5"/>
    <circle cx="137" cy="114" r="30" fill="#FFFDF6" stroke="${INK}" stroke-width="5"/>
    <circle cx="88" cy="116" r="17" fill="url(#oLens)" stroke="${INK}" stroke-width="3"/>
    <circle cx="136" cy="116" r="17" fill="url(#oLens)" stroke="${INK}" stroke-width="3"/>
    <circle cx="93" cy="109" r="5" fill="#fff"/><circle cx="141" cy="109" r="5" fill="#fff"/>
    <path d="M112 136 L121 146 L112 156 L103 146 Z" fill="#FFC94A" stroke="${INK}" stroke-width="4" stroke-linejoin="round"/>
    <circle cx="112" cy="70" r="6" fill="#FFC94A" stroke="${INK}" stroke-width="3"/>`;
  return svgWrap(size, body, defs);
}

// B. Bulb: a light-bulb buddy; he points by shining a beam of light on the target.
function bulb({ pose = 'idle', size = 300 } = {}) {
  const defs = `
    <radialGradient id="bGlass" cx="42%" cy="34%" r="72%"><stop offset="0" stop-color="#FFFDF2"/><stop offset=".7" stop-color="#FFF0B3"/><stop offset="1" stop-color="#FFDD7A"/></radialGradient>
    <radialGradient id="bAura"><stop offset="0" stop-color="#FFE08A" stop-opacity=".55"/><stop offset="1" stop-color="#FFE08A" stop-opacity="0"/></radialGradient>
    <linearGradient id="bBeam" x1="0" y1="0" x2="1" y2="0"><stop offset="0" stop-color="#FFE08A" stop-opacity=".95"/><stop offset="1" stop-color="#FFE08A" stop-opacity="0"/></linearGradient>`;
  const armL = `<ellipse cx="40" cy="128" rx="10" ry="17" transform="rotate(30 40 128)" fill="#FFF0B3" stroke="${INK}" stroke-width="5"/>`;
  const armR = pose === 'point'
    ? `<polygon points="222,84 262,50 262,150 222,106" fill="url(#bBeam)"/>
       <path d="M170 118 Q194 110 208 100" stroke="${INK}" stroke-width="15" stroke-linecap="round" fill="none"/>
       <path d="M170 118 Q194 110 208 100" stroke="#FFF0B3" stroke-width="6" stroke-linecap="round" fill="none"/>
       <rect x="204" y="84" width="22" height="24" rx="6" transform="rotate(-20 215 96)" fill="#9AA8B8" stroke="${INK}" stroke-width="4"/>`
    : `<ellipse cx="180" cy="128" rx="10" ry="17" transform="rotate(-30 180 128)" fill="#FFF0B3" stroke="${INK}" stroke-width="5"/>`;
  const body = `
    <circle cx="110" cy="100" r="100" fill="url(#bAura)"/>
    ${armL}${armR}
    <rect x="80" y="166" width="60" height="16" rx="6" fill="#9AA8B8" stroke="${INK}" stroke-width="5"/>
    <rect x="84" y="180" width="52" height="14" rx="6" fill="#B8C2CF" stroke="${INK}" stroke-width="5"/>
    <path d="M96 194 Q110 212 124 194 Z" fill="${INK}"/>
    <path d="M110 20 C160 20 186 56 180 96 C176 122 156 138 148 166 L72 166 C64 138 44 122 40 96 C34 56 60 20 110 20 Z" fill="url(#bGlass)" stroke="${INK}" stroke-width="6"/>
    <path d="M62 70 Q66 46 88 38" stroke="#fff" stroke-width="7" stroke-linecap="round" fill="none" opacity=".85"/>
    <ellipse cx="90" cy="96" rx="9" ry="12" fill="${INK}"/><ellipse cx="130" cy="96" rx="9" ry="12" fill="${INK}"/>
    <circle cx="93" cy="91" r="3.5" fill="#fff"/><circle cx="133" cy="91" r="3.5" fill="#fff"/>
    <path d="M98 117 Q110 127 122 117" stroke="${INK}" stroke-width="4.5" stroke-linecap="round" fill="none"/>
    <ellipse cx="72" cy="114" rx="8" ry="5" fill="#FFB3A7" opacity=".9"/><ellipse cx="148" cy="114" rx="8" ry="5" fill="#FFB3A7" opacity=".9"/>`;
  return svgWrap(size, body, defs);
}

// C. Arrow: a friendly mouse-cursor arrow with a face; his whole body points.
function arrow({ pose = 'idle', size = 300 } = {}) {
  const defs = `<linearGradient id="aBody" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#FFFFFF"/><stop offset="1" stop-color="#DCE8F5"/></linearGradient>`;
  const shape = `
    <path d="M60 30 L60 190 L98 154 L124 208 L150 196 L124 144 L176 144 Z" fill="url(#aBody)" stroke="${INK}" stroke-width="8" stroke-linejoin="round"/>
    <ellipse cx="78" cy="104" rx="7" ry="10" fill="${INK}"/><ellipse cx="100" cy="112" rx="7" ry="10" fill="${INK}"/>
    <circle cx="80" cy="100" r="2.6" fill="#fff"/><circle cx="102" cy="108" r="2.6" fill="#fff"/>
    <path d="M78 126 Q90 136 102 130" stroke="${INK}" stroke-width="4" stroke-linecap="round" fill="none"/>
    <ellipse cx="70" cy="124" rx="6" ry="4" fill="#FFB3A7"/><ellipse cx="112" cy="132" rx="6" ry="4" fill="#FFB3A7"/>
    <ellipse cx="46" cy="150" rx="9" ry="14" transform="rotate(30 46 150)" fill="url(#aBody)" stroke="${INK}" stroke-width="5"/>`;
  const body = pose === 'point'
    ? `<rect x="6" y="6" width="74" height="32" rx="10" fill="#FFF6D6" stroke="${INK}" stroke-width="4"/>
       <rect x="0" y="0" width="86" height="44" rx="14" fill="none" stroke="#FFC94A" stroke-width="5" opacity=".9"/>
       <text x="43" y="29" text-anchor="middle" font-family="Segoe UI, Arial" font-weight="700" font-size="18" fill="${INK}">OK</text>
       <g transform="translate(46 18)">${shape}</g>
       <path d="M232 70 h18 M226 92 h24 M236 114 h14" stroke="${INK}" stroke-width="5" stroke-linecap="round" opacity=".35"/>`
    : `<g transform="translate(14 0)">${shape}</g>`;
  return svgWrap(size, body, defs);
}

// D. Retro: a beige 90s monitor with a phosphor face; a cable arm ends in a cartoon glove.
function retro({ pose = 'idle', size = 300 } = {}) {
  const defs = `<linearGradient id="rCase" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#F3ECDD"/><stop offset="1" stop-color="#D7CBB1"/></linearGradient>`;
  const glove = (x, y, rot) => `<g transform="translate(${x} ${y}) rotate(${rot})">
      <rect x="-4" y="-22" width="9" height="20" rx="4.5" fill="#fff" stroke="${INK}" stroke-width="4"/>
      <circle cx="0" cy="2" r="11" fill="#fff" stroke="${INK}" stroke-width="4"/></g>`;
  const armR = pose === 'point'
    ? `<path d="M182 116 C206 116 210 96 224 84" stroke="${INK}" stroke-width="10" stroke-linecap="round" fill="none"/>
       <path d="M182 116 C206 116 210 96 224 84" stroke="#BFB29A" stroke-width="4" stroke-linecap="round" fill="none"/>
       ${glove(230, 80, 55)}`
    : `<path d="M182 118 C200 126 198 150 190 160" stroke="${INK}" stroke-width="10" stroke-linecap="round" fill="none"/>
       <path d="M182 118 C200 126 198 150 190 160" stroke="#BFB29A" stroke-width="4" stroke-linecap="round" fill="none"/>
       ${glove(188, 168, 170)}`;
  const body = `
    <path d="M38 118 C20 126 22 150 30 160" stroke="${INK}" stroke-width="10" stroke-linecap="round" fill="none"/>
    <path d="M38 118 C20 126 22 150 30 160" stroke="#BFB29A" stroke-width="4" stroke-linecap="round" fill="none"/>
    ${glove(32, 168, 190)}
    ${armR}
    <path d="M90 186 L130 186 L142 206 L78 206 Z" fill="#D7CBB1" stroke="${INK}" stroke-width="5" stroke-linejoin="round"/>
    <rect x="36" y="38" width="148" height="150" rx="26" fill="url(#rCase)" stroke="${INK}" stroke-width="6"/>
    <rect x="52" y="54" width="116" height="94" rx="22" fill="#1E3640" stroke="${INK}" stroke-width="5"/>
    <g stroke="#9CF5A6" stroke-width="2" opacity=".14"><path d="M60 70 H160 M60 82 H160 M60 94 H160 M60 106 H160 M60 118 H160 M60 130 H160"/></g>
    <rect x="78" y="84" width="15" height="24" rx="7.5" fill="#9CF5A6"/><rect x="127" y="84" width="15" height="24" rx="7.5" fill="#9CF5A6"/>
    <path d="M95 122 Q110 133 125 122" stroke="#9CF5A6" stroke-width="5" stroke-linecap="round" fill="none"/>
    <rect x="60" y="162" width="44" height="8" rx="4" fill="#BFB29A"/>
    <circle cx="156" cy="166" r="6" fill="#FFC94A" stroke="${INK}" stroke-width="3"/>
    <circle cx="140" cy="166" r="4" fill="#7FD3C8" stroke="${INK}" stroke-width="2.5"/>`;
  return svgWrap(size, body, defs);
}

const AVATARS = [
  ['A', 'Owl Mate', owl, 'A wise little owl robot: big lens eyes, ear tufts, points with a wing.'],
  ['B', 'Bulb Mate', bulb, 'A light-bulb buddy who shines a beam on the right button.'],
  ['C', 'Arrow Mate', arrow, 'A friendly mouse cursor with a face: his whole body points.'],
  ['D', 'Retro Mate', retro, 'A 90s computer monitor with a glowing face and a gloved cable hand.'],
];
