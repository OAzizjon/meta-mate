# Video: storyboard, demo script v3, tester interview, consent

Target video length: **3:00** (180 seconds).
Language: English narration and subtitles, with bilingual live demonstrations (English and Russian).
Production rule: Show only what genuinely works in the codebase—never invent UI dialogs, buttons, or simulated features. Every live demo segment must be recorded from real application runs.

---

## Opening hook (first 10 seconds)

**[Must be recorded live]**

* **0:00–0:03**: Screen shows a suspicious browser window with an intrusive pop-up: a fake *"SYSTEM SECURITY WARNING"* from a made-up "PCShield Pro Support" (`fixtures/demo/scam-popup.html`, no real brand) with urgent warnings and a *"Call Support now: +1 (888) 555-0142"* phone number.
* **0:03–0:06**: The person holds **Right Ctrl** and asks aloud: *"Is this safe?"* As the person speaks, what Mate hears appears live in his speech bubble in real time.
* **0:06–0:10**: Mate wakes up, turns toward the window, flies over (`Flight::fly_to`), and shines his flashlight beam directly on the phone number. Mate switches his mood to `<mood sad>` (scam verdict) and speaks via **MMS-TTS**: *"This looks like a scam. Do not call this number."*
* **Invariants demonstrated**: Mate inspects only the active window, delivers a verdict with a mood, names the red flag he actually sees, and points at the worst one (the phone number). Mate never clicks and never tells the person to click a link, call, or pay.
* **Lower third**: **Muse Spark 1.3** (safety check) · **Muse Voice Transcribe** (realtime ASR) · **MMS-TTS** (Meta open weights on Modal T4).

---

## Shot list (3:00 total)

| Time | Scene & Visuals | Audio & Dialogue | Meta Model in Action | System & UX Invariants | Recording Type |
|---|---|---|---|---|---|
| **0:00–0:10** | **Opening: "Is this safe?" Scam Alert**<br>Fake 'SYSTEM SECURITY WARNING' pop-up (made-up brand, `fixtures/demo/scam-popup.html`) with a 'Call now' number. Person holds Right Ctrl and asks *"Is this safe?"*. Mate turns, flies over, shines his beam on the phone number, sets mood to sad. | **User**: *"Is this safe?"*<br>**Mate (MMS-TTS)**: *"This looks like a scam. Do not call this number."*<br>Soft warning cue (`assets/sounds/cues/ping.ogg`). | **Muse Spark 1.3**: Safety check evaluation.<br>**Muse Voice Transcribe**: Real-time ASR from key-down.<br>**MMS-TTS**: Spoken safety verdict. | Invariant: Mate looks only at active window, gives mood verdict, points at worst red flag, never clicks or suggests calling/paying. | **Recorded Live** |
| **0:10–0:40** | **Push-to-Talk & Live Speech Bubble**<br>Push-to-talk on Right Ctrl. What Mate hears appears live in his bubble while the person speaks. User asks: *"Where is the zoom slider?"* Mate flies across desktop (`whoosh.ogg`) and illuminates the slider with beam and highlight ring. Settings screen displays model option (Muse Wonder 1.0 thorough mode; lessons also escalate to Wonder after two misses on Quick). | **User**: *"Where is the zoom slider?"*<br>**Mate (MMS-TTS)**: *"Here it is. Look where I'm pointing."* (`robot.found_it`)<br>Stereo target ping (`assets/sounds/cues/ping.ogg`). | **Muse Voice Transcribe**: Real-time ASR streaming into bubble.<br>**Muse Spark 1.3**: Direct WHERE intent & coordinate resolution.<br>**Muse Wonder 1.0**: Mentioned via thorough setting; lessons escalate to Wonder after two misses on Quick.<br>**MMS-TTS**: Meta open weights on Modal T4. | `WS_EX_NOACTIVATE`: Mate never steals window focus.<br>Offset placement: Mate stops 24 px away, never obscuring target control. | **Recorded Live** |
| **0:40–1:15** | **Teacher Mode in Settings (Human Clicks Every Step)**<br>Multi-step lesson in Windows Settings (Text size). Mate shines beam on Accessibility, then Text size slider. Mate **waits** without auto-clicking. User moves real mouse and clicks each step themselves. Mate spins happily on click (`success.ogg`). | **Mate (MMS-TTS)**: *"Step 1 of 2: Click Accessibility."*<br>[User clicks]<br>**Mate (MMS-TTS)**: *"Yes, that's it. Well done."* (`robot.right_click`)<br>**Mate (MMS-TTS)**: *"Step 2 of 2: Move the slider."* | **Muse Spark 1.3**: Computer-use teacher loop returning commentary and element coordinates.<br>**MMS-TTS**: Natural spoken guidance. | Invariant: Mate never clicks payment, delete, transfer, or password actions; Mate teaches, the person clicks. | **Recorded Live** |
| **1:15–1:50** | **Keepsake Lesson Card & Printing**<br>Lesson finishes. Muse Glimmer 30B rewrites the steps in simple words and drafts a scene. Muse Image renders the scene in soft gouache with Mate in the corner and NO text. Card window (`card.html`) pops up with picture, simple steps, Print and Close buttons. PNG saved in `Pictures\MetaMate`. User clicks Print. | Narrator: *"When a lesson ends, Mate creates a keepsake card with simple steps to remember."*<br>**Mate (MMS-TTS)**: *"All done. Nice work! I made a card of what we did."*<br>Sound: `done.ogg`, printer hum. | **Muse Glimmer 30B**: Meta open weights on Modal L4 GPU (step simplification & scene description).<br>**Muse Image**: Gouache illustration generation without text. | Asynchronous card generation without blocking interaction. Clean PNG saved locally to `Pictures\MetaMate`. | **Recorded Live** |
| **1:50–2:15** | **Zero-Accessibility Canvas & SAM 3.1**<br>Complex web app rendered on an HTML5 `<canvas>` element where Windows UI Automation tree is empty (`(none)`). Capture is passed to SAM 3.1. SAM finds the button from the pixels alone and the highlight ring settles on its box. | Narrator: *"When an application has no accessibility tree, Meta SAM 3.1 segments controls directly from pixels."*<br>Sound: ping when the ring settles. | **SAM 3.1**: Zero-shot pixel instance segmentation.<br>**Muse Spark 1.3**: Visual grounding and point coordinates. | Seamless fallback: transitions from UIA tree to SAM 3.1 visual segmentation when UIA is empty. | **Recorded Live** |
| **2:15–2:40** | **Tech Map & System Architecture**<br>Single comprehensive architecture slide: Rust + Tauri v2 host, release installer with built-in capped judge invite (no account needed, Google sign-in optional; GitHub sign-in not in judges' build yet), Meta model pipeline on Modal. Narrator walks through the system design. | Narrator: *"MetaMate pairs a lightweight native Windows client with Meta's model family running on Modal GPUs."* | **Meta AI Stack**:<br>• Muse Spark 1.3 & Muse Wonder 1.0 (Thorough setting; lessons escalate after two misses on Quick)<br>• Muse Voice Transcribe<br>• SAM 3.1<br>• Muse Glimmer 30B (Modal L4)<br>• Muse Image<br>• MMS-TTS (Modal T4) | Strict fail-closed local privacy: passwords and background windows painted black before cloud transit. | **Slide / Motion Graphic** |
| **2:40–3:00** | **Tester Reaction & Call to Action**<br>Genuine older adult tester clip: smiling, empowered, using computer independently. Title card with open-source repository link and Cerebral Valley portal submission. | **Tester**: *"Mate shows me where to click without taking over. I feel confident doing it myself."*<br>Narrator: *"MetaMate: Mate lights the button. Your parent clicks it."*<br>Sound: closing chord (`done.ogg`). | — | Authentic user validation and call to action. | **Recorded Live (Tester) + Title Card** |

---

## Detailed scene breakdown & recording script

### Scene 0 (0:00–0:10): Opening Scam Alert & "Is This Safe?"
**[Must be recorded live]**
* **Visual**:
  1. Full desktop capture. A web browser displays a deceptive full-screen pop-up: *"SYSTEM SECURITY WARNING — Your computer is infected with 3 viruses — Call Support now: +1 (888) 555-0142"*.
  2. The person holds down **Right Ctrl**. Mate's aura illuminates to listening mode.
  3. The person asks: *"Is this safe?"*
  4. Mate's speech bubble shows the transcript live as the words are spoken.
  5. Mate turns toward the pop-up, flies over (`Flight::fly_to`), and aims his warm flashlight beam at the telephone number.
  6. Mate switches to `<mood sad>` (the scam mood indicator).
* **Audio**:
  - User: *"Is this safe?"*
  - Mate (MMS-TTS): *"This looks like a scam. Do not call this number."*
  - Sound: `assets/sounds/cues/ping.ogg`.
* **Lower third**:
  - **Muse Spark 1.3**: Safety check evaluation (`crates/mm-core/src/api/request.rs:247`, `ReasoningEffort::SAFETY_CHECK`).
  - **Muse Voice Transcribe**: Real-time push-to-talk speech transcription.
  - **MMS-TTS**: Meta open weights speech synthesis on Modal T4.
* **UX Invariant**: Mate looks only at the active window, names the red flag he actually sees (toll-free phone number on a fake alert), points at the worst one, and never clicks or advises calling.

### Scene 1 (0:10–0:40): Push-to-Talk, Realtime Speech Bubble & Question Flow
**[Must be recorded live]**
* **Visual**:
  1. Clean Windows 11 desktop with an application open. Mate hovers in his idle position in the corner.
  2. The user holds **Right Ctrl**. Mate leans forward with bright eyes.
  3. The user speaks: *"Where is the zoom slider?"*
  4. Live speech bubble: as the user speaks, the exact transcribed text streams into Mate's on-screen bubble without delay.
  5. On key release, Mate plays `whoosh.ogg` and glides to the zoom slider, stopping 24 px away so the control remains fully visible.
  6. Mate shines his flashlight beam, and the highlight overlay draws a glowing rounded ring around the zoom slider.
  7. Brief cut or overlay showing Settings (`settings.html`): model selection shows Quick answers · **Muse Spark 1.3** and Think it through · **Muse Wonder 1.0**.
* **Audio**:
  - User: *"Where is the zoom slider?"*
  - Mate (MMS-TTS): *"Here it is. Look where I'm pointing."* (`robot.found_it`)
  - Sound: `assets/sounds/cues/whoosh.ogg`, `assets/sounds/cues/ping.ogg`.
* **Lower third**:
  - **Muse Voice Transcribe**: Real-time microphone PCM streaming.
  - **Muse Spark 1.3**: Intent routing and element coordinate resolution.
  - **Muse Wonder 1.0**: Thorough thinking mode option (lessons also escalate to Wonder after two misses on Quick).
  - **MMS-TTS**: Natural speech synthesis.
* **UX Invariant**: `WS_EX_NOACTIVATE` window style ensures Mate never steals focus from the user's active window.

### Scene 2 (0:40–1:15): Teacher Mode in Settings (Human Clicks Every Step)
**[Must be recorded live]**
* **Visual**:
  1. Windows Settings window is open.
  2. The user asks: *"How do I make the text bigger?"*
  3. Mate does not take over the mouse. The cursor stays where the human left it.
  4. Mate flies to the *"Accessibility"* menu item and shines a pulsing beam on it.
  5. The user physically moves the mouse and clicks *"Accessibility"*.
  6. Global mouse hook (`WH_MOUSE_LL`) detects the human click on the target.
  7. Mate spins with bright sparkles (`success.ogg`), praising the user.
  8. Mate flies to *"Text size"* and points at the slider. The user drags the slider themselves.
* **Audio**:
  - Mate (MMS-TTS): *"Step 1 of 2: Click Accessibility."*
  - [Physical mouse click]
  - Mate (MMS-TTS): *"Yes, that's it. Well done."* (`robot.right_click`)
  - Mate (MMS-TTS): *"Step 2 of 2: Drag the text size slider until the letters look comfortable."*
  - Sound: `assets/sounds/cues/success.ogg`.
* **Lower third**:
  - **Muse Spark 1.3**: Computer-use teacher loop (`crates/mm-core/src/teacher/`).
  - **MMS-TTS**: Spoken turn guidance.
* **UX Invariant**: Mate never clicks payment, delete, transfer, or password actions; it teaches, the person clicks.

### Scene 3 (1:15–1:50): Keepsake Lesson Card & Printing
**[Must be recorded live]**
* **Visual**:
  1. The text size lesson completes. Mate chimes `done.ogg`: *"All done. Nice work!"*
  2. In the background, **Muse Glimmer 30B** simplifies the completed steps into plain language and writes a scene prompt. **Muse Image** generates a soft gouache painting of the scene with Mate in the corner and no text.
  3. The lesson card window (`card.html`, `src-tauri/src/card.rs`) smoothly appears on screen.
  4. The card shows the gouache painting on top, a clear title, and concise numbered steps (e.g. *"1. Open Settings 2. Choose Accessibility and Text size 3. Move the slider"*).
  5. A toast confirms: *"Saved in Pictures\MetaMate"*.
  6. The user clicks the **Print** button in the card window. The native Windows print preview opens. The user prints the keepsake card.
* **Audio**:
  - Mate (MMS-TTS): *"All done. Nice work! I made a card of what we did so you can keep it."*
  - Sound: `assets/sounds/cues/done.ogg`, soft mechanical printer sound.
* **Lower third**:
  - **Muse Glimmer 30B**: Meta open weights on Modal L4 GPU (`inference/glimmer_modal.py`, step simplification & scene writing).
  - **Muse Image**: Soft gouache illustration generation (strictly without text).
* **UX Invariant**: Card generation is asynchronous and fail-safe; physical PNG is preserved in `Pictures\MetaMate`.

### Scene 4 (1:50–2:15): Zero-Accessibility Canvas Page & SAM 3.1 Outlining
**[Must be recorded live]**
* **Visual**:
  1. An application rendered entirely on an HTML5 `<canvas>` element (such as an online drawing tool or custom web dashboard) where the Windows UI Automation tree returns no child elements (`(none)`).
  2. The user asks: *"Where is the brush tool?"*
  3. UIA inspection confirms zero bounding boxes.
  4. The capture frame and predicted coordinate are sent to **SAM 3.1**.
  5. SAM 3.1 finds the control under the point from the pixels alone (its box; the mask outline is not decoded yet).
  6. Mate flies over and shines his beam; the highlight ring settles on the box SAM found around the canvas button.
* **Audio**:
  - Narrator: *"When an application has no accessibility tree, Meta SAM 3.1 identifies controls directly from raw pixels."*
  - Sound: `assets/sounds/cues/ping.ogg` when the ring settles.
* **Lower third**:
  - **SAM 3.1**: Zero-shot pixel instance segmentation (`crates/mm-core/src/api/sam.rs`).
  - **Muse Spark 1.3**: Visual grounding coordinate proposal.
* **UX Invariant**: Seamless fallback from Windows UI Automation to SAM 3.1 visual segmentation when UIA is empty.

### Scene 5 (2:15–2:40): Tech Map Slide & System Architecture
**[Slide / Motion Graphic]**
* **Visual**:
  A clean, high-resolution architecture diagram slide:
  - **Windows Client**: Rust + Tauri v2, Windows Graphics Capture, Win32 hooks (`WH_KEYBOARD_LL`, `WH_MOUSE_LL`), zero-activation overlay (`WS_EX_NOACTIVATE`), DPAPI secure credential storage.
  - **Authentication**: Release installer with built-in capped judge invite, no account needed; Google sign-in optional (GitHub sign-in is not in the judges' build yet).
  - **Meta AI Model Pipeline**:
    - **Muse Spark 1.3**: Fast answers, computer-use teacher, "Is this safe?" check.
    - **Muse Wonder 1.0**: Thorough reasoning mode (Thorough setting; lessons also escalate from Quick after two misses).
    - **Muse Voice Transcribe**: Real-time push-to-talk speech recognition from key-down.
    - **SAM 3.1**: Instance segmentation on canvas and non-standard interfaces.
    - **Muse Glimmer 30B**: Meta open weights on Modal L4 GPU via llama.cpp (lesson step simplification & scene drafting).
    - **Muse Image**: Soft gouache keepsake illustration (no text in image).
    - **MMS-TTS**: Meta open weights on Modal T4 GPU (natural voice in English and Russian).
  - **Local Privacy**: Fail-closed redaction—password fields and background windows painted solid black locally before cloud transit.
* **Audio**:
  - Narrator: *"MetaMate combines a lightweight native Rust client with Meta's model family hosted on Modal GPUs. Every screen frame is redacted on-device before transit."*

### Scene 6 (2:40–3:00): Tester Reaction & Call to Action
**[Must be recorded live (tester reaction) + Title Card]**
* **Visual**:
  1. Real Zoom / over-the-shoulder video of older adult tester smiling at the monitor.
  2. Final title card:
     - Project: **MetaMate**
     - Tagline: *"Mate lights the button. Your parent clicks it."*
     - Links: GitHub repository & Cerebral Valley submission portal (https://cerebralvalley.ai/).
* **Audio**:
  - Tester: *"It's like someone sitting beside me pointing with a little flashlight. I don't feel nervous anymore."*
  - Narrator: *"MetaMate: Mate lights the button. Your parent clicks it."*
  - Sound: gentle closing chord (`assets/sounds/cues/done.ogg`).

---

## Recording over Zoom: how to make it reliable

1. **In advance (1–2 days before):** Conduct a 10-minute test call. Verify microphone clarity, lighting, and Zoom local recording settings.
2. **Installation (tester pilot):** Provide the release installer (`mm release`) with the built-in capped judge invite (no account needed, Google sign-in optional; GitHub sign-in is not in the judges' build yet). If Windows SmartScreen appears ("Windows protected your PC"), guide the tester in advance: click *"More info" → "Run anyway"*. Have a family member assist locally if possible, or assist via Zoom Remote Control.
3. **Tester configuration:** Enable hands-free mode in settings: *"Press to start and stop"* (tap Right Ctrl or tap Mate, no need to hold key down continuously). This reduces motor fatigue for older adults.
4. **During recording:**
   - Zoom → Settings → Recording:
     - Enable **Record a separate audio file for each participant**.
     - Enable **Optimize for 3rd party video editor**.
     - Choose **Record locally** (higher bitrate than cloud recording).
   - The tester must share their **entire screen (Desktop / Screen 1), not a single application window**. If they share only a single app window, Tauri's transparent overlay windows (Mate and the highlight ring) will not be visible in the recording!
   - In parallel, ask a family member to record over the tester's shoulder with a smartphone (capturing physical hands, keyboard, and screen).
   - Ensure tester wears headphones, or mute your own microphone during Mate's speech to prevent Zoom echo cancellation from cutting Mate's voice.
5. **Directing the tester:** Do not script the tester's lines. Give them natural goals: *"Please try asking Mate if this pop-up is safe"*, *"Try making the text bigger in Settings"*, *"Look at the keepsake card Mate made for you"*. Natural pauses and genuine clicks showcase the true user experience.
6. **Capture duration:** Record a 20–30 minute session; edit down the best 30–40 seconds of genuine interaction for the final cut.

---

## Plan B: Contingency options

* **Backup demo capture:** Record all scenarios on the development demo machine (Windows 11, clean demo profile) using OBS Studio at 60 FPS on the frozen release build. This guarantees flawless backup footage if Zoom call quality drops.
* **Secondary tester:** Have a secondary older adult tester scheduled for 02.11 / 03.11.
* **Audio backup:** If Zoom microphone audio suffers compression artifacts, have the tester record audio simultaneously on a smartphone placed beside their keyboard and sync during video editing.

---

## Backup video (Day 7 freeze)

Recorded on the evening of Day 7 (`docs/plan.md:94`) on the dedicated Windows 11 demo machine (`setup-windows.md:161`, clean "Demo" profile, 125% display scaling). This 90-second cut is the standalone "can't-miss minimum" demo that showcases all core capabilities and invariants without relying on third-party call quality or tester availability.

### Shot list (90 seconds total)

| Time | Scene & Scenario | Visuals & Actions | Spoken Audio & Sound Cues | Meta Model in Action | System & UX Invariants |
|---|---|---|---|---|---|
| **0:00–0:20** (20s) | **Take 1: Scam Check on Phishing Email**<br>MailBox mock fixture (`fixtures/demo/scam-email.html`) with fake urgent security notice from Nordway Bank. | Browser displays urgent email (*"Urgent: your account will be closed today"*). Demonstrator holds **Right Ctrl** alone and asks: *"Is this safe?"* Mate wakes up, flies over (`Flight::fly_to`), sets `<mood sad>`, and shines flashlight beam directly on the deceptive sender domain (`security@nordway-bank-verify.co`) and *"Verify now"* button. | **User**: *"Is this safe?"*<br>**Mate (MMS-TTS)**: *"This looks like a scam. Do not click this link or enter your information."*<br>Sound: `assets/sounds/cues/ping.ogg`. | **Muse Voice Transcribe**: Real-time push-to-talk speech recognition into live speech bubble.<br>**Muse Spark 1.3**: Safety check reasoning (`ReasoningEffort::SAFETY_CHECK`).<br>**MMS-TTS**: Spoken verdict on Modal T4 GPU. | Active window only; passwords and background windows redacted client-side; Mate points at worst red flag and never auto-clicks or suggests verifying. |
| **0:20–0:38** (18s) | **Take 2: Push-to-Talk WHERE-Question**<br>Direct element location in active window. | Demonstrator holds **Right Ctrl**, speech streams live into bubble: *"Where is the zoom slider?"* On key-up, Mate plays `whoosh.ogg`, glides to the zoom slider, stops 24 px away, and shines beam while highlight overlay draws glowing ring (`highlight.html`). | **User**: *"Where is the zoom slider?"*<br>**Mate (MMS-TTS)**: *"Here it is. Look where I'm pointing."* (`robot.found_it`)<br>Sound: `assets/sounds/cues/whoosh.ogg`, `assets/sounds/cues/ping.ogg`. | **Muse Voice Transcribe**: Low-latency PTT streaming.<br>**Muse Spark 1.3**: Coordinate localization (`<point eNN>`).<br>**MMS-TTS**: Fast spoken answer. | `WS_EX_NOACTIVATE`: Mate never takes focus from the active app.<br>Placement offset: Mate hovers 24 px away (`OFFSET_PX`), never covering the target button. |
| **0:38–1:15** (37s) | **Take 3: Teacher Mode Lesson with Wrong-Click Recovery**<br>Step-by-step guidance in Windows Settings (Text size) with human recovery. | Windows Settings open. User asks: *"How do I make the text bigger?"*<br>• **Step 1**: Mate points to *Accessibility*. User clicks it. Mate chimes `success.ogg` and spins: *"Yes, that's it. Well done."* (`robot.right_click`).<br>• **Step 2 (Wrong Click)**: Mate points to *Text size* slider. Demonstrator intentionally clicks away from the target (distance > `hit_tolerance_px`). Mate plays `oops.ogg`, speaks *"Almost. Click right where I'm pointing."* (`robot.wrong_place`), and maintains beam on slider without taking over mouse.<br>• **Step 2 (Correct Click)**: Demonstrator moves mouse and drags slider. Mate detects hit, chimes `success.ogg`, and praises: *"That's right. Very good."* (`robot.right_click_2`). | **Mate (MMS-TTS)**: *"Step 1 of 2: Click Accessibility."*<br>[User clicks]<br>**Mate**: *"Yes, that's it. Well done."*<br>**Mate**: *"Step 2 of 2: Drag the text size slider."*<br>[User clicks wrong spot]<br>**Mate**: *"Almost. Click right where I'm pointing."*<br>[User drags slider]<br>**Mate**: *"That's right. Very good."*<br>Sounds: `assets/sounds/cues/success.ogg`, `assets/sounds/cues/oops.ogg`. | **Muse Spark 1.3**: Computer-use teacher loop (`crates/mm-core/src/teacher/state.rs:172`).<br>**MMS-TTS**: Spoken turn guidance and corrective feedback. | Teacher invariant: Mate never auto-clicks; human clicks every step.<br>Click recognizer (`crates/mm-core/src/teacher/clicks.rs:39`) verifies distance; off-target click triggers `LineKey::WrongPlace` (`state.rs:336`) and re-points instead of failing. |
| **1:15–1:30** (15s) | **Take 4: Keepsake Lesson Card & Native Print Preview**<br>Post-lesson keepsake generation and printing. | Lesson completes. Mate chimes `done.ogg` and speaks: *"All done. Nice work! I made a card of what we did."* (`robot.done`). Card window (`card.html`, `src-tauri/src/card.rs`) opens with gouache illustration (no text in image), simple numbered steps, and *Print* button. Toast confirms PNG saved in `Pictures\MetaMate`. Demonstrator clicks *Print*, opening Windows print preview dialog. | **Mate (MMS-TTS)**: *"All done. Nice work! I made a card of what we did."*<br>Sound: `assets/sounds/cues/done.ogg`. | **Muse Glimmer 30B**: Meta open weights on Modal L4 simplifying completed steps (`inference/glimmer_modal.py`).<br>**Muse Image**: Gouache illustration synthesis.<br>**MMS-TTS**: Completion phrase. | Asynchronous card pipeline never blocks UI; local PNG preserved under `Pictures\MetaMate`; standard Webview print preview. |

---

### Recording steps with FFmpeg gdigrab (one command per take)

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

### Cutting takes together with FFmpeg concat

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

## Questions for the tester (English & Русский)

1. **EN**: What on your computer makes you feel stuck or worried most often?  
   **RU**: Что за компьютером чаще всего вызывает у вас затруднения или тревогу?
2. **EN**: When you get a scary pop-up or get stuck, what do you usually do?  
   **RU**: Когда выскакивает пугающее окно или вы заходите в тупик, что обычно делаете?
3. **EN**: *(Task)* Please ask Mate to check if this alert is safe, and then ask him how to make the text bigger. *(Remain quiet and observe)*  
   **RU**: *(Задание)* Попросите Мейта проверить, безопасно ли это окно, а затем спросите, как сделать текст крупнее. *(Молча наблюдаем)*
4. **EN**: Was Mate's flashlight beam and keepsake card clearer than someone explaining over the phone? Why?  
   **RU**: Луч Мейта и карточка-памятка понятнее, чем объяснения по телефону? Почему?
5. **EN**: Was anything confusing, too fast, or annoying? The sounds, the voice, or the character?  
   **RU**: Показалось ли что-то непонятным, слишком быстрым или навязчивым? Звуки, голос, сам робот?
6. **EN**: Would you use Mate on your own when nobody is around to help? What would make you trust him more?  
   **RU**: Стали бы вы пользоваться Мейтом самостоятельно, когда рядом никого нет? Что помогло бы доверять ему больше?
7. **EN**: If you could change one thing about Mate, what would it be?  
   **RU**: Если бы вы могли изменить в Мейте одну вещь, что бы это было?

*Post-recording follow-up*: Confirm consent preferences regarding showing face, name, or screen only.

---

## Consent form (for the tester)

> Send in advance and obtain written agreement via email or messaging before recording.

---

**Recording consent — MetaMate test session**

Hello {first name},

Thank you for helping test MetaMate, an accessible desktop helper designed to make computers easier and friendlier to use. I would like to record our video call to showcase the application in our submission for the Meta Global AI Developer Hackathon.

**What will be recorded:**
Your voice, your face (only with your permission below), and your computer screen during the test session. Please close any private windows (banking, personal email, private messages) before we begin.

**How the recording will be used:**
Short excerpts (approximately 30–60 seconds in total) may appear in our hackathon submission video, on the open-source project page, and in project documentation. The full, unedited recording remains strictly private and will never be published.

**Your preferences (please select):**
- [ ] You may show my face in the video.
- [ ] Please use only my voice and screen, not my face.
- [ ] You may mention my first name / [ ] Please keep my identity completely anonymous.

**Revocation:**
You can change your mind at any time before **3 November 2026**, and your recording will be promptly deleted and excluded from the video. Participation is voluntary, and you may end the session whenever you wish.

If you agree, please reply with: *"I agree"* along with your choices above.

Warm regards,  
{your name}  
{contact information}

## Tester session plan (2026-10-29): MetaMate (Windows Accessibility & Scam Helper)

*Target Participant: 1 Older Adult (65+) | Duration: 10–15 Minutes | Setting: In-person or Zoom (Full Desktop Share)*  
*Purpose: Validate core live interaction flows and capture authentic, empowering tester footage for the Meta Global AI Developer Hackathon video and X announcement.*

---

### 1. Setup Checklist (T-minus 5 minutes)

- [ ] **Windows Environment**: Windows 10/11 with display scaling set to 100% or 125%; clear sensitive files/icons from desktop.
- [ ] **Privacy Invariant Check**: Ensure private personal windows (banking, emails, chats) are closed; fail-closed redaction active (docs/architecture.md:30).
- [ ] **MetaMate Application**: Launch release build (`mm.exe`). Confirm PTT key is active on **Right Ctrl** (or enable Hands-Free "Press to start and stop" in `settings.html` if the participant has motor fatigue).
- [ ] **Audio & TTS**: Verify microphone input level; ensure speakers/headphones output MMS-TTS voice cues clearly (`assets/sounds/cues/`).
- [ ] **Fixtures Ready in Edge/Chrome**:
  - Tab 1: Pop-up scam alert (`fixtures/demo/scam-popup.html`).
  - Tab 2: Bank phishing email (`fixtures/demo/scam-email.html`).
- [ ] **Windows Settings**: Minimize Settings app ready on desktop (for the text size lesson).
- [ ] **Camera & Recording**:
  - Over-the-shoulder smartphone angle (capturing keyboard hand, screen, and facial expressions/smiles).
  - Screen capture recording the **entire desktop (Screen 1)** so transparent overlay windows (`WS_EX_NOACTIVATE` Mate bubble and highlight ring) are visible.
- [ ] **Consent Signed**: Tester signed written consent form (below).

---

### 2. Five Practical Tasks (Exact Script & Filming Guide)

#### Task 1: The Push-to-Talk Greeting & Orientation (2 min)
*Goal: Familiarize the tester with holding Right Ctrl and seeing Mate listen in real time.*
* **What to say (EN)**: *"Hold down the Right Control key at the bottom right of your keyboard, say 'Hello Mate, where is the Recycle Bin?', and release the key."*  
  **Exact words (RU)**: *«Зажмите клавишу Правый Ctrl в правом нижнем углу клавиатуры, скажите вслух: „Привет, Мейт, где корзина?“, и отпустите клавишу».*
* **What to film**: Close-up of the tester’s right hand on Right Ctrl, cutting to Mate waking up, streaming speech live into the speech bubble, flying across the screen, and settling the flashlight beam on the icon.

#### Task 2: Urgent Scam Check — Deceptive Pop-up (3 min)
*Goal: Show fast visual scam evaluation and pointing at the worst red flag without auto-clicking.*
* **Setup**: Open `fixtures/demo/scam-popup.html` (*"SYSTEM SECURITY WARNING — infected with 3 viruses"*).
* **What to say (EN)**: *"A scary warning popped up. Don't click anything on the screen. Just hold Right Ctrl and ask: 'Is this safe?'"*  
  **Exact words (RU)**: *«Появилось тревожное окно с предупреждением. Не нажимайте ничего на экране. Просто зажмите Правый Ctrl и спросите: „Это безопасно?“».*
* **What to film**: Tester's tense face relaxing; screen capture showing Mate switching to `<mood sad>`, flying over, shining his beam directly onto the fake telephone number (`+1 (888) 555-0142`), and speaking: *"This looks like a scam. Do not call this number."*

#### Task 3: Phishing Email Evaluation (2 min)
*Goal: Verify second scam fixture inspection on phishing bank notice.*
* **Setup**: Switch to `fixtures/demo/scam-email.html` (*"Nordway Bank — Urgent: your account will be closed today"*).
* **What to say (EN)**: *"Here is an urgent email from a bank. Hold Right Ctrl and ask: 'Is this an authentic bank letter?'"*  
  **Exact words (RU)**: *«Вот срочное письмо якобы из банка. Зажмите Правый Ctrl и спросите: „Это настоящее письмо из банка?“».*
* **What to film**: Mate targeting the suspicious sender domain (`@nordway-bank-verify.co`) and deceptive *"Verify now"* button, cautioning the tester not to verify or click links.

#### Task 4: Interactive Lesson — "Make the Text Bigger" (3–4 min)
*Goal: Showcase Teacher Mode where Mate guides with flashlight beam and the user clicks every step.*
* **Setup**: Open Windows Settings.
* **What to say (EN)**: *"Let's make Windows text easier to read. Hold Right Ctrl and say: 'Mate, how do I make the text bigger?' Mate will shine a light where to click—you move the mouse and click it yourself."*  
  **Exact words (RU)**: *«Давайте сделаем текст в системе крупнее и удобнее для чтения. Зажмите Правый Ctrl и скажите: „Мейт, как сделать текст крупнее?“. Мейт подсветит нужную кнопку лучом, а вы сами наведите мышку и нажмите её».*
* **What to film**: Split/over-the-shoulder view: Mate beams on *Accessibility*, tester moves the mouse and clicks. Mate says the step is right and moves on. Mate advances to the *Text size* slider, tester adjusts it, and feels the joy of doing it independently.

#### Task 5: The Keepsake Lesson Card & Print (2 min)
*Goal: Complete lesson flow, show gouache keepsake card popup, and print prompt.*
* **Setup**: Teacher lesson finishes; `card.html` opens automatically.
* **What to say (EN)**: *"Look, Mate prepared a printed keepsake card of the steps you just mastered so you never forget them! Click the blue 'Print' button on the card."*  
  **Exact words (RU)**: *«Посмотрите, Мейт подготовил памятную карточку с шагами, которые вы только что выполнили, чтобы они всегда были под рукой! Нажмите синюю кнопку „Печать“ на карточке».*
* **What to film**: Tester's smile seeing the card illustration and simple numbered steps; mouse click on *Print* opening Windows print preview; camera pans to physical printer or the saved file notification in `Pictures\MetaMate`.

---

### 3. Post-Session Interview (3 Questions)

1. **Empowerment vs. Remote Control**
   * **EN**: *"Did you prefer Mate shining a flashlight so you could click yourself, compared to someone taking over your screen or explaining over the phone? Why?"*
   * **RU**: *«Вам понравилось, что Мейт светит фонариком, а вы нажимаете сами, по сравнению с тем, когда кто-то перехватывает управление экраном или объясняет по телефону? Почему?»*
2. **Safety & Peace of Mind**
   * **EN**: *"When that scary virus warning popped up, did Mate's answer and flashlight beam help you feel safe and in control?"*
   * **RU**: *«Когда выскочило пугающее окно с вирусом, помогли ли ответ Мейта и его луч почувствовать себя спокойнее и увереннее?»*
3. **Daily Confidence**
   * **EN**: *"Would you feel comfortable having Mate open on your computer every day to ask whenever you get stuck?"*
   * **RU**: *«Было бы вам комфортно оставить Мейта на компьютере на каждый день, чтобы обращаться к нему всякий раз, когда что-то непонятно?»*

---

### 4. Informed Consent Form / Форма согласия

#### English Version

**MetaMate Tester Video & Voice Consent Form**

I hereby grant permission to the MetaMate project team to record my voice, computer screen, and video appearance during the user testing session conducted on ____________________, 2026.

I understand and agree that:
1. **Public Use**: Brief excerpts of audio, video, and screen footage (approx. 15–45 seconds) may be published publicly in the Meta Global AI Developer Hackathon submission video, in the open-source code repository, and on public social media channels (including X / Twitter).
2. **Privacy**: No sensitive personal data, passwords, or banking credentials were captured. Background windows were excluded or redacted.
3. **Preferences (Please check applicable)**:
   - [ ] You may show my face and voice.
   - [ ] Use my voice and screen recording only (do not show my face).
   - [ ] You may credit my first name: ___________________ / [ ] Keep my identity anonymous.
4. **Voluntary**: I may withdraw consent anytime before final submission (November 3, 2026) by contacting the team, whereupon my footage will be deleted.

**Tester Signature**: _____________________________  
**Date**: ______________________

---

#### Русская версия

**Согласие участника на запись видео и голоса (MetaMate)**

Настоящим я даю разрешение команде проекта MetaMate на запись моего голоса, экрана компьютера и видеоизображения в ходе тестовой сессии, проведённой «___» ____________ 2026 г.

Я подтверждаю и соглашаюсь со следующим:
1. **Публичное использование**: Короткие фрагменты видео, голоса и записи экрана (примерно 15–45 секунд) могут быть опубликованы в демонстрационном видеоролике для хакатона Meta Global AI Developer Hackathon, на странице открытого репозитория проекта и в социальных сетях (включая X / Twitter).
2. **Конфиденциальность**: Личные конфиденциальные данные, пароли и банковская информация не записывались. Фоновые окна скрыты или закрашены.
3. **Пожелания участника (отметьте нужное)**:
   - [ ] Разрешаю показывать лицо и использовать голос.
   - [ ] Использовать только запись экрана и голос (не показывать лицо).
   - [ ] Можно указать моё имя: ___________________ / [ ] Сохранить анонимность.
4. **Добровольность**: Я могу отозвать своё согласие в любой момент до финальной сдачи проекта (3 ноября 2026 г.), после чего видеозаписи с моим участием будут удалены.

**Подпись участника**: _____________________________  
**Дата**: ______________________

---
