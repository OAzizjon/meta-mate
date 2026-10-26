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

Production: the SVG in `assets/art/src/mate-lib.js` (`mate()` = `bulbMate('classic')`) is the reference. For the app, split it into layers (aura, glass, face, cheeks, base, status light, arms, flashlight, beam, shadow) → animate with CSS/Web Animations (or Rive if we need a state machine with blends). Muse Image is used for extra concept art and stickers in the same style (prompts below).

## States (each one an animation)

Two independent channels: the **state** (what Mate is doing: listening, flying, pointing…) is set by the app; the **mood** (his face: calm, happy, thinking, surprised, sad, proud, sleepy) is set by MateScript `<mood>`. A state can override the face briefly (e.g. `oops`), then the mood returns.

| State | Animation | Sound |
|---|---|---|
| idle | slow hover bob, blink every 3–6 s, eyes follow the mouse a little; status light steady | — |
| listening | leans toward the user, the bulb glows brighter, eyes wide | listen |
| thinking | eyes look up/sideways, the glow pulses slowly | think (loop, soft) |
| speaking | mouth follows the voice level, glow breathes with it, small gestures | — |
| flying | tilts toward the direction, motion trail, arrives with a small bounce | whoosh |
| pointing | flashlight up, beam lands on the target, target gets a soft glow + contour | ping (panned ≤ ±30%) |
| waiting for click | the beam pulses gently on the target every 2 s | — |
| success | quick spin or jump, happy eyes, bright flash | success |
| oops | head tilt, "hmm" eyes, re-aims the beam | oops |
| can't find | shrug, palms up, beam off | — |
| sleeping | eyes closed, "z", glow dims to off, sinks into the tray icon | sleep |
| fun mode | warmer, brighter glow, bigger moves | — |

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
