# Changelog

All notable changes to MetaMate are recorded in this file.

This changelog follows [Keep a Changelog 1.1.0](https://keepachangelog.com/en/1.1.0/) and [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

<!-- Hackathon build: 2026-10-26 .. 2026-11-04 -->

- Guest trial: up to 12 messages an hour per device without signing in; Mate and the sign-in window show what is left (`/auth/guest`).
- Click Mate to talk: one click starts listening, another click asks.
- Language choice in Settings: Automatic (as in Windows), English or Русский.
- A chat window for typed messages with history; Mate still speaks and points on screen.
- Home, Settings, sign-in, card and chat windows work from 360 px wide.
- Mate says when the microphone cannot be opened or heard nothing.
- Home's "bigger text" question opens Windows Display settings first.
- Spotlight: while Mate points, the rest of the screen dims around a soft opening on the target (Settings: "Dim the rest of the screen while I point", on by default).
- Warmer, warmer: during a lesson step the ring and Mate's glow brighten and pulse faster as the mouse nears the right button, and hold steady once it is on it.
- A dotted trail of light from a wrong click to the ring during a lesson (no red, no cross); under reduced motion it only fades.
- Drop Mate on it: drag Mate onto any control and click him to hear what it does; nothing leaves the PC until that click, and then only the control's name, type and the app's name.
- X-ray look: while Mate thinks, faint outlines sweep over the elements he is reading and painted-over fields show as dark tiles with a lock; only rectangles reach the overlay, and the overlay is never in a screenshot.
- Sign-in is required: the sign-in window appears at startup and when a question is asked; the proxy can enforce it with `PROXY_REQUIRE_SIGN_IN`.
- The talk key is a setting: Right Ctrl, Right Alt, Caps Lock, Insert, F9 or F10, for laptops without Right Ctrl.
- Quit MetaMate from the Home window; right-click Mate for Home, Settings, Hide for 1 hour and Quit.
- Drag Mate with the mouse; he keeps the new spot as his home.
- A live team progress board for the owner in a GitHub issue.
- Privacy calibration explains that normal apps such as Telegram are looked at, while remote desktops and canvas-only windows ask first.
- The Home window explains how to talk to Mate, suggests three questions to try, and says what Mate never does; it opens at startup and from the tray.
- The landing page is served from the proxy URL.
- The lesson card has a new layout and a print page.
- An architecture diagram and a 12-point test matrix.
- Draft release v0.9.0-rc1.
- The judges' proxy is live on Modal; Google sign-in has been verified through it.
- Mate's answer bubble shows which Meta model answered; the tray offers "Try a question" for people whose microphone does not work.
- The team added a second Claude worker for design and backup lead coverage, plus a fourth Gemini account.
- Sign-in hardening: a new sign-in replaces a forgotten one, links opened elsewhere are refused, and sessions are sealed with DPAPI in local app data.
- Lessons escalate to Muse Wonder after the person clicks away from the beam twice, with Mate explaining the escalation.
- Keepsake card filenames use local time, and each card draws exactly one Mate.
- Glimmer on Modal may use an A10G or L40S when no L4 is free.
- `mm check` byte-compiles Python; `--card-selftest` checks keepsake card generation.
- The proxy is prepared for Modal deployment (`server/deploy/modal_proxy.py`, `deploy_modal_proxy.py`).
- `docs/architecture.md` maps the Meta models.
- Mate shows the words he hears while you speak; "Is this safe?" asks Mate to check the window for scams; a successful lesson leaves a keepsake card drawn by Muse Image in `Pictures\MetaMate`.
- App sign-in through the browser with a loopback callback and a saved session; proxy `/v1/image` (Muse Image) and `/v1/glimmer` endpoints; provider-side PKCE; sign-in floods are refused instead of evicting live sign-ins.
- Proxy sign-in with Google and GitHub using PKCE between the app and proxy, HMAC session tokens, and judge tiers by e-mail; Muse Glimmer 30B (Meta open weights) served by llama.cpp on a Modal L4 GPU; MMS-TTS deployed on Modal; hackathon rules read (D52).
- Rust workspace in `Cargo.toml` with `crates/mm-core` for pure core names and coordinate types, and `crates/mm-platform` for OS service traits.
- `mm_core::geom` physical, screenshot, and CSS pixel types, per-frame `FrameTransform`, per-monitor `MonitorTransform`, tests, and a compile-fail doctest.
- English and Russian locale files in `locales/en.json` and `locales/ru.json`, based on `docs/copy.md`, with a model-choice setting, plus `tools/tests/test_locales.py`.
- `mm probe --reasoning EFFORT` for Muse Spark reasoning before answering; `minimal` reduces first-token latency from about 10 seconds to about 2 seconds, and `META_KEY` aliases the Meta API key.
- Spike S4 Meta API replays in `fixtures/replay/`.
- Responses API request builders and response parser in `crates/mm-core/src/api/`.
- Teacher state machine in `crates/mm-core/src/teacher/`.
- Proxy pure model and spending policy in `server/metamate_proxy/policy.py`.
- Golden tests for the MateScript specification examples.
- Tauri 2 app shell with Mate's B1 SVG in the WebView (`src-tauri/`, `src/robot/`).
- `crates/mm-win32` (no-focus overlays, Per-Monitor DPI v2, UI Automation snapshots) with the S1/S3 self-tests.
- The redaction step and screenshot pixels in `crates/mm-core/src/redaction.rs` and `shot.rs`.
- The multilingual password-word detector in `crates/mm-core/src/privacy.rs`.
- The WebView audio engine and i18n in `src/audio/` and `src/i18n/`.
- The proxy app (`server/metamate_proxy/app.py`): invite codes, `/v1/responses` with the policy, the speech recognition relay, and `/v1/tts` for MMS-TTS; `server/run_dev.py --tts` starts the local MMS-TTS service (`tts/serve_dev.py`).
- The app's proxy client (`src-tauri/src/proxy.rs`) and `metamate --voice-selftest FILE.wav`, which runs speech recognition, the answer and speech synthesis end to end and reports every latency.
- Push-to-talk: the keyboard hook (`crates/mm-win32/src/keyboard.rs`), its driver and `--ptt-probe` (`src-tauri/src/ptt.rs`), and microphone capture (`src-tauri/src/mic.rs`).
- The microphone framer, a streaming SSE parser and a WAV reader in `crates/mm-core`.
- The WebView timeline player and its IPC parser (`src/player/`, `src/ipc/`).
- The live voice loop (`src-tauri/src/voice.rs`): hold Right Ctrl, speak, release, and Mate answers aloud in his window, sentence by sentence, with a rotating "let me think" phrase while the answer starts; a new press interrupts him. Development builds run it against `server/run_dev.py --tts`.
- `metamate --ask "question"` plays one answer in Mate's window and reports how it played, with no microphone (spike S7).
- Text normalization for MMS-TTS (`tts/text_norm.py`): numbers, percent, key names and symbols become words; Latin words are transliterated for the Russian voice; Russian numbers after "из" take the genitive, and common abbreviations are spelled out.
- "Where's the button?": a question about the screen gets one redacted look at the foreground window and its menus (`crates/mm-win32/src/window.rs`, `capture.rs`, `uia.rs`; `src-tauri/src/screen.rs`), and Mate flies beside the answer's target while the highlight window rings it, in time with his speech (`src-tauri/src/pointing.rs`, `flight.rs`, `highlight.rs`, `src/highlight.ts`).
- Pure pieces for it in `crates/mm-core`: intent routing (`api/intent.rs`), Mate's placement beside a target (`placement.rs`) and his flight path (`flight.rs`).
- Cached phrases: `mm phrases build|check` synthesizes Mate's short lines into `assets/phrases/` once; Mate speaks them at once (the wait phrase, "I can't see that here").
- MMS-TTS on a Modal GPU (`tts/modal_app.py`) and its deploy script (`tts/deploy_modal.py`); `server/run_dev.py --tts-modal` uses it.
- `metamate --look-probe` measures the foreground window's UI Automation tree.
- A mouse hook and click recognizer for the teacher; `src-tauri/src/teacher.rs` turns "How do I...?" questions into step-by-step lessons, with Mate pointing at each step and waiting for the person's own click.
- One highlight window per monitor.
- `--debug-window` mirrors the answer stream in a debug window.
- The robot's mouth follows the voice level while listening.
- `mm_core::geom::uncovered` computes the areas of a region not covered by other rectangles.
- Keyboard chords for teacher steps (`crates/mm-core/src/teacher/keys.rs`), with key aliases such as `Return` and `Enter`; drag steps complete near the drag's end point.
- The settings store (`src-tauri/src/settings.rs`): `settings.json` written atomically, defaults when the file is broken.
- "Do it for me" input (`crates/mm-win32/src/input.rs`): one confirmed left click, with an English and Russian blocklist of payment, delete and password actions (`privacy::forbidden`) and foreground-window and target-point checks.
- Three more praise lines that rotate.
- Teacher core support for offering a gated spoken-confirmation "Do it for me" action after two misses or on request, with one left click completing the current step (`crates/mm-core/src/teacher/`).
- A tray menu and settings window with live settings updates, language-aware labels, answer model, TTS speed, motion, sound mode, and robot size (`src-tauri/src/lib.rs`, `src-tauri/src/settings.rs`, `src/settings/`).
- Teacher keypress steps driven by events from the push-to-talk hook (`src-tauri/src/teacher.rs`, `crates/mm-core/src/teacher/keys.rs`).
- For screen answers and lesson steps, SAM 3.1 outlines a target when the UI Automation tree has fewer than three named interactive elements; rich trees still refuse uncertain targets (`crates/mm-core/`, `src-tauri/src/sam.rs`).
- Toggle push-to-talk starts on a tap and ends on a second tap or 1.2 seconds of speech silence (30 seconds maximum); microphone level animates Mate (`crates/mm-core/src/vad.rs`, `src-tauri/`, `src/`).

### Fixed

- Home, Settings, Chat and sign-in share one warm look (`src/theme.css`): Mate's yellow and navy, soft rounded cards, 18 px text, one main yellow button per view, switches for on/off settings, matching dark mode and focus rings; Mate's bubble and menu use the same colours.
- Mate presses again when asked: "click Font", "нажми Шрифт" and "открой настройки" start a guided lesson even when the classifier is down, and "do it for me" ("сделай за меня", "нажми сам") offers each click at once; every click still needs its own spoken yes.
- Clearer answers: one to three short sentences with the next action first, the exact button label and where it is, no Markdown or links read aloud; an answer cut by the network does not speak its half-word. The voice reads a minus sign, "5/10" ("five of ten") and "=" instead of dropping them.
- A sign-in the proxy no longer accepts (after its signing key changed) no longer looks like a lost connection: Mate signs out, opens the sign-in window and says so, instead of "I can't connect" on every question.
- A saved Toggle push-to-talk mode survives restarts.
- A microphone failure also ends a click-to-talk recording.
- A second click on Sign in with the same provider, from the sign-in window or Settings, joins the sign-in already waiting in the browser (2 minutes) instead of opening another tab; signing out ends it at once.
- The app's own Rust tests (153) run on Windows again: the test binaries get the Common Controls manifest the app has (`src-tauri/build.rs`).
- During a lesson, Mate's question whether he may look at the screen again no longer pops up for 30 seconds after its moment has passed when the person was talking at the time.
- An answer the proxy has to cut (too slow, or an oversized event) ends with an error Mate can say instead of silence; a rejected Meta key on the proxy no longer signs the person out (502 instead of 401), and Meta's own rate limit no longer looks like the person's allowance being used up (503 instead of 429).
- The proxy's daily allowances survive a restart with calls still in flight (they stay charged), log any call that cost more than reserved, and refund a SAM outline Meta refused.
- A window that pops up over the target only while the screen is copied no longer reaches the model unpainted: the look is refused as changed. A password field the search finds slightly shifted hides its walked original whole.

### Changed

- The browser's last sign-in page is clearer and closes itself after a successful sign-in when the browser allows.
- Home: the try-it cards wrap by width, Quit comes last and stands apart, dark-mode buttons stand out more, and a busy question shows a spinner.
- The chat's empty state suggests an example question; opening the chat from the tray brings it to the front.
- Mate appears sooner after startup, and push-to-talk polls less often while idle.
- Privacy round 7: Telegram windows with an empty UI Automation tree ask for consent on each look; login and email codes are always painted.
- Idle audio and animations sleep to save CPU; screenshot downscaling and redaction are faster.
- Privacy round 6: inputs beside private labels are painted unless their name shows a non-secret purpose; long pages are checked for them before truncation clears; article images no longer ask for consent.
- Privacy round 5: consent never waives painting an unnamed edit beside a password label; short Edge articles no longer ask for consent.
- First-minute guidance for older users: keycaps show the printed key and where to find it, instructions say to press and hold, and Settings cards and the voice-speed slider are clearer.
- Mate's right-click menu is drawn in his own window and never takes focus; Hide shows him sleeping and when he'll return, the tray offers "Bring Mate back now", and Home steps aside before Try.
- Privacy: browser chrome is no longer evidence that a canvas-only page can be read; such windows are looked at only after the user presses F8 twice in that window.
- Mate's v1.1 design adds eyebrows and a filament, with a softer aura.
- "Do it for me" re-checks the element and window at click time; yes counts only after the prompt was heard, and its blocklist covers more languages and money amounts.
- Accessibility improvements: a longer, more readable bubble, clearer settings, and announced errors.
- Greetings are handled as chat.
- Release builds use a built-in proxy URL and capped demo invite (`METAMATE_PROXY_URL`, `METAMATE_INVITE`); `mm release` refuses to build without them.
- Provider search hides walked originals, treats links and images as reveal toggles, searches a toggle's grandparent, and searches inside found fields.
- SAM is never asked about a painted pixel.
- The robot's "looking" state is sent around every screen look (`src-tauri/src/voice.rs`).
- SAM outlines no longer clear a newer answer's ring (`src-tauri/src/pointing.rs`).
- The teacher asks SAM for all unlisted clicks in a turn at once (`src-tauri/src/teacher.rs`).
- Redaction now includes revealed-password neighbours and descendants of hidden UI Automation elements (`UiSnapshot::hidden_ids`).
- `crates/mm-win32/src/window.rs` finds windows lying over the target (`occluders`).
- Safety fixes from a deep review: model points are checked against the screenshot and the target, only the person's own left click confirms a step, screenshots enter requests only as `RedactedImage`, rejected points clear the highlight.
- README status now says the hackathon build is in progress and Windows-only; Muse Wonder is an optional choice in settings.
- Linux X11 support was cut, and the JustLog3 shutdown checklist in `docs/server.md` was extended (decisions D39-D42 in `docs/decisions.md`).
- `mm test` now also runs Rust doctests; agent Rust builds run without sccache and incremental compilation.
- Proxy hardening from security reviews (D45): only the app's request shapes are accepted, text is priced per UTF-8 byte and continuations by their reported history, Meta's errors and stream events never carry the key, a refused call costs nothing, and speech recognition time is reserved before connecting.
- Spoken answers may use up to 1000 output tokens, since reasoning alone took up to 398 (D45).
- Every window starts WebView2 with autoplay allowed, so Mate can speak after push-to-talk (spike S7, passed: D46).
- Review fixes in the voice path: the push-to-talk watchdog keeps polling while a press is pending and reinstalls a hook Windows removed; microphone sends never block a cancel; speech socket writes time out; proxy addresses are parsed and replies capped.
- The teacher driver retries a failed model turn and refuses after a second failure, retries a first-turn miss once at low reasoning effort, speaks refusal and completion lines before the lesson ends, stops after three idle minutes or ten minutes in all with a sleepy line, ignores clicks made before a step was shown, re-checks the window before each pointing step, and enforces the turn budget before asking for another screenshot.
- Barge-in cancels the old answer before clearing the ring; onboarding no longer re-enters; the architecture document describes per-monitor highlights and the teacher driver.
- Screen looks mask everything outside the target app and its popups, then verify hidden windows and fields remain unchanged before using the capture (`src-tauri/src/screen.rs`, `crates/mm-core/src/redaction.rs`).
- "Do it for me" privacy checks now inspect control ancestors and metadata, reject unnamed controls, and cover more English and Russian password and action terms (`crates/mm-win32/src/input.rs`).
- Teacher targets are refused when they touch painted areas; a failed first look ends idle so its line bubble stays visible (`src-tauri/src/teacher.rs`).
- TTS speed is validated and passed through the proxy to MMS-TTS; the settings window snaps speed changes to 0.05 increments (`server/metamate_proxy/`, `src/settings/`).
- Prompts put the answer first, avoid invented names, and treat black boxes as hidden content (`crates/mm-core/src/api/request.rs`).
- Debug builds report the time spent in each part of a screen look (`src-tauri/src/screen.rs`).
- The highlight follows only the target window's movement, so tooltips and child windows no longer clear it (`src-tauri/`).
- Password-word redaction no longer hides custom controls painted on long password words (`crates/mm-win32/`).
- Lesson offers to do a step require safe, visible controls, expire after 30 seconds, and re-check the foreground window before clicking (`src-tauri/src/teacher.rs`).
- During a lesson, push-to-talk first handles a pending "Do it for me" offer; yes or no stays in the lesson, while unrelated speech starts a new question (`src-tauri/src/teacher.rs`).
- "Yes" is recognized as confirmation, and `mm probe sam` reports failed answers instead of recording them (`crates/mm-core/src/teacher/`, `tools/mm_probe.py`).
- "What's on my screen?", "what should I click here?" and "что это за окно?" now trigger a screen look; "help me send/open/print..." starts a lesson.
- "Is this safe?" warnings now start with "do not click, call or pay", and Mate's beam marks the warning sign.
- The look-consent question and sign-in lines ("sign in again", "sign in first", "trial used up", "private window") now appear in Mate's bubble; the consent question stays visible for its full 30 seconds.
- If Mate's voice fails for a sentence, its text appears in his bubble instead of silence.
- Mate's recorded phrases are included in the installer.
- The guest trial now appears even if the server is still waking up at first launch.
- The browser no longer says sign-in succeeded before the app does.
- Every window uses the light, warm theme; MetaMate no longer switches to dark mode with Windows.
- Mate's beam is warm light that fades toward the button and keeps the text beneath it readable.
- The target ring glows in Mate's bright yellow, and the step number is larger.
- Mate's right-click menu stays open instead of closing as soon as it appears.
- Questions like "help me save money" no longer start a lesson or take a screenshot.
- Scam reports, infection warnings and requests about bank codes are checked for safety before Mate points; longer safety questions need screen, message or computer context.
- "How do I find..." and "Покажи, как..." start lessons, as do English text-size and sound commands; learning for oneself takes precedence over delegation.
- Screen answers may begin by honestly saying Mate cannot find the target instead of guessing.

## Prepared before the kickoff (2026-10-24 .. the morning of 2026-10-26)

- Design documents in `docs/`: [strategy](docs/strategy.md), [decisions](docs/decisions.md), [plan](docs/plan.md), [edge](docs/edge.md), [architecture](docs/architecture.md), [MateScript](docs/matescript.md) and its [test cases](docs/matescript-cases.md), [pointing risks](docs/pointing-risks.md), [sounds](docs/sounds.md), [robot](docs/robot.md), [prompts](docs/prompts.md), [copy](docs/copy.md), [video](docs/video.md), [Reddit research](docs/research-reddit.md), [server](docs/server.md), [Windows setup](docs/setup-windows.md), and [project structure](docs/project-structure.md).
- The `mm` CLI and checks in `tools/`: doctor, status, plan, check, test, uia, art, sounds, agents, keys, and the restricted agent mode.
- The UI Automation dump tool (`tools/uia_dump.ps1`): owned popups, black gaps between frames, refusal of screenshots with a password-like field (including revealed passwords), `-SelfTest`.
- `mm probe` (`tools/mm_probe.py`, `tools/mm_fixture.py`): the Meta Model API from the lead's machine (stream timings, one computer-use or `<point>` turn on a fixture with a hit test, replay recordings, a dollar ledger), tested on a development fallback with a synthetic fixture.
- Claude Code project skills in `.claude/skills/`: Windows core, Meta API, submission.
- Synthesized cue sounds (our own, CC0), CC0/public-domain sound effects and ambience from Wikimedia Commons with a licence-checked fetcher, every file listed in `assets/sounds/LICENSES.md`.
- Mate concept art in `assets/art/`.
- Secret protection: a Claude Code hook and deny rules that keep secrets files out of reach, and the variable names (never values) in `env_keys_names.txt`.

No application code existed before the kickoff, as noted in the [README](README.md#prepared-before-the-kickoff).
