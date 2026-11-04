# MetaMate

**Ask out loud how to do something on your PC. Mate, a small light bulb on your desktop, answers aloud and shines his beam on the exact button. You click it yourself, step by step, or say "do it for me" and he presses an ordinary button only after your yes; he never presses Pay, Delete, money-transfer or password buttons.** Made for older people and anyone lost in an unfamiliar app. Meta's models do the work: Muse Voice Transcribe listens, Muse Spark 1.3 and Muse Wonder 1.0 find the button and teach, MMS-TTS speaks; Muse Glimmer 30B and Muse Image make a printable card of each lesson, and SAM 3.1 outlines controls in apps Windows cannot read.

![Mate points right here](assets/art/meme-right-here.png)

**Website and download: [meta-mate.stream](https://meta-mate.stream)** · Windows 10/11 x64.

## Try it in 60 seconds

1. **Download** [MetaMate for Windows](https://meta-mate.stream/download/MetaMate-setup.exe) — no API keys or invite codes needed.
2. **SmartScreen:** Windows may say 'Windows protected your PC': click More info, then Run anyway. MetaMate is new and not yet code-signed; Microsoft Defender found nothing in the 0.2.1 installer on 2026-11-04. [Code-signing details](docs/code-signing.md).
3. **Start:** **Try without signing in** (12 messages/hour, count shown under Mate) or **Sign in with Google** for your own daily allowance.
4. **Ask:** Click Mate once, ask *"How do I make the text bigger?"*, then click him again. Or hold **Ctrl** alone while you speak; pressing any other key cancels. You can also ask him to open an app or site by name: *"Open the calculator"* or *"Открой ютуб."* Mate opens only apps or sites named in your own request, never names that appear only on a page or in a screenshot. Prefer another key? Pick Right Alt, Caps Lock, Insert, F9 or F10 in **Settings** (Home window or tray icon).
5. **Or type:** Right-click the tray icon (or Mate) → **Chat with Mate**. Mate still points on screen.

## Links

- [Website: meta-mate.stream](https://meta-mate.stream) (the installer downloads from there)
- [Download the installer](https://meta-mate.stream/download/MetaMate-setup.exe) ([SHA-256](https://meta-mate.stream/download/SHA256.txt))
- Demo video: linked from the submission page
- [Decisions log](docs/decisions.md)
- [Architecture](docs/architecture.md)
- [Submission notes](docs/submission.md)
- [MateScript specification](docs/matescript.md)
- [Day-by-day roadmap](docs/plan.md)
- [Changelog](CHANGELOG.md)

## What Mate does

### New in 0.2

- **Mate's brain:** Choose among three Meta models on Home.
- **Voice bar:** See the words Mate hears as you speak.
- **Sounds:** Cues and sound effects.
- **"What's on this page?"** Mate describes the window.
- **Automatic update check:** Checks for new versions automatically.
- **Scam fire drill:** Practice spotting scams with fictional examples.
- **Developer mode for judges** shows how Mate works under the hood.

MetaMate is Mate, a small light-bulb companion living on the Windows desktop designed to help older adults and anyone navigating unfamiliar interfaces. Click Mate or hold **Ctrl** (or another talk key chosen in Settings) and ask in English or Russian; Mate answers aloud with a live transcript in his bubble. While you speak, a bar at the bottom of the screen follows your voice and shows the words Mate heard. The core:

- **Mate's beam:** "Where is the button?" works live across multi-monitor setups: Mate flies directly beside the target control without covering it and projects a focused beam onto it. With the accessibility element list he named the right control in all 610 measured answers (below); where it cannot read the window, he says he can't find it rather than guess.
- **Step-by-step lessons:** For "How do I..." requests, Mate acts as a patient teacher rather than an autonomous takeover. He guides one step at a time, verifies the person clicked each target before continuing, and if the person clicks away from the beam twice on Quick, Muse Wonder 1.0 automatically plans the next step.
- **Scam check ("Is this safe?"):** Evaluates suspicious emails, pop-ups, and websites in the active window. Muse Spark 1.3 delivers an immediate verdict via Mate's expression (`<mood calm>`, `<mood surprised>`, or `<mood sad>`), explains the specific red flags detected (mismatched domains, artificial urgency, fake security warnings), and shines a beam on the worst one—without ever telling the user to click, call a number from the page, or pay.
- **A note from family:** Whoever sets Mate up may leave one line on Home. Mate says it after a finished lesson and it is printed on the card; it is only spoken and printed, never part of a question (D85).
- **Keepsake lesson cards:** After completing a lesson, Mate produces a printable recap card saved in `Pictures\MetaMate` with steps rewritten into plain language by Muse Glimmer 30B and illustrated in a soft gouache style by Muse Image.

Also inside:

- **Spotlight:** While Mate points, the rest of the screen dims around a soft opening on the target, so the button stands out even in a small video call window (`src/highlight.ts`; Settings: "Dim the rest of the screen while I point").
- **Warmer, warmer:** During a lesson step, the ring and Mate's glow brighten as the mouse nears the right button and hold steady once it is on it, guiding the hand before the click (`src-tauri/src/pointing.rs`).
- **A trail, not a scolding:** After a click in the wrong place during a lesson, a dotted line of light runs from that spot to the ring while Mate says "Almost"; no red, no cross (`src-tauri/src/teacher/lesson.rs`).
- **X-ray look:** While Mate thinks, faint outlines sweep top to bottom over the elements he is reading, and every field painted over for privacy shows as a dark tile with a lock, so you can see what he is reading and which fields were painted over for privacy (`src-tauri/src/highlight.rs`, `src/highlight.ts`).
- **Drop Mate on it:** Drag Mate onto any button or icon and click him: he lights it up and explains what it does (`src-tauri/src/explain.rs`). Nothing leaves the PC until that click, and then only the control's name, its type and the app's name, plus the name and the line about yourself if you saved them; password fields, sign-in codes, tabs and window titles, and any name with an e-mail or web address or a run of four digits are skipped.
- **Home window:** An accessible starting hub (`home.html`) featuring an animated Ctrl keycap explaining push-to-talk, one-click "Try it" sample questions, a clear summary of actions Mate never takes, and a direct path to saved keepsake cards.
- **Chat window:** Type a question instead of speaking (`chat.html`): Mate's answers appear in comic-style speech balloons tinted by his mood, button names look like key chips, and steps appear as lists. Mate still points on screen.
- **Look with permission:** In Chrome, when Mate asks if he may look at the window, click him to answer yes.
- **He knows your name:** on the first Home visit Mate asks what to call you, with an optional line about yourself; both reach his answers as data, never as instructions.
- **"What can you do?"** gets a short spoken tour with three example questions on screen, and the **Guide** on Home (button or "?") shows everything one card at a time.
- **My lessons:** every lesson card on Home, newest first; each saved card is a captioned comic page (picture, title, numbered steps, date), so the PNG makes sense on its own.
- **A living bulb:** Mate floats, squashes and leans on springs and lands with a wobble, but never while his beam is on a target; a step done right gets a "CLICK!" burst.

## Built with Meta

| Meta model | Role in MetaMate | Measured performance |
|---|---|---|
| **Muse Spark 1.3** | Quick answers (the default), pointing beam, step-by-step lessons, and "Is this safe?" scam check | First token in 1.4–2.6 s at reasoning effort `minimal` (D41, D45) |
| **Muse Wonder 1.0** | Thorough answers; on Quick, a lesson's next step after two clicks away from the beam | High-reasoning model for complex guidance and fallback escalation (D56) |
| **Muse Voice Transcribe** | Realtime push-to-talk speech recognition (Ctrl) | Final transcript 250–360 ms after key-up (the socket opens when listening starts, not after you finish) (D44, D45) |
| **MMS-TTS** | Spoken voice output (Meta open weights, served on Modal GPU) | 90–110 ms GPU synthesis per sentence on a Modal T4 (D44, D53) |
| **Muse Glimmer 30B** | Chat answers and keepsake lesson card text (Meta open weights on our own Modal GPU) | Q4_K_M ~15 tokens/s on a Modal L4 / A10G GPU (D53, D57) |
| **Muse Image** | Keepsake lesson card illustration (soft gouache art with Mate, without text; printable) | Generates custom scene illustration for completed lessons (D55) |
| **SAM 3.1** | Experimental fallback: outlines what lies under a model-chosen point where the accessibility tree has nothing | No accuracy claim: it does not choose the point (see "Where is it?") |

Choose Mate's brain on the Home screen: Muse Spark 1.3 (quick, default), Muse Wonder 1.0 (thorough), or Muse Glimmer 30B. Muse Glimmer answers chat without the screen: when it is chosen, a question that needs the screen goes to Muse Spark.

The reports behind the next three sections are in [docs/evidence](docs/evidence/README.md).

### Measured: "Is this safe?"

`mm probe safety` sends the app's real safety request (the same prompt, image size and reasoning effort) for 40 fictional pages: 10 scams and 10 legitimate pages in English and in Russian, with invented brands and `.example` addresses, rendered at 1280×800 by headless Edge. Each page went 10 times to each model on 2026-10-31 (800 answers, no errors):

| Model | Language | Scams called a scam | Legitimate pages called a scam | Legitimate pages called "be careful" | Median full answer |
|---|---|---:|---:|---:|---:|
| Muse Spark 1.3 | English | 100 % (100/100) | 0 % | 5 % | 5.2 s |
| Muse Spark 1.3 | Russian | 100 % (100/100) | 8 % | 31 % | 5.3 s |
| Muse Wonder 1.0 | English | 100 % (100/100) | 0 % | 16 % | 4.2 s |
| Muse Wonder 1.0 | Russian | 100 % (100/100) | 10 % | 32 % | 5.1 s |

- No answer called a scam safe, and none told the person to click, call or pay from the page: every "call" in the 800 answers meant a number they already know or the one on their card.
- 16 of the 18 false alarms are one Russian email whose mail app shows a green "verified address" badge: the prompt treats any text that claims to be verified as a red flag on purpose, because pages can talk to the model. "Be careful" answers come from pages that ask for a code, a payment or act on a countdown, which is the cautious answer we want.
- 20 harder pages (`en-hard2`, `ru-hard2`: fake tax, bailiff, toll, pension and cloud-share notices next to real-looking legitimate ones; 400 answers): both models again called every scam a scam. Legitimate pages were called scams 0-2 % of the time in English but 34-38 % in Russian, almost all on fictional government pages: a tax or bailiff page outside the one real government portal does not match who it claims to be, which the models treat as a red flag.
- Before this prompt (D68), an unfamiliar address alone was enough for a scam verdict: Muse Wonder called 28 % of the Russian and 9 % of the English legitimate pages scams. These are fictional pages, not a real-world rate.

### Measured: "Where is it?"

`mm probe point` sends the app's real "Where is it?" request, with the element list UI Automation gives the app or with none (as for canvas and Qt apps), for 71 spoken questions on 20 fictional windows in English and Russian (mail, messenger, browser zoom, photos, display settings, video call, word processor, save and print dialogs, a clinic booking). Each question went 5 times to each model in each mode on 2026-10-31 (1,420 answers):

| Model | With the element list (en / ru) | Without it, on the control (en / ru) | Without it, within 24 px (en / ru) | Median time to the point (en, with list) |
|---|---:|---:|---:|---:|
| Muse Spark 1.3 | 100 % / 100 % | 46 % / 41 % | 68 % / 67 % | 2.0 s |
| Muse Wonder 1.0 | 100 % / 100 % | 35 % / 33 % | 58 % / 49 % | 1.6 s |

- With the element list, all 610 answers to a question whose button is on the window named the right control. Without it, a point lands on the control well under half of the time, so the app uses the UI Automation list whenever the window has one and SAM 3.1 only to outline what lies under a point.
- For the 10 questions whose button is not on the window, the answers say so ("I cannot see a home visit choice on this page") and then show where to look next (a menu, Search, Home) instead of pointing at a guess.

### Measured: "What is going on on this page?" and chat

`mm probe safety --kind describe` asked "What is going on on this page?" about the same 60 fictional pages (scam, legitimate and hard pages that look like scams; English and Russian), 3 times per model, on 2026-11-02 (360 answers, no errors). Muse Wonder 1.0 graded each answer from 1 to 5 for an older listener (what the page is, what to do, a warning only with a named sign, short, no personal data read out):

| Model | English (scam / legit / hard) | Russian (scam / legit) |
|---|---:|---:|
| Muse Spark 1.3 | 3.9 / 4.1 / 3.8 | 4.3 / 4.5 |
| Muse Wonder 1.0 | 4.4 / 4.3 / 4.4 | 4.9 / 4.3 |

The first run (before two prompt rules about naming a real warning sign) had Russian legitimate pages at 3.4 and 3.6. For chat, 120 typical spoken questions (`fixtures/eval/chat`) went to both models; the same grading moved Spark from 3.8 to 4.0 in English and from 3.6 to 3.8 in Russian once the prompt told it to answer general questions itself instead of "ask me where something is" (D84). These are model-graded scores on our own fictional set, not a user study.

## Privacy and safety

- **Never steals focus:** Overlay windows use `WS_EX_NOACTIVATE` and `SWP_NOACTIVATE`, never taking focus away from the user's active application.
- **Target window only:** Captures exclusively the foreground window and its owned popups when a question needs the screen (where something is, how to do something, whether a window is safe, or a question about a button, a link or the screen). Background windows and occluded gaps are painted black.
- **Automatic password redaction:** UI Automation identifies password fields (`IsPassword`, password-like labels, and reveal toggle siblings). They are painted solid black before transmission; if redaction cannot be guaranteed, Mate fails closed.
- **Teacher, not an autonomous agent:** The human performs each action; Mate never clicks Pay, Delete, money transfer, or password buttons. Gated "Do it for me" assistance requires spoken voice confirmation and strictly enforces an action blocklist.
- **Safe advice:** The scam check evaluates the active window and never advises clicking suspicious links, calling numbers from the page, or sending payments.
- **Honest limits:** "I can't find it" beats pointing at a guess.
- **Key security:** Meta API credentials reside exclusively on the proxy backend; no secrets or private keys are shipped in the client binary.

## How it's built

MetaMate is built in **Rust + Tauri v2** with a TypeScript WebView (SVG character animations, multi-monitor highlight overlays, and Web Audio API engine).

### Hard problems

| Problem | How MetaMate solves it | Check it yourself |
|---|---|---|
| Overlay windows must never steal focus | Applies `WS_EX_NOACTIVATE`, `SWP_NOACTIVATE`, and `SW_SHOWNOACTIVATE` before showing or moving overlays; refuses windows where no-activate styles fail (`crates/mm-win32/src/overlay.rs`). | `metamate --s1-selftest` |
| Mixed-DPI multi-monitor screens and a per-frame coordinate transform | Uses Per-Monitor DPI v2 physical pixels, per-frame `FrameTransform` for downscaled screenshots, and `MonitorTransform` for WebView CSS drawing (`crates/mm-core/src/geom.rs`). | `metamate --s1-selftest` |
| Passwords, including one revealed with the eye button, must never leave the PC | Paints black `IsPassword`, password-like, and reveal-toggle sibling fields (`hidden_ids`); fails closed on truncated or unlocated fields (`crates/mm-core/src/redaction.rs`, `src-tauri/src/screen.rs`). | `metamate --s3-selftest` |
| Push-to-talk on bare Ctrl with no `RegisterHotKey` key-up and a release watchdog | Pumps `WH_KEYBOARD_LL` on a dedicated thread, requires 300 ms solo Ctrl hold, and polls `GetAsyncKeyState` as a release watchdog (`crates/mm-win32/src/keyboard.rs`, `crates/mm-core/src/ptt.rs`). | `metamate --ptt-probe` |
| Pointing accuracy: UI Automation element list versus screenshot only | Sends UI Automation element IDs with screenshots and resolves points to controls (`src-tauri/src/screen.rs`): 100% (610/610) with list vs. 33–46% without. | `mm probe point` |
| A lesson that verifies each click the person makes | Hooks mouse clicks after each step is shown, checks target bounds before advancing, and draws a miss trail (`src-tauri/src/teacher/lesson.rs`). | `./mm test` (`crates/mm-core/tests/clicks.rs`) |
| Streaming MateScript that keeps speech, pointing, and moods in step | Incrementally parses streaming `<point>`, `<mood>`, and `<sfx>` tags and binds them to each spoken sentence item (`crates/mm-core/src/matescript/parser.rs`). | `metamate --voice-selftest FILE.wav` |

```text
Voice:  Ctrl (WH_KEYBOARD_LL) → Muse Voice Transcribe (WSS) → Muse Spark 1.3 / Muse Wonder 1.0 → MateScript → MMS-TTS (Modal GPU)
Screen: Active window → Per-Monitor DPI v2 GDI capture → UI Automation + computer use → SAM 3.1 fallback → Beam & flight
```

- **MateScript:** A lightweight streaming protocol that coordinates spoken sentences with pointing tags (`<point eNN>`), character moods (`<mood calm|surprised|sad>`), and sound effects (`<sfx ping soft>`). See the [MateScript specification](docs/matescript.md).
- **Backend proxy:** A FastAPI service deployed on Modal manages daily allowance ledgers, streams model responses, and handles Google/GitHub OAuth with PKCE and Windows DPAPI credential sealing.

## FAQ for judges

**For judges: under the hood.** In Settings → "For judges and developers" → Developer mode, "Under the hood" shows each answer's model, timings from key-up, the parsed MateScript timeline, and how many fields were blacked out—never your words or the screenshot. The same menu offers "Mate's surprise entrance."

**Do I need an account?** No. You can try MetaMate right away with 12 messages an hour as a guest (the count shows under Mate); sign in with Google for your own daily allowance.

**What leaves my PC?** Your words, and a screenshot only when the question needs the screen: where something is, how to do something, whether a window is safe, or a question about a button, a link or the screen. Then only the window you are using is captured, with password fields painted black first; if that cannot be guaranteed, nothing is sent. Small talk and general questions send no screenshot, and nothing is captured or sent between questions (D65).

**Does Mate click for me?** Only when you ask. Mate lights up the button and guides you step by step; say "do it for me" and he asks "Shall I press it?" before each click and presses only after you say yes. He never presses payment, purchase, delete, money transfer or password buttons.

**Will Ctrl get in the way of my shortcuts?** Mate listens only after Ctrl is held alone for 300 ms, and any other key pressed with it (Ctrl+C, Ctrl+V) cancels quietly. You can also pick Right Alt, Caps Lock, Insert, F9 or F10 in Settings, or simply click Mate once to start and again to stop talking. You can also type in the chat window.

**Which Meta models?** Muse Voice Transcribe hears you while you talk, Muse Spark 1.3 answers and finds the button, Muse Wonder 1.0 plans a lesson's next step after two clicks away from the beam, SAM 3.1 outlines controls in apps without an accessibility tree, MMS-TTS (Meta open weights) speaks, and Muse Glimmer 30B with Muse Image make the keepsake lesson card.

**Languages?** English and Russian for speech, voice and the interface; it follows Windows, and you can switch it in Settings under Language.

**How do I quit?** Right-click Mate and choose Quit, or right-click the MetaMate icon in the notification area.

## Developer setup

### Prerequisites

- Windows 10/11 x64
- Rust 1.99+ with `rustup` and the `stable-msvc` toolchain
- Node.js 24

### Build from source

```powershell
npm install
npm run build
cargo build -p metamate --features custom-protocol
.\target\debug\metamate.exe
```

### Run it as a developer

Start the development proxy and MMS-TTS server in one terminal:

```powershell
python server/run_dev.py --tts        # or --tts-modal for Modal GPU TTS
```

In another terminal, build the WebView and launch the app:

```powershell
npm run build
cargo run -p metamate --features custom-protocol
```

Hold **Ctrl** (or your talk key from Settings) and speak. You can also test speech headless:

```powershell
cargo run -p metamate --features custom-protocol -- --ask "Where is the text size slider?"
```

### Self-tests and verification

- `metamate --s1-selftest`: Verifies overlay windows never take focus and Per-Monitor DPI v2 behavior.
- `metamate --s3-selftest`: Tests UI Automation walking and password redaction on a test fixture.
- `metamate --voice-selftest FILE.wav`: Runs end-to-end speech recognition, answer streaming, and TTS synthesis.
- `metamate --ptt-probe`: Monitors push-to-talk keyboard hook events.
- Project checks and tests: `./mm check` and `./mm test`.

### Repository map

| Path | Contents |
|---|---|
| [`docs/`](docs/) | Architecture, decisions log, MateScript spec, safety, and submission checklist |
| [`crates/`](crates/) | Rust core crates: `mm-core` (pure models/geometry), `mm-platform` (traits), `mm-win32` (OS integration) |
| [`src-tauri/`](src-tauri/) | Tauri shell: voice loop, screen capture, teacher state machine, and overlay management |
| [`src/`](src/) | Frontend WebView: Mate SVG robot, audio mixer, highlight canvas, settings, and Home window |
| [`server/`](server/) | FastAPI proxy backend for Meta Model API routing, allowances, and authentication |
| [`tts/`](tts/) | MMS-TTS service and Modal deployment scripts |
| [`tools/`](tools/) | Developer CLI (`mm`), project checks, phrase generation, and sound utilities |
| [`assets/`](assets/) | Robot artwork, audio cues, sound effects, and pre-recorded voice phrases |
| [`fixtures/`](fixtures/) | UI Automation replays and synthetic test fixtures |

### Prepared before the kickoff

The hackathon build runs from October 26 to November 4, 2026. Before the kickoff we prepared design documentation in [`docs/`](docs/), developer utilities in [`tools/`](tools/), concept art in [`assets/art/`](assets/art/) and sound cues in [`assets/sounds/`](assets/sounds/). We also fetched and compiled the expected dependencies and built an empty Tauri template to warm the caches ([`docs/setup-windows.md`](docs/setup-windows.md)), and wrote a throwaway local draft of the MateScript parser on October 24 that did not work and was not used ([`docs/plan.md`](docs/plan.md)). Nothing was committed before the kickoff: the first commit is from the kickoff night, and the application code was written during the hackathon (see [CHANGELOG.md](CHANGELOG.md)).

This repository keeps the project's working notes as they were written: the decision log, strategy and plans, and the instructions for the AI coding agents that helped build it.

### License

License: chosen before the repository goes public (see the Links). The MMS-TTS model weights are licensed **CC-BY-NC-4.0**; see [architecture notes](docs/architecture.md) for details.
