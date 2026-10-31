---
name: metamate-mate-motion
description: Use when adding or changing how Mate (MetaMate's light-bulb character) moves, glows or reacts: idle float, state loops, mood reactions, one-shot gestures (wave, jump, flinch), sparks, the beam's pulse, flight tilt, and the app's light/dark looks. Covers src/robot (states.ts, svg.ts, geometry.ts, robot.css), reduced motion, the test harness, and what never to animate.
---

# Mate in motion

Mate is calm, kind, a little playful, never childish (docs/robot.md). Motion must read at 40 px, never distract an older person from the button he lights, and stop entirely with reduced motion.

## Where things live

| What | File |
|---|---|
| Layers and their paint order (shadow, aura, arms, base, status light, glass, face, cheeks, sparks, z) | `src/robot/svg.ts` (`LAYERS`, `buildMateSvg`) |
| Shapes in viewBox units (260 × 240; glass centre 110, 96; base status light 128, 173) | `src/robot/geometry.ts` |
| Loops keyed on `data-state`, `data-mood`, `data-brows`, `data-cheeks` | `src/robot/robot.css` |
| One-shots (Web Animations): state moves, mood reactions, wave, spark burst | `src/robot/states.ts` |
| Flying across the screen | Rust `crates/mm-core/src/flight.rs` (eased arc, 16 ms frames) + `src/robot/flight.ts` (in-page tilt) |

## Rules

1. **Only transform and opacity** (and `filter` on the aura) animate. Never animate layout, the beam's geometry, or anything the pointing code measures.
2. **Compose, do not fight.** Loops use the individual properties `translate`, `rotate`, `scale`; states set `transform`; one-shot state moves animate `transform`; mood reactions animate the individual properties. Two loops on one element must use different properties (the idle float is `translate`, the sway `rotate`).
3. **Never put CSS transforms on the groups that use transform attributes**: `.mate-arms`, `.mate-aim`, `.mate-eyes` (gaze and aim are SVG attributes). Animate the layers inside them (`.mate-layer-arm-l`, `.mate-layer-arm-r`).
4. **Reduced motion** (the setting or the OS): every new loop is covered by `.reduce-motion * { animation: none }`; every new one-shot checks `isMotionReduced()` first. Fades may stay.
5. **Sleeping and hidden**: no one-shots (`playOneShots`, `gesture`), loops paused by robot.css.
6. **Lifetimes**: state one-shots go in `oneShots` (cancelled by the next state); mood reactions go in `gestures` (they survive the `speaking` that follows a `<mood>` tag); the wave is `waving` (a new state lowers the hand). `destroy()` and `setHidden(true)` cancel all.
7. **Amplitudes**: idle float ±2.5 px × `--mate-bob` (−4 px, −7 px in fun mode), sway ±1.5°, talk ±1.2°, ponder ±2.5°, jumps ≤ 14 px, flinch ≤ 2 px. Loops 1–9 s, one-shots 0.3–1.2 s, `ease-in-out` for loops, `ease-out` for arrivals.
8. **Meaning first**: sad (a scam verdict) never looks cute: it dims and flinches. Success is the only burst. Nothing moves while he points except the beam's gentle wait pulse.
9. **Looks**: colours come from the theme tokens (`src/theme.css`, light and `:root[data-theme="dark"]`); Mate's own colours (navy ink #1F2A44, bulb yellow, mint status light) stay the same in both themes.

## Tests

`src/robot/states.test.ts` runs the real `buildMateSvg` on a fake DOM that records `animate()` calls (`element.animations`, `.cancelled`, `.options`, `.keyframes`). For every new motion add: it plays once when it should, not again when it should not, is cancelled by what ends it, and never plays with `setReduceMotion(true)`. Run `npm test`. CSS loops have no unit tests: check them in the running app (`npm run build`, then the dev stack) at sizes S and XL, both themes, and with reduced motion on.
