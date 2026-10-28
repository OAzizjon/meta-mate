# MetaMate

MetaMate is Mate, a small light-bulb helper on the Windows desktop: hold Right Ctrl or tap for hands-free voice, and he answers aloud, flies next to the right button, and teaches you step by step. In a lesson, say yes to let him click the verified target—one left click per yes, never Pay, Delete, or password actions.

- The voice loop works end to end: hold Right Ctrl, or tap, talk, and pause; Mate answers aloud.
- “Where is the button?” works live on Windows Settings, with Mate flying beside the target and a ring around it.
- Password fields are blacked out before anything is sent, and Mate fails closed when he cannot safely inspect a window.

## Try it

1. Hold **Right Ctrl** and ask aloud: “Where is the volume control?” or “How do I make the text bigger?”
2. Release the key. Mate answers and points to the control.
3. During a lesson, say “do it for me,” then answer “yes” to let him press one harmless button.
4. Right-click the tray icon to open **Settings**; try hands-free mode, **Quick**, or **Thorough**.

Meta models: Muse Spark 1.3, Muse Voice Transcribe, and MMS-TTS.

<!-- demo GIF here -->

![Mate concept art](assets/art/mate-sheet.png)

> **Project status:** Hackathon build in progress (October 26 - November 4, 2026). Windows only.

## What it does

![Mate points right here](assets/art/meme-right-here.png)

- Hold to talk or use hands-free mode: tap, talk, and pause.
- Mate points at the right button in any app, then waits for you to click—or, in a lesson, say yes for one verified left click.
- SAM 3.1 outlines targets in apps without an accessibility tree, including Qt apps such as Telegram Desktop and games.
- A tray icon opens Settings.
- Hear helpful sound cues, or use calm mode designed for older users.

## Why it is different

Mate is a **teacher, not an autonomous agent**: he guides one step at a time instead of taking over. He checks that you clicked the right target before moving on, and says when he cannot find something rather than guessing. Screen capture is limited to the active window and its popups, only for a request; password fields are blacked out before anything is sent. Provider keys stay out of the desktop app.

## How it works

```text
Voice: hold to talk → Muse Voice Transcribe → Muse Spark → MateScript → MMS-TTS
Screen: active window → UI Automation (UIA) + computer use → SAM 3.1 fallback
                                                    ↓
                                   Mate points; you click or say yes in a lesson
```

UIA grounds suggestions in app controls; computer use helps plan steps, and SAM 3.1 outlines targets when accessibility data is missing, including in Qt apps and games. In a lesson, Mate clicks the verified target only after a spoken yes, one left click per yes, and never Pay, Delete, or password actions.

## Safety and privacy

- Mate's windows never take focus away from your app.
- A screenshot is taken only during a question and includes only the app you are asking about and its popups. Password fields, gaps, and every other app's window are painted black before it leaves your computer; if this cannot be guaranteed, Mate fails closed.
- Mate never clicks Pay, Delete, or password actions.
- In a lesson, Mate clicks the verified target only after you say yes, one left click per yes.
- If Mate cannot find something, he says "I can't find it" rather than pointing at a guess.
- Provider keys live only on the proxy server, never in the app.

## Built with Meta

| Meta model | Role in MetaMate |
|---|---|
| Muse Spark 1.3 | Spoken answers, pointing, computer use in lessons |
| Muse Wonder 1.0 | Thorough answers; lessons escalate to it after two misses |
| Muse Voice Transcribe | Realtime speech |
| SAM 3.1 | Outlines where the accessibility tree has nothing |
| Muse Image | The lesson card |
| Muse Glimmer 30B | Open weights, served on Modal: quick routing |
| MMS-TTS | Meta open-weight voices, on Modal |

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
