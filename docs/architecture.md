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
| `mic` | Capture while PTT is active | `cpal` (WASAPI shared mode) | Capture at the device's native rate, then frame as 80 ms PCM16 at 24 kHz. Frames buffer while the speech socket connects; there is no always-open microphone. |
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
1. **Offline routing:** Questions are routed immediately by `guess_intent` using English and Russian cue words ("where", "find", "где", "найди", "покажи", "show me"; D47), bypassing the ~2 s online classifier call.
2. **One look (`take_look` in `src-tauri/src/screen.rs`):** A single look at a time (held by the `LOOKING` mutex).
   - **Target window and popups:** `window::foreground_target(own)` finds the foreground window (`DWMWA_EXTENDED_FRAME_BOUNDS` without drop shadow), its visible owned popups and menu windows (`#32768`), and their bounding region clipped to the monitor. Shell/taskbar windows (`Progman`, `WorkerW`, `Shell_TrayWnd`, `Shell_SecondaryTrayWnd`), minimized, cloaked, zero-sized, and our own windows (`own`: Mate, highlight) are rejected.
   - **UI Automation snapshot:** `UiaWalker::snapshot_with_popups` walks the window and its popups into a single snapshot sharing one continuous id space (`e0`, `e1`, ...) on a dedicated MTA thread, bounded by `WalkLimits` (`max_nodes: 400`, `max_depth: 12`).
   - **Our windows hidden for one frame:** Mate and the highlight overlay are hidden (`hide_for_capture`, opacity 0) for one composed frame (`HIDDEN_FRAME = 50 ms`) before the copy and restored on drop (`show_after_capture`). A cancelled question (`!wanted()`) aborts before capturing.
   - **GDI capture:** `capture::capture_region` copies the target region via `BitBlt` (`SRCCOPY | CAPTUREBLT`) into a 32-bit top-down DIB section (BGRA). Refuses empty, off-screen, or pure black captures (protected/DRM content).
   - **Change re-check:** After capturing, the foreground target is re-checked against the original. If the HWND, frame bounds, popups, or region changed (`LookFailure::Changed`), the look is retried once (`look_once`); if it changed again, it fails closed.
   - **Redaction and fail-closed refusals (`redact` / `screen::prepare`):** Every field marked `is_password` or `password_like` (multilingual keyword heuristics, D37) is painted black and omitted from the prompt. Only `RedactedImage` can enter requests. Redaction fails closed on:
     - `SnapshotTruncated` (walk hit node/depth limits or a popup could not be walked),
     - `UnclassifiedSensitiveField` (unclassified edit on a login or payment page),
     - `CaptureMismatch` (capture dimensions do not match the target region),
     - `StaleSnapshot` (window moved/resized between UIA walk and capture),
     - `InvalidField` (sensitive field has empty or inverted bounds),
     - `UnlocatedSensitive` (sensitive field on screen with no bounds),
     - `Shot` (encoding failure).
     Top-level `LookFailure` maps to spoken lines: `NoWindow` speaks `robot.cant_find`; `Capture::Black` speaks `robot.protected_window`; `Uia`, `Capture`, `Prepare`, `Changed`, `OwnWindows`, and `Cancelled` speak `robot.cant_look`.
   - **Element list:** `render_element_list` outputs up to 60 (`MAX_LISTED`) visible, interactive elements in screenshot coordinates (`e17 Button "Text size" [x,y,w,h]`). The redacted image is downscaled to at most 1280 px on the long edge (`MAX_EDGE`).
3. **Model call & MateScript:** `point_request` sent to `muse-spark-1.3` (or `muse-wonder-1.0` if chosen in Settings; Responses API, streaming, `reasoning.effort: "minimal"`). The answer streams MateScript with `<point e17>` (or `<point x,y>`).
4. **WebView playback:** The WebView timeline player (`src/player/`, `src/mate.ts`) invokes the Tauri command `point_at(generation, point, step)` when playback reaches the sentence that points, so the beam lights up in sync with the spoken phrase.
5. **`Look::resolve` rules:**
   - The window must still be shown and unmoved (`window::still_shown`).
   - `Point::Id(id)`: `id` must be in the prompt's listed ids (`list.ids`), and the snapshot element must be pointable (`interactive`, `parent.is_some()`, `enabled`, and not `offscreen`).
   - `Point::Pixel(x, y)`: mapped to physical screen space (`shot_to_phys_checked`) within image bounds and snapped to the pointable element under the pixel via `snapshot.target_at(phys)`.
   - The resolved rectangle is clipped to `snapshot.frame`. If unresolvable or missing, returns `None` (clears the highlight; never guesses, invariant 5).
6. **Placement and flight:** `place_mate` calculates Mate's target position adjacent to the element (offset by `GAP = 24` CSS px, scaled by monitor DPI, preferring free space) without covering it or clipping off the work area. `Flight::fly_to` animates Mate there; his eyes and beam aim at the target in relative coordinates (`in_mate`).
7. **Highlight ring and beam:** Tauri emits `HighlightMsg::Target` to the highlight overlay window (`src/highlight.ts`), which draws a glowing target ring (padded by 6 px, rounded corners, 1.5 s pulse) around the element rect and a warm-white beam polygon from Mate to the nearest edge of the target rect (plus an optional step badge).
8. **Stopping pointing:** The beam and ring linger for 3 seconds (`POINT_LINGER = Duration::from_secs(3)`) after the answer finishes playing (`done`), or clear immediately on a new PTT press or cancellation. `stop_pointing` clears the highlight and flies Mate home (`flight.land`).
9. **What is still planned:**
   - **SAM 3.1 fallback:** Visual contour refinement when UIA is flat or empty (canvas, Qt/Telegram Desktop, games).
   - **`EVENT_OBJECT_LOCATIONCHANGE` tracking:** Follow the target if the window moves or scrolls while pointing.
   - **Per-monitor highlight windows (in progress):** Multi-monitor highlight overlays (currently restricted to the primary monitor; targets on secondary monitors speak `robot.other_monitor`).
   - **Bigger UIA limits for Chromium:** Adaptive/larger limits for deep DOM trees (probed via `--look-probe`; 400 nodes / depth 12 currently causes fail-closed refusals on Electron apps like VS Code).

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
PTT key-down ─► open speech socket; robot "listening" ─► mic frames ─► proxy
PTT key-up   ─► Muse Spark 1.3 (streaming, reasoning.effort: minimal)
             ─► MateScript parser ─► one ordered Tauri channel to Mate's window
sentence ─► proxy ─► Modal MMS-TTS ─► PCM ─► channel ─► speech
```

- The speech socket opens on key-down. Frames buffer while it connects (about **1.9 s**, D44). The microphone is captured at the device's own rate and framed as **80 ms PCM16 at 24 kHz**.
- After key-up, Muse Spark streams the answer with `reasoning.effort: minimal`; the parser sends each sentence to MMS-TTS as it emits it.
- If no sentence has arrived **700 ms** after key-up, the cached "let me think" phrase plays. It uses locale key `robot.thinking` and is synthesized once at startup in both voices.
- `subscribe_answer_stream(channel, locale)` sends one ordered Rust-to-Mate-window Tauri channel. Each message is tagged by `type`: `state` (`listening`, `thinking`, `idle`); `item` (a MateScript timeline item `{kind, value}`); `pcm` (base64 PCM16LE mono, one chunk per sentence with `eos`; sentence 0 is a cached phrase outside the timeline, and an empty `eos` chunk ends a sentence with no audio); `cancel` (barge-in: generations up to N stop); or `line` (a locale key shown in the bubble).
- A new press cancels the answer still playing: speech, queued TTS and sfx stop at once, and the window drops any late message of a cancelled generation. The parser never invents `<end>`, so the answer path appends `End` itself and Mate returns to idle.
- **Measured latency (D45):** key-up to first sound is **3.5–4.5 s** with CPU TTS. The **2–3 s** goal needs the cached phrase plus a GPU on Modal.
- Still planned: more cached phrases per language (greeting, "Look here", "Well done", "Almost…", errors), shipped as audio files; if the speech socket fails, the recorded clip goes to the file endpoint (`POST /v1/asr/transcribe`) on key-up; a fallback voice (Windows WinRT `SpeechSynthesizer`) when Modal is slow (> 1.5 s) or down, switched only at a sentence boundary. **[verify]** which Muse Voice Transcribe languages have MMS-TTS checkpoints.

## 5. Audio path and mixer

- Transport: the ordered Tauri channel described above carries sentence PCM as base64 PCM16LE mono chunks, tagged with `eos`; an empty `eos` chunk ends a sentence that has no audio. Sentence 0 is the cached phrase and is outside the MateScript timeline. An **AudioWorklet** ring buffer (120 s) plays them, resampling to the context rate; a sentence that does not fit is skipped but still ends, so the timeline never hangs.
- Autoplay (spike S7, passed 2026-10-27): every window starts WebView2 with `--autoplay-policy=no-user-gesture-required`, and the AudioContext runs without a gesture; the output device's clock can take ~1.5 s to start, so the resume check waits for it. `metamate --ask "question"` proves it unattended: the window reports each sentence's end and the whole answer played out.
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
- Speech socket → Muse Voice Transcribe (`muse-voice-transcribe-1.0`) realtime; it opens on PTT key-down while microphone frames buffer during connection.
- `POST /v1/tts` → Modal MMS-TTS (shared secret).
- Auth: invite code; per-code daily quota; global daily budget (`PROXY_DAILY_BUDGET_USD`).
- Privacy: no request/response bodies stored; only counters (code, model, tokens, time).
- Landing page + downloads (GitHub Releases) on the same host.

## 8. TTS service (Modal)

- Python, `transformers` `VitsModel` + tokenizer for `facebook/mms-tts-<iso>`; preload eng, rus + a few common languages; others lazily.
- One warm container on demo and judging days; scale to zero otherwise.
- Input is normalized before synthesis because the MMS alphabets drop anything else: numbers and percent to words, keyboard names and symbols to words, and Latin words transliterated for the Russian voice.
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
