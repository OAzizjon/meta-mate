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
