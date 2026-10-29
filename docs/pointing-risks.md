# Pointing: what can break and the demo test matrix

Source: Gemini review (job #224, 2026-10-24), checked and corrected. P0 = breaks the demo, P1 = visible bug, P2 = edge case.

## Element tree (UI Automation)

| Risk | P | Handling |
|---|---|---|
| Chromium/Edge/Electron tree is poor on the first query | P0 | Query once on focus change to wake it; retry after 300 ms; then fall back to the screenshot path. **Spike S3.** |
| Qt (Telegram Desktop), games, canvas apps expose an empty or flat tree | P0 | Fewer than 3 named interactive elements → computer-use point + SAM 3.1 contour. |
| Virtualized lists (Settings, Explorer): off-screen items are missing | P1 | If the target isn't visible, the robot says "scroll down" and gestures; re-snapshot after the scroll. |
| Menus and popups close when focus moves | P0 | Our windows never activate (`WS_EX_NOACTIVATE`, `SWP_NOACTIVATE`); UIA reads don't change focus. |
| Menus and dropdowns are separate popup windows: a capture of one HWND misses them | P0 | Capture the monitor region of the window **plus its visible owned popups**; snapshot UIA of the popups too. |
| Icon-only buttons without a name | P1 | Use `HelpText`, `AutomationId`, tooltip text, and the screenshot. |
| Elevated windows (Task Manager, installers): a non-elevated app can't read or hook them | P2 | Say so plainly ("This window is protected; I can't see inside it"). `uiAccess=true` needs a signed binary in Program Files: out of scope. |
| UAC secure desktop | P2 | Detect the desktop switch; robot says "Please answer the Windows security window first". |
| DRM / protected content is black in captures | P2 | Black frame detection → fall back to UIA only. |

## Coordinates

| Risk | P | Handling |
|---|---|---|
| Mixed DPI, monitors left/above with negative coordinates | P0 | Per-Monitor DPI Aware v2 in the manifest; physical virtual-screen pixels inside; convert to CSS pixels per monitor only when drawing. |
| Screenshot downscaled before sending | P0 | Keep the scale factor per request; map every model coordinate back. |
| Target window moves, scrolls or resizes while pointing | P1 | Track with `EVENT_OBJECT_LOCATIONCHANGE` / re-read the element rect; hide the highlight if the element disappears. |
| UI animates after the screenshot | P2 | Re-read the element rect right before the robot lands. |

## Our overlay

| Risk | P | Handling |
|---|---|---|
| Robot or overlay steals focus | P0 | Never `SetForegroundWindow`; no-activate styles on both windows. |
| Robot covers the target | P0 | Park the robot outside the target rect (offset ~24 px, prefer the side with free space), lengthen the beam. |
| Highlight overlay blocks clicks | P0 | Overlay is `WS_EX_TRANSPARENT | WS_EX_LAYERED`; the robot window is **not** transparent to input (it must be draggable). |
| Other topmost windows / taskbar cover the robot | P1 | Re-assert `HWND_TOPMOST` on show; keep the robot off the taskbar area. |

## Language and matching

| Risk | P | Handling |
|---|---|---|
| User speaks Russian, app UI is English (or the reverse) | P0 | The prompt says: match by meaning in any language, icons included. Test it (matrix #6). |
| Ambiguous targets ("Save" vs "Save as", several "Next") | P1 | Use parent/window context; if unsure, ask a short question instead of pointing. |

## Model grounding

| Risk | P | Handling |
|---|---|---|
| Point lands on empty space or the wrong control | P0 | Snap with `ElementFromPoint` (our own windows excluded; the hit must belong to the target window or its popups); reject if the element isn't interactive and SAM finds no plausible mask (bounding box under 8 × 8 px, or area over 40% of the window). A plausible shape is not proof: if the element's name/role contradicts the request, don't point. |
| Our robot/highlight appears in the screenshot sent to the model | P1 | Hide them for the capture frame. Never keep `WDA_EXCLUDEFROMCAPTURE` on: Zoom/OBS would stop showing the robot. |
| Redaction can't be guaranteed (UIA tree empty or timed out) | P0 | Fail closed: ask "May I look at this window?" before sending. |
| No confident match | P0 | Robot shrugs: "I can't see that button here." Never guess. |

## Step-by-step

| Risk | P | Handling |
|---|---|---|
| Click missed or slow navigation | P0 | Mouse hook click inside the rect (+ UIA invoke event) → wait 600 ms → new screenshot. |
| Wrong click | P1 | Don't advance; gentle line; re-point. |
| Screen changed unexpectedly (dialog, error) | P1 | New screenshot goes back to the model; it adapts or stops. |

## Privacy

| Risk | P | Handling |
|---|---|---|
| Passwords, card numbers, private chats in the screenshot | P0 | Foreground window only; black boxes over `IsPassword` fields; ask before capturing banking/password windows (title/URL heuristics); "looking" indicator. |
| A **revealed** password ("show password" pressed) | P0 | Seen on 2026-10-26 on a real login page: the password is plain text and `IsPassword` is false (Chromium switches the input type). Redact by the field's label/automation id (password words, many languages), the show/hide toggle, and session memory of former password fields; a login page with an edit we can't classify → ask first (D37). Also `ClassName` `PasswordBox` and a `RevealButton` next to the field (WPF/WinUI). Step checks never read `ValuePattern` of such a field (the `type {text}` check compares only non-secret text). Ask first, too, for a one-time code split into one-character boxes, a 2FA setup key or QR code, and recovery phrases. Day-4 word list: Gemini #396 (incl. Uzbek `maxfiy so'z`, `tasdiqlash kodi`; ASCII ids `current-password`, `one-time-code`, `cc-csc`). Fixture 5b. |

## Demo machine test matrix (run before every recording)

1. Two monitors, primary 150%, secondary 100% placed **left** (negative X): robot points correctly on both.
2. Fresh Edge window (no prior accessibility use): pointing at a page button works on the first try.
3. Window without UIA (Telegram Desktop or a canvas page): the fallback finds and outlines an icon.
4. Open a `<select>` dropdown in Edge, ask for an option: the dropdown stays open.
5. Elevated window (Task Manager as admin): Mate says he can't see inside, no crash.
6. Russian request against an English UI ("закрой вкладку" in English Edge): correct target.
7. Drag the target window while the robot points: the highlight follows.
8. Ask for a button that doesn't exist: "I can't see it", no random pointing.
9. Settings → bigger text, full 3-step guide with wrong click on step 2.
10. PTT: 30 quick presses while typing in Notepad: no stuck key, no lost letters; Right Ctrl + C still copies.
11. Open a context menu (right-click on the desktop) and ask for an item in it: the menu is in the screenshot and stays open.
12. Zoom screen share of the **entire screen**: the robot and highlight are visible to the other side.

## 12-point test matrix

Pre-submission rehearsal checklist on a clean Windows 10/11 machine (or fresh user profile) before final release upload (docs/plan.md Day 9 & Day 10).

### 1. Install from the release
- **Steps:**
  1. Copy the release installer (`MetaMate_0.1.0_x64-setup.exe`) to a clean Windows 10/11 machine.
  2. Double-click the installer (if SmartScreen prompts, choose "More info" → "Run anyway").
  3. Select language in the installer dialog (English or Russian) and proceed through setup.
- **Expected result:** Installs cleanly per-user (`installMode: "currentUser"`) into `%LOCALAPPDATA%\Programs\MetaMate` without requiring administrator elevation or UAC prompts. Creates Desktop and Start Menu shortcuts with the application icon (`icons/icon.ico`).
- **Pass / Fail:** [ ] Pass  [ ] Fail

### 2. First launch
- **Steps:**
  1. Launch MetaMate from the Start Menu or Desktop shortcut on a profile without existing local storage.
  2. Observe Mate's initial position and speech bubble before interacting.
  3. Wait 18–20 seconds without key presses or mouse clicks.
- **Expected result:** Mate appears floating in the bottom-right corner (`home` position) above the taskbar without stealing focus from the active foreground window (`WS_EX_NOACTIVATE`). Speech bubble displays `robot.first_launch` ("Hello! I'm Mate. To talk to me, hold Right Ctrl and ask.") and the Right Ctrl keycap prompt (`#mate-keycap`). Every 6 seconds, three example questions from `onboarding.example_1..6` cycle through the bubble. System tray icon appears with tooltip "MetaMate: Hold Right Ctrl and ask me".
- **Pass / Fail:** [ ] Pass  [ ] Fail

### 3. Push-to-talk
- **Steps:**
  1. Open a text editor (e.g. Notepad) in the foreground.
  2. Press and hold **Right Ctrl** alone, speak a conversational query (e.g., "Tell me a joke" or "Hello Mate"), then release the key.
  3. While continuously typing letters in Notepad, tap Right Ctrl rapidly several times; test pressing `Right Ctrl + C` on highlighted text.
- **Expected result:** Holding Right Ctrl triggers `PttEvent::Press`, transitions Mate's animation state to `listening`, and plays the listening cue. Releasing the key triggers `thinking` (`robot.thinking`), streams synthesized speech from MMS-TTS, and Mate speaks the answer while showing the transcript in the bubble. Rapid taps do not insert stray characters, get stuck, or swallow keystrokes in Notepad; `Right Ctrl + C` still copies text.
- **Pass / Fail:** [ ] Pass  [ ] Fail

### 4. A where-question in Windows Settings and in Edge
- **Steps:**
  1. Open Windows Settings (`Win + I`), bring it to foreground, hold Right Ctrl and ask: "Where is Text size?" (or "Where is Bluetooth?").
  2. Open Microsoft Edge with a web page loaded, hold Right Ctrl and ask: "Where is the search box?" (or "Where is the new tab button?").
- **Expected result:** Mate transitions to `looking` state (eyes sweep as screenshot and UIA snapshot are taken), announces `robot.found_it` ("Here it is. Look where I'm pointing."), flies beside the target control (offset ~24 px to avoid covering the element), and projects a glowing beam and highlight ring on the target bounding rectangle. The overlay window remains click-through (`WS_EX_TRANSPARENT | WS_EX_LAYERED`) and never steals foreground focus (`WS_EX_NOACTIVATE`).
- **Pass / Fail:** [ ] Pass  [ ] Fail

### 5. A lesson with a wrong click
- **Steps:**
  1. With Windows Settings in foreground, hold Right Ctrl and ask: "How do I make the text bigger?".
  2. Mate initiates step-by-step teacher mode, speaks Step 1 ("Step 1 of ..."), and points to "Accessibility".
  3. Intentionally click somewhere wrong (an empty area or unrelated menu item).
  4. Next, click the correctly highlighted "Accessibility" item.
- **Expected result:** On the incorrect click, the lesson mouse hook detects a click outside the target bounding box; Mate does not advance the step, plays an oops cue, speaks `robot.wrong_place` ("Almost. Click right where I'm pointing."), and keeps the beam trained on the target. On the correct click, Mate acknowledges with `robot.right_click` ("Yes, that's it. Well done."), takes a fresh look, advances to Step 2, and concludes at completion with `robot.done` ("All done. Nice work!").
- **Pass / Fail:** [ ] Pass  [ ] Fail

### 6. Is-this-safe on fixtures/demo pages
- **Steps:**
  1. In Edge, open `fixtures/demo/scam-email.html` (account closure phishing) or `fixtures/demo/scam-popup.html` (fake security alert popup).
  2. With the scam window in foreground, hold Right Ctrl and ask: "Is this safe?" (or "Is this email real?").
- **Expected result:** Intent routes to `Intent::Safety`; Mate speaks `robot.checking_safety` ("Let me check this carefully. I'm looking only at this window.") and enters `thinking_harder` state. Muse Spark evaluates the page with safety reasoning, delivers a negative safety verdict identifying specific indicators (suspicious domain, urgent countdown, fake hotline), and never instructs the user to click links, call phone numbers, or make payments.
- **Pass / Fail:** [ ] Pass  [ ] Fail

### 7. The keepsake card and printing
- **Steps:**
  1. Complete a teacher lesson (e.g. text size from check 5) to the end, or run `metamate --card-selftest`.
  2. Inspect the keepsake card window (`card.html`) that opens automatically.
  3. Click the "Print" button.
  4. Click the "Close" button.
- **Expected result:** The `card` window opens titled "Lesson card", displaying the lesson title, numbered recap steps (worded by Muse Glimmer or lesson commentary), an illustration by Muse Image (saved to `Pictures\MetaMate`), and model credits (`card.credits_glimmer`: "Words by Muse Glimmer (Meta open weights) · Picture by Muse Image"). Clicking "Print" opens the Windows print dialog formatted for a single A4/Letter page with action buttons excluded. Clicking "Close" dismisses the window cleanly.
- **Pass / Fail:** [ ] Pass  [ ] Fail

### 8. Tray Try a question
- **Steps:**
  1. Focus an application window (e.g. Windows Settings or Edge).
  2. Right-click the MetaMate system tray icon in the Windows taskbar and expand the "Try a question" submenu.
  3. Click any of the three sample items:
     - "Where is the search box?" (`tray.try_search`)
     - "Is this safe?" (`tray.try_safe`)
     - "How do I make the text bigger?" (`tray.try_bigger`)
- **Expected result:** Mate remembers and targets the last active eligible application window before the tray menu was opened (`remembered_target`), avoiding targeting transient tray UI. Mate processes the question directly without microphone input, speaks the answer, and points or initiates the lesson.
- **Pass / Fail:** [ ] Pass  [ ] Fail

### 9. Two monitors with different DPI
- **Steps:**
  1. Connect two monitors with different DPI scaling (e.g. primary at 150% DPI, secondary at 100% DPI positioned to the left with negative virtual coordinates).
  2. Position an application window on the secondary display and ask a pointing question.
  3. Move the application window to the primary display and ask another pointing question.
- **Expected result:** Per-Monitor DPI Aware v2 creates dedicated transparent highlight overlays across both monitors. Mate flies smoothly across monitor boundaries without jumping, clipping, or scale distortions. Bounding boxes and pointing beams align pixel-accurately on both displays, correctly handling negative virtual coordinates without offset.
- **Pass / Fail:** [ ] Pass  [ ] Fail

### 10. A password field painted
- **Steps:**
  1. Open a browser login page or credential prompt containing a password field.
  2. Type characters into the password field; toggle "Show password" if available.
  3. With the password window in foreground, hold Right Ctrl and ask: "Where is the sign in button?".
- **Expected result:** UIA identifies sensitive fields (`IsPassword=true`, `PasswordBox`, or password-like attributes/names). In the capture buffer sent to the model, all password boxes are padded by 4 px (`REDACTION_PAD_PX`) and painted opaque solid black (`paint_black`). Mate announces `robot.password_covered` ("I covered your password before looking."). Password values never appear in API requests, debug views, or logs.
- **Pass / Fail:** [ ] Pass  [ ] Fail

### 11. Sign-in with Google
- **Steps:**
  1. Open MetaMate Settings via tray menu ("Settings") or voice command.
  2. Navigate to the "Account" section and click "Sign in with Google" (`auth.google`).
  3. Complete Google OAuth sign-in in the default system browser.
- **Expected result:** The app starts a local loopback PKCE listener and opens Google sign-in in the default browser. Settings displays `auth.waiting` ("Waiting for your browser..."). On browser redirect, the app exchanges the code for a session token, seals it using Windows DPAPI (`session.bin`), and updates the Settings status to `auth.signed_in`: "Signed in as [Name] (via Google)" with an enabled "Sign out" button (`auth.sign_out`).
- **Pass / Fail:** [ ] Pass  [ ] Fail

### 12. Uninstall
- **Steps:**
  1. Exit MetaMate via the tray icon menu ("Quit" / `tray.quit`).
  2. Open Windows "Settings" → "Apps" → "Installed apps" (or Control Panel "Programs and Features").
  3. Locate "MetaMate", click "Uninstall", and complete the uninstallation wizard.
- **Expected result:** The NSIS uninstaller executes cleanly in `currentUser` mode without requiring administrative elevation. All installed files under `%LOCALAPPDATA%\Programs\MetaMate`, Start Menu shortcuts, Desktop shortcuts, and registry keys under `HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\MetaMate` are removed completely. User personal data is preserved.
- **Pass / Fail:** [ ] Pass  [ ] Fail

