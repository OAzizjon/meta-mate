# Video: storyboard, demo script v2, tester interview, consent

Target video length: **2:30** (150 seconds), maximum 3:00.
Language: English narration and subtitles, with bilingual live demonstrations (English and Russian).
Production rule: Show only what genuinely works in the codebase—never invent UI dialogs, buttons, or simulated features.

---

## First 10 seconds: two alternative hooks

Produce and test both 10-second opening hooks during editing; select the strongest for the final submission.

### Hook Option A: The Relatable Frustration (Problem-first)
* **0:00–0:05**: Over-the-shoulder video shot of an older adult sitting before a desktop monitor, overwhelmed by small icons and nested browser settings. Text on screen: *"I call my grandson every time."*
* **0:05–0:10**: The tester sighs on Zoom: *"I just wanted to make the text larger so I could read my letter."* Narrator voiceover: *"Technology shouldn't feel like a test you can fail alone. Meet MetaMate."*

### Hook Option B: The Instant Magic (Solution-first)
* **0:00–0:04**: Close-up of hand holding **Right Ctrl** (or tapping Mate in hands-free mode). The user asks aloud: *"Mate, how do I make the text bigger?"*
* **0:04–0:10**: Instant whoosh: Mate (the warm light-bulb character) flies across the screen, hovers beside the Edge zoom slider, and shines a glowing flashlight beam and a golden ring around the slider. Soft audio chime (`ping`). Narrator: *"He doesn't take over your computer. He lights the button, so you stay in control."*

---

## Shot list (2:30 total)

| Time | Scene & Visuals | Audio & Dialogue | Meta Model in Action | System & UX Invariants |
|---|---|---|---|---|
| **0:00–0:10** | **Hook (Option A or B)**<br>Option A: Frustrated user calling for help.<br>Option B: Right Ctrl press, Mate flies and illuminates control. | Live tester voice / punchy voiceover.<br>Soft cue ping (`assets/sounds/cues/ping.ogg`). | **Muse Voice Transcribe** (speech)<br>**Muse Spark 1.3** (intent & look) | Demonstrates immediate value in under 10 seconds. |
| **0:10–0:35** | **Push-to-Talk & Real Voice (English)**<br>User presses & holds **Right Ctrl** (or taps in hands-free toggle mode). Mate wakes up, aura glows (`listen`). User asks: *"Where is the zoom slider?"* Mate flies across desktop (`whoosh.ogg`) and shines flashlight beam; glowing rounded ring marks the slider. | **User**: *"Where is the zoom slider?"*<br>**Mate (MMS-TTS)**: *"Here it is. Look where I'm pointing."* (`robot.found_it`)<br>Sound: spatial stereo ping towards target. | **Muse Voice Transcribe**: Real-time microphone PCM streaming.<br>**Muse Spark 1.3**: Direct `WHERE` intent & UI element detection.<br>**MMS-TTS**: Meta open weights natural voice synthesis. | `WS_EX_NOACTIVATE`: Mate never steals window focus.<br>Offset placement: Mate never obscures target button or taskbar. |
| **0:35–1:00** | **Teacher Mode: Human Hand & Praise**<br>Multi-step guidance. User asks: *"How do I make the text bigger?"* Mate points to zoom control with a gentle pulsing beam. Mate **waits** without auto-clicking. User moves real mouse and clicks the slider. Mate spins happily with bright flash (`success.ogg`). | **Mate (MMS-TTS)**: *"Step 1 of 2: Click the zoom slider."*<br>Pulsing beam audio.<br>User clicks.<br>**Mate (MMS-TTS)**: *"Yes, that's it. Well done."* (`robot.right_click`) | **Muse Spark 1.3**: Computer-use turn planning (`tools: [{"type": "computer"}]`, commentary + coordinates).<br>**MMS-TTS**: Spoken encouragement per step. | Teacher loop: user's physical click completes step; computer action checks confirmed by human action. |
| **1:00–1:25** | **"Do It For Me" Spoken Voice Confirmation & Safety Refusal**<br>1) User says: *"Can you do it for me?"* Mate asks verbally. **No popup dialog, no on-screen buttons**. User answers *"Yes"*. Mate executes click via `SendInput`.<br>2) User asks to click *"Pay $45"* / *"Delete account"*. Mate refuses to click, shines beam, leaves click to human. | **User**: *"Do it for me."*<br>**Mate (MMS-TTS)**: *"Should I press this button for you? Say yes or no."* (`robot.do_it_for_me`)<br>**User**: *"Yes."*<br>**Mate (MMS-TTS)**: *"All right, pressing it now."* (`robot.pressing`)<br><br>**User**: *"Click Pay now."*<br>**Mate (MMS-TTS)**: *"I don't press Pay or Delete. I'll show you where it is."* (`robot.wont_press`) | **Muse Voice Transcribe**: Real-time spoken confirmation recognition.<br>**Muse Spark 1.3**: Computer-use action routing and safety evaluation.<br>**MMS-TTS**: Clear verbal confirmations and safety notices. | **Invariant 4**: Mate never auto-clicks payment, deletion, or passwords.<br>Zero synthetic UI dialogs: pure voice-driven confirmation protocol. |
| **1:25–1:45** | **Zero-Accessibility Canvas & SAM 3.1 Outlining**<br>Complex web app rendered on an HTML5 `<canvas>` element (or Qt/Telegram app) where Windows UI Automation tree is empty. Visual prompt sends screenshot to **SAM 3.1**. SAM calculates exact segmentation mask and draws pixel-perfect contour around the canvas button. | Narrator voiceover: *"What if an app has no accessibility tree? Meta SAM 3.1 segments visual elements directly from pixels."*<br>Sound: resonant contour lock chime. | **SAM 3.1**: Zero-shot instance segmentation on raw pixel capture.<br>**Muse Spark 1.3**: Visual grounding coordinate prediction. | Fallback pipeline: seamless transition from UIA tree to SAM 3.1 visual segmentation when UIA is empty. |
| **1:45–2:05** | **Privacy Redaction & Russian Bilingual Demo**<br>1) User opens login window with passwords and banking apps behind. The capture engine paints passwords and background windows solid black.<br>2) The developer **Debug Window** (`debug.html`) is shown side-by-side, proving the redacted blacked-out image sent to the API.<br>3) User speaks Russian: *«Как сделать текст крупнее?»* Mate answers and guides in Russian. | Narrator: *"Your private data never leaves your device."*<br><br>**User**: *«Как сделать текст крупнее?»*<br>**Mate (MMS-TTS)**: *«Вот она. Посмотрите, куда я показываю.»*<br>User clicks.<br>**Mate (MMS-TTS)**: *«Да, всё верно. Отлично.»* | **Muse Voice Transcribe**: Multilingual speech recognition.<br>**Muse Spark 1.3**: Russian MateScript streaming generation.<br>**MMS-TTS**: Meta open weights Russian checkpoint (`rus`). | **Invariant 3**: Fail-closed redaction—passwords and occluded apps painted black before cloud transit.<br>Bilingual parity across English and Russian. |
| **2:05–2:30** | **Tester Reaction & Meta Tech Stack Architecture**<br>1) 12 seconds of genuine tester feedback: smiling, relieved, empowered.<br>2) Title card & full architecture summary: Windows Native (Rust + Tauri v2) powered by Meta Model API & MMS-TTS on Modal. Call to action. | **Tester**: *"It's like someone sitting beside me pointing with a small flashlight. I feel confident doing it myself."*<br>Narrator: *"MetaMate: He lights the button. Your parent clicks it."*<br>Sound: gentle closing chord (`assets/sounds/cues/done.ogg`). | **Meta AI Stack Recap**:<br>• Muse Spark 1.3<br>• Muse Voice Transcribe<br>• SAM 3.1<br>• MMS-TTS (open weights) | Clean sign-off and hackathon submission branding. |

---

## Detailed scene breakdown & recording script

### Scene 1 (0:10–0:35): Push-to-Talk & Real Voice Interaction
* **Visual**: Windows 11 desktop. Settings app or Edge browser is open. In the bottom corner, Mate hovers gently in his idle state (slow breathing bob, soft mint status light).
* **Action**:
  1. The user presses and holds **Right Ctrl** (or taps in hands-free toggle mode). Mate's status light switches to listening mode, leaning toward the user with bright eyes.
  2. The user speaks: *"Where is the zoom slider?"*
  3. On key-up, Mate enters thinking state (gentle glow pulse).
  4. Mate plays `whoosh.ogg` and smoothly flies to the zoom slider (`Flight::fly_to`), stopping 24 px away so he doesn't obstruct the control.
  5. Mate raises his flashlight; a warm-white beam shoots to the slider, and the highlight overlay renders a glowing rounded ring around the button.
* **Audio**:
  - User voice: *"Where is the zoom slider?"*
  - Mate voice (synthesized via **MMS-TTS**): *"Here it is. Look where I'm pointing."* (`robot.found_it`)
  - Sound effects: `assets/sounds/cues/whoosh.ogg` during flight, `assets/sounds/cues/ping.ogg` panned to the target coordinate.
* **Meta models shown on screen / voiceover**:
  - **Muse Voice Transcribe**: Streaming speech-to-text directly from the microphone.
  - **Muse Spark 1.3**: Instant routing and coordinate resolution.
  - **MMS-TTS**: Voice playback.

### Scene 2 (0:35–1:00): Teacher Mode — Waiting for User Click & Praise
* **Visual**: Multi-step task in an email or web app.
* **Action**:
  1. User asks: *"How do I make the text bigger?"*
  2. Mate points at the Zoom In (+) button with a gentle pulsing beam (pulses every 2 seconds).
  3. **Crucial**: Mate does *not* click the button. The mouse cursor remains stationary. Mate waits patiently.
  4. The user takes the mouse, navigates to the highlighted ring, and clicks it.
  5. The global mouse hook detects the click within the target bounds.
  6. Mate does a joyful spin animation with bright sparkles (`success.ogg`), smiling warmly.
* **Audio**:
  - Mate voice (MMS-TTS): *"Step 1 of 2: Click the zoom button."*
  - [Click sound from user's mouse]
  - Mate voice (MMS-TTS): *"Yes, that's it. Well done."* (`robot.right_click`)
  - Sound effect: `assets/sounds/cues/success.ogg`.
* **Meta model shown**:
  - **Muse Spark 1.3**: Computer-use turn planning returning commentary and target coordinates.

### Scene 3 (1:00–1:25): "Do It For Me" Spoken Voice Confirmation & Pay/Delete Refusal
* **Visual**: User feels hesitant to click and decides to ask Mate for direct help.
* **Action & Audio Part 1 (Spoken Confirmation — NO DIALOG BUTTONS)**:
  1. User speaks: *"Mate, can you do it for me?"*
  2. **Do NOT show any modal dialog, confirmation popup, or buttons**. Mate speaks aloud via MMS-TTS:
     *"Should I press this button for you? Say yes or no."* (`robot.do_it_for_me`)
  3. User speaks aloud into microphone: *"Yes."*
  4. Mate speaks: *"All right, pressing it now."* (`robot.pressing`)
  5. Mate executes a single left click via `SendInput`. The action completes and Mate chimes: *"All done. Nice work!"* (`robot.done`).
* **Action & Audio Part 2 (Safety Refusal — Pay / Delete)**:
  1. User opens a shopping cart or subscription page with a prominent **"Pay $45.00"** or **"Delete Account"** button.
  2. User speaks: *"Mate, press Pay for me."*
  3. Mate's safety validator detects the blocklist keyword (`crates/mm-core/src/teacher/refusal.rs`, Invariant 4).
  4. Mate refuses to click. He points his flashlight beam at the button and announces firmly:
     *"I don't press Pay or Delete. I'll show you where it is."* (`robot.wont_press`)
  5. Narrator: *"MetaMate enforces hard safety boundaries: financial and destructive actions can only be clicked by the human user."*
* **Meta models shown**:
  - **Muse Voice Transcribe**: Audio voice recognition for "yes" confirmation.
  - **Muse Spark 1.3**: Safety policy evaluation and computer-use action checks.

### Scene 4 (1:25–1:45): Zero-Accessibility Canvas Page & SAM 3.1 Visual Outlining
* **Visual**: Split view showing a custom HTML5 `<canvas>` application (or Telegram / graphical tool) where standard UI Automation returns zero child elements.
* **Action**:
  1. User asks: *"Where is the brush tool?"*
  2. UIA inspector shows an empty accessibility tree (`e0: Canvas`).
  3. The capture is passed to **SAM 3.1** alongside Muse Spark's suggested point coordinates.
  4. SAM 3.1 generates a precise visual segmentation polygon directly from pixels.
  5. Mate points his beam, and the highlight overlay draws a custom SVG contour tracing the exact shape of the icon.
* **Audio**:
  - Narrator: *"When Windows accessibility APIs fail, Meta's Segment Anything Model (SAM 3.1) visually identifies and contours the exact control."*
* **Meta model shown**:
  - **SAM 3.1**: Deep-learning pixel segmentation and outline generation.
  - **Muse Spark 1.3**: Computer-use visual grounding.

### Scene 5 (1:45–2:05): Privacy Redaction Debug Window & Russian Bilingual Demo
* **Visual**:
  1. Browser window with a password login form. Behind it are visible banking tabs and personal messenger windows.
  2. The developer debug window (`debug.html`) opens beside the app.
  3. The capture frame is shown in real time inside `debug.html`: the password field is painted solid black (`robot.password_covered`), and background windows are painted black (`painted_areas`).
* **Audio (Privacy)**:
  - Mate (MMS-TTS): *"I covered your password before looking."* (`robot.password_covered`)
  - Narrator: *"Before any screenshot leaves your device, passwords and background windows are masked out locally in Rust."*
* **Visual & Audio (Russian Demonstration)**:
  1. The user presses Right Ctrl and speaks in Russian: *«Как сделать текст крупнее?»*
  2. Mate flies across screen, rings the control, and speaks in Russian via MMS-TTS:
     *«Вот она. Посмотрите, куда я показываю.»* (`robot.found_it`)
  3. User clicks the button. Mate spins:
     *«Да, всё верно. Отлично.»* (`robot.right_click`)
* **Meta models shown**:
  - **Muse Voice Transcribe**: Russian speech transcription.
  - **Muse Spark 1.3**: Russian language MateScript generation.
  - **MMS-TTS**: High-fidelity Russian speech synthesis (`rus` weights).

### Scene 6 (2:05–2:30): Genuine Tester Feedback & Architecture Summary
* **Visual**:
  1. Real Zoom / phone footage of tester smiling:
     *"I don't have to wait for my children to visit. Mate shows me where to click, and I can do it myself."*
  2. Clean graphical title card showing the full technical stack:
     - **Desktop Host**: Rust + Tauri v2, Windows Graphics Capture, Win32 hooks (`WH_KEYBOARD_LL`, `WH_MOUSE_LL`), zero-activation overlay (`WS_EX_NOACTIVATE`).
     - **Meta Model Stack**:
       - **Muse Spark 1.3**: Intent classification, MateScript stream, computer use.
       - **Muse Voice Transcribe**: Real-time push-to-talk speech recognition.
       - **SAM 3.1**: Visual element segmentation on canvas interfaces.
       - **MMS-TTS**: Meta open weights natural voice synthesis.
     - **Hackathon Tagline**: *"MetaMate: He lights the button. Your parent clicks it."*

---

## Recording over Zoom: how to make it reliable

1. **In advance (1–2 days before):** Conduct a 10-minute test call. Verify microphone clarity, lighting, and Zoom local recording settings.
2. **Installation (tester pilot):** Provide the NSIS installer (`mm release`) and invite code. If Windows SmartScreen appears ("Windows protected your PC"), guide the tester in advance: click *"More info" → "Run anyway"*. Have a family member assist locally if possible, or assist via Zoom Remote Control.
3. **Tester configuration:** Enable hands-free mode in settings: *"Press to start and stop"* (tap Right Ctrl or tap Mate, no need to hold key down continuously). This reduces motor fatigue for older adults.
4. **During recording:**
   - Zoom → Settings → Recording:
     - Enable **Record a separate audio file for each participant**.
     - Enable **Optimize for 3rd party video editor**.
     - Choose **Record locally** (higher bitrate than cloud recording).
   - The tester must share their **entire screen (Desktop / Screen 1), not a single application window**. If they share only a single app window, Tauri's transparent overlay windows (Mate and the highlight ring) will not be visible in the recording!
   - In parallel, ask a family member to record over the tester's shoulder with a smartphone (capturing physical hands, keyboard, and screen).
   - Ensure tester wears headphones, or mute your own microphone during Mate's speech to prevent Zoom echo cancellation from cutting Mate's voice.
5. **Directing the tester:** Do not script the tester's lines. Give them natural goals: *"Please try making the text bigger in your browser"*, *"Try saving this recipe as a PDF"*. Natural pauses and genuine clicks showcase the true user experience.
6. **Capture duration:** Record a 20–30 minute session; edit down the best 30–40 seconds of genuine interaction for the final cut.

---

## Plan B: Contingency options

* **Backup demo capture:** Record all scenarios on the development demo machine (Windows 11, clean demo profile) using OBS Studio at 60 FPS on the frozen release build. This guarantees flawless backup footage if Zoom call quality drops.
* **Secondary tester:** Have a secondary older adult tester scheduled for 02.11 / 03.11.
* **Audio backup:** If Zoom microphone audio suffers compression artifacts, have the tester record audio simultaneously on a smartphone placed beside their keyboard and sync during video editing.

---

## Questions for the tester (English & Русский)

1. **EN**: What on your computer makes you feel stuck most often?  
   **RU**: Что за компьютером чаще всего вызывает у вас затруднения?
2. **EN**: When you get stuck, what do you usually do?  
   **RU**: Когда вы заходите в тупик, что обычно делаете?
3. **EN**: *(Task)* Please ask Mate to help you make the text bigger. *(Remain quiet and observe)*  
   **RU**: *(Задание)* Попросите Мейта помочь сделать текст крупнее. *(Молча наблюдаем)*
4. **EN**: Was Mate's flashlight beam and ring clearer than someone explaining over the phone? Why?  
   **RU**: Луч и подсветка Мейта понятнее, чем объяснения по телефону? Почему?
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
