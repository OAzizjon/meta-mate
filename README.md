# MetaMate

Windows 10/11 x64 (the video can be watched anywhere).

## Try it in 60 seconds

1. **Download:** Get the release installer (built for our hosted proxy with a capped judge invite — no account, invite codes, or API keys needed).
2. **SmartScreen:** The installer is not code-signed yet. When Windows SmartScreen displays *"Windows protected your PC"*, click **More info**, then **Run anyway**.
3. **Ask:** Open Settings, hold **Right Ctrl**, ask aloud *"How do I make the text bigger?"*, and release. Mate answers aloud with a live transcript in the bubble and shines his beam on the target.
4. **Sign-in (optional):** Right-click the tray icon to open **Settings**. Google sign-in is completely optional (GitHub sign-in is not in the judges' build yet) and gives you your own daily allowance.

MetaMate is Mate, a small light-bulb helper on the Windows desktop: hold Right Ctrl for push-to-talk voice, and he answers aloud, flies next to the right button, and teaches you step by step.

- **Push-to-talk voice:** Hold Right Ctrl, speak, and release; Mate answers aloud with a live transcript in the bubble.
- **Mate's beam:** "Where is the button?" works live in any app—Mate flies beside the target and shines his beam right on it.
- **Step-by-step lessons:** For "how do I..." requests, Mate guides you step by step where you click each target yourself (escalating to Muse Wonder after two misses on Quick).
- **"Is this safe?" scam check:** Gives an instant verdict, lists red flags, and shines a beam on the worst one—never telling anyone to click, call, or pay.
- **Keepsake lesson card:** Muse Glimmer words (when its GPU answers) and a Muse Image picture (without text) that you can print.
- **Privacy by design:** Only captures the target window during a request, paints password fields black before sending, and fails closed.

<!-- demo GIF here -->

![Mate concept art](assets/art/mate-sheet.png)

> **Project status:** Hackathon build in progress (October 26 - November 4, 2026). Windows only.

## What it does

![Mate points right here](assets/art/meme-right-here.png)

- Hold **Right Ctrl** for push-to-talk voice with a live transcript in the bubble (or use hands-free mode).
- Mate points his beam at the right button in any app, and guides you step by step in lessons where you click each target (escalating to Muse Wonder after two misses on Quick).
- SAM 3.1 outlines targets in apps without an accessibility tree, including Qt apps such as Telegram Desktop and games.
- "Is this safe?" scam check: inspects the active window, delivers a verdict, highlights red flags, and shines a beam on the worst one—never telling anyone to click, call, or pay.
- Keepsake lesson card: Muse Glimmer writes the steps in simple words (when its GPU answers) and Muse Image paints a soft gouache picture without text, ready to Print.
- A tray icon opens Settings (optional Google sign-in for individual allowances, with GitHub sign-in not in the judges' build yet; model selection between Quick and Thorough).
- Hear helpful sound cues, or use calm mode designed for older users.

## Why it is different

Mate is a **teacher, not an autonomous agent**: he guides one step at a time instead of taking over. He checks that you clicked the right target before moving on, and says when he cannot find something rather than guessing. Screen capture is limited to the active window and its popups, only for a request; password fields are blacked out before anything is sent. Provider keys stay out of the desktop app.

## How it works

```text
Voice: hold to talk → Muse Voice Transcribe → Muse Spark → MateScript → MMS-TTS
Screen: active window → UI Automation (UIA) + computer use → SAM 3.1 fallback
                                                    ↓
                                   Mate points; you click each step in a lesson
```

UIA grounds suggestions in app controls; computer use helps plan steps, and SAM 3.1 outlines targets when accessibility data is missing, including in Qt apps and games. In a lesson, the person clicks each verified target themselves (Mate escalates to Muse Wonder after two misses on Quick); Mate never touches Pay, Delete, or password actions.

## Safety and privacy

- Mate's windows never take focus away from your app.
- A screenshot is taken only during a question and includes only the app you are asking about and its popups. Password fields, gaps, and every other app's window are painted black before it leaves your computer; if this cannot be guaranteed, Mate fails closed.
- Mate never clicks Pay, Delete, or password actions.
- In a lesson, the person clicks each verified target themselves.
- "Is this safe?" scam check evaluates only the active window and never tells anyone to click, call, or pay.
- If Mate cannot find something, he says "I can't find it" rather than pointing at a guess.
- Provider keys live only on the proxy server, never in the app.

## Built with Meta

| Meta model | Role in MetaMate | Measured performance |
|---|---|---|
| **Muse Spark 1.3** | Spoken answers, pointing beam, step-by-step lessons, and "Is this safe?" scam check | First token in 1.4–2.6 s at reasoning effort `minimal` (D41, D45) |
| **Muse Wonder 1.0** | Thorough answers; lessons escalate to Wonder after two misses on Quick | High-reasoning model for complex guidance and fallback escalation |
| **Muse Voice Transcribe** | Realtime push-to-talk speech recognition (Right Ctrl) | Final transcript 250–360 ms after key-up (socket opens at key-down) (D44, D45) |
| **MMS-TTS** | Spoken voice output (Meta open weights, served on Modal) | 90–110 ms GPU synthesis per sentence on a Modal T4 (D44, D53) |
| **Muse Glimmer 30B** | Keepsake lesson card text: rewrites lesson steps in simple words (Meta open weights on Modal) | Q4_K_M ~15 tokens/s on an L4 GPU (D53) |
| **Muse Image** | Keepsake lesson card illustration (soft gouache art with Mate, without text; printable) | Generates custom scene illustration for completed lessons (D55) |
| **SAM 3.1** | Outlines UI targets where the accessibility tree has nothing (canvas, Qt apps, games) | Target segmentation masks and boundary guidance |

## MateScript in 10 lines

MateScript is a small streaming format for speech, pointing, mood, and sound cues. [Read the draft specification](docs/matescript.md).

```text
<lang en>
<mood calm>
I can show you how to make the text bigger.
<step 1/2 e3>
Open Accessibility.
<wait click>
<step 2/2 e8>
Choose Text size.
<wait click>
<sfx success soft><mood proud>That's it. Well done!
```

## Repository map

| Path | Contents |
|---|---|
| [`docs/`](docs/) | Strategy, architecture, plan, MateScript, safety, and design |
| [`tools/`](tools/) | Project checks, tests, and developer utilities |
| [`assets/art/`](assets/art/) | Mate concepts and artwork |
| `crates/`, `src-tauri/`, `src/` | Planned Rust core, Tauri shell, and TypeScript interface |
| `server/`, `tts/` | Planned API proxy and MMS-TTS service |

The `mm` helper is available from Git Bash as `./mm <command>` or PowerShell as `.\mm <command>`. Common commands:

| Command | Purpose |
|---|---|
| `mm doctor` | Check development tools |
| `mm status` | Show project and schedule status |
| `mm check` | Run fast project checks |
| `mm test` | Run available unit tests |

## Build from source (Windows)

1. Install Rust 1.99+ with rustup and the `stable-msvc` toolchain, and install Node.js 24.
2. Run `npm install`.
3. Run `npm run build`.
4. Run `cargo build -p metamate --features custom-protocol`.
5. Run `target/debug/metamate.exe`.

Self-tests of the Windows layer: `metamate --s1-selftest` (overlay windows never take focus, DPI per monitor) and `metamate --s3-selftest` (UI Automation and password detection on a test page).

Checks and tests: `./mm check` and `./mm test`.

## Run it as a developer

Start the development proxy and MMS-TTS server in one terminal:

```powershell
C:/mmw/py-proxy/Scripts/python.exe server/run_dev.py --tts        # or --tts-modal: MMS-TTS on a Modal GPU, after tts/deploy_modal.py
```

The proxy reads the configured Meta provider credentials, creates a development invite for the app, and listens on `127.0.0.1:8787`; with `--tts`, the MMS-TTS server listens on `127.0.0.1:8788`.

In another terminal, build the WebView and run the app:

```powershell
npm run build
cargo run -p metamate --features custom-protocol
```

Hold **Right Ctrl** and speak. To test the same answer path without the microphone, run `cargo run -p metamate --features custom-protocol -- --ask "question"`; it plays the answer and prints a JSON report with subscription, audio, answer, and playback details.

The voice self-test streams a WAV file through speech recognition, the answer path, and speech synthesis:

```powershell
cargo run -p metamate --features custom-protocol -- --voice-selftest FILE.wav
```

It prints a JSON report with timings and sentence results, and writes each spoken sentence next to the input as `NAME.answer-N.wav` (NAME is the WAV without `.wav`). To inspect push-to-talk events without opening a window, run `cargo run -p metamate --features custom-protocol -- --ptt-probe`; it prints a prompt and each event with its elapsed time, then exits after 60 seconds.

## Status, roadmap, and privacy

The hackathon build started at kickoff on October 26, 2026 and ends November 4, 2026. See the [day-by-day roadmap](docs/plan.md).

Privacy principles: inspect only the active window and its popups, and only during a request; black out password fields before sending a screenshot; keep provider keys out of the app. The proxy is designed not to store request or response content.

## Prepared before the kickoff

The hackathon build runs from October 26 to November 4, 2026. Before the kickoff we prepared only non-product material, in the open: the design documents in `docs/`, the developer tooling in `tools/` (project CLI, checks, UI Automation dump, sound synthesis and licence-checked sound fetching), the concept art in `assets/art/` and the synthesized cue sounds in `assets/sounds/`. No application code existed before the kickoff; the git history starts after it.

See [CHANGELOG.md](CHANGELOG.md) for the pre-kickoff preparation record.

## License

Project license: **TBD**. The MMS-TTS model weights are licensed **CC-BY-NC-4.0**; see the [architecture notes](docs/architecture.md) for details.
