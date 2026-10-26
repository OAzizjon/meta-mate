# MetaMate architecture (draft v0.2, 2026-10-24)

Status: design only. No product code exists yet; the build starts at the hackathon kickoff (2026-10-26).
Items marked **[spike]** must be proven early; **[verify]** depends on facts to confirm at kickoff.
v0.2 folds in the reviews of Codex (#233) and Gemini (#234).

## 1. Big picture

```
┌──────────────────────────── Windows PC ────────────────────────────┐
│                                                                    │
│  Tauri app (one process)                                           │
│  ┌──────────────── Rust core ─────────────────┐  ┌── WebView ───┐  │
│  │ hotkey   (WH_KEYBOARD_LL)  push-to-talk    │  │ robot (SVG)  │  │
│  │ mouse    (WH_MOUSE_LL)     step checks     │  │ highlight    │  │
│  │ mic      (cpal)            → ASR stream    │  │ Web Audio    │  │
│  │ uia      (UI Automation)   element tree    │◄►│  mixer       │  │
│  │ capture  (Graphics.Capture) target region  │  │ transcript   │  │
│  │ teacher  (computer-use loop, step state)   │  │ settings     │  │
│  │ matescript (streaming parser → timeline)   │  │ debug view   │  │
│  │ act      (SendInput, "do it for me")       │  └──────────────┘  │
│  │ net      (HTTPS/SSE/WS to the proxy)       │                    │
│  └────────────────────────────────────────────┘                    │
└───────────────────────────────┬────────────────────────────────────┘
                                │ HTTPS + WSS, invite code
                    ┌───────────▼───────────┐
                    │  MetaMate proxy (VPS) │  keys, quotas, no content logging
                    └───┬───────────┬───────┘
          Meta Model API│           │Modal
  (Muse Spark / Wonder, │           │ MMS-TTS (Meta open weights)
   Voice Transcribe,    │           │
   SAM 3.1, Muse Image) │           │
```

The desktop app never ships with our provider keys. It holds an invite code, or optionally a key the user enters themselves (their own Meta key, stored with Windows DPAPI).

## 2. Windows layer

| Module | Responsibility | Library / API | Notes |
|---|---|---|---|
| `hotkey` | Push-to-talk while any app is focused | `windows` crate, `SetWindowsHookEx(WH_KEYBOARD_LL)` on its own thread with a message pump | The callback only enqueues `{key, down/up, time, injected}` and returns (slow hooks are silently removed by Windows). **Right Ctrl is not suppressed**: PTT starts when Right Ctrl is held alone for 250 ms; any other key during the hold cancels PTT (it was a shortcut). Ignore auto-repeat and injected events; recover from a missed key-up via `GetAsyncKeyState` polling. Modes: **hold** (default), **tap to start, auto-stop on silence** (VAD/endpointing; recommended for the elderly tester), press-and-hold on the robot. `RegisterHotKey` cannot report key-up. |
| `mouse` | User clicks for step verification | `WH_MOUSE_LL`, same thread rules | Position + button only. |
| `mic` | Capture while PTT is active | `cpal` (WASAPI shared mode) | Take the device's native format (often 48 kHz float stereo), downmix + resample to 16 kHz PCM16 **off the audio callback thread**. No pre-roll (that would need an always-open mic, which Windows shows as "in use"); the `listen` blip plays when the stream is actually open, telling the user to speak. |
| `uia` | Snapshot of the target: id, control type, name, automation id, help text, rect, enabled, offscreen, is-password; `ElementFromPoint`; events | `uiautomation` crate or raw `windows` COM; dedicated MTA worker; `IUIAutomation2` connection/transaction timeouts set | A Rust-side deadline does not cancel a hung provider call: if the worker is stuck > 2 s, abandon it and start a fresh worker (move to a helper process if this happens in practice). Bounded walk (~400 nodes). Exclude our own HWNDs from every result. Chromium's tree may be poor on the first query **[spike S3]**. |
| `capture` | Image of the **target region**: the foreground window's visible frame **plus its owned popups** (menus and dropdowns are separate windows) | Windows.Graphics.Capture on the monitor, cropped to the union of the window's `DWMWA_EXTENDED_FRAME_BOUNDS` and its visible owned popups; `BitBlt` fallback only with a timeout, and a capture is rejected (not silently accepted) if it is black or incomplete | Our own windows must not appear in the image: hide the robot and highlight for the capture frame (opacity 0 for ~1 frame). **Do not** keep `WDA_EXCLUDEFROMCAPTURE` on our windows, or Zoom/OBS would not show the robot in the demo. **[spike]** |
| `overlay` | Robot window + highlight overlay | Tauri windows created with `transparent: true`, `decorations: false`, `focus: false`, `skip_taskbar: true`, `always_on_top: true`, transparent WebView background and CSS; then extended styles added on the HWND **keeping existing bits** | Robot: `WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW`, draggable, shown/moved only with `SWP_NOACTIVATE`. Highlight: one window per monitor, `set_ignore_cursor_events(true)` + `WS_EX_TRANSPARENT | WS_EX_LAYERED | WS_EX_NOACTIVATE`; verify that clicks reach another process. Unique window labels; **one** WebView owns audio. Per-Monitor DPI Aware v2 in the manifest; handle `WM_DPICHANGED` and display changes. **[spike S1]** If WebView2 transparency misbehaves, the highlight can be a native Win32 layered window (Direct2D) while the robot stays in the WebView. |
| `act` | "Do it for me": one action after confirmation | `SendInput` | Before injecting: the foreground window must still be the target (HWND + process), the element under the point must still be the target; UIPI blocks elevated windows. Blocklist: pay, buy, delete, send money, password fields, any `pending_safety_checks`. |

### Coordinates: one transform per captured frame

Each frame stores `{capture_origin_px, crop_rect_px, sent_width, sent_height, scale_x, scale_y}` (all physical virtual-screen pixels; negative values allowed).
- Model point `(sx, sy)` → screen: `(crop.left + sx / scale_x, crop.top + sy / scale_y)`.
- Candidate element rects are sent **in the same screenshot space** (transformed with the same frame), so ids and raw points agree.
- `ElementFromPoint` takes physical screen coordinates.
- Drawing: `css = (screen_point − overlay_client_origin_px) / overlay_monitor_scale` (also divide by WebView zoom if it is not 100%).

## 3. Pointing ("teacher") pipeline

Two paths, chosen by a short intent call (`WHERE` / `HOW` / `CHAT` / `DO`) that always runs on `muse-spark-1.3` with `reasoning.effort: "minimal"`. For the main teacher call, Settings ("How Mate thinks", D40) lets the user pick **Quick** (`muse-spark-1.3`, default) or **Thorough** (`muse-wonder-1.0`); the model is never switched by voice or by a MateScript tag. Every Spark model reasons before answering (`reasoning.effort` accepts `minimal`, `low`, `medium`, `high`, `xhigh`, `max`, not `none`, D41).

**A. "Where is X?" (one target):**
1. `uia` snapshot + `capture` of the target region (same frame transform).
2. `muse-spark-1.3` (or `muse-wonder-1.0` if chosen in Settings; Responses API, streaming, `reasoning.effort: "minimal"`) gets: request, screenshot, compact element list (`e17 Button "Text size" [x,y,w,h]`, max ~150 interactive elements, screenshot coordinates).
3. The answer is MateScript with `<point e17>` (or `<point 812,344>` if the element is not in the list).
4. Point → rect: id → rect; raw point → `ElementFromPoint` (own windows excluded, must lie inside the target window or its popups) → rect; still nothing → **SAM 3.1** with that point refines the outline. SAM refines a grounded target; it does not prove it is the right one. If the element's name/role clearly contradicts the request, don't point.
5. Robot flies next to the rect (never on it), points; highlight draws the contour; tracked via `EVENT_OBJECT_LOCATIONCHANGE`.

**B. "How do I …?" (multi-step): computer use with the human as the hand.**
1. Responses API, `tools: [{"type": "computer"}]`, `muse-spark-1.3` by default or the user's `muse-wonder-1.0` choice; `reasoning.effort: "minimal"` by default (it hit the S4 fixture target `e11 "Large"` in 5.3 s with 3702 input / 356 output tokens; `low` hit it too in 6.9 s, D41), `low` when retrying a step that failed. Manual Wonder never escalates, and automatic escalation from Spark to Wonder stays off unless the Day-5 gate turns it on (D39, D40). First screenshot as `input_image`. The screenshot size stays fixed for the whole task.
2. Each computer-use turn returns a `reasoning` item, a `message` with `phase: "commentary"` (the spoken line Mate says), and a `computer_call` containing an `actions` array (e.g. `{"type": "click", "button": "left", "x", "y"}`), a `meta_safety_replay_receipt`, and `pending_safety_checks` (`computer_action_requires_confirmation`) **on every click** (D41). **We do not execute the actions.** The prompt asks for one action per call; a longer `actions` array becomes an ordered list of **sub-steps**, each with its own completion rule:

| Action | Shown as | Completed when |
|---|---|---|
| `click {x,y}` (`button: "left"`) | robot points at the snapped element | mouse hook click inside the rect (± 8 px), plus UIA invoke/selection event if available |
| `double_click` | points, "double-click" bubble | two clicks inside the rect within the double-click time |
| `type {text}` | bubble with the text to type | the focused element's value contains the text (UIA ValuePattern), or the user says "done"/"next" |
| `keypress {keys}` | keycaps bubble | keyboard hook sees that chord |
| `scroll` | robot gestures up/down | a scroll event over the target area, then the next screenshot shows the change |
| `drag` | animated path | mouse up near the end point |
| `wait` / `screenshot` / `move` | nothing to do | immediately |

3. After the last sub-step: 600 ms settle → new capture (same frame size) → `computer_call_output` (with the acknowledged safety check; whether `meta_safety_replay_receipt` must be passed back too: **[verify]** on the second S4 turn). If the screen changed unexpectedly mid-batch (a dialog, an error), we stop the batch and send the screenshot at once, so the model replans.
4. Wrong click (> 40 px outside): no advance; robot says the `wrong_place` line from copy.md ("Almost. Click right where I'm pointing.") and re-points.
5. "Do it for me": `act` performs the current sub-step after explicit human confirmation, unless blocked.
6. Because the API attaches `pending_safety_checks` (`computer_action_requires_confirmation`) to every click (D41), the human's own click on the pointed target is what confirms the check in teaching mode (and the human's confirmation prompt answers it in "Do it for me"), so invariant 4 costs no extra UI step; checks are never auto-acknowledged without a human action.
7. Budget: max 12 model turns per task, then stop and say so.
8. Spoken line per step (D21, confirmed by S4 in D41): the API returns a `message` item with `phase: "commentary"` alongside `computer_call`, which is the line Mate speaks. A fast parallel `muse-spark-1.3` (`reasoning.effort: "minimal"`) call is used only if `commentary` is absent, while a **pre-cached phrase** ("Step 2 of 3", "Look here") plays immediately while the turn runs.

**Refusal rule:** if the target cannot be validated, Mate says he can't find it. Pointing at the wrong place is worse than saying "I can't see it".

## 4. Voice pipeline

```
PTT start ─► robot "listening" ─► mic PCM ─► proxy WSS ─► Muse Voice Transcribe (realtime)
PTT end   ─► finalize transcript (VAD/endpoint) ─► intent ─► Muse Spark 1.3 / Wonder (streaming, effort: minimal)
          ─► MateScript parser ─► one ordered playback timeline
say(sentence) ─► proxy ─► Modal MMS-TTS (voice from <lang>) ─► PCM frames ─► speech bus
```

- Sentence-level TTS; sentence N+1 is synthesized while N plays.
- **Pre-cached phrases**: ~20 frequent lines per language (greeting, "One moment", "Look here", "Well done", "Almost…", "Step N of M", errors) synthesized once with MMS-TTS and shipped as audio files → the robot reacts at once while the real answer is generated.
- ASR fallback: if the WSS path through the proxy adds > 500 ms or fails, send the recorded clip to the file endpoint (`POST /v1/asr/transcribe`) on PTT end.
- Language: the model starts each answer with `<lang ru>`; the proxy maps it to an MMS checkpoint. **[verify]** which of the 25 Muse Voice Transcribe languages have MMS-TTS checkpoints.
- **Reasoning effort and measured S4 first-token latency (D41):** every Spark model reasons before answering. At default effort, a two-sentence answer took **7–10 s** to first token (`muse-spark-1.3`: **9.9 s**, 1703 output tokens). Setting `reasoning.effort: "minimal"` cuts first-token time to **1.9 s** (131 output tokens; `low`: **3.2 s**); a full computer-use turn on the S4 fixture took **5.3 s** at `minimal` (vs **6.9 s** at `low`). All spoken and intent calls therefore force `reasoning.effort: "minimal"`.
- **Latency budget** (PTT end → first audio of the real answer), measured warm and cold, p50/p95: endpoint/finalize 0.3 s + intent 0.3 s (can run in parallel with capture + UIA 0.3 s) + model first sentence ~1.9 s at `minimal` (target 0.8–1.9 s) + TTS 0.3–0.5 s + network/IPC/playback start 0.2 s ≈ **2.5–3.0 s** (was 2.0–2.5 s before S4). Pre-cached "One moment" covers the wait. A late fallback TTS result is discarded once the primary arrives (generation ids).
- Fallback voice: Windows WinRT `SpeechSynthesizer` if Modal is slow (> 1.5 s) or down; switch only at a sentence boundary.
- Barge-in: starting PTT while the robot speaks cancels the current generation id: speech, queued TTS and sfx stop at once.

## 5. Audio path and mixer

- Transport: Rust sends PCM16 frames (with generation id and sentence id) to the audio-owner WebView over a Tauri channel; an **AudioWorklet** ring buffer plays them; bounded queue, resampling to the context rate, cancellation by generation id. SFX/ambience files are preloaded and decoded once.
- Autoplay: native PTT is not a DOM gesture, so the WebView2 is started with `--autoplay-policy=no-user-gesture-required` (Tauri additional browser args) and the AudioContext is resumed on startup **[spike S7]**. Fallback if this is unreliable: play all audio in Rust (`rodio` + `symphonia`) and keep the WebView silent.
- Buses → master with a limiter: **speech**, **sfx**, **ambience** (looping, 1.5 s crossfade). Ducking while speech plays: ambience −12 dB, sfx −6 dB (300 ms attack, 600 ms release).
- Volume classes: `soft`, `normal`, `loud`; **calm mode** caps at `soft`, blocks `calm: no` sounds, max 2 sfx per answer.
- Spatial hint: the `ping` is panned (StereoPanner) toward the target's horizontal position (clamped to ±30 %).

## 6. MateScript

The model's answer format, spec: [matescript.md](matescript.md). The parser (Rust, unit- and fuzz-tested) produces **one ordered timeline**; the player consumes it in order, so `wait`, `pause` and ambience changes happen at their position in the answer, not at parse time. A **debug view** shows the raw stream with highlighted tags.

## 7. Proxy (server)

Small HTTPS service on the VPS (FastAPI preferred; upstream `https://api.meta.ai/v1`, D41).
- `POST /v1/responses` → Meta Responses API:
  - **Model allowlist** (D40, D41): `muse-spark-1.3` (*Quick*, default) and `muse-wonder-1.0` (*Thorough*), plus `sam-3.1`; never switched by voice or MateScript tags.
  - **Server-side `max_output_tokens` caps** per call type (prevents runaway reasoning token spend such as the 1703 output tokens seen at default effort in S4, D40, D41).
  - **Wonder allowance & quiet fallback** (D40): inside `PROXY_DAILY_BUDGET_USD`, each invite has a daily `muse-wonder-1.0` allowance (calls and dollars per day), while a budget reserve is kept exclusively for `muse-spark-1.3`. When an invite's Wonder allowance or the global Wonder pool runs out, or if Wonder fails, the proxy falls back quietly to `muse-spark-1.3` (the response reports which model answered so the debug view shows it).
- `WSS /v1/asr` → Muse Voice Transcribe (`muse-voice-transcribe-1.0`) realtime; `POST /v1/asr/transcribe` → file fallback.
- `POST /v1/tts` → Modal MMS-TTS (shared secret).
- Auth: invite code; per-code daily quota; global daily budget (`PROXY_DAILY_BUDGET_USD`).
- Privacy: no request/response bodies stored; only counters (code, model, tokens, time).
- Landing page + downloads (GitHub Releases) on the same host.

## 8. TTS service (Modal)

- Python, `transformers` `VitsModel` + tokenizer for `facebook/mms-tts-<iso>`; preload eng, rus + a few common languages; others lazily.
- CPU container first (36M params per language) **[spike S6]**; GPU only if CPU latency > 400 ms per sentence.
- One warm container on demo and judging days; scale to zero otherwise.
- Some languages need romanization (`uroman`) before tokenization **[verify]**.
- Text normalization before synthesis: numbers → words, common abbreviations, strip emoji/markup.
- Output: PCM16 at the model's sampling rate; the client resamples.
- License: MMS-TTS weights are **CC-BY-NC 4.0** (fine for the hackathon; a commercial version needs another voice).

## 9. Privacy and safety rules (invariants)

1. Capture only the target region (foreground window + its popups), only during a request, with a "looking" indicator.
2. Password fields are painted black before anything leaves the PC. A field counts as a password field when UIA says `IsPassword` **or** when it is a revealed one: after "show password" (the eye button) Chromium turns the input into plain text and `IsPassword` becomes false, so we also use the field's label / automation id (password words in many languages), a show/hide-password toggle next to it, and fields that were password fields earlier in the same session (D37). **Fail closed:** if redaction can't be established (UIA tree empty or timed out, geometry changed between the UIA query and the capture), the robot asks first ("May I look at this window?") and can show a thumbnail of what will be sent. Card numbers and chat text are not detected automatically: the user is told that the active window is sent.
3. Banking/password windows (title/URL heuristics) always ask first.
4. Never auto-click payment, purchase, delete, send-money or password actions; never auto-acknowledge safety checks; re-check the foreground window before any injected input.
5. No provider keys in the client; no content stored on the proxy.
6. Never steal focus from the user's app.
7. Say "I can't find it" rather than point at a guess.

## 10. Distribution

- NSIS installer from the Tauri bundler, per-user install (no admin).
- **SmartScreen**: an unsigned installer shows "Windows protected your PC". Options, in order: Azure Trusted Signing (cheap, needs identity validation; check eligibility on day 1), a standard code-signing certificate (slower to get), or clear "More info → Run anyway" instructions with a screenshot on the website and for the tester.
- Auto-update: not in the MVP (the website always links the latest release).

## 11. Linux (cut for the hackathon)

Windows only (D39). A Linux backend (XInput2, XShm + AT-SPI2 behind the `mm-platform` traits) is post-hackathon; Wayland is out of scope.

## 12. Spikes (gates)

| # | Day | Spike | Pass criterion |
|---|---|---|---|
| S1 | 1 | Transparent robot window + click-through highlight overlay, no focus stealing, mixed DPI, two monitors | Clicks reach another app under the overlay; an open dropdown in Edge stays open when the robot moves |
| S3 | 1 | UIA snapshot from Rust | Windows Settings and Edge return named buttons with correct rects in < 300 ms |
| S4 | 1 | Meta API | Streaming Muse Spark answer; one computer-use turn on a real screenshot returns a click; learn the batch/output/safety-check semantics and whether text comes with the call |
| S2 | 2 | PTT hook | 50 presses: no stuck key, typing and Right-Ctrl shortcuts unaffected |
| S5 | 2 | Muse Voice Transcribe realtime via the proxy path | Russian and English phrase transcribed, endpoint detected, added latency < 500 ms |
| S6 | 2 | MMS-TTS on Modal | English sentence warm in < 500 ms end to end |
| S7 | 2 | Audio in WebView2 | AudioWorklet plays streamed PCM without a user gesture; cancel works |

**Gate, day 1 at 21:00** (16:00 before the kickoff time was known, D33)**:** if S1 or S3 fails in Tauri/Rust, either draw the highlight natively (Direct2D layered window) or switch the Windows layer to C# .NET (WPF), keeping the same design.
