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

## Pointing benchmark

| Benchmark command (lead only; requires Edge and Cargo) | Output |
|---|---|
| `mm probe point fixtures/eval/point [--models muse-spark-1.3,muse-wonder-1.0] [--elements on,off] [--repeat N] [--jobs K] [--max-edge 1280] [--reasoning EFFORT] [--budget USD] [--max-unpriced N] [--out DIR] [--dry-run] [--only GLOB]` | Validates each manifest element's inline absolute rectangle and target ids, renders cached 1280x800 PNG files, asks every question for each model/mode/repeat using the app request, and writes report.json/report.md (default point-report/). Defaults: both models/modes, 1 repeat, 4 workers, minimal reasoning, $10 ledger cap and 30 unpriced calls, sharing safety's reservations. First point only: HIT inside target, NEAR within 24 CSS px, otherwise WRONG or NO-POINT; absent targets score CORRECT/FALSE-POINT. Unknown ids and off-image coordinates count as wrong points. Rates exclude errors; tag/end median and p90, tokens and priced/unpriced ledger totals are grouped by model/mode/language. Dry run renders and prints one image-elided request without loading keys or calling HTTP. Agents use offline tests only. |

## Build the installer

On the Windows build machine, install the locked npm dependencies and build from the repository root:

```powershell
npm ci
.\mm release
```

`mm release` runs `mm check` first and refuses a dirty git tree, including untracked files. Use `mm release --allow-dirty` for an intentional local build; checks must still pass. `mm release --dry-run` prints the steps without running checks or build commands. Release is a lead-only command and is refused in agent mode, including dry runs.

After checks and git status, the command resolves Cargo's target directory with offline metadata, runs `npm run build`, then `npx tauri build --bundles nsis --target x86_64-pc-windows-msvc`. It overrides Tauri's `beforeBuildCommand` for this invocation because the frontend has already been built. It prints the produced installer's full path, size in bytes and SHA-256. A failed command or missing installer returns a non-zero exit code; an unchanged installer left by an earlier build does not count as a produced artifact.

The MetaMate setup executable is under `target/x86_64-pc-windows-msvc/release/bundle/nsis/` (or the equivalent in Cargo's configured target directory, including `CARGO_TARGET_DIR`). Product name and existing icons come from `src-tauri/tauri.conf.json`. The release build needs both `METAMATE_PROXY_URL` and `METAMATE_INVITE` at build time (`option_env!`; D51, `mm release` refuses without them), but the installed app never sends that invite: it starts on the guest trial and works out of the box for judges with zero configuration (D58). NSIS installs for the current user without administrator rights and offers English and Russian. It does not register Start with Windows; that setting remains store only. Audio autoplay is already configured in `overlay.rs`.

`offlineInstaller` embeds the full WebView2 runtime installer, increasing the download size substantially. Both downloaded and embedded bootstrappers still fetch the runtime from the internet, so neither supports a judge installing offline. The build machine needs network access to fetch bundler tools and the runtime unless already cached; installation needs no network. MetaMate's cloud answers still require network access.

Keep Tauri 2's standard NSIS template: its uninstaller offers an unchecked application-data deletion checkbox. Keeping the default preserves `%APPDATA%\app.metamate.desktop` (including `settings.json`); only explicitly checking the box should remove it. No custom uninstall hook should delete settings unconditionally.

Before handing the installer to judges, the lead must verify on a real build:

- Install as a standard user without UAC, on a clean x64 Windows profile with network disabled and no WebView2 runtime; launch successfully after runtime installation.
- Check both language choices in setup and uninstall, the existing app icons, and playback on first launch. Confirm no Startup entry or Run registry value is created, including after saving Start with Windows.
- Save settings, uninstall with the deletion box untouched, and reinstall: settings must survive. Repeat with the box checked: the app's config folder must be removed. Verify an upgrade preserves settings too. These checks must confirm the pinned CLI's standard-template checkbox behavior before release.
- Document the unsigned installer's SmartScreen steps (**More info** → **Run anyway**); then verify the zero-config judge path (holding Right Ctrl to ask with no account, keys, or invite codes required; optional sign-in in Settings with the providers the proxy enables).

## Judge path (Windows 10/11 x64)

MetaMate supports Windows 10 and 11 x64 only. The evaluation path is designed to be frictionless for judges (D51, D54):

1. **Download and installation**:
   - Download the release installer from `https://github.com/OAzizjon/meta-mate/releases/latest`.
   - Because the installer is not code-signed yet, Windows SmartScreen will display **"Windows protected your PC"**.
   - Click **More info**, then click **Run anyway**.
   - Complete setup (NSIS installs for the current user without administrator privileges or UAC elevation).

2. **Zero-configuration evaluation (no account, keys, or invite codes)**:
   - The release installer is built for our hosted proxy (`METAMATE_PROXY_URL` and `METAMATE_INVITE` embedded at build time per D51). The installed app never answers on the built-in invite (`invite_for_proxy` clears it): without an account it runs the guest trial, 12 messages an hour counted by the proxy (D58).
   - A judge installs, clicks **Try without signing in** in the sign-in window, holds **Right Ctrl**, and asks aloud—with no account, no API keys, and no invite codes required.
   - Live speech recognition: Muse Voice Transcribe opens its WebSocket at key-down and delivers the final transcript 250–360 ms after key-up, rendering live text in Mate's speech bubble.
   - Spoken answers and beam pointing: Mate responds aloud using MMS-TTS (90–110 ms GPU synthesis per sentence on a Modal T4), flies beside the target control in any app, and shines his beam on the right button.
   - Reasoning latency: Muse Spark 1.3 produces its first token in 1.4–2.6 s at minimal reasoning effort.

3. **Step-by-step teaching & model escalation**:
   - For "how do I..." requests, Mate guides the user step by step while the user clicks each step themselves (Mate never clicks Pay, Delete, or passwords).
   - If the person clicks away from the beam twice on Quick (`muse-spark-1.3`), Mate automatically hands the next step to Muse Wonder 1.0, then returns to Quick.
   - Keepsake lesson cards: when a lesson finishes, Muse Glimmer 30B (Meta open weights on a Modal L4 GPU, ~15 tokens/s) writes simplified steps and a scene description when its GPU answers; Muse Image renders an accompanying gouache picture without text. The card window displays the text and image with Print and Close buttons.

4. **"Is this safe?" scam check**:
   - Hold Right Ctrl and ask "Is this safe?" with any suspicious message or window open.
   - Mate inspects only the target window, gives a verdict with red flags, and shines his flashlight beam on the worst red flag. He never tells anyone to click, call, or pay.
   - Privacy guarantee: only the active target window is captured, passwords and non-target regions are painted black on the client, and inspection fails closed if safety cannot be guaranteed.

5. **Optional sign-in**:
   - Signing in is optional while the guest trial lasts (D54, D58); when its 12 answers for the hour are used up, the sign-in window opens.
   - If a judge or tester wants their own personal daily allowance, they can right-click Mate's tray icon, open **Settings**, and sign in. The sign-in window and Settings show only the providers the proxy enables (Google; GitHub once it is configured on the proxy).

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
