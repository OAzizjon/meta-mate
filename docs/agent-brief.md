# MetaMate: brief for drafting agents

**What it is.** A Windows desktop helper (Rust + Tauri v2, a TypeScript WebView). Mate, a small light-bulb character, answers spoken or typed questions aloud and shines a beam on the button to click in any app. Lessons go one step at a time and check each click; Mate presses a button himself only after a spoken yes. Users: older people and anyone lost in an unfamiliar app. The UI is English and Russian.

**Models.** Meta's by default (invariant 8): Muse Spark 1.3 (answers, pointing, lessons, scam check), Muse Wonder 1.0 (escalation), Muse Voice Transcribe (speech), SAM 3.1 (outlines where UI Automation is empty), Muse Image and Muse Glimmer 30B (lesson cards), MMS-TTS (voice, on Modal). Provider keys live only on the proxy (`server/`), never in the app.

**Context.** Built for the Meta Global AI Developer Hackathon. Submission: 2026-11-04 21:00 ET. Judged on Impact 30 %, Use of Meta AI tooling 25 %, Technical depth 20 %, Demo 25 %. This week, prefer changes that are small, tested and safe to ship over big rewrites.

**Invariants (breaking one is a bug).**
1. Our windows never take focus from the person's app (`WS_EX_NOACTIVATE`, `SWP_NOACTIVATE`).
2. Per-Monitor DPI Aware v2: physical virtual-screen pixels inside, CSS pixels only when drawing, one transform per captured frame.
3. Screenshots: only the target window (and its popups), only during a request that needs the screen (D65), password fields painted over before sending; fail closed when that cannot be guaranteed.
4. Mate never clicks payment, purchase, delete, money-transfer or password actions by himself; safety checks are confirmed by a person.
5. "I can't find it" beats pointing at a guess.
6. The product name lives in one constant (`APP_NAME`); the robot is `ROBOT_NAME = "Mate"` ("he").
7. Every user-facing string lives in `locales/en.json` and `locales/ru.json`, never inline in code.
8. Meta models by default; other providers only as development fallbacks.

**Conventions.** English in code, comments, docs and commits. Win32 calls only in `crates/mm-win32`; `crates/mm-core` stays pure and testable. No new `unwrap()` in production Rust. Files stay under 800 lines. Match the surrounding code's naming, idiom and comment density. Decisions are logged in `docs/decisions.md` (new entries only).

**Tools.** `mm agent cover PATH` before you change a file (which tests to run), `mm agent gate PATCH` on your own patch, `mm agent legacy` for dead code (verify every hit). The full list with who may run what: `docs/tools.md`.
