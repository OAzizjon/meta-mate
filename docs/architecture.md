# MetaMate architecture

Status: implementation in progress; this document records current behavior and remaining design constraints.
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
| `hotkey` | Push-to-talk while any app is focused | `windows` crate, `SetWindowsHookEx(WH_KEYBOARD_LL)` on its own thread with a message pump | The callback only enqueues `{key, down/up, time, injected}` and returns (slow hooks are silently removed by Windows). The talk key is a setting: Right Ctrl by default, or Right Alt, Caps Lock, Insert, F9, or F10. It is not suppressed except when Caps Lock is selected, to preserve its lock state. In **hold** mode PTT starts when the key is held alone for 250 ms; any other key during the hold cancels PTT. Ignore auto-repeat and injected events; recover from a missed key-up via `GetAsyncKeyState` polling. Modes: **hold** (default) or **toggle**. `RegisterHotKey` cannot report key-up. |
| `mouse` | User clicks for step verification | `WH_MOUSE_LL`, same thread rules | Position + button only. |
| `mic` | Capture while PTT is active | `cpal` (WASAPI shared mode) | Capture at the device's native rate, then frame as 80 ms PCM16 at 24 kHz. Frames buffer while the speech socket connects; there is no always-open microphone. |
| `uia` | Snapshot of the target: id, control type, name, automation id, help text, rect, enabled, offscreen, is-password; `ElementFromPoint`; events | `uiautomation` crate or raw `windows` COM; dedicated MTA worker; `IUIAutomation2` connection/transaction timeouts set | A Rust-side deadline does not cancel a hung provider call: if the worker is stuck > 2 s, abandon it and start a fresh worker (move to a helper process if this happens in practice). The walk skips offscreen subtrees and stops at 500 nodes or depth 32; a cut walk is completed by the provider's sensitive-field search (D50). Exclude our own HWNDs from every result. Chromium's tree may be poor on the first query **[spike S3]**. |
| `capture` | Image of the **target region**: the foreground window's visible frame **plus its owned popups** (menus and dropdowns are separate windows) | Windows.Graphics.Capture on the monitor, cropped to the union of the window's `DWMWA_EXTENDED_FRAME_BOUNDS` and its visible owned popups; `BitBlt` fallback only with a timeout, and a capture is rejected (not silently accepted) if it is black or incomplete | Our own windows must not appear in the image: hide the robot and highlight for the capture frame (opacity 0 for ~1 frame). **Do not** keep `WDA_EXCLUDEFROMCAPTURE` on our windows, or Zoom/OBS would not show the robot in the demo. **[spike]** |
| `overlay` | Robot window + highlight overlay | Tauri windows created with `transparent: true`, `decorations: false`, `focus: false`, `skip_taskbar: true`, `always_on_top: true`, transparent WebView background and CSS; then extended styles added on the HWND **keeping existing bits** | Robot: `WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW`; mouse dragging uses `SWP_NOACTIVATE` and saves the released position as home. Highlight: one window per monitor, `set_ignore_cursor_events(true)` + `WS_EX_TRANSPARENT | WS_EX_LAYERED | WS_EX_NOACTIVATE`; verify that clicks reach another process. Unique window labels; **one** WebView owns audio. Per-Monitor DPI Aware v2 in the manifest; handle `WM_DPICHANGED` and display changes. **[spike S1]** If WebView2 transparency misbehaves, the highlight can be a native Win32 layered window (Direct2D) while the robot stays in the WebView. |
| `home` | Start page and sample questions | Tauri WebView (`home.html`) | A normal interactive window that opens without taking focus from the user's app; includes Quit. |
| `tray` / `presence` | App controls and temporary hide | Tauri tray, Mate's right-click menu drawn inside his own page (`src/mate-menu.ts`), shared presence state | Home, Settings, Hide for 1 hour, and Quit. A native popup menu would call `SetForegroundWindow`, so Mate's menu is HTML: his window grows with `SWP_NOACTIVATE` while it is open. Hide shows a sleeping Mate and his return time; any question or the tray's "Bring Mate back now" wakes him early. |
| `signin` | Required sign-in | `src-tauri/src/signin.rs`, `signin.html`, `auth.rs` (D54) | The installed app answers only after Google or GitHub sign-in: the sign-in window opens at start and on any question, and the built-in invite is never used. The proxy refuses invite codes when `PROXY_REQUIRE_SIGN_IN=1`. |
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
1. **Offline routing:** Questions are routed immediately by `guess_intent` using English and Russian cue words ("where", "find", "где", "найди", "покажи", "show me"; D47), bypassing the ~2 s online classifier call. Words that only make sense about the screen (screen, button, in the window, tab, link, pop-up, click, press, экран, кнопка, в окне, нажать; a bare "what is this?") also take the screenshot route, and "help me" / "помоги" with a computer verb starts a lesson (D65).
2. **One look (`take_look` in `src-tauri/src/screen.rs`):** A single look at a time (held by the `LOOKING` mutex).
   - **Target window and popups:** `window::foreground_target(own)` finds the foreground window (`DWMWA_EXTENDED_FRAME_BOUNDS` without drop shadow), its visible owned popups and menu windows (`#32768`), and their bounding region clipped to the monitor. Shell/taskbar windows (`Progman`, `WorkerW`, `Shell_TrayWnd`, `Shell_SecondaryTrayWnd`), minimized, cloaked, zero-sized, and our own windows (`own`: Mate, highlight) are rejected.
   - **UI Automation snapshot:** `UiaWalker::snapshot_with_popups` walks the window and its popups into a single snapshot sharing one continuous id space (`e0`, `e1`, ...) on a dedicated MTA thread. The walk skips offscreen subtrees and stops at 500 nodes or depth 32. If it is cut, `UiaWalker::sensitive` completes it with one provider `FindAll` per window and popup for on-screen password/password-like fields and reveal toggles; a failed search fails the look. A complete walk keeps the walk-and-rewalk path (D50).
   - **Our windows hidden for one frame:** Mate and the highlight overlay are hidden (`hide_for_capture`, `SWP_HIDEWINDOW`, never activated); both must report hidden before the copy. After `HIDDEN_FRAME = 50 ms`, `DwmFlush` waits for a composed frame before copying. They are restored on drop (`show_after_capture`). A cancelled question (`!wanted()`) aborts before capturing.
   - **GDI capture:** `capture::capture_region` copies the target region via `BitBlt` (`SRCCOPY | CAPTUREBLT`) into a 32-bit top-down DIB section (BGRA). Refuses empty, off-screen, or pure black captures (protected/DRM content).
   - **Change re-check:** The windows above the target are read just before the copied frame is composed and again right after the copy; if they differ, the look fails as changed (`same_areas`), since a window that came and went in between could be in the picture yet in neither read. After capturing, the foreground target, its popups, and windows above it are re-checked against the original; the walk or sensitive-field search is repeated and must find the same hidden fields in the same places (`same_hidden`). If anything changed, the look is retried once (`look_once`); if it changed again, it fails closed.
   - **Redaction and fail-closed refusals (`redact` / `screen::prepare`):** Every field marked `is_password` or `password_like` (multilingual keyword heuristics, D37) is painted black and omitted from the prompt. Other apps' windows above the target and gaps beside its popups are also painted black (`painted_areas`); nothing there is listed or pointable. Only `RedactedImage` can enter requests. Redaction fails closed on:
     - `SnapshotTruncated` (the walk was cut and the sensitive-field search could not complete it),
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
6. **Placement and flight:** There is now one highlight window per monitor and Mate flies to the monitor holding the target (`src-tauri/src/overlay.rs`, `highlight.rs`, `pointing.rs show_target`). `place_mate` calculates Mate's target position adjacent to the element (offset by `GAP = 24` CSS px, scaled by monitor DPI, preferring free space) without covering it or clipping off the work area. `Flight::fly_to` animates Mate there; his eyes and beam aim at the target in relative coordinates (`in_mate`).
7. **Highlight ring and beam:** Tauri emits `HighlightMsg::Target` to the highlight overlay window for that monitor (`src/highlight.ts`), which draws a target ring in Mate's bulb yellow (padded by 6 px, rounded corners, a navy hairline, a glow that grows with "warmer, warmer") around the element rect, a beam of yellow light from Mate to the nearest edge of the target rect (faint at Mate, strongest at the target, soft edges), an optional step badge (navy with a yellow rim, 16 px) and, with Spotlight, a warm navy dim around an opening on the target.
8. **Stopping pointing:** The beam and ring linger for 3 seconds (`POINT_LINGER = Duration::from_secs(3)`) after the answer finishes playing (`done`), or clear immediately on a new PTT press or cancellation. `stop_pointing` clears the highlight and flies Mate home (`flight.land`).
9. **Fallbacks and tracking:**
   - **SAM 3.1 fallback:** Outlines a pixel when the tree is flat, or when the element under it is a container with no interactive element inside (`sam::outline_allowed`). SAM is prefetched while the answer is parsed and awaited when Mate points.
   - **Window movement:** The ring follows pure moves of the target window; resize, scroll, or an uncertain change clears it (`pointing::Follower`, `winevent::follow`).

**B. "How do I …?" (multi-step): computer use with the human as the hand.**
The teacher driver now exists (`src-tauri/src/teacher.rs`): how-do-I questions are routed to it.
1. Responses API, `tools: [{"type": "computer"}]`, `muse-spark-1.3` by default or the user's `muse-wonder-1.0` choice; `reasoning.effort: "minimal"` by default (it hit the S4 fixture target `e11 "Large"` in 5.3 s with 3702 input / 356 output tokens; `low` hit it too in 6.9 s, D41), `low` when retrying a step that failed. A first-turn miss is asked once more at reasoning effort low. Manual Wonder never escalates, and automatic escalation from Spark to Wonder stays off unless the Day-5 gate turns it on (D39, D40). First screenshot as `input_image`; each turn's frame maps its own points, and its size may follow the window (D48).
2. Each computer-use turn returns a `reasoning` item, a `message` with `phase: "commentary"` (the spoken line Mate speaks), and a `computer_call` containing an `actions` array (e.g. `{"type": "click", "button": "left", "x", "y"}`), a `meta_safety_replay_receipt`, and `pending_safety_checks` (`computer_action_requires_confirmation`) **on every click** (D41). **We do not execute the actions.** The prompt asks for one action per call; a longer `actions` array becomes an ordered list of **sub-steps**, each with its own completion rule:

| Action | Shown as | Completed when |
|---|---|---|
| `click {x,y}` (`button: "left"`) | robot points at the snapped element | mouse hook click inside the rect (± 8 px), plus UIA invoke/selection event if available |
| `double_click` | points, "double-click" bubble | two clicks inside the rect within the double-click time |
| `type {text}` | bubble with the text to type | refused until a value source exists that never logs the text (D48) |
| `keypress {keys}` | keycaps bubble | push-to-talk hook key events complete the step; injected keys never count (D48) |
| `scroll` | robot gestures up/down | a scroll event over the target area, then the next screenshot shows the change |
| `drag` | animated path | button-up near the path's end or on the slider |
| `wait` / `screenshot` / `move` | nothing to do | immediately |

3. After the last sub-step: 600 ms settle → new capture (each turn's frame maps its own points; size may follow the window, D48) → `computer_call_output` (with the acknowledged safety check; whether `meta_safety_replay_receipt` must be passed back too: **[verify]** on the second S4 turn). If the screen changed unexpectedly mid-batch (a dialog, an error), we stop the batch and send the screenshot at once, so the model replans.
4. Wrong click (> 40 px outside): no advance; robot says the `wrong_place` line from copy.md ("Almost. Click right where I'm pointing.") and re-points.
5. "Do it for me": after a spoken yes, `act` performs only a left click when the element under the point belongs to the target, an ancestor names it, and no level (containers included) has a name, automation id, class, or help text matching a password term or English/Russian blocklist entry. The blocklist errs toward refusing (D49).
6. Because the API attaches `pending_safety_checks` (`computer_action_requires_confirmation`) to every click (D41), the human's own click on the pointed target is what confirms the check in teaching mode (and the human's confirmation prompt answers it in "Do it for me"), so invariant 4 costs no extra UI step; checks are never auto-acknowledged without a human action.
7. Budget: max 12 model turns per task, then stop and say so.
8. Spoken line per step (D21, confirmed by S4 in D41): the API returns a `message` item with `phase: "commentary"` alongside `computer_call`, which is the line Mate speaks. A fast parallel `muse-spark-1.3` (`reasoning.effort: "minimal"`) call is used only if `commentary` is absent, while a **pre-cached phrase** ("Step 2 of 3", "Look here") plays immediately while the turn runs.

**Refusal rule:** if the target cannot be validated, Mate says he can't find it (a first-turn miss is asked once more at reasoning effort low). Pointing at the wrong place is worse than saying "I can't see it".

## 4. Voice pipeline

```
Configured PTT key-down ─► open speech socket; robot "listening" ─► mic frames ─► proxy
Configured PTT key-up   ─► Muse Spark 1.3 (streaming, reasoning.effort: minimal)
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

1. Capture only the target region (foreground window + its popups), only during a request, with a "looking" indicator. Flat, remote, and canvas windows that require consent are not captured until the user presses F8 twice in that same window (releasing between presses). Paint other apps' windows above it and gaps beside popups black; nothing there is listed or pointable (D49).
2. Password fields are painted black before anything leaves the PC. UIA `IsPassword` fields and on-screen password-like fields are found; a reveal toggle also hides its on-screen Edit, Document, Custom, and Text siblings. A complete walk is rechecked directly; if the walk is cut, a provider `FindAll` search completes it, and that search is repeated after the copy (D49, D50). **Fail closed:** our windows must report hidden before the copy, which waits for a compositor frame after the 50 ms hide; after copying, the target, popups, and windows above it must be unchanged, and the same hidden fields must be found in the same places (`same_hidden`). Otherwise the look is retried once, then refused. Card numbers and chat text are not detected automatically: the user is told that the active window is sent.
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
| S2 | 2 | PTT hook | 50 presses: no stuck key; the selected talk key and ordinary shortcuts behave correctly |
| S5 | 2 | Muse Voice Transcribe realtime via the proxy path | Russian and English phrase transcribed, endpoint detected, added latency < 500 ms |
| S6 | 2 | MMS-TTS on Modal | English sentence warm in < 500 ms end to end |
| S7 | 2 | Audio in WebView2 | AudioWorklet plays streamed PCM without a user gesture; cancel works |

**Gate, day 1 at 21:00** (16:00 before the kickoff time was known, D33)**:** if S1 or S3 fails in Tauri/Rust, either draw the highlight natively (Direct2D layered window) or switch the Windows layer to C# .NET (WPF), keeping the same design.

## Meta models in MetaMate

| Model | Where in the code | What the person sees | Why this model |
|---|---|---|---|
| **Muse Spark 1.3** | crates/mm-core/src/api/request.rs, src-tauri/src/voice.rs, src-tauri/src/teacher.rs, server/metamate_proxy/policy.py | Mate answers aloud, flies to controls, and guides multi-step desktop tasks step-by-step (*Quick* mode, default). In "Is this safe?" checks, Mate inspects only the active window, delivers a verdict with a mood, names up to 3 visible red flags, and points at the worst one (never clicks, never tells the person to click a link, call, or pay). Mate never clicks payment, delete, transfer, or password actions; it teaches, the person clicks. | **Latency & cost:** Fastest Spark model (first token in 1.4–2.6 s at `reasoning.effort: "minimal"`; S4 hit targets in 5.3 s); low per-call token footprint keeps daily proxy reserve sustainable; `reasoning.effort: "low"` for safety checks and retrying failed steps. |
| **Muse Wonder 1.0** | crates/mm-core/src/api/request.rs, src-tauri/src/settings.rs, server/metamate_proxy/policy.py | Mate answers and plans harder or complex multi-step tasks when selected in Settings under "How Mate thinks" (*Thorough* mode). | **Reasoning depth over cost:** Deeper reasoning for difficult visual/UI tasks; higher cost ($4/$20 per 1M tokens) is managed via server-side allowances, token caps, and quiet fallback to Spark. |
| **Muse Voice Transcribe** | server/metamate_proxy/app.py, src-tauri/src/proxy.rs, src-tauri/src/voice.rs | Mate starts listening when the configured talk key is pressed; what Mate hears appears live in his bubble while the person speaks, transcribing prompt questions in multiple languages. | **Latency & privacy:** Real-time WebSocket connection initiated at key-down masks network handshake latency (~1.9 s), yielding final transcripts 250–360 ms after key-up; zero microphone audio pre-buffering prior to intentional key-down. |
| **SAM 3.1** | crates/mm-core/src/api/sam.rs, src-tauri/src/sam.rs, src-tauri/src/pointing.rs, server/metamate_proxy/policy.py | Precise bounding ring around controls in non-accessible applications (Qt apps like Telegram Desktop, games, canvas UI). | **Accuracy & latency:** Outlines visual boundaries under target pixels without accessibility trees; prefetched asynchronously while the language model streams to mask the ~3.3 s warm inference latency. Capped at $0.02 reserve per call. |
| **Muse Image** | server/metamate_proxy/app.py, server/metamate_proxy/policy.py, src-tauri/src/card.rs, src-tauri/src/proxy.rs | Keepsake lesson card illustration: draws the completed lesson's scene in soft gouache with Mate in a corner and NO text in the picture; shown alongside steps in the card window (with Print and Close) and saved as a PNG in `Pictures\MetaMate`. | **Cost & quality:** Generates 1024×1024 visual illustrations at a predictable fixed cost ($0.01/call) under strict daily quotas, keeping asset generation off real-time interaction critical paths. |
| **Muse Glimmer 30B (open weights on Modal)** | inference/glimmer_modal.py, server/metamate_proxy/app.py, server/metamate_proxy/policy.py, src-tauri/src/card.rs, src-tauri/src/proxy.rs | Keepsake lesson card wording: after a finished lesson, rewrites the commentary steps into simple words in the person's language and writes a scene description; shown as text alongside the illustration in the card window with Print and Close buttons. | **Cost, privacy & autonomy:** Meta open weights running on our own Modal L4 GPU via llama.cpp Q4_K_M; zero token billing to provider APIs, no external logging, scaling to zero when idle. |
| **MMS-TTS** | tts/mms.py, tts/modal_app.py, src-tauri/src/voice.rs, server/metamate_proxy/app.py | Mate speaks responses aloud sentence-by-sentence in English or Russian using an expressive, friendly synthetic voice. | **Latency & cost:** Meta VITS open weights hosted on a Modal serverless T4 GPU; 90–110 ms GPU synthesis per short sentence (replacing 1.5 s CPU latency) streaming PCM directly to the Web Audio ring buffer at minimal infrastructure cost. |
