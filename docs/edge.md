# Edge: where we can fall behind, and how we stay ahead

Written on 2026-10-25, the evening before the kickoff. Sources: Gemini #358 (forecast), #359 (acceleration), #360 (asymmetric moves), DeepSeek #369 and oss #366 (creative moves), the Claude worker #365 (AI workspace), each checked against our docs. Decision: D36. Update this file every evening together with `plan.md`.

The rule behind everything here (D32): our chance of the top 5 goes from ~6 % to ~20 % only if the **full** outcome works. A solo developer wins by being better at a few things judges actually see, not by doing more.

## 1. Day 1 with a 22:00 kickoff (Tashkent, UTC+5)

The kickoff is 12:00 PM GMT-5 = 17:00 UTC = 22:00 Tashkent (D38; ready at 21:00). The Meta API credits arrive only after the welcome stream, so nothing on the critical path may wait for them, and the agents work through the night.

| When | What | Needs credits? |
|---|---|---|
| Until 21:00, before the kickoff | Record the UIA fixtures on the Demo user (`mm uia --save NAME`, now with dropdowns and menus); `modal setup`; confirm both testers; `mm doctor`. No product code yet | no |
| Kickoff + stream | Read the rules: pre-event code, "Meta" in the name, video length, required models, repository visibility → `decisions.md` | no |
| Right after the rules | First commit (docs, tooling, art; README "Prepared before the kickoff", `CHANGELOG.md`); Tauri v2 scaffold, `.cargo/config.toml` (sccache, short target dir) | no |
| About 00:30, night | Team: `mm-core` MateScript parser + the 25 golden cases of `matescript-cases.md`, coordinates, `mm-platform` traits, locales (pure code, offline-testable in agents' copies); the owner sleeps | no |
| Morning of 27.10 | Me: review the night's patches; spikes S1 (overlay, DPI, focus) and S3 (UIA from Rust) | no |
| When the credits arrive | S4: streaming Muse Spark + one computer-use turn; batch format, `pending_safety_checks`, is there text next to `computer_call`? | **yes** |
| If the credits are late | S2 (the PTT keyboard hook) instead; S4 the moment they arrive. The dev fallbacks (`OPENROUTER_KEY`, `GROQ_KEY`) can drive the streaming code meanwhile | no |
| **Checkpoint 27.10 15:00** | S1 or S3 failing → native Direct2D highlight window, or the Windows layer in C# WPF | no |

## 2. Where we can fall behind

Each day has a warning sign with an hour; when it fires, the cut happens that evening, not later.

| Day | Most likely slip | Warning sign (by) | Pre-agreed cut |
|---|---|---|---|
| 1 | Transparent WebView2 window steals focus or blocks clicks on mixed-DPI monitors; Edge's cold UIA tree is empty | 21:00: an open Edge `<select>` closes when Mate moves; a UIA query > 300 ms or 0 buttons | Native Direct2D highlight; WPF layer; warm-up UIA query on focus change |
| 2 | Voice round trip over 2.5 s (realtime ASR, Modal cold starts or slow warm TTS, WebView2 audio without a gesture) | 15:00 per spike; 19:00 the full loop above 3.5 s p50 (`hyperfine`) | File ASR on key release; Modal GPU if > 400 ms per sentence; audio in Rust (`rodio`) |
| 3 | SVG states, the mixer and the parser all on one day | 14:00 a golden case fails; 18:00 pre-recorded phrases or ducking not clean | 5 states only (idle, listening, thinking, speaking, pointing; `plan.md` Day 3); calm cues only, fun mode later |
| 4 | Coordinates off at 150 % or on a negative-X monitor; popups missing from the capture | 16:00 menus missing or Mate in the screenshot; 19:00 a point > 8 px off | Element rects (`<point ID>`) over raw pixels; the monitor cropped to window + margin |
| 5 | The teacher loop misreads step completion (slider drags, UIA lag) | 16:00 scenario 1 fails 3 runs in a row | Click-only completion + a "Next" voice/button; no Muse Wonder |
| 6 | JustLog3 shutdown + installer + SmartScreen eat the day before the pilot | 13:00 JustLog3 not done; 16:00 installer + invite code not tested end to end | The VPS snapshot is the rollback; a minimal proxy first, landing polish later; installer from GitHub Releases + a SmartScreen picture |
| 7 | Pilot fixes collide with SAM, "do it for me", scenario 2 and the freeze | 15:00 both still open; 19:00 no `v0.9` tag | Cut "do it for me" and scenario 2. Never skip the backup video |
| 8 | Tester cancels or shares one window only (Mate invisible) | 2 h before the call: no tech check with full-screen sharing | Backup tester; phone audio; feature freeze |
| 9 | Retakes, README and the clean-machine run go late (Linux is cut, D39) | 16:00: no rough cut or no 8-second README loop | Use the backup takes and the Day-8 recording; finish subtitles and the clean-profile installer check |
| 10 | Editing, subtitles, clean-machine test run past 18:00 UTC | 19:00 Tashkent: no exported video or the installer fails on a clean profile | Ship the frozen `v0.9` + the backup video |

## 3. Where better-staffed teams beat us, so we don't compete there

| They win at | Why | What we do instead |
|---|---|---|
| Autonomous multi-app agents | Engineers for brittle automation loops | Refusing autonomy is the thesis: Mate intercepts `computer_call` and waits for the human's click |
| Many supported apps | Headcount for per-app tuning | Two apps that work every time (Settings, Edge) + one no-UIA app via SAM; "I can't find it" beats a guess |
| 3D characters, dashboards, three platforms | Designers and frontend people | One flat B1 SVG whose beam *is* the pointer; depth shown through the debug view |
| Cloud infrastructure (accounts, billing, history) | Full-stack teams | Zero storage as a privacy feature: a stateless proxy, invite codes, local settings |
| Studio voices and slick trailers | Budgets and editors | The whole pipeline on Meta models; an unedited session with a real older tester |

### The closest products (Hacker News and Reddit, searched 2026-10-26)

| Product | What it is | How we differ, and what to learn |
|---|---|---|
| [Ourguide](https://news.ycombinator.com/item?id=46769422) (Show HN, Jan 2026, 52 points; the site is up, but the last push to `Ourguide-ai/electron-app` was on 2026-03-03) | Electron app, OS-wide "Guide mode" that highlights the next element to click, plus a vision chat; screenshots only on request, a PII filter, inference in hardware enclaves | Vision only (no UIA), no click verification, no voice or character, not built for older adults. The thread itself asks for it: "target it towards the elderly", "Can I put it on my mom's computer yet?"; the founder found that residents of care homes don't use laptops at all. Our users are independent 55–75-year-olds who already use a PC; their **adult children** are often the ones who install it for them |
| [Frigade on-screen guides](https://news.ycombinator.com/item?id=49627872) (Show HN, Sep 2026) | In-app guides for SaaS agents: a map of one product's UI | Works inside one vendor's product; we work across any Windows app |
| [Agent-desktop](https://news.ycombinator.com/item?id=47982708) (Show HN, May 2026, 99 points) | Automation CLI for AI agents built on the OS accessibility APIs instead of screenshots | The thread agrees structured accessibility beats pixel guessing: our UIA-first grounding is the right bet. macOS only so far |
| Copilot Vision "Highlights" | Microsoft's own "show me how" | [The Register: "Copilot Vision on Windows 11 sends data to Microsoft servers"](https://news.ycombinator.com/item?id=44659137) (83 points): the privacy objection is ready-made. Our answer is visible: the redacted thumbnail of exactly what leaves the PC, open source |

The criticism Ourguide got is our checklist: "sending whole screenshots to an indie mystery box is a non-starter" → only the target window, passwords blacked out (D37), the thumbnail on request, the code public.

## 4. Asymmetric moves, ranked by impact per hour

| # | Move | Why it wins | When | Effort |
|---|---|---|---|---|
| 1 | **The first 10 seconds**: a real tester's hand, "How do I make the text bigger?", Mate shines his beam, the tester clicks the wrong place, "Almost — right here", then the real click and the success chime. No logo before 0:10 | Kills "Copilot already does this" and proves click verification in one shot. Replaces the 20-second draft in `strategy.md` if the owner agrees | backup cut Day 7; tester recording Day 8, final cut Day 9 | M |
| 2 | **A judge path under 60 seconds**: installer with a pre-filled judge invite code, a SmartScreen picture next to the download, a 3-line judge script ("Hold either Ctrl key alone for 0.3 seconds, ask to make the text bigger; if Mate asks to look, click Mate (or press Fn + F8; F8 on a desktop keyboard); click where Mate shines") | Most entries need keys or setup; ours works on the first try | Day 6 | M |
| 3 | **Cut scope traps now** (§6) | Hours go into the full outcome instead of extras | Day 1 after the rules | 0 |
| 4 | **The credit delay as a head start**: parser, frame transform and replay tests are built offline on Day 1 | Others wait for keys; we arrive at S4 with tests ready | Day 1 | — |
| 5 | **A visible Meta pipeline**: the debug view (`Ctrl+Shift+D`) shows the transcript with its language, the MateScript stream, the redacted screenshot with UIA box vs. SAM contour, and the TTS latency | Judges see six Meta models doing real work, not one wrapped call | Day 3 (view), Day 7 (SAM) | M |
| 6 | **Real older testers, unscripted, criticism included**, plus over-the-shoulder phone footage and our real Reddit findings (150–175 % scaling, tap-to-talk) | Almost nobody tests with a real senior on camera; honest criticism makes the praise credible | pilot Day 6, recording Day 8 | M |
| 7 | **Privacy shown, not claimed**: the password field goes black in the debug view before anything leaves the PC; "I don't press Pay or Delete, I'll show you where it is" | Everyone claims privacy; few show it | Day 4, Day 7 | S |
| 8 | **Accessibility engineering as a story**: cues in the 300 Hz – 3.2 kHz band, quieter than speech, panning capped at ±30 % for asymmetric hearing loss, Mate up to 220 px at 150–175 % scaling, "reduce motion" | Reads as gerontechnology, not a toy mascot | already built; mixer Day 3 | S |
| 9 | **Ask in Russian, click an English UI**: Mate matches the control by meaning, answers in Russian, reads the on-screen English label | A 10-second proof of the multilingual Meta stack | Day 2 test, video Day 9 | S |
| 10 | **README above the fold**: an 8-second loop from the real build, then three links (try it, download, video), the "AI that clicks for you vs. AI that shows you where" meme | Judges scan a README in 15 seconds | daily; final Day 9 | S |
| 11 | **Radical honesty**: the pre-kickoff disclosure + `CHANGELOG.md`, and a limits table (elevated windows, what is and isn't redacted, MMS-TTS licence) | Trust; no judge finds a hidden flaw first | Day 1, Day 9 | S |
| 12 | **The Day-7 freeze + the 12-point test matrix** as a gate for every later change | Most teams break their build two hours before the deadline | Day 7 | M |
| 13 | **A name for the idea**: "Mate waits; you click." on the title card and in the README | A slogan judges can repeat in their notes | Day 9 | S |
| 14 | **The open MateScript spec** (`matescript.md`) linked from the README as a small public format | Signals depth; others keep their formats private | Day 9 | S |
| 15 | A physical Mate (a desk lamp or a printed bulb) in the over-the-shoulder shot | Makes the character memorable beyond the screen | Day 8 | S |
| 16 | **Speak to the adult child, too**: "Put Mate on your mom's computer in two minutes" on the landing page and in the video's end card; installation, the invite code and the voice language need no manual setup | The Ourguide thread shows who asks for this: the children of older users, who install the software | Day 6 (landing), Day 9 (video) | S |

Drop: "works on any app" claims, Muse Wonder unless it is really used and logged, loud fun sounds in the main demo flow.

## 5. Acceleration

Prepared on 2026-10-25 (outside the repository, so no product code exists before the kickoff):

- **Caches warmed** in `C:\mmw`: the crates we expect (Tauri 2 with plugins, `windows` with the features we need, `uiautomation`, `windows-capture`, `cpal`, `rubato`, `ringbuf`, `reqwest`, `tokio`, `tokio-tungstenite`, `image`, `rodio`, `tauri-specta`, `strsim`, `proptest`, `insta`) fetched and compiled through sccache; a Tauri v2 template built; Python envs for the proxy (FastAPI, uvicorn, httpx, websockets) and for MMS-TTS (CPU torch, transformers, uroman) with the `facebook/mms-tts-eng` and `-rus` weights in the Hugging Face cache. The resolved versions are listed in `setup-windows.md`. Agents' `--offline` builds can now find these crates.
- **Tools installed**: Modal CLI (`modal setup` is the owner's), OBS Studio (backup video), `cargo-insta` (golden tests), `jq`.
- **Tooling fixed for Day 1**: `mm test` no longer forces `--offline` on the owner; `mm check` knows Cargo/Rust/Vite build variables and `env::var_os`; `mm check --full` runs the web `build`; `mm uia` captures dropdowns and menus (owned popups).

Day-1 setup (verify each on Day 1):

- `tauri@2` and `@tauri-apps/cli@2`, never the 3.0 alpha (D34).
- Crate features moved since the agents' training data: `reqwest`'s TLS feature is now `rustls` (not `rustls-tls`). Take versions and feature names from `cargo add` / `cargo info`, never from memory or an agent's answer.
- `.cargo/config.toml`: `rustc-wrapper = "sccache"`, a short `target-dir` (MAX_PATH, `LNK1104`). sccache does not cache incremental builds: keep `incremental` for the owner's dev loop, `CARGO_INCREMENTAL=0` for agents and clean builds [verify].
- In agents' copies the sccache server may be unreachable (no network): make `mm agent test` run without the wrapper (empty `RUSTC_WRAPPER`) or with `SCCACHE_IGNORE_SERVER_IO_ERROR=1` [verify].
- Dev profile: `[profile.dev.package."*"] opt-level = 2` (fast image and audio code in debug builds), `debug = "line-tables-only"` for our crates.
- Keep `mm-core` free of `tauri` and `windows`: `cargo nextest run -p mm-core` then takes about a second and runs in every agent's copy.
- `team.py watch -C . --name cargo --ext rs,toml -- cargo check --workspace` from Day 1 (`CLAUDE.md`).
- Avoid: `RegisterHotKey` / `tauri-plugin-global-shortcut` (no key-up), `rdev`, `enigo` (hide UIPI errors), Tone.js/Howler (raw PCM streaming), `nom`/`pest` for the streaming parser, `WDA_EXCLUDEFROMCAPTURE`.

Field notes from a Tauri v2 push-to-talk overlay on Windows ([r/tauri, Jul 2026](https://www.reddit.com/r/tauri/comments/1ulhi5p/how_i_built_a_sub200ms_localfirst_voice_dictation/)), to check in S1/S2:

- No focus steal: keep the window hidden at start, and call `set_ignore_cursor_events(true)` and `set_focusable(false)` **before** showing it (our invariant 1; we still add `WS_EX_NOACTIVATE` on the HWND).
- Chrome sometimes swallows the key-up of a global shortcut, leaving the app "listening" forever. Their fix: once PTT starts, poll `GetAsyncKeyState` for the held key at ~30 Hz to detect the release. Our `WH_KEYBOARD_LL` hook gets key-ups, but Windows silently unhooks a slow hook, so keep this poll as a watchdog.
- Mixed DPI: on every PTT press they take `GetCursorPos`, find the monitor with `monitor_from_point`, move the overlay there and use that monitor's scale factor (we key on the target window's monitor instead).
- Short `target-dir` for MAX_PATH (we have it).

Parallel work with the team: the parser and its fuzz tests (Codex), the proxy skeleton with a mock SSE stream (Codex/Claude), copy and translations (Copilot), edge cases and reviews (Gemini ×3), creative variants (oss + DeepSeek). The lead keeps the Win32 core.

## 6. Cuts (decided by the owner at the kickoff, D39)

1. **Linux X11**: cut now. ✅
2. **Muse Wonder escalation**: only if Spark fails scenario 1 on Day 5; **plus a model choice in Settings** (D40). ✅
3. **Fun mode**: the calm cues and the sounds we have stay; **5–6 well-chosen fun sounds now**, the rest last. ✅
4. **JustLog3**: ~~keep it running next to the proxy~~ rejected: it is snapshotted, backed up and shut down on Day 6 (D10, `server.md`).

## 7. To verify at the kickoff

- **Credits: $150 of Meta Model API credits** per participant, according to the organisers' promotional email (search of 2026-10-25). Everything (development, the testers, the judges' sessions through the proxy) must fit in it: `PROXY_DAILY_BUDGET_USD` sized from the real per-request cost measured in S4, Muse Wonder ($4 / $20 per 1M tokens) only when it is really needed, screenshots downscaled before sending. Ask at the kickoff whether more credits can be requested.
- Judging criteria and their weights (not published yet; an unofficial preview expects reliability, traceable decisions, handling of bad input and practical use beyond a demo, which fits "I can't find it", the debug view and the real tester); whether judges run Windows binaries; the video length.
- Whether pre-kickoff docs, art and sounds are allowed as they are (D31).
- The Meta API base URL, the computer-use batch format, and whether text comes next to `computer_call`.
