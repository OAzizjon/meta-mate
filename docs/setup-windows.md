# Development setup (Windows 11)

`./mm doctor` checks everything below and prints versions. Installed on the owner's laptop on 2026-10-24 with winget (the Build Tools UAC prompt was approved by the owner; nothing bypasses UAC).

## Required

| Tool | Why | Install |
|---|---|---|
| MSVC Build Tools 2022 + Windows SDK | Linker and Windows SDK for Rust/Tauri (`VCTools` workload) | `winget install --id Microsoft.VisualStudio.2022.BuildTools --override "--wait --passive --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"` (UAC) |
| Rust (rustup, stable-msvc) | Core and Tauri shell (1.99.1 installed) | `winget install --id Rustlang.Rustup` (not `--scope user`: winget finds no installer then; rustup installs per user anyway), then `rustup component add rust-analyzer` |
| Node.js 24 + npm | WebView build, Tauri CLI via `npx @tauri-apps/cli` | already installed |
| WebView2 runtime | Tauri's WebView on Windows | part of Windows 11 / Edge |
| Python 3.14 (`py`) | `mm` CLI and checks (standard library only) | already installed |
| git, gh | Repository, releases | already installed |

## Run the development stack

From the repository root, start the proxy and MMS-TTS server in a terminal:

```powershell
C:/mmw/py-proxy/Scripts/python.exe server/run_dev.py --tts        # or --tts-modal: MMS-TTS on a Modal GPU, after tts/deploy_modal.py
```

The proxy reads the configured Meta provider credentials, generates an invite for this run, and listens on `127.0.0.1:8787`. With `--tts`, it also starts MMS-TTS on `127.0.0.1:8788`. Leave this terminal running.

In a second terminal, build the WebView and launch MetaMate:

```powershell
npm run build
cargo run -p metamate --features custom-protocol
```

Hold **Right Ctrl** and speak. Right Ctrl must be held alone; pressing another key during the hold cancels push-to-talk.

For a microphone-free answer test, use `cargo run -p metamate --features custom-protocol -- --ask "question"`. It plays one answer and prints a JSON report with subscription, audio, answer, and playback details.

To exercise the full voice path from a WAV file, run `cargo run -p metamate --features custom-protocol -- --voice-selftest FILE.wav`. It prints a JSON report with timings and sentence results and writes each spoken sentence next to the input as `NAME.answer-N.wav` (NAME is the WAV without `.wav`).

To inspect push-to-talk without opening a window, run `cargo run -p metamate --features custom-protocol -- --ptt-probe`. It prints a prompt and each event with elapsed time, then exits after 60 seconds.

## Chosen helper tools (why each one earns its place)

| Tool | What we use it for | Install |
|---|---|---|
| **ffmpeg** | `mm sounds normalize` (loudness, format), cutting the demo video, converting Zoom recordings | `winget install --id Gyan.FFmpeg` (9.0.2 was already installed) |
| **uv** | Fast Python environments for the Modal TTS service and the proxy server (`uv venv`, `uv pip`) | `winget install --id astral-sh.uv` |
| **typos** | Spell checker for code and docs, run by `mm check` (also safe for agents: offline) | `winget install --id Crate-CI.Typos` |
| **sccache** | Compiler cache: Tauri's ~400 crates rebuild much faster across clean builds and branches (`RUSTC_WRAPPER=sccache`) | `winget install --id Mozilla.sccache` |
| **hyperfine** | Repeatable timing of the latency path (e.g. TTS round trip) for the latency budget | `winget install --id sharkdp.hyperfine` |
| **cargo-nextest** | Faster, clearer Rust test runner used by `mm test` when present | `cargo install cargo-nextest --locked` |
| Windows SDK **Inspect.exe** | Visual UI Automation inspector (comes with the SDK in Build Tools); `mm uia` is the text version for us and fixtures | with Build Tools |
| headless **Edge** | `mm art` renders the Mate art pages to PNG | part of Windows |
| **Modal CLI** (1.6.2) | Deploy the MMS-TTS service; `modal setup` (the owner's browser sign-in) is still to do | `uv tool install modal` (25.10) |
| **OBS Studio** | Recording the backup video (Day 7) and demo takes | `winget install --id OBSProject.OBSStudio` (25.10) |
| **cargo-insta** (1.49) | Reviewing snapshot changes of the MateScript golden tests (`insta`) | `cargo install cargo-insta --locked` (25.10) |
| **jq** | JSON in shell scripts (UIA fixtures, API replies) | `winget install --id jqlang.jq` (25.10; Git Bash wrapper in `~/bin`) |

Verified on 2026-10-24: a Tauri v2 template app (`npm create tauri-app`, vanilla-ts) builds in 2 min (debug, cold), MSVC linker and WebView2 included. Use sccache in the project with `.cargo/config.toml` → `[build] rustc-wrapper = "sccache"` (added with the workspace on day 1).

Considered and skipped for now: Accessibility Insights (not in winget; Inspect.exe + `mm uia` cover it), Tauri CLI via cargo (slow to compile; `npx @tauri-apps/cli` is enough).

## Cache warm-up before the kickoff (2026-10-25)

Done outside the repository, so no product code existed before the kickoff. It makes the Day-1 build fast and lets agents build with `--offline` (the crates are in `~/.cargo/registry`):

| Where | What |
|---|---|
| `C:\mmw\crates-warm` | A throwaway crate with the dependencies we expect, fetched and built through sccache (5.5 min cold; sccache now holds ~1 GiB) |
| `C:\mmw\tauri-warm` | `npm create tauri-app` (vanilla-ts), `npm install`, `tauri build --debug --no-bundle`: 4 min cold |
| `C:\mmw\py-proxy` | Python 3.12 venv: FastAPI 0.162, uvicorn 0.54, httpx 0.28, websockets 17.3, pydantic-settings 2.16 |
| `C:\mmw\py-tts` | Python 3.12 venv: torch 2.14 (CPU), transformers 5.20, uroman 1.3, scipy, numpy; `facebook/mms-tts-eng` and `facebook/mms-tts-rus` in `~/.cache/huggingface` (for local tests and the pre-recorded phrases) |
| `C:\ct\…` | Short Cargo target dirs (MAX_PATH) |

Versions `cargo add` resolved on 2026-10-25 (take them from `cargo add` again on Day 1, never from memory):

| Crate | Version | Note |
|---|---|---|
| tauri | 2.12.5 | `@tauri-apps/cli` 2.12.4. crates.io also lists `3.0.0-alpha`: stay on 2 (D34) |
| windows | 0.62.2 | the only `windows` version in the tree: tauri, tao, cpal, uiautomation and windows-capture all use it |
| uiautomation | 0.25.1 | |
| windows-capture | 2.0.1 | |
| cpal / rubato / ringbuf | 0.18.2 / 5.0.1 / 0.5.3 | |
| reqwest | 0.13.5 | the TLS feature is now `rustls` (not `rustls-tls`) |
| tokio / tokio-tungstenite | 1.54.1 / 0.30.0 | |
| image / rodio | 0.25.10 / 0.23.0 | |
| tauri-specta | 2.0.0-rc.26 | release candidates only: pin with `=` |
| tauri-plugin-single-instance / -log / -opener | 2.5.4 / 2.10.1 / 2.7.2 | |
| proptest / insta | 1.12.0 / 1.49.0 | dev-dependencies |

The Hugging Face cache warns that Windows symlinks need Developer Mode; it works without them, using more disk.

## Claude Code plugins (project scope)

rust-analyzer-lsp, typescript-lsp, Context7 (up-to-date library docs: Tauri v2, windows-rs, tauri-specta). Restart VS Code after Rust is installed so rust-analyzer starts.

## Modal (TTS service, day 2)

```powershell
uv tool install modal
modal setup        # opens a browser to sign in; the token goes to ~/.modal.toml, not into the project
```

## Demo machine (or this laptop)

- Windows 11, 125% or 150% scaling (Settings → Display): exercises DPI handling.
- A second monitor to the left of the primary one, if available: negative coordinates.
- Edge, Zoom, a clean desktop, dark and light themes (Mate must be visible on both).
- A separate Windows user "Demo", so personal windows and notifications never appear on screen; Focus assist on.
