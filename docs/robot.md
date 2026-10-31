# Robot character brief (v1: the light bulb)

## Character

A small, kind helper shaped like a light bulb. Calm and patient, a little playful, never childish, never sarcastic. He is proud when the user succeeds and never blames them. He speaks briefly: one instruction per sentence.

Name: **Mate** (male, "he"; "mate" = buddy). Look chosen on 2026-10-24 after two rounds: four directions (`assets/art/avatar-options.png`: owl, bulb, cursor arrow, retro monitor), then three bulb refinements (`assets/art/bulb-options.png`); the owner picked **B1 "classic glow"**. In code: `ROBOT_NAME = "Mate"`. Concept art: `assets/art/mate-sheet.png`.

Why a bulb: **his light is the highlight**. He points by shining a beam on the right button, so the product idea reads in one second; a beam works in every direction without twisting the body. (The cursor-arrow option was dropped because an older user could mistake him for their real mouse pointer.)

## Look

- Flat vector, thick soft navy outlines (`#1F2A44`), readable at **40 px** and beautiful at 200 px.
- Warm glass bulb (`#FFFDF0` → `#FFEFAE` → `#FFD866` at the edge) with a soft glow aura; a white glass highlight.
- Face on the glass: two big navy eyes with highlights, a small smile, pink cheeks (`#FFB3A7`).
- Navy two-ring screw base (`#3B6688` / `#2E5070`) with a small mint **status light** (`#8FF5E9`).
- Two small arms; the right one holds a little **flashlight** whose beam is the pointing gesture.
- He hovers (no legs) above a soft shadow.

### v1.1 refresh (2026-10-29)

A subtle polish asked for by the owner (WP79). The identity, colours, states, moods, public API and beam geometry stay the same; the app (`src/robot/`) and `mate()` in `assets/art/src/mate-lib.js` match (B2/B3 in `bulb-options` are unchanged).

- Aura: two radial layers, a warm core that hugs the glass and a wider, softer haze; the layer's opacity is still the glow strength.
- Glass: a crisper highlight (a curved stroke plus a small dot) and a thin warm-orange filament zigzag low in the glass (opacity .45) that brightens with a soft glow in listening and speaking and dims in sleeping (`mate({ glow: true })` in the art).
- Eyes: slightly larger (open 11 × 14, wide 13 × 17), both shapes with a big and a tiny catch-light.
- Brows: thin navy arcs set by the face (`data-brows`): calm rest, happy and proud up, thinking one raised, sad tilted (inner ends up), surprised high, sleepy low. States lend a pose: listening and success up, thinking and oops one raised, can't find tilted, sleeping low, looking rest. They glide in 0.25 s and jump with reduced motion. In the art `mate({ brows })` takes the same names and defaults from the eyes.
- Base: a three-stop gradient on the rings (a soft sheen on top) and a tiny shine on the status light.

Production: the SVG in `assets/art/src/mate-lib.js` (`mate()` = `bulbMate('classic')`) is the reference. For the app, split it into layers (aura, glass, face, cheeks, base, status light, arms, flashlight, beam, shadow) → animate with CSS/Web Animations (or Rive if we need a state machine with blends). Muse Image is used for extra concept art and stickers in the same style (prompts below).

## States (each one an animation)

Two independent channels: the **state** (what Mate is doing: listening, flying, pointing…) is set by the app; the **mood** (his face: calm, happy, thinking, surprised, sad, proud, sleepy) is set by MateScript `<mood>`. A state can override the face briefly (e.g. `oops`), then the mood returns.

| State | Animation | Sound |
|---|---|---|
| idle | floats in a slow figure of eight (three bobs to one sideways swing, 8.4 s) with a gentle ±1.5° sway, the glow breathes (4.2 s), the mint status light beats softly, blink every 3–6 s, eyes follow the mouse a little; waves hello with the left hand on his first idle, after waking and when he comes back from hiding | — |
| listening | leans toward the user, the bulb glows brighter, eyes wide, the status light beats fast | listen |
| thinking | eyes look up/sideways, the glow pulses slowly, he ponders side to side and three sparks circle the glass | think (loop, soft) |
| speaking | mouth follows the voice level, glow breathes with it, arms gesture, a small nod in the rhythm of speech | — |
| flying | tilts toward the direction, motion trail, arrives with a small bounce | whoosh |
| pointing | flashlight up, beam lands on the target, target gets a soft glow + contour | ping (panned ≤ ±30%) |
| waiting for click | the beam pulses gently on the target every 2 s | — |
| success | a jump, happy eyes, a bright flash and six sparks bursting outward | success |
| oops | head tilt, "hmm" eyes, re-aims the beam | oops |
| can't find | shrug, palms up, beam off | — |
| sleeping | eyes closed, "z", glow dims to off, sinks into the tray icon | sleep |
| fun mode | warmer, brighter glow, bigger moves | — |

Moods react once when they arrive (skipped with reduced motion): surprised ("be careful") jumps, sad (a scam verdict) flinches and the glow dims a little, happy wiggles, proud swells; joy and pride brighten the glow. Rules for new motion: `.claude/skills/metamate-mate-motion/SKILL.md`.

## Interactions

- **Drag** anywhere on the body to move; he remembers his place per monitor.
- **Press and hold** = talk (same as holding Right Ctrl).
- **Mouse wheel over Mate** or the corner handle = resize: S 64 px, M 96 px (default), L 160 px, XL 220 px (for low vision).
- **Right-click** = menu: Hide, Size, Calm/Fun sounds, Settings, Quit.
- **Double-click** = hide to tray; tray click = show.
- Settings: "reduce motion" (no flying; he fades to the target instead).
- Mate never covers the target and never sits on the taskbar.

## Muse Image prompts (stickers and extra concept art, same style)

1. *A cute friendly light bulb character, flat vector illustration, thick soft dark-navy outlines, warm pale-yellow glass with a soft glow, two big dark kind eyes with white highlights, small smile, pink cheeks, navy two-ring screw base with a tiny mint status light, small arms, holding a little flashlight that casts a soft yellow beam to the right, hovering above a soft shadow, plain white background, no text.*
2. *The same light bulb character waving hello, happy closed eyes, glowing brighter, flat vector, white background.*
3. *The same light bulb character shrugging with palms up, apologetic face, beam switched off, flat vector, white background.*
4. *The same light bulb character asleep, eyes closed, glow dimmed, small "z", flat vector, white background.*

Pick criteria: readable at 40 px (shrink test), the beam reads as "here", looks kind to an 80-year-old (ask the tester), easy to split into layers.
