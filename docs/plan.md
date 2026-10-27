# Plan: October 24–November 4 (v2, after Gemini review #234)

Submission deadline: **November 4, 21:00 ET = November 5, 02:00 UTC** (07:00 Tashkent, 05:00 Moscow — check your own time zone).
Our internal deadline: **November 4, 18:00 UTC**, leaving 8 hours for upload and unexpected issues.
Pace: 8+ hours a day. Every evening, spend 10 minutes on “what’s broken / what do we cut?” and update this file.

What changed after the review: Day 1 is lighter (3 spikes instead of 6, with a checkpoint: 16:00, moved to 21:00 by D33); the server and installer are ready **before** the tester pilot; the demo build is frozen on the evening of Day 7, and a backup video is recorded that same day; SmartScreen, pre-recorded audio phrases, and a README that grows every day have been added.

## Before kickoff (October 24–25): no product code

| Who | What | Done by |
|---|---|---|
| Me | Documents: strategy, decisions, architecture, MateScript, risks, sounds, robot, prompts, copy, video, Reddit, server | ✅ 24.10 |
| Me | Secret protection in the project (hook, deny, `env_keys_names.txt`, `.gitignore`), plugins (Rust/TS LSP, Context7) | ✅ 24.10 |
| Me | Toolchain (MSVC Build Tools, Rust, ffmpeg, uv, typos, sccache, hyperfine, cargo-nextest) — see `setup-windows.md` | ✅ 24.10 |
| Me | Foundation: git + secrets hook, `mm` CLI with enforced agent mode, `mm check`/`mm test`, `mm uia` fixtures, `mm art`; Mate concept art, pixel art, memes; README; `project-structure.md` | ✅ 24.10 |
| You | Add the `./mm` allow rules to `.claude/settings.json` (Claude may not edit its own permissions) | 25.10 |
| You | Record UIA fixtures on the Demo user (`project-structure.md`, last section) | 25–26.10 |
| You | Reddit post(s) (`research-reddit.md`); check subreddit rules first | 25.10 |
| You | Find **two** older adult testers (primary + backup); agree on a pilot on the evening of 31.10 and a recording on 02.11; send the consent form (`video.md`) | 25–26.10 |
| You | Demo machine: Windows 11, 125–150% scaling, second monitor (if available), separate “Demo” user, Edge, Zoom | 25.10 |
| Me + agents | First 12 sounds (CC0), licenses documented | ✅ 24.10, 11 of 12 (`rimshot` still missing) |
| You | Email: hackathon acceptance | ✅ 25.10 (confirmed by email) |
| You | Meta API credits | after the welcome stream on 26.10 |
| Me | Dev-fallback keys from JustLog3 (only the names in `env_keys_names.txt`: OpenRouter, Groq, ChatGPT, Tavily); DeepSeek joins the drafting team (dev only, D35) | ✅ 25.10 |
| Me | Cache warm-up outside the repo (`C:\mmw`): the crates we expect, a Tauri v2 template build, Python envs for the proxy and MMS-TTS with the eng/rus weights; Modal CLI, OBS, cargo-insta, jq (`setup-windows.md`) | ✅ 25.10 |
| Me | Forecast, asymmetric moves and acceleration plan: [`edge.md`](edge.md); `CHANGELOG.md` | ✅ 25.10 |
| You | `modal setup` (browser sign-in; the token goes to `~/.modal.toml`) | morning of 26.10 |

## Day 1 — 26.10 (Mon): rules, scaffold, three key spikes

The kickoff is at **12:00 PM GMT-5 = 17:00 UTC = 22:00 Tashkent** (the organisers' email and calendar invite; be ready at 21:00 in case they meant daylight time), and the Meta API credits arrive only after the welcome stream (D38, which replaces the 14:00/16:00 guess of D33). Day 1 is therefore an evening and a night: the spikes move to the morning of 27.10. Details and timings: [`edge.md`](edge.md) §1; step by step with the agents' prompts: [`kickoff.md`](kickoff.md).

- **Until 21:00, before the kickoff (still no product code):** record the UIA fixtures on the Demo user (`mm uia --save NAME`, it now captures dropdowns and menus too); `modal setup`; confirm the testers; `mm doctor`. *(Fixtures, `modal setup` and testers still open on 26.10 23:50.)*
- Kickoff ✅. **Official rules**: not on the public page; the owner pastes them from the participants' page (D42) → `decisions.md`.
- ✅ The owner's cuts (D39), the model choice (D40), S4 (D41): the key works, `minimal` reasoning for speech.
- ✅ Cargo workspace with `mm-core` and `mm-platform` (no `.cargo/config.toml`: the project path is short enough; agents build without sccache). First commit locally, private GitHub repository (D42). The Tauri shell itself comes with S1 in the morning.
- ✅ Night jobs started at 23:38: the MateScript parser (Codex, done: 25 golden cases + chunk fuzzing), the coordinate types (Codex, done), the locales (Copilot, done), the `mm-platform` traits (Claude worker); S1/S2/S3 checklists (Gemini #430–#432).
- **Morning of 27.10:** spikes **S1** (overlay/DPI/focus) and **S3** (UIA from Rust), no credits needed; the night's patches reviewed first.
- ✅ Spike **S4** (`mm probe`, D41): Spark reasons before it answers (first token 7–10 s by default, 1.9 s at `minimal`); computer use returns a commentary line + one click with a safety check; still to measure: the cost per request (Billing page) and the Settings / Edge fixtures once they are recorded.
- **Checkpoint 27.10 at 15:00** (was 26.10 16:00, then 21:00; moved with the kickoff): if S1 or S3 doesn't work, use native-window highlighting (Direct2D) or a Windows layer in C# WPF.
- Check whether code signing (Azure Trusted Signing) is available to you.

## Day 2 — 27.10 (Tue): voice loop

- Spikes **S2** (PTT), **S5** (Muse Voice Transcribe through the proxy), **S6** (MMS-TTS on Modal), **S7** (audio in WebView2 without a user gesture).
- PTT → transcription → Muse Spark (stream) → MMS-TTS → audio; “hold to talk” and “press — talk — pause” modes; automatic language: `<lang>` from the model selects the TTS voice, answers in the user's language (D5).
- Robot: idle / listening / thinking / speaking (temporary SVG).
- Evening goal: ask by voice and hear a reply within 2–3 seconds.

## Day 3 — 28.10 (Wed): MateScript and sound

- MateScript parser (Rust): finish the Day-1 draft and wire it to the player; golden cases from `matescript-cases.md`, fuzzing by chunks, segmentation, timeline order (the local draft from 24.10 got stuck in a loop and isn't usable).
- Mixer: speech / effects / background, ducking, limiter, calm and fun modes; debug window with MateScript highlighting.
- About 20 pre-recorded phrases per language, synthesized with MMS-TTS and included in the build.
- Mate's states from the locked B1 SVG (`assets/art/src/mate-lib.js`, D29): idle, listening, thinking, speaking and pointing first.

## Day 4 — 29.10 (Thu): “Where's the button?”

- UIA snapshot + capture of the window area, including its pop-up menus; mask passwords (fail closed) → Muse Spark → `<point>`.
- The redaction step (lead): the only producer of `RedactedImage` (`mm_core::api::request`), painting every `must_hide` element and refusing on a truncated snapshot; `point_ids` from `ElementList::ids` (safety review #460).
- Highlighting overlay across multiple monitors, DPI, robot flight, “don't cover the target” rule.
- Evening goal: accurately point out buttons in Windows Settings and Edge.

## Day 5 — 30.10 (Fri): teacher (computer use)

- Computer use loop: batches → substeps with completion rules, verify the click, take a new screenshot, replan.
- Complete scenario 1: **increase text size in Windows** (with an incorrect click on step 2).
- The model choice in Settings (Spark by default, Wonder optional, proxy caps: D40). Automatic escalation to Wonder after 2 failed steps only if Spark fails scenario 1 today.

## Day 6 — 31.10 (Sat): server, installer, pilot

- Snapshot the VPS, back up JustLog3 and shut it down (D39; checklist `server.md`, only after you say “yes” that day).
- Proxy (keys, invite codes, limits) + landing page; keep Modal warm on demo days.
- NSIS installer (per-user), icon, first launch with an invite code; SmartScreen instructions (or signing).
- **In the evening: pilot with a tester** using the installed build (scenario 1): note what's unclear and what's annoying.

## Day 7 — 01.11 (Sun): fallbacks, scenario 2, freeze

- SAM 3.1 + `ElementFromPoint` for apps without UIA.
- “Do it for me” with confirmation + blocklist; `pending_safety_checks` → human. Move `HumanConfirmation` behind a platform `ask_confirmation(&action, target)` and re-check the confirmed element right before injecting (safety review #460).
- Scenario 2: **video call / browser**.
- Fix issues from the pilot.
- **In the evening: freeze demo build v0.9** (tag it and save the installer) and **record a backup video** of all scenarios on the demo machine — the “can't-miss minimum” in case the Zoom recording fails. After that, make changes only on a branch; put only verified changes in the demo build.

## Day 8 — 02.11 (Mon): record with the tester and polish

- **Zoom session with the tester, recorded** (plan in `video.md`); keep the backup tester available.
- Fix whatever tripped up the tester; polish animations, settings, and copy.

## Day 9 — 03.11 (Tue): video, README, judge path

- Record demo scenes from the storyboard (multiple takes): the 10-second wrong-click hook, the ask-in-Russian / English-UI scene, the end card for the adult child (`edge.md` §4).
- Finish the README above the fold (an 8-second loop from the real build, three links, the limits table), the architecture diagram, and the GIF.
- The time Linux would have taken (cut, D39): the 12-point test matrix and the judge's first 60 seconds rehearsed on a clean Windows profile.

## Day 10 — 04.11 (Wed): submission

- Edit, add subtitles, final build, GitHub release, links on the website.
- Test matrix (`pointing-risks.md`) on a clean machine.
- **Submit by 18:00 UTC.**

## Ongoing

- README grows every day (Copilot maintains a draft from the day's commits; I review it).
- Every evening: Codex and Gemini review the day's changes; `mm agents stats`.

## What to cut first if we're behind

1. Fun sounds beyond the first set (keep the calm cues). 2. Automatic Wonder escalation (the manual choice stays). 3. Scenario 2. 4. “Do it for me”. (Linux is already cut, D39.)
Never cut: the main scenario, step-by-step mode with click verification, screenshot privacy, a working installer, the backup video.

## Agent team (hackathon roles)

| Agent | Role |
|---|---|
| Me (Claude) | Architecture, core (Rust: UIA/overlay/teacher), decisions, final code, checks, deployment |
| Codex | Module and test drafts (MateScript parser, proxy), review every change, bug hunting (24.10: 15 accurate architecture comments) |
| Gemini (agy) | Meta/Tauri documentation, edge cases, plan and risk review |
| Copilot | Frontend details, copy, translations, README, GitHub (releases, issues), proofreading |
| Claude worker | One worker account (`claude@alpha`): hard multi-file edits and deep reviews only (its limits are the scarcest) |
| Mistral | Third reviewer, second edit draft (free plan: one job at a time) |
| local | Summaries, test data, private tasks (weak model: verify results) |
| oss | Free `gpt-oss:120b-cloud` only (Ollama Cloud paid balance ran out, 402): second opinions; on 24.10 it got facts wrong twice — verify everything |
| DeepSeek | Owner's paid API key (`deepseek-v4-pro`): now and then the same creative ask as oss, to take the better answer (first run 25.10 beat oss) |
| (new) | You'll add agents. Candidate — **Muse Code** from Meta: judges will appreciate a Meta agent on the team |
