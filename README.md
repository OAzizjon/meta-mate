# MetaMate

Windows 10/11 x64 (the video can be watched anywhere).

## Try it in 60 seconds

1. **Download:** Get the release installer (built for our hosted proxy with a capped judge invite — no account, invite codes, or API keys needed).
2. **SmartScreen:** If SmartScreen shows "Windows protected your PC", click the small **More info** link, then **Run anyway** (the installer is not code-signed yet).
3. **Ask:** Press **Win+I** to open Windows Settings. Hold **Right Ctrl** while you speak, ask *"How do I make the text bigger?"*, then let go. Hold only **Right Ctrl**: pressing any other key cancels the question. Mate answers aloud with a live transcript in the bubble and shines his beam on the target.
4. **Sign-in (optional):** Right-click the tray icon > **Settings** > **Sign in with Google**: you get your own daily allowance of questions instead of the shared judges' one.

## Links

- [Download release](https://github.com/OAzizjon/meta-mate/releases/latest)
- Demo video: TBD before Nov 4
- [Decisions log](docs/decisions.md)
- [Architecture](docs/architecture.md)
- [Submission notes](docs/submission.md)
- [MateScript specification](docs/matescript.md)
- [Day-by-day roadmap](docs/plan.md)
- [Changelog](CHANGELOG.md)

## What Mate does

![Mate points right here](assets/art/meme-right-here.png)

MetaMate is Mate, a small light-bulb companion living on the Windows desktop designed to help older adults and anyone navigating unfamiliar interfaces. Hold **Right Ctrl** for push-to-talk voice in English or Russian, and Mate answers aloud with a live transcript in his bubble.

- **Home window:** An accessible starting hub (`home.html`) featuring an animated Right Ctrl keycap explaining push-to-talk, one-click "Try it" sample questions, a clear summary of actions Mate never takes, and a direct path to saved keepsake cards.
- **Scam check ("Is this safe?"):** Evaluates suspicious emails, pop-ups, and websites in the active window. Muse Spark 1.3 delivers an immediate verdict via Mate's expression (`<mood calm>`, `<mood surprised>`, or `<mood sad>`), explains the specific red flags detected (mismatched domains, artificial urgency, fake security warnings), and shines a beam on the worst one—without ever telling the user to click, call, or pay.
- **Mate's beam:** "Where is the button?" works live across multi-monitor setups: Mate flies directly beside the target control without covering it and projects a focused beam onto it.
- **Step-by-step lessons:** For "How do I..." requests, Mate acts as a patient teacher rather than an autonomous takeover. He guides one step at a time, verifies the person clicked each target before continuing, and automatically escalates to Muse Wonder 1.0 after two misses on Quick.
- **Keepsake lesson cards:** After completing a lesson, Mate produces a printable recap card saved in `Pictures\MetaMate` with steps rewritten into plain language by Muse Glimmer 30B and illustrated in a soft gouache style by Muse Image.

## Built with Meta

| Meta model | Role in MetaMate | Measured performance |
|---|---|---|
| **Muse Spark 1.3** | Spoken answers, pointing beam, step-by-step lessons, and "Is this safe?" scam check | First token in 1.4–2.6 s at reasoning effort `minimal` (D41, D45) |
| **Muse Wonder 1.0** | Thorough answers; lessons escalate to Wonder after two misses on Quick | High-reasoning model for complex guidance and fallback escalation (D56) |
| **Muse Voice Transcribe** | Realtime push-to-talk speech recognition (Right Ctrl) | Final transcript 250–360 ms after key-up (socket opens at key-down) (D44, D45) |
| **MMS-TTS** | Spoken voice output (Meta open weights, served on Modal GPU) | 90–110 ms GPU synthesis per sentence on a Modal T4 (D44, D53) |
| **Muse Glimmer 30B** | Keepsake lesson card text: rewrites lesson steps in simple words (Meta open weights on Modal GPU) | Q4_K_M ~15 tokens/s on a Modal L4 / A10G GPU (D53, D57) |
| **Muse Image** | Keepsake lesson card illustration (soft gouache art with Mate, without text; printable) | Generates custom scene illustration for completed lessons (D55) |
| **SAM 3.1** | Outlines UI targets where the accessibility tree has nothing (canvas, Qt apps, games) | Target segmentation masks and boundary guidance |

## Privacy and safety

- **Never steals focus:** Overlay windows use `WS_EX_NOACTIVATE` and `SWP_NOACTIVATE`, never taking focus away from the user's active application.
- **Target window only:** Captures exclusively the foreground window and its owned popups, only during an active request. Background windows and occluded gaps are painted black.
- **Automatic password redaction:** UI Automation identifies password fields (`IsPassword`, password-like labels, and reveal toggle siblings). They are painted solid black before transmission; if redaction cannot be guaranteed, Mate fails closed.
- **Teacher, not an autonomous agent:** The human performs each action; Mate never clicks Pay, Delete, money transfer, or password buttons. Gated "Do it for me" assistance requires spoken voice confirmation and strictly enforces an action blocklist.
- **Safe advice:** The scam check evaluates the active window and never advises clicking suspicious links, calling phone numbers, or sending payments.
- **Honest limits:** "I can't find it" beats pointing at a guess.
- **Key security:** Meta API credentials reside exclusively on the proxy backend; no secrets or private keys are shipped in the client binary.

## How it's built

MetaMate is built in **Rust + Tauri v2** with a TypeScript WebView (SVG character animations, multi-monitor highlight overlays, and Web Audio API engine).

```text
Voice:  Right Ctrl (WH_KEYBOARD_LL) → Muse Voice Transcribe (WSS) → Muse Spark / Wonder → MateScript → MMS-TTS (Modal GPU)
Screen: Active window → Per-Monitor DPI v2 GDI capture → UI Automation + computer use → SAM 3.1 fallback → Beam & flight
```

- **MateScript:** A lightweight streaming protocol that coordinates spoken sentences with pointing tags (`<point eNN>`), character moods (`<mood calm|surprised|sad>`), and sound effects (`<sfx ping soft>`). See the [MateScript specification](docs/matescript.md).
- **Backend proxy:** A FastAPI service deployed on Modal manages daily allowance ledgers, streams model responses, and handles Google/GitHub OAuth with PKCE and Windows DPAPI credential sealing.

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

Hold **Right Ctrl** and speak. You can also test speech headless:

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

The hackathon build runs from October 26 to November 4, 2026. Prior to kickoff, only non-product scaffolding was prepared in the open: design documentation in [`docs/`](docs/), developer utilities in [`tools/`](tools/), concept art in [`assets/art/`](assets/art/), and sound cues in [`assets/sounds/`](assets/sounds/). No application code existed before kickoff (see [CHANGELOG.md](CHANGELOG.md)).

### License

Project license: **TBD**. The MMS-TTS model weights are licensed **CC-BY-NC-4.0**; see [architecture notes](docs/architecture.md) for details.
