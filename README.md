# MetaMate

**Ask out loud how to do something on your PC. Mate, a small light bulb on your desktop, answers aloud and shines his beam on the exact button so you can click it yourself. He presses a button only when you ask and say yes, and never Pay, Delete, money transfer or password buttons.** Made for older people and anyone lost in an unfamiliar app; built on seven Meta models (Muse Spark 1.3, Muse Wonder 1.0, Muse Voice Transcribe, SAM 3.1, MMS-TTS, Muse Image, Muse Glimmer 30B).

![Mate points right here](assets/art/meme-right-here.png)

Windows 10/11 x64.

## Try it in 60 seconds

1. **Download** the release installer — no API keys or invite codes needed.
2. **SmartScreen:** click **More info**, then **Run anyway** (not code-signed yet).
3. **Start:** **Try without signing in** (12 messages/hour, count shown under Mate) or **Sign in with Google** for your own daily allowance.
4. **Ask:** Press **Win+I**, hold **Right Ctrl**, ask *"How do I make the text bigger?"*, then let go. Or click Mate once to start speaking, again to stop (or just pause). Pressing any other key cancels. No Right Ctrl? Pick Right Alt, Caps Lock, Insert, F9 or F10 in **Settings** (Home window or tray icon).
5. **Or type:** Right-click the tray icon (or Mate) → **Chat with Mate**. Mate still points on screen.

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

MetaMate is Mate, a small light-bulb companion living on the Windows desktop designed to help older adults and anyone navigating unfamiliar interfaces. Hold **Right Ctrl** (or another talk key chosen in Settings) for push-to-talk voice in English or Russian, and Mate answers aloud with a live transcript in his bubble.

- **Home window:** An accessible starting hub (`home.html`) featuring an animated Right Ctrl keycap explaining push-to-talk, one-click "Try it" sample questions, a clear summary of actions Mate never takes, and a direct path to saved keepsake cards.
- **Chat window:** Type a question instead of speaking (`chat.html`): the answer appears as text after a "Mate is thinking..." bubble, and Mate still points on screen.
- **Scam check ("Is this safe?"):** Evaluates suspicious emails, pop-ups, and websites in the active window. Muse Spark 1.3 delivers an immediate verdict via Mate's expression (`<mood calm>`, `<mood surprised>`, or `<mood sad>`), explains the specific red flags detected (mismatched domains, artificial urgency, fake security warnings), and shines a beam on the worst one—without ever telling the user to click, call, or pay.
- **Mate's beam:** "Where is the button?" works live across multi-monitor setups: Mate flies directly beside the target control without covering it and projects a focused beam onto it.
- **Spotlight:** While Mate points, the rest of the screen dims around a soft opening on the target, so the button stands out even in a small video call window (`src/highlight.ts`; Settings: "Dim the rest of the screen while I point").
- **X-ray look:** While Mate thinks, faint outlines sweep top to bottom over the elements he is reading, and every field painted over for privacy shows as a dark tile with a lock, so you see what he looks at and what he never sees (`src-tauri/src/highlight.rs`, `src/highlight.ts`).
- **Step-by-step lessons:** For "How do I..." requests, Mate acts as a patient teacher rather than an autonomous takeover. He guides one step at a time, verifies the person clicked each target before continuing, and if the person clicks away from the beam twice on Quick, Muse Wonder 1.0 automatically plans the next step.
- **Warmer, warmer:** During a lesson step, the ring and Mate's glow brighten as the mouse nears the right button and hold steady once it is on it, guiding the hand before the click (`src-tauri/src/pointing.rs`).
- **A trail, not a scolding:** After a click in the wrong place during a lesson, a dotted line of light runs from that spot to the ring while Mate says "Almost"; no red, no cross (`src-tauri/src/teacher/lesson.rs`).
- **Drop Mate on it:** Drag Mate onto any button or icon and click him: he lights it up and explains what it does (`src-tauri/src/explain.rs`). Nothing leaves the PC until that click, and then only the control's name, its type and the app's name; password fields and sign-in codes are skipped.
- **Keepsake lesson cards:** After completing a lesson, Mate produces a printable recap card saved in `Pictures\MetaMate` with steps rewritten into plain language by Muse Glimmer 30B and illustrated in a soft gouache style by Muse Image.

## Built with Meta

| Meta model | Role in MetaMate | Measured performance |
|---|---|---|
| **Muse Spark 1.3** | Spoken answers, pointing beam, step-by-step lessons, and "Is this safe?" scam check | First token in 1.4–2.6 s at reasoning effort `minimal` (D41, D45) |
| **Muse Wonder 1.0** | Thorough answers; on Quick, a lesson's next step after two clicks away from the beam | High-reasoning model for complex guidance and fallback escalation (D56) |
| **Muse Voice Transcribe** | Realtime push-to-talk speech recognition (Right Ctrl) | Final transcript 250–360 ms after key-up (socket opens at key-down) (D44, D45) |
| **MMS-TTS** | Spoken voice output (Meta open weights, served on Modal GPU) | 90–110 ms GPU synthesis per sentence on a Modal T4 (D44, D53) |
| **Muse Glimmer 30B** | Keepsake lesson card text: rewrites lesson steps in simple words (Meta open weights on Modal GPU) | Q4_K_M ~15 tokens/s on a Modal L4 / A10G GPU (D53, D57) |
| **Muse Image** | Keepsake lesson card illustration (soft gouache art with Mate, without text; printable) | Generates custom scene illustration for completed lessons (D55) |
| **SAM 3.1** | Outlines UI targets where the accessibility tree has nothing (canvas, Qt apps, games) | Target segmentation masks and boundary guidance |

## Privacy and safety

- **Never steals focus:** Overlay windows use `WS_EX_NOACTIVATE` and `SWP_NOACTIVATE`, never taking focus away from the user's active application.
- **Target window only:** Captures exclusively the foreground window and its owned popups when a question needs the screen (where something is, how to do something, whether a window is safe, or a question about a button, a link or the screen). Background windows and occluded gaps are painted black.
- **Automatic password redaction:** UI Automation identifies password fields (`IsPassword`, password-like labels, and reveal toggle siblings). They are painted solid black before transmission; if redaction cannot be guaranteed, Mate fails closed.
- **Teacher, not an autonomous agent:** The human performs each action; Mate never clicks Pay, Delete, money transfer, or password buttons. Gated "Do it for me" assistance requires spoken voice confirmation and strictly enforces an action blocklist.
- **Safe advice:** The scam check evaluates the active window and never advises clicking suspicious links, calling phone numbers, or sending payments.
- **Honest limits:** "I can't find it" beats pointing at a guess.
- **Key security:** Meta API credentials reside exclusively on the proxy backend; no secrets or private keys are shipped in the client binary.

## How it's built

MetaMate is built in **Rust + Tauri v2** with a TypeScript WebView (SVG character animations, multi-monitor highlight overlays, and Web Audio API engine).

```text
Voice:  Right Ctrl (WH_KEYBOARD_LL) → Muse Voice Transcribe (WSS) → Muse Spark 1.3 / Muse Wonder 1.0 → MateScript → MMS-TTS (Modal GPU)
Screen: Active window → Per-Monitor DPI v2 GDI capture → UI Automation + computer use → SAM 3.1 fallback → Beam & flight
```

- **MateScript:** A lightweight streaming protocol that coordinates spoken sentences with pointing tags (`<point eNN>`), character moods (`<mood calm|surprised|sad>`), and sound effects (`<sfx ping soft>`). See the [MateScript specification](docs/matescript.md).
- **Backend proxy:** A FastAPI service deployed on Modal manages daily allowance ledgers, streams model responses, and handles Google/GitHub OAuth with PKCE and Windows DPAPI credential sealing.

## FAQ for judges

**Do I need an account?** No. You can try MetaMate right away with 12 messages an hour as a guest (the count shows under Mate); sign in with Google for your own daily allowance.

**What leaves my PC?** Your words, and a screenshot only when the question needs the screen: where something is, how to do something, whether a window is safe, or a question about a button, a link or the screen. Then only the window you are using is captured, with password fields painted black first; if that cannot be guaranteed, nothing is sent. Small talk and general questions send no screenshot, and nothing is captured or sent between questions (D65).

**Does Mate click for me?** Only when you ask. Mate lights up the button and guides you step by step; say "do it for me" and he asks "Shall I press it?" before each click and presses only after you say yes. He never presses payment, purchase, delete, money transfer or password buttons.

**No Right Ctrl on my laptop?** Pick Right Alt, Caps Lock, Insert, F9 or F10 in Settings, or simply click Mate to start and stop talking. You can also type in the chat window.

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

Hold **Right Ctrl** (or your talk key from Settings) and speak. You can also test speech headless:

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
