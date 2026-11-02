# Video: 3-Minute Plan, Recording Checklist, Backup Video & Tester Session

> **2026-11-02:** the current cut with the rendered motion-graphics clips is in [video-edit.md](video-edit.md); this file keeps the recording checklist and the backup plan.

Target video length: **3:00** (180 seconds).  
Language: English narration and subtitles, with bilingual live demonstrations (English and Russian).  
Production rule: Show only what genuinely works in the codebase—never invent UI dialogs, buttons, or simulated features. Every live demo segment must be recorded from real application runs.

---

## 1. The Final 3-Minute Plan

| Time | Scene & Scenario | Visuals & Actions | Audio, Dialogue & Sound Cues | Meta Model in Action & Lower Third | System & UX Invariants | Recording Type |
|---|---|---|---|---|---|---|
| **0:00–0:10** | **Opening Hook: Scam Alert & "Is This Safe?"** | Full desktop capture. Suspicious browser window with an intrusive pop-up: fake *"SYSTEM SECURITY WARNING — Your computer is infected with 3 viruses — Call Support now: +1 (888) 555-0142"* (made-up "PCShield Pro Support" brand, `fixtures/demo/scam-popup.html`, no real brand). User holds either **Ctrl** key alone for 0.3 s; Mate's aura illuminates to listening mode. User asks: *"Is this safe?"*. Speech bubble streams transcript live in real time. Mate wakes up, turns toward window, flies over (`Flight::fly_to`), sets `<mood sad>` (scam verdict), and shines flashlight beam directly on the phone number. | **User**: *"Is this safe?"*<br>**Mate (MMS-TTS)**: *"This looks like a scam. Do not call this number."*<br>Soft warning cue (`assets/sounds/cues/ping.ogg`). | **Muse Spark 1.3**: Safety check evaluation (`crates/mm-core/src/api/request.rs:247`, `ReasoningEffort::SAFETY_CHECK`).<br>**Muse Voice Transcribe**: Real-time push-to-talk speech transcription from key-down.<br>**MMS-TTS**: Spoken safety verdict (Meta open weights on Modal T4 GPU). | Invariant: Mate looks only at the active window, delivers a verdict with a mood, names the red flag actually seen (toll-free number on fake alert), points at the worst one, and never clicks or advises calling/paying. Passwords and background windows redacted client-side. | **Recorded Live** |
| **0:10–0:25** | **Click-to-Talk & Guest Trial** | Clean desktop. The guest trial counter under Mate reads *"Trial: 12 of 12 this hour"*. User clicks Mate directly (no key held); Mate enters listening state and the speech bubble streams: *"Where is the zoom slider?"*. The counter drops to *"Trial: 11 of 12 this hour"*. Mate glides 24 px from the slider, shining the beam and highlight ring on it while Spotlight dims the rest of the screen. | **User**: *"Where is the zoom slider?"*<br>**Mate (MMS-TTS)**: *"Here it is. Look where I'm pointing."*<br>Sounds: `assets/sounds/cues/listen.ogg`, `whoosh.ogg`, `ping.ogg`. | **Muse Voice Transcribe**: Real-time microphone streaming from click-to-talk.<br>**Muse Spark 1.3**: Visual grounding and point coordinate proposal.<br>**MMS-TTS**: Spoken feedback. | No keyboard required; guest trial badge visible beneath Mate; Mate never steals focus (`WS_EX_NOACTIVATE`). | **Recorded Live** |
| **0:25–0:40** | **Typed Chat Window & Language Choice** | User right-clicks Mate to open his menu, then clicks **Chat with Mate** (the language is chosen in Settings > Language: Automatic, English, Русский) and types: *"Where is the search box?"*. Mate turns, glides to the search box, and highlights it with the beam and ring. Quick cut: the user drags Mate onto a toolbar icon and clicks him; Mate lights it and explains what it does. Brief overlay of Settings showing Quick (**Muse Spark 1.3**) / Thorough (**Muse Wonder 1.0**). | **Narrator**: *"Prefer typing? Type a question, or drop Mate onto an icon and click him to hear what it does."*<br>Sound: `assets/sounds/cues/whoosh.ogg`. | **Muse Spark 1.3**: Coordinate resolution from typed text prompt.<br>**Muse Wonder 1.0**: Thorough reasoning mode. | Typing fallback for noisy environments or speech impairments; Mate never steals focus. Drop Mate on it sends only the control's name, type and the app's file name, and only after the click. | **Recorded Live** |
| **0:40–1:15** | **Teacher Mode in Settings (Human Clicks Every Step)** | Windows Settings window is open. User asks: *"How do I make the text bigger?"*. Mate does not take over mouse; cursor stays where human left it. Mate flies to *"Accessibility"* menu item and shines a pulsing beam. User physically moves real mouse and clicks *"Accessibility"*. Global mouse hook (`WH_MOUSE_LL`) detects human click on target. Mate spins happily with bright sparkles (`assets/sounds/cues/success.ogg`), praising user. Mate flies to *"Text size"* and points beam at slider. As the mouse nears it the ring brightens (warmer, warmer); the user clicks just off target and a dotted trail of light leads from the click to the ring. The user retries and drags the slider themselves. | Mate speaks the model's step guidance without a spoken step-number prefix; the on-screen badge shows *"1 / 2"* and *"2 / 2"*.<br>**Mate (MMS-TTS)**: *"Click Accessibility."*<br>[User clicks]<br>**Mate (MMS-TTS)**: *"Yes, that's it. Well done."* (`robot.right_click`)<br>**Mate (MMS-TTS)**: *"Drag the text size slider until the letters look comfortable."*<br>[User clicks just off target]<br>**Mate (MMS-TTS)**: *"Almost. Click right where I'm pointing."* (`robot.wrong_place`)<br>Sound: `assets/sounds/cues/success.ogg`. | **Muse Spark 1.3**: Computer-use teacher loop returning commentary and element coordinates (`crates/mm-core/src/teacher/`).<br>**MMS-TTS**: Spoken turn guidance and praise. | Invariant: Mate never auto-clicks payment, delete, transfer, or password actions; Mate teaches with light, the human clicks every step. | **Recorded Live** |
| **1:15–1:50** | **Keepsake Lesson Card & Native Printing** | Text size lesson completes; Mate chimes `done.ogg`: *"All done. Nice work!"*. In background, **Muse Glimmer 30B** simplifies completed steps into plain language and writes a scene prompt. **Muse Image** renders scene in soft gouache illustration with Mate in the corner and strictly NO text. Card window (`card.html`, `src-tauri/src/card.rs`) smoothly appears on screen showing gouache painting on top, clear title, concise numbered steps (*"1. Open Settings 2. Choose Accessibility and Text size 3. Move the slider"*), Print and Close buttons. PNG is saved in the background under `Pictures\MetaMate`; no toast appears. User clicks **Print**; native Windows print preview opens and user prints keepsake card. | Narrator: *"When a lesson ends, Mate creates a keepsake card with simple steps to remember."*<br>**Mate (MMS-TTS)**: *"All done. Nice work!"*<br>Sound: `assets/sounds/cues/done.ogg`, soft mechanical printer hum. | **Muse Glimmer 30B**: Meta open weights on Modal L4 GPU via llama.cpp (`inference/glimmer_modal.py`, step simplification & scene prompt writing).<br>**Muse Image**: Soft gouache illustration generation without text.<br>**MMS-TTS**: Spoken completion phrase. | Asynchronous card generation pipeline never blocks UI; physical PNG preserved locally in `Pictures\MetaMate`; standard WebView print preview. | **Recorded Live** |
| **1:50–2:15** | **Zero-Accessibility Canvas & SAM 3.1 Segmentation** | Complex web application rendered entirely on an HTML5 `<canvas>` element (e.g. online drawing tool or custom web dashboard) where Windows UI Automation tree returns no child elements (`(none)`). User asks: *"Where is the brush tool?"*. UIA inspection confirms zero bounding boxes. Screen capture frame and proposed coordinate sent to **SAM 3.1**. SAM finds control under point from raw pixels alone (its bounding box; mask outline is not decoded yet). Mate flies over and shines beam; highlight ring settles on box SAM identified around canvas button. | Narrator: *"When an application has no accessibility tree, Meta SAM 3.1 segments controls directly from raw pixels."*<br>Sound: Target ping (`assets/sounds/cues/ping.ogg`) when ring settles. | **SAM 3.1**: Zero-shot pixel instance segmentation (`crates/mm-core/src/api/sam.rs`).<br>**Muse Spark 1.3**: Visual grounding and point coordinate proposal. | Seamless fallback: transitions from UIA tree to SAM 3.1 visual segmentation when UIA is empty. | **Recorded Live** |
| **2:15–2:40** | **Tech Map & System Architecture Slide** | Comprehensive architecture diagram slide:<br>• **Windows Client**: Rust + Tauri v2, Windows Graphics Capture, Win32 low-level hooks (`WH_KEYBOARD_LL`, `WH_MOUSE_LL`), zero-activation overlay (`WS_EX_NOACTIVATE`), DPAPI secure credential storage.<br>• **Access & Input**: Click-to-talk, push-to-talk key, or typed chat window; bilingual EN/RU; guest trial (12 messages an hour) or Google sign-in.<br>• **Meta AI Model Pipeline**:<br>&nbsp;&nbsp;- **Muse Spark 1.3**: Fast answers, computer-use teacher loop, "Is this safe?" check.<br>&nbsp;&nbsp;- **Muse Wonder 1.0**: Thorough reasoning mode (Thorough setting; on Quick, a lesson's next step after two clicks away from the beam).<br>&nbsp;&nbsp;- **Muse Voice Transcribe**: Real-time speech recognition from click-to-talk or key-down.<br>&nbsp;&nbsp;- **SAM 3.1**: Instance segmentation on canvas and non-standard interfaces.<br>&nbsp;&nbsp;- **Muse Glimmer 30B**: Meta open weights on Modal L4 GPU via llama.cpp (`inference/glimmer_modal.py`, step simplification & scene drafting).<br>&nbsp;&nbsp;- **Muse Image**: Soft gouache keepsake illustration (no text in image).<br>&nbsp;&nbsp;- **MMS-TTS**: Meta open weights on Modal T4 GPU (natural voice in English and Russian).<br>• **Local Privacy**: Fail-closed redaction—password fields and background windows painted solid black locally before cloud transit. | Narrator: *"MetaMate pairs a lightweight native Windows client with Meta's model family running on Modal GPUs. Every screen frame is redacted on-device before transit."* | **Meta AI Stack**:<br>• Muse Spark 1.3 & Muse Wonder 1.0<br>• Muse Voice Transcribe<br>• SAM 3.1<br>• Muse Glimmer 30B (Modal L4)<br>• Muse Image<br>• MMS-TTS (Modal T4) | Strict fail-closed local privacy: passwords and background windows painted black before cloud transit (`docs/architecture.md:30`). | **Slide / Motion Graphic** |
| **2:40–3:00** | **Tester Reaction & Call to Action** | Real Zoom / over-the-shoulder video of older adult tester smiling at monitor, empowered, using computer independently.<br>Final title card:<br>• Project: **MetaMate**<br>• Tagline: *"Mate lights the button. Your parent clicks it."*<br>• Links: GitHub repository & Cerebral Valley submission portal (`https://cerebralvalley.ai/`). | **Tester**: *"Mate shows me where to click without taking over. I feel confident doing it myself."* (or *"It's like someone sitting beside me pointing with a little flashlight. I don't feel nervous anymore."*)<br>Narrator: *"MetaMate: Mate lights the button. Your parent clicks it."*<br>Sound: Gentle closing chord (`assets/sounds/cues/done.ogg`). | Authentic user validation and call to action. | User empowerment: human retains agency; Mate never takes over screen. | **Recorded Live (Tester) + Title Card** |

---

## 2. Recording Checklist

### Advance Preparation (1–2 Days Before)
- [ ] **10-minute test call**: Verify tester's microphone clarity, room lighting, and Zoom local recording settings.
- [ ] **Installer delivery**: Build installer via `mm release` (NSIS installer for our hosted proxy; no account needed: "Try without signing in" gives 12 messages an hour, Google sign-in optional; the sign-in window shows only the providers the proxy enables).
- [ ] **SmartScreen guidance**: Guide the tester in advance: if Windows SmartScreen appears (*"Windows protected your PC"*), click *"More info"* → *"Run anyway"*. Have a family member assist locally if possible, or assist via Zoom Remote Control.
- [ ] **Tester consent**: Send consent form (see Section 4) and obtain written agreement via email or messaging before recording.
- [ ] **Backup tester**: Schedule secondary older adult tester for 02.11 / 03.11 (`docs/plan.md:98`).

### Pre-Flight Setup Checklist (T-minus 5 Minutes)
- [ ] **Windows Environment**: Windows 10/11 with display scaling set to 100% or 125% (clean "Demo" profile; `setup-windows.md:161`); desktop cleared of sensitive personal files/icons.
- [ ] **Privacy Invariant Check**: Close all private personal windows (banking, emails, personal chats); verify fail-closed redaction active (`docs/architecture.md:30`).
- [ ] **MetaMate Application**: Launch release build (`mm.exe`). Confirm PTT key is active on either **Ctrl** key (held alone for 0.3 s). For participants with motor fatigue, select *"Press to start and stop"* in `settings.html` to toggle listening with the talk key: press Ctrl once to start and again to stop; no need to hold it down.
- [ ] **Audio & TTS**: Verify microphone input level; ensure speakers/headphones output MMS-TTS voice cues and sound effects clearly (`assets/sounds/cues/`).
- [ ] **Browser Fixtures Ready (Edge/Chrome)**:
  - Tab 1: Pop-up scam alert (`fixtures/demo/scam-popup.html`).
  - Tab 2: Bank phishing email (`fixtures/demo/scam-email.html`).
- [ ] **Windows Settings**: Minimize Windows Settings app ready on desktop (for the text size lesson).
- [ ] **Cameras & Capture Ready**:
  - Over-the-shoulder smartphone angle (capturing keyboard hand, screen, and facial expressions/smiles).
  - Screen capture recording the **entire desktop (Screen 1)**.

### Zoom & Screen Capture Rules
- **Full Desktop Share (Mandatory)**: The tester must share their **entire screen (Desktop / Screen 1), not a single application window**. If they share only a single app window, Tauri's transparent overlay windows (`WS_EX_NOACTIVATE` Mate bubble and highlight ring) will **not** be visible in the recording!
- **Zoom Local Recording Settings**:
  - Enable **Record a separate audio file for each participant**.
  - Enable **Optimize for 3rd party video editor**.
  - Choose **Record locally** (higher bitrate than cloud recording).
- **Audio Isolation**: Ensure tester wears headphones, or mute your own microphone during Mate's speech to prevent Zoom echo cancellation from cutting Mate's voice.
- **Directing the Tester**: Do not script the tester's lines. Give them natural conversational goals (*"Please try asking Mate if this pop-up is safe"*, *"Try making the text bigger in Settings"*, *"Look at the keepsake card Mate made for you"*). Allow natural pauses and genuine clicks.
- **Capture Duration**: Record a 20–30 minute session; edit down the best 30–40 seconds of genuine interaction for the final cut.

### Plan B Contingency Options
- **Backup demo capture**: Record all scenarios on the dedicated Windows 11 demo machine (clean "Demo" profile, 125% display scaling) using OBS Studio at 60 FPS on the frozen release build v0.9 (Day 7 freeze, `docs/plan.md:94`). This guarantees flawless backup footage if Zoom call quality drops.
- **Secondary tester**: Scheduled backup tester available for 02.11 / 03.11.
- **Audio backup**: If Zoom microphone audio suffers compression artifacts, have the tester record audio simultaneously on a smartphone placed beside their keyboard and sync during video editing.

---

## 3. Backup Video (Day 7 Freeze)

Recorded on the evening of Day 7 (`docs/plan.md:94`) on the dedicated Windows 11 demo machine (`setup-windows.md:161`, clean "Demo" profile, 125% display scaling). This 90-second cut is the standalone "can't-miss minimum" demo that showcases all core capabilities and invariants without relying on third-party call quality or tester availability.

### Shot List (90 Seconds Total)

| Time | Scene & Scenario | Visuals & Actions | Spoken Audio & Sound Cues | Meta Model in Action | System & UX Invariants |
|---|---|---|---|---|---|
| **0:00–0:20** (20s) | **Take 1: Scam Check on Phishing Email**<br>MailBox mock fixture (`fixtures/demo/scam-email.html`) with fake urgent security notice from Nordway Bank. | Browser displays urgent email (*"Urgent: your account will be closed today"*). Demonstrator holds either **Ctrl** key alone for 0.3 s and asks: *"Is this safe?"*. Mate wakes up, flies over (`Flight::fly_to`), sets `<mood sad>`, and shines flashlight beam directly on the deceptive sender domain (`security@nordway-bank-verify.co`) and *"Verify now"* button. | **User**: *"Is this safe?"*<br>**Mate (MMS-TTS)**: *"This looks like a scam. Do not click this link or enter your information."*<br>Sound: `assets/sounds/cues/ping.ogg`. | **Muse Voice Transcribe**: Real-time push-to-talk speech recognition into live speech bubble.<br>**Muse Spark 1.3**: Safety check reasoning (`ReasoningEffort::SAFETY_CHECK`).<br>**MMS-TTS**: Spoken verdict on Modal T4 GPU. | Active window only; passwords and background windows redacted client-side; Mate points at worst red flag and never auto-clicks or suggests verifying. |
| **0:20–0:38** (18s) | **Take 2: Push-to-Talk WHERE-Question**<br>Direct element location in active window. | Demonstrator holds either **Ctrl** key alone for 0.3 s, speech streams live into bubble: *"Where is the zoom slider?"*. On key-up, Mate plays `whoosh.ogg`, glides to the zoom slider, stops 24 px away (`OFFSET_PX`), and shines beam while highlight overlay draws glowing ring (`highlight.html`). | **User**: *"Where is the zoom slider?"*<br>**Mate (MMS-TTS)**: *"Here it is. Look where I'm pointing."* (`robot.found_it`)<br>Sound: `assets/sounds/cues/whoosh.ogg`, `assets/sounds/cues/ping.ogg`. | **Muse Voice Transcribe**: Low-latency PTT streaming.<br>**Muse Spark 1.3**: Coordinate localization (`<point eNN>`).<br>**MMS-TTS**: Fast spoken answer. | `WS_EX_NOACTIVATE`: Mate never takes focus from active app.<br>Placement offset: Mate hovers 24 px away, never covering target button. |
| **0:38–1:15** (37s) | **Take 3: Teacher Mode Lesson with Wrong-Click Recovery**<br>Step-by-step guidance in Windows Settings (Text size) with human recovery. | Windows Settings open. User asks: *"How do I make the text bigger?"*<br>• **Step 1**: Mate points to *Accessibility*. User clicks it. Mate chimes `success.ogg` and spins: *"Yes, that's it. Well done."* (`robot.right_click`).<br>• **Step 2 (Wrong Click)**: Mate points to *Text size* slider. Demonstrator intentionally clicks away from target (distance > `hit_tolerance_px`). Mate plays `oops.ogg`, speaks *"Almost. Click right where I'm pointing."* (`robot.wrong_place`), and maintains beam on slider without taking over mouse.<br>• **Step 2 (Correct Click)**: Demonstrator moves mouse and drags slider. Mate detects hit, chimes `success.ogg`, and praises: *"That's right. Very good."* (`robot.right_click_2`). | Mate speaks the model's step guidance without a spoken step-number prefix; the on-screen badge shows *"1 / 2"* and *"2 / 2"*.<br>**Mate (MMS-TTS)**: *"Click Accessibility."*<br>[User clicks]<br>**Mate**: *"Yes, that's it. Well done."*<br>**Mate**: *"Drag the text size slider."*<br>[User clicks wrong spot]<br>**Mate**: *"Almost. Click right where I'm pointing."*<br>[User drags slider]<br>**Mate**: *"That's right. Very good."*<br>Sounds: `assets/sounds/cues/success.ogg`, `assets/sounds/cues/oops.ogg`. | **Muse Spark 1.3**: Computer-use teacher loop (`crates/mm-core/src/teacher/state.rs:172`).<br>**MMS-TTS**: Spoken turn guidance and corrective feedback. | Teacher invariant: Mate never auto-clicks; human clicks every step.<br>Click recognizer (`crates/mm-core/src/teacher/clicks.rs:39`) verifies distance; off-target click triggers `LineKey::WrongPlace` (`crates/mm-core/src/teacher/state.rs:336`) and re-points instead of failing. |
| **1:15–1:30** (15s) | **Take 4: Keepsake Lesson Card & Native Print Preview**<br>Post-lesson keepsake generation and printing. | Lesson completes. Mate chimes `done.ogg` and speaks exactly: *"All done. Nice work!"* (`robot.done`). Card window (`card.html`, `src-tauri/src/card.rs`) opens with gouache illustration (no text in image), simple numbered steps, and *Print* button. PNG is saved in the background under `Pictures\MetaMate`; no toast appears. Demonstrator clicks *Print*, opening Windows print preview dialog. | **Mate (MMS-TTS)**: *"All done. Nice work!"*<br>Sound: `assets/sounds/cues/done.ogg`. | **Muse Glimmer 30B**: Meta open weights on Modal L4 simplifying completed steps (`inference/glimmer_modal.py`).<br>**Muse Image**: Gouache illustration synthesis.<br>**MMS-TTS**: Completion phrase. | Asynchronous card pipeline never blocks UI; local PNG preserved under `Pictures\MetaMate`; standard WebView print preview. |

---

### Recording Steps with FFmpeg gdigrab (One Command per Take)

Recording is performed directly from PowerShell on the demo machine using FFmpeg (`winget install --id Gyan.FFmpeg`, `setup-windows.md:103`). Screen capture uses `gdigrab` at 30 fps, and audio is captured from the Windows default recording device via DirectShow (`dshow`).

#### 1. Pre-take check: verify audio device name

Run this once to identify the exact DirectShow audio name of the default microphone:

```powershell
ffmpeg -list_devices true -f dshow -i dummy
```

If your default microphone name is `Microphone (Realtek(R) Audio)`, you can use `audio="Microphone (Realtek(R) Audio)"` or `audio="default"` (DirectShow maps `"default"` to the primary Windows audio recording endpoint).

#### 2. Take commands (30 fps, default audio)

Run each command in PowerShell before starting the corresponding scenario. Press `q` in the terminal when the take finishes (or rely on the `-t` duration flag):

* **Take 1: Scam check on `fixtures/demo/scam-email.html` (20 seconds)**
  ```powershell
  ffmpeg -f gdigrab -framerate 30 -i desktop -f dshow -i audio="default" -c:v libx264 -preset ultrafast -crf 20 -pix_fmt yuv420p -c:a aac -b:a 192k -t 20 take1_scam.mp4
  ```

* **Take 2: Push-to-Talk WHERE-question (18 seconds)**
  ```powershell
  ffmpeg -f gdigrab -framerate 30 -i desktop -f dshow -i audio="default" -c:v libx264 -preset ultrafast -crf 20 -pix_fmt yuv420p -c:a aac -b:a 192k -t 18 take2_where.mp4
  ```

* **Take 3: Teacher mode with wrong click & recovery (37 seconds)**
  ```powershell
  ffmpeg -f gdigrab -framerate 30 -i desktop -f dshow -i audio="default" -c:v libx264 -preset ultrafast -crf 20 -pix_fmt yuv420p -c:a aac -b:a 192k -t 37 take3_lesson.mp4
  ```

* **Take 4: Keepsake card & print preview (15 seconds)**
  ```powershell
  ffmpeg -f gdigrab -framerate 30 -i desktop -f dshow -i audio="default" -c:v libx264 -preset ultrafast -crf 20 -pix_fmt yuv420p -c:a aac -b:a 192k -t 15 take4_card.mp4
  ```

---

### Cutting Takes Together with FFmpeg Concat

After recording all four takes, cut them together into the final 90-second video (`backup-demo-90s.mp4`) using FFmpeg's concat demuxer or concat filter.

#### Method A: Fast lossless concat (demuxer, no re-encoding)

Create `takes.txt` in the folder containing the MP4 files:

```powershell
@"
file 'take1_scam.mp4'
file 'take2_where.mp4'
file 'take3_lesson.mp4'
file 'take4_card.mp4'
"@ | Set-Content -Encoding ASCII takes.txt
```

Concatenate streams without re-encoding:

```powershell
ffmpeg -f concat -safe 0 -i takes.txt -c copy backup-demo-90s.mp4
```

#### Method B: Filter concat with audio normalization & fast-start (recommended for submission upload)

If takes have slight audio level discrepancies or timestamp jitter from gdigrab start-up:

```powershell
ffmpeg -i take1_scam.mp4 -i take2_where.mp4 -i take3_lesson.mp4 -i take4_card.mp4 -filter_complex "[0:v][0:a][1:v][1:a][2:v][2:a][3:v][3:a]concat=n=4:v=1:a=1[v][a]" -map "[v]" -map "[a]" -c:v libx264 -preset fast -crf 20 -pix_fmt yuv420p -c:a aac -b:a 192k -movflags +faststart backup-demo-90s.mp4
```

This produces a clean, web-ready 90-second video file with synchronized audio and fast-start moov atoms for playback.

---

## 4. Tester Session & Consent (Bilingual EN / RU)

*Target Participant: 1 Older Adult (65+) | Duration: 10–15 Minutes | Setting: In-person or Zoom (Full Desktop Share)*  
*Purpose: Validate core live interaction flows and capture authentic, empowering tester footage for the Meta Global AI Developer Hackathon video and X announcement.*

### Five Practical Tasks (Exact Script & Filming Guide)

#### Task 1: The Push-to-Talk Greeting & Orientation (2 min)
*Goal: Familiarize the tester with holding either Ctrl key alone for 0.3 seconds and seeing Mate listen in real time.*
* **What to say (EN)**: *"Hold either Ctrl key by itself for 0.3 seconds, say 'Hello Mate, where is the Recycle Bin?', and release the key."*  
  **Exact words (RU)**: *«Нажмите и удерживайте любую клавишу Ctrl отдельно 0,3 секунды, скажите вслух: „Привет, Мейт, где корзина?“, и отпустите клавишу».*
* **What to film**: Close-up of the tester holding either Ctrl key alone, cutting to Mate waking up, streaming speech live into the speech bubble, flying across the screen, and settling the flashlight beam on the icon.

#### Task 2: Urgent Scam Check — Deceptive Pop-up (3 min)
*Goal: Show fast visual scam evaluation and pointing at the worst red flag without auto-clicking.*
* **Setup**: Open `fixtures/demo/scam-popup.html` (*"SYSTEM SECURITY WARNING — infected with 3 viruses"*).
* **What to say (EN)**: *"A scary warning popped up. Don't click anything on the screen. Hold either Ctrl key alone for 0.3 seconds and ask: 'Is this safe?'"*  
  **Exact words (RU)**: *«Появилось тревожное окно с предупреждением. Не нажимайте ничего на экране. Удерживайте любую клавишу Ctrl отдельно 0,3 секунды и спросите: „Это безопасно?“».*
* **What to film**: Tester's tense face relaxing; screen capture showing Mate switching to `<mood sad>`, flying over, shining his beam directly onto the fake telephone number (`+1 (888) 555-0142`), and speaking: *"This looks like a scam. Do not call this number."*

#### Task 3: Phishing Email Evaluation (2 min)
*Goal: Verify second scam fixture inspection on phishing bank notice.*
* **Setup**: Switch to `fixtures/demo/scam-email.html` (*"Nordway Bank — Urgent: your account will be closed today"*).
* **What to say (EN)**: *"Here is an urgent email from a bank. Hold either Ctrl key alone for 0.3 seconds and ask: 'Is this an authentic bank letter?'"*  
  **Exact words (RU)**: *«Вот срочное письмо якобы из банка. Удерживайте любую клавишу Ctrl отдельно 0,3 секунды и спросите: „Это настоящее письмо из банка?“».*
* **What to film**: Mate targeting the suspicious sender domain (`security@nordway-bank-verify.co`) and deceptive *"Verify now"* button, cautioning the tester not to verify or click links.

#### Task 4: Interactive Lesson — "Make the Text Bigger" (3–4 min)
*Goal: Showcase Teacher Mode where Mate guides with flashlight beam and the user clicks every step.*
* **Setup**: Open Windows Settings.
* **What to say (EN)**: *"Let's make Windows text easier to read. Hold either Ctrl key alone for 0.3 seconds and say: 'Mate, how do I make the text bigger?' Mate will shine a light where to click—you move the mouse and click it yourself."*  
  **Exact words (RU)**: *«Давайте сделаем буквы на экране крупнее и удобнее для чтения. Нажмите и удерживайте любую клавишу Ctrl отдельно 0,3 секунды и скажите: „Мейт, как сделать текст крупнее?“. Мейт подсветит нужную кнопку лучом, а вы сами наведите мышку и нажмите её».*
* **What to film**: Split/over-the-shoulder view: Mate beams on *Accessibility*, tester moves the mouse and clicks. Mate confirms the step and moves on. Mate advances to the *Text size* slider, tester adjusts it, and feels the joy of doing it independently.

#### Task 5: The Keepsake Lesson Card & Print (2 min)
*Goal: Complete lesson flow, show gouache keepsake card popup, and print prompt.*
* **Setup**: Teacher lesson finishes; `card.html` opens automatically.
* **What to say (EN)**: *"Look, Mate prepared a printed keepsake card of the steps you just mastered so you never forget them! Click the yellow 'Print' button on the card."*  
  **Exact words (RU)**: *«Посмотрите, Мейт подготовил карточку с шагами, которые вы только что выполнили, чтобы они всегда были под рукой! Нажмите жёлтую кнопку „Печать“ на карточке».*
* **What to film**: Tester's smile seeing the card illustration and simple numbered steps; mouse click on *Print* opening Windows print preview; camera pans to physical printer or the saved file notification in `Pictures\MetaMate`.

---

### Tester Interview Questions (English & Русский)

1. **Pre-Task Experience & General Computer Anxiety**  
   **EN**: What on your computer makes you feel stuck or worried most often? When you get a scary pop-up or get stuck, what do you usually do?  
   **RU**: Что за компьютером чаще всего вызывает у вас затруднения или тревогу? Когда выскакивает пугающее окно или вы заходите в тупик, что обычно делаете?
2. **Empowerment vs. Remote Control**  
   **EN**: Did you prefer Mate shining a flashlight so you could click yourself, compared to someone taking over your screen or explaining over the phone? Was Mate's flashlight beam and keepsake card clearer? Why?  
   **RU**: Вам больше понравилось, что Мейт светит фонариком, а вы нажимаете сами, чем если бы кто-то перехватил управление экраном или объяснял по телефону? Луч фонарика Мейта и карточка с шагами понятнее? Почему?
3. **Safety & Peace of Mind**  
   **EN**: When that scary virus warning popped up, did Mate's answer and flashlight beam help you feel safe and in control?  
   **RU**: Когда выскочило пугающее окно с вирусом, помогли ли ответ Мейта и его луч почувствовать себя спокойнее и увереннее?
4. **Daily Confidence & Trust**  
   **EN**: Would you feel comfortable having Mate open on your computer every day to ask whenever you get stuck? Would you use Mate on your own when nobody is around to help? What would make you trust him more?  
   **RU**: Было бы вам комфортно оставить Мейта на компьютере на каждый день, чтобы обращаться к нему всякий раз, когда что-то непонятно? Стали бы вы пользоваться Мейтом самостоятельно, когда рядом никого нет? Что помогло бы доверять ему больше?
5. **Usability & Impressions**  
   **EN**: Was anything confusing, too fast, or annoying? The sounds, the voice, or the character? If you could change one thing about Mate, what would it be?  
   **RU**: Показалось ли что-то непонятным, слишком быстрым или навязчивым? Звуки, голос, сам робот? Если бы вы могли изменить в Мейте одну вещь, что бы это было?

*Post-recording follow-up*: Confirm consent preferences regarding showing face, name, or screen only.

---

### Consent Forms

#### Advance Email Invitation / Remote Consent Template

> Send in advance and obtain written agreement via email or messaging before recording.

**Recording consent — MetaMate test session**

Hello {first name},

Thank you for helping test MetaMate, an accessible desktop helper designed to make computers easier and friendlier to use. I would like to record our video call to showcase the application in our submission for the Meta Global AI Developer Hackathon.

**What will be recorded:**  
Your voice, your face (only with your permission below), and your computer screen during the test session. Please close any private windows (banking, personal email, private messages) before we begin.

**How the recording will be used:**  
Short excerpts (approximately 15–45 seconds in total) may appear in our hackathon submission video, on the open-source project page, and in project documentation. The full, unedited recording remains strictly private and will never be published.

**Your preferences (please select):**  
- [ ] You may show my face in the video.  
- [ ] Please use only my voice and screen, not my face.  
- [ ] You may mention my first name: ___________________ / [ ] Please keep my identity completely anonymous.

**Revocation:**  
You can change your mind at any time before **3 November 2026**, and your recording will be promptly deleted and excluded from the video. Participation is voluntary, and you may end the session whenever you wish.

If you agree, please reply with: *"I agree"* along with your choices above.

Warm regards,  
{your name}  
{contact information}

---

#### Formal In-Person / Written Consent Form (English)

**MetaMate Tester Video & Voice Consent Form**

I hereby grant permission to the MetaMate project team to record my voice, computer screen, and video appearance during the user testing session conducted on ____________________, 2026.

I understand and agree that:
1. **Public Use**: Brief excerpts of audio, video, and screen footage (approx. 15–45 seconds) may be published publicly in the Meta Global AI Developer Hackathon submission video, in the open-source code repository, and on public social media channels (including X / Twitter).
2. **Privacy**: No sensitive personal data, passwords, or banking credentials will be recorded. Background windows will be hidden or redacted.
3. **Preferences (Please check applicable)**:
   - [ ] You may show my face and voice.
   - [ ] Use my voice and screen recording only (do not show my face).
   - [ ] You may credit my first name: ___________________ / [ ] Keep my identity anonymous.
4. **Voluntary**: I may withdraw consent anytime before final submission (November 3, 2026) by contacting the team (by email or messenger), whereupon my footage will be deleted.

**Tester Signature**: _____________________________  
**Date**: ______________________

---

#### Согласие участника на запись видео и голоса (Русская версия)

**Согласие участника на запись видео и голоса (MetaMate)**

Настоящим я даю разрешение команде проекта MetaMate на запись моего голоса, экрана компьютера и видеоизображения в ходе тестовой сессии, проведённой «___» ____________ 2026 г.

Я подтверждаю и соглашаюсь со следующим:
1. **Публичное использование**: Короткие фрагменты видео, голоса и записи экрана (примерно 15–45 секунд) могут быть опубликованы в демонстрационном видеоролике для хакатона Meta Global AI Developer Hackathon, на странице открытого репозитория проекта и в социальных сетях (включая X / Twitter).
2. **Конфиденциальность**: Личные данные, пароли и данные банковских карт не будут записываться. Посторонние окна будут скрыты или закрашены.
3. **Пожелания участника (отметьте нужное)**:
   - [ ] Разрешаю показывать лицо и использовать голос.
   - [ ] Использовать только запись экрана и голос (не показывать лицо).
   - [ ] Можно указать моё имя: ___________________ / [ ] Сохранить анонимность.
4. **Добровольность**: Я могу отозвать своё согласие в любой момент до финальной сдачи проекта (3 ноября 2026 г.), сообщив об этом (по почте или в мессенджере), после чего видеозаписи с моим участием будут удалены.

**Подпись участника**: _____________________________  
**Дата**: ______________________
