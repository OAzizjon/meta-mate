# MetaMate

Desktop helper for the Meta Global AI Developer Hackathon (Indie Developer Track, goal: top 5). Push-to-talk voice, answers with sound effects, and **Mate**, a small light-bulb helper on the screen (male, "he"; "mate" = buddy) who shines a beam on the right button in any app and guides the user step by step (look: `docs/robot.md`). Windows only (Linux was cut at the kickoff, D39).

Stack: **Rust + Tauri v2**, TypeScript WebView (robot SVG, Web Audio). Models: Meta Model API (Muse Spark 1.3, Muse Wonder, Muse Voice Transcribe, SAM 3.1, Muse Image) + MMS-TTS (Meta open weights) on Modal. Details: `docs/architecture.md`.

Talk to the owner in **Russian**. Everything in the repository (code, comments, commits, docs, README) is **English**. The UI is English + Russian.

## Status

The build started at the kickoff on 2026-10-26, 22:00 Tashkent (D38); submissions are due 2026-11-04 21:00 ET, live finals 09.11. The Meta API key works (`mm probe`, D41: use `reasoning.effort` `minimal` for spoken answers). The rules are read (D52): public repo and a 3-minute video at submission; judged on Impact 30 %, Use of Meta AI tooling 25 %, Technical depth 20 %, Demo 25 %. Plan: `docs/plan.md`. Decisions: `docs/decisions.md` (log new decisions there, never rewrite old entries). Where we can fall behind and how we stay ahead: `docs/edge.md`. Changes: `CHANGELOG.md`.

## Commands

`./mm <command>` from Git Bash, `.\mm <command>` from PowerShell (`tools/mm.py`, standard library only):

| Command | What |
|---|---|
| `mm doctor` | build tools and versions (Rust, MSVC, WebView2, Node, ffmpeg, uv, typos, ...) |
| `mm status` | git, deadline countdown, today's plan, agent queue |
| `mm release [--dry-run] [--allow-dirty]` | require one changed NSIS installer; print its path, size, SHA-256, commit (+dirty when applicable), proxy host and app version |
| `mm plan [today\|N]` | today's (or day N's) plan |
| `mm check [--full] [--only CHECKS] [--changed] [--base REF] [--dry-run]` | fast checks: secrets, env names, Markdown links, typos; Rust/TS once they exist |
| `mm test [--changed] [--base REF] [--dry-run]` | unit tests (tools, then cargo/npm); `--changed`: only the packages the changed files can affect (reverse dependency closure) |
| `mm phrases [build\|check] [--tts-python PATH] [--force]` | build or verify cached English and Russian MMS-TTS phrases |
| `mm where TERM [--all] [--limit N] [--json]` | where a name is defined and used across Rust, TS, Python, locales and docs: spelling variants (`sign_out` = `signOut` = `sign-out`), Tauri commands with their `invoke()` callers, event names through their constants, a locale key with its en/ru text, and words quoted from a window (`mm where "Написать Mate"`) to their key and code |
| `mm cite FILE\|-\|#JOB [-C N]` | an agent's answer with the code behind every `path:line` it cites, next to each claim; flags missing files, lines not in the file, ambiguous names; never opens a secrets file |
| `mm map [--check]` | regenerate `docs/code-map.md`: every source file's purpose and public items, Tauri commands ↔ callers, events ↔ emit/listen/constants, proxy routes ↔ client, tests per area (the pre-commit hook refuses a stale map) |
| `mm stats [--save] [--json] [--since DAYS]` | code by language and area, largest and most complex files, test counts, churn and hot spots without tests, TODOs, production `unwrap`s, unused locale keys, the agents' table; `--save` keeps a snapshot and shows deltas |
| `mm uia [--delay S] [--depth N] [--save NAME]` | UI Automation tree + screenshot of the foreground window → `fixtures/uia/` (Demo user's windows only) |
| `mm art [NAME ...]` | render `assets/art/src/*.html|svg` to PNG (headless Edge) |
| `mm shot [PAGE ...]` | render WebView pages to `shots/` with fixture IPC and headless Edge; also `mm agent shot` |
| `mm locales merge PATCH` / `mm locales add KEY EN RU` | merge a patch's locale keys key by key, then git-apply the rest; or add a key to both locale files |
| `mm sounds [check\|normalize] [--force]` | sound licences check / convert + loudness-normalize (ffmpeg; cues keep their levels) |
| `mm agents [status\|stats]` | drafting team queue |
| `mm keys SOURCE_DIR [--apply]` | owner only: copy key values from another project's secrets file (prints names only) |
| `mm probe models\|stream\|chat\|click\|sam\|asr\|safety\|ledger` | lead only: the Meta API from this machine: stream with timings and cost, the app's real chat request (`chat QUESTION [--lang] [--name] [--about] [--repeat N]`: raw MateScript, parsed items, a table over repeats), one computer-use (or `<point eNN>`) turn on a fixture with the UIA element it hit, a SAM 3.1 outline under a pixel, speech recognition on a WAV, replay recordings, a dollar ledger (skill `metamate-meta-api`) |
| `mm probe safety DIR [--models muse-spark-1.3,muse-wonder-1.0] [--repeat N] [--jobs K] [--max-edge 1280] [--reasoning EFFORT] [--budget USD] [--max-unpriced N] [--out REPORT_DIR] [--dry-run] [--only GLOB]` | synthetic HTML safety benchmark: subfolder manifests list file, label (scam/legit), lang (en/ru), flags and notes. Headless Edge screenshots cached beside report.json/report.md; Rust builds the app request with an empty element list and the locale's tray question. Defaults: 4 workers, 1 repeat, low reasoning, $10 ledger cap, 30 unpriced calls; in-flight calls reserve ledger slots (known costs can overshoot by at most the active calls). Verdict uses MateScript mood then bilingual keywords; click/call/pay advice uses a conservative sentence heuristic. Dry run renders all pages and prints one request with image elided, without keys or HTTP. Requires Edge and Cargo; reports default to safety-report/. |
| `mm probe point DIR [--models muse-spark-1.3,muse-wonder-1.0] [--elements on,off] [--repeat N] [--jobs K] [--max-edge 1280] [--reasoning EFFORT] [--budget USD] [--max-unpriced N] [--out DIR] [--dry-run] [--only GLOB]` | lead only: pointing accuracy on subfolder manifests; validates inline HTML rectangles before any call. Shares safety's Edge PNG cache, workers and ledger caps. Defaults: both models and element modes, 1 repeat, 4 workers, minimal reasoning, $10 cap, 30 unpriced, point-report/. App request and MateScript parser; first point wins. Reports per language/model/mode accuracy, near misses, absent-target refusals, tag/end median and p90, tokens and ledger cost. Dry run prints an image-elided request without keys or HTTP. |
| `mm difflint [PATCH\|-]` | an agent patch's added lines: conflict markers, debug prints, `test.only`, new `unwrap`s, `unsafe` outside mm-win32, UI strings outside the locales, secret-looking values (never printed), files over 800 lines |
| `mm legacy [--only rust,ts,py,locales,markers,docs,external] [--limit N] [--json] [--strict]` | legacy / dead-code candidates: `allow(dead_code)`, Rust/TS/Python items nobody uses, TS exports used only in their own file, unused locale keys, legacy comment markers, docs pointing at missing paths, plus `cargo machete`, `vulture`, `ruff` when installed; false positives go to `tools/legacy_allow.txt` |
| `mm gate PATCH\|- [--json]` | read-only triage of an agent patch before applying it: `git apply --check` (draft.sh header dates stripped), difflint leftovers, which invariants the touched paths can break (focus, DPI, redaction, no auto-click, can't-find, locales, Meta default, proxy redeploy, lead-only areas), the tests to run, files over 800 lines; exit 1 on FAIL |
| `mm |difflint|contracts|legacy|gate` | [--verbose]` | Rust ↔ TypeScript: every Tauri command registered and called by name with the right argument names, events emitted and listened, locale keys present (also a `mm check` check) |
| `mm agent check [--only CHECKS]` or `mm agent test\|where\|cite\|map\|stats\|difflint|contracts|legacy|gate` |\|contracts\|legacy\|gate` | the **only** commands agents may run: offline, no secrets, no GUI (`agent test` runs the pure crates' Rust tests with a short `CARGO_TARGET_DIR` in TEMP) |

Agent mode is enforced in code: without `.git` (agent copies), inside a draft-agents sandbox, or with `MM_AGENT=1`, every command except `mm agent ...` is refused.

## Map

| Where | What |
|---|---|
| `docs/` | `code-map.md` (generated by `mm map`: start here to find code), `tools.md` (every tool and command, who may run it, quick recipes), strategy, decisions, plan, edge (forecast, asymmetric moves, acceleration), architecture, MateScript, pointing risks, sounds, robot, prompts, copy, video, Reddit research, server, setup, project structure |
| `tools/` | `mm.py` CLI (`mm release`), code tools `mm_where.py`, `mm_cite.py`, `mm_map.py`, `mm_stats.py`, `mm_changed.py`, `mm_checks.py`, `uia_dump.ps1`, `synth_cues.py` (cue sounds), `sounds_commons.py` (CC0/PD sounds: `--dry-run`, `--pin`), `tools/tests/` |
| `assets/sounds/` | `src/{cues,sfx,amb}` sources → `cues/`, `sfx/`, `amb/` OGG; `LICENSES.md` row per file; `sources.json` Commons pins |
| `assets/art/` | robot art: `src/*.svg|html` sources → rendered PNG |
| `crates/` | `mm-core` (pure: geom, matescript, api request/response/sse/intent, api/sam (SAM requests and box parser), vad (silence detector), teacher/keys, teacher, shot, redaction, privacy, ptt, mic, wav, placement, flight), `mm-platform` (traits, mock/replay), `mm-win32` (the only Win32 crate: capture, dpapi, dpi, focus, input, keyboard, locale, monitors, mouse, overlay, shell, time, uia, window, winevent) — layout in `docs/project-structure.md` |
| `src-tauri/` | the Tauri 2 shell: voice (`voice.rs`, `proxy.rs`, `mic.rs`, `ptt.rs`, `answer_stream.rs`), pointing (`screen.rs`, `pointing.rs`, `flight.rs`, `highlight.rs`, `explain.rs`), windows and identity (`home.rs`, `chat.rs`, `signin.rs`, `auth.rs`, `card.rs`), debug/performance (`debug_view.rs`, `perf.rs`), SAM (`sam.rs`), settings (`settings.rs`), and tray (`lib.rs`). Flags: `--ask "question"`, `--voice-selftest FILE.wav`, `--ptt-probe`, `--look-probe`, `--s1-selftest`, `--s3-selftest`, `--card-selftest [ru]`, `--debug-window` (build: `npm run build`, then `cargo build -p metamate --features custom-protocol`; dev stack: `server/run_dev.py --tts`) |
| `site/` | static landing page |
| `src/`, `mate.html`, `highlight.html`, `s3.html`, `home.html`, `settings.html`, `chat.html`, `signin.html`, `card.html`, `debug.html` | the WebView: `robot` (Mate's B1 SVG), `audio`, `i18n`, `player` (MateScript timeline), `ipc` (answer stream), `highlight`, and page modules; Node tests with `npm test` |
| `assets/phrases/` | Mate's cached lines as WAV (`mm phrases build|check`; MMS-TTS, CC-BY-NC) |
| `locales/` | `en.json`, `ru.json`: every user-facing string (invariant 7), checked by `tools/tests/test_locales.py` |
| `server/`, `tts/` | the proxy (`metamate_proxy/`, `run_dev.py`); MMS-TTS (`mms.py`, `text_norm.py`, `serve_dev.py` locally, `modal_app.py` + `deploy_modal.py` on Modal) |
| `fixtures/` | replay fixtures (UIA JSON + screenshots) for tests without a live desktop |

## Git

Nothing was committed before the kickoff (D31); the first commit (2026-10-26, after the kickoff) holds the pre-kickoff docs, tooling and art, disclosed in the README section "Prepared before the kickoff". GitHub: a **private** repository until the rules are read or the owner says otherwise (D42). Before every push: `mm check`, the pre-commit hook (`tools/git-hooks`), `git status --ignored`, a look at new fixtures and large files.

## Secrets

- Secrets files (`.env`, `.env.*`) are **never** opened, printed or shown to agents; enforced by `.claude/hooks/protect_env.py` and `deny` rules. Variable names live in `env_keys_names.txt` (add new names there; `mm check` verifies code against it).
- Never split the secrets file name to get past the hook. If code must contain that path, write it with Edit/Write.
- Provider keys live only on the proxy server and in the developer's local secrets file. Never in the client binary.
- Moving keys from JustLog3: the **owner** runs `./mm keys "../JustLog3 Cloud"` (dry run), then with `--apply`.

## Invariants (breaking one is a bug)

1. Our windows **never take focus** from the user's app (`WS_EX_NOACTIVATE`, `SWP_NOACTIVATE`).
2. Per-Monitor DPI Aware v2; physical virtual-screen pixels inside; CSS pixels only when drawing; one transform per captured frame.
3. Screenshot = the target window region (+ its popups), only during a request, password fields blacked out before sending; fail closed when redaction can't be guaranteed.
4. Mate never auto-clicks payment, purchase, delete, money transfer or password actions; `pending_safety_checks` are confirmed by a human only.
5. "I can't find it" beats pointing at a guess.
6. Product name in one constant (`APP_NAME`), robot name `ROBOT_NAME = "Mate"`: the product name may change after the rules.
7. All user-facing strings live in the locale files (en + ru), never inline in code.
8. Meta models by default; non-Meta providers only as development fallbacks.

## Drafting team: Codex, Claude, Gemini, Mistral, Copilot, local, oss, DeepSeek, Kimi, Meta, Command Code, Cursor

Global rules apply (skill `draft-agents`): the lead is Superclaude and never sits idle; before code, Codex `edit` + Gemini `ask` split over all its accounts (`team.py fan`); when done, `review` by both (Mistral is the main second opinion on every decision and risky patch since 2026-11-01, the Claude worker for hard logic); rate what you read (`team.py rate N good|partial|bad`); Copilot, local, oss and Mistral never sit idle. DeepSeek (`team.py submit deepseek ask`, text only, `deepseek-v4-pro`) gets the same creative ask as oss now and then; its key `DEEP_SEEK` stays in the secrets file and only the skill's `deepseek.py` reads it (D35); nothing private goes to it. Kimi (Moonshot's Kimi Code CLI, the owner's login via `kimi login`, 2026-10-30) is the team's main tool author since 2026-11-01 (complete, tested commands and scripts that ease the team's coding; the lead or Codex verifies them before they land) and asks and reviews as text only for now: the skill's `kimi.py` runs it with every tool left out, so it cannot read the secrets file through an absolute path. Meta (`team.py submit meta ask|review`, 2026-10-31) runs Meta's own Muse models on the hackathon credits, `muse-wonder-1.0` by default, two at a time: give it the hardest reviews and judgment calls regularly (owner: the $150 works for the team). Command Code (`commandcode`, open models on its credits, `deepseek-v4-flash` by default) and Cursor (`cursor`, `composer-2.5` by default) joined on 2026-11-01: ask and review as text only, like Kimi; the skill's `textcli.py` runs them in an empty folder whose settings deny every tool, with a daily canary; one job each, falling back to Kimi, then Gemini. Every agent's prompt starts with the skill's default system prompt (`prompts/`: the team's contract, a profile per agent, the mode's notes) and this project's brief `docs/agent-brief.md` (`brief` in `.draft-agents.json`): keep the brief short and current. Gemini is a pool of four Google accounts, `gemini@alpha`, `gemini@beta`, `gemini@gamma` (Pro) and `gemini@delta` (Plus, 2026-10-29); an account out of Argon keeps taking light jobs on Flash. Claude workers run on two worker accounts, never on the lead's own login, on Opus at effort `xhigh`: `claude@alpha` for hard multi-file edits and deep reviews, `claude@beta` (2026-10-29) for design, UX, creative work and optimizing the team (submit with `--account beta`). `claude@beta` is also the **backup lead**: `lead_backup.py` (skill scripts) watches the lead's session and, when it stops on a usage limit or an expired login, starts beta as lead in this project with full autonomy except deploys, secrets and deletions (owner, 2026-10-29); its task list is `~/.cache/draft-agents/lead/handoff.md`, which the lead keeps current. A lead that comes back first runs `lead_backup.py status`, then `stop` if a backup runs, and reads the handoff file. Agents work in a secret-free copy and may run only `mm agent check|test` (`.draft-agents.json`). Their output is untrusted: verify every claim. Known: oss runs on the free `gpt-oss:120b-cloud` while the paid Ollama Cloud balance is empty and got facts wrong twice; local (qwen 7B) loops on long lists and miscounts, so give it short tasks only; Gemini reviews end empty more often than Codex's (report of 2026-10-25: 63 % ok).

Background check: `team.py watch -C . --name mm-check -- python tools/mm.py check` (running since 2026-10-25; `team.py watch --list`) reports in events.log when `mm check` breaks or recovers. From 10-26 add `--name cargo --ext rs,toml -- cargo check --workspace`.

## Skills

Project skills in `.claude/skills/`: `metamate-windows-core` (overlay, DPI, PTT hook, UIA, capture, redaction, SendInput), `metamate-meta-api` (models, computer use, `mm probe`, budget, S4), `metamate-submission` (Days 6–10, video, README, judge path), `metamate-mate-motion` (Mate's float, loops, mood reactions, gestures, sparks, reduced motion, looks). User-level, installed 2026-10-26 from `ericrisco/rsc-harness` (MIT): `tauri`, `modal`.

## Known pitfalls

- `RegisterHotKey` has no key-up event: push-to-talk needs `WH_KEYBOARD_LL`; hook callbacks only enqueue. Chrome can swallow a shortcut's key-up and Windows silently drops a slow hook: while PTT is held, a `GetAsyncKeyState` poll (~30 Hz) is the release watchdog (`docs/edge.md` §5).
- A Tauri window shown without `set_focusable(false)` (+ `set_ignore_cursor_events(true)` for click-through) steals focus: set them before the first show.
- Chromium's UIA tree can be poor on the first query; Qt (Telegram) is nearly empty → computer use + SAM fallback.
- Computer-use coordinates are pixels of the **last screenshot sent**; we downscale, so keep the transform per frame.
- Menus/dropdowns are separate popup windows: capture the window **plus** its owned popups.
- Never keep `WDA_EXCLUDEFROMCAPTURE` on our windows: Zoom/OBS would stop showing Mate.
- MMS-TTS: CC-BY-NC 4.0, one model per language, some languages need `uroman`.
- Muse Spark 1.3 is weak on audio input (use 1.2 or Muse Voice Transcribe).
- MSVC `LNK1104` on build scripts = the path is too long (MAX_PATH 260): never build in deep temp folders; the project path is fine. A Tauri template built in 2 min on 2026-10-24 once `CARGO_TARGET_DIR` was short.
- Tools installed by winget sit in per-package folders on the **user** PATH: shells opened before the install don't see them (`mm` reads the registry PATH). In Git Bash never prepend `WindowsApps` (its `bash.exe` is the WSL stub).
- `tools/*.ps1` have no BOM, so Windows PowerShell 5.1 reads them as ANSI: keep them ASCII and write other characters as `\uXXXX` escapes inside .NET regex strings (a Cyrillic literal broke `uia_dump.ps1` on 2026-10-26). `uia_dump.ps1 -SelfTest` checks the password words.
- A password revealed with the eye button is plain text and `IsPassword` is false (Chromium switches the input type): redaction never relies on `IsPassword` alone (D37).
