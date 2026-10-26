---
name: metamate-windows-core
description: Use when writing or reviewing MetaMate's Windows layer in Rust (crates/mm-win32, src-tauri windows): the no-focus overlay and robot windows, click-through highlight, Per-Monitor DPI v2 and the frame transform, the push-to-talk keyboard hook, UI Automation snapshots from Rust, window capture with owned popups, password redaction before any upload, and SendInput for "do it for me". Also for spikes S1, S2, S3 and the Day-4 pointing work.
---

# MetaMate Windows core

The invariants in `CLAUDE.md` are hard rules: breaking one is a bug. Design: `docs/architecture.md` §3–§4, risks: `docs/pointing-risks.md`, crate split: `docs/project-structure.md`. Versions resolved on 2026-10-25: `docs/setup-windows.md` (tauri 2.12, windows 0.62 — the one version in the tree, uiautomation 0.25, windows-capture 2.0).

## Overlay and robot windows (S1)

- Never take focus (invariant 1). Create the Tauri window hidden with `focus: false`, `decorations: false`, `transparent: true`, `skip_taskbar: true`, `always_on_top: true`; before the first show call `set_focusable(false)`, and for the highlight `set_ignore_cursor_events(true)`. Then OR `WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW` (highlight also `WS_EX_TRANSPARENT | WS_EX_LAYERED`) into `GWL_EXSTYLE`, keeping the existing bits. Show and move only with `SWP_NOACTIVATE`; never `SetForegroundWindow`.
- Test: an open Edge `<select>` must stay open while Mate moves; the target app's title bar must not dim.
- Fallback at the 21:00 checkpoint: draw the highlight in a native Direct2D layered window, or move the Windows layer to C# WPF (`docs/plan.md` Day 1).
- Never keep `WDA_EXCLUDEFROMCAPTURE` on our windows (Zoom/OBS would lose Mate); hide them for one frame during our own capture instead.

## DPI and coordinates (invariant 2)

- Per-Monitor DPI Aware v2 in the manifest. Inside the app everything is physical virtual-screen pixels (`PhysPx`); screenshots sent to the model are `ShotPx` (downscaled); drawing uses `CssPx` of one monitor. Distinct newtypes, conversion only through the `FrameTransform` of that captured frame (`docs/project-structure.md` convention 2).
- Monitors left of the primary have negative X. Mixed DPI: take the monitor of the target window, not of the cursor.

## Push-to-talk (S2)

- `RegisterHotKey` has no key-up: use `WH_KEYBOARD_LL` on its own thread with a message pump; the callback only enqueues `{key, down/up, time, injected}` and returns (slow hooks are silently removed).
- Right Ctrl held alone for 250 ms starts PTT and is never suppressed; another key during the hold cancels.
- Watchdog: while PTT is active, poll `GetAsyncKeyState` (~30 Hz) for the release; Chrome can swallow a shortcut's key-up and Windows can drop the hook (`docs/edge.md` §5 field notes).

## UI Automation (S3)

- A dedicated MTA worker (`CoInitializeEx(COINIT_MULTITHREADED)`), never the Tauri main thread. One `IUIAutomationCacheRequest` for Name, ControlType, AutomationId, HelpText, BoundingRectangle, IsEnabled, IsOffscreen, IsPassword, ClassName; a bounded subtree walk (about 400 nodes).
- Set `IUIAutomation2` connection/transaction timeouts; if a call hangs over 2 s, abandon the worker and start a fresh one.
- Chromium's first query can return an almost empty tree: warm up on focus change and retry after 300 ms. Qt (Telegram) is nearly empty: SAM + computer-use fallback.
- Filter our own windows out of every snapshot.

## Capture and privacy (invariant 3, D37)

- Capture = the foreground window's `DWMWA_EXTENDED_FRAME_BOUNDS` plus its owned popups (menus and dropdowns are separate windows; same-thread `#32768`/`ComboLBox`; unowned Chromium popups only with `WS_POPUP`, `WS_EX_TOOLWINDOW`, no `WS_EX_APPWINDOW`, intersecting the frame). Only walked frames are copied; gaps stay black. Reject a black or stale frame.
- A password field is `IsPassword` **or** a revealed one: label/automation id/help text with password words (many languages), a show/hide toggle or `RevealButton`, class `PasswordBox`, or a field that was a password field earlier in the session. Paint them black before anything leaves the PC; `RedactedImage` is the only type the network client accepts.
- Fail closed: tree empty, truncated or timed out, geometry changed between the walk and the capture, or an unclassifiable edit on a login/payment page → ask the user first (with a thumbnail). Never read `ValuePattern` of such a field.
- `tools/uia_dump.ps1` implements the same rules for fixtures; `-SelfTest` checks the password words.

## "Do it for me" (invariant 4)

- `SendInput` only after the human confirmed, and only if the foreground window is still the target (HWND + process) and `ElementFromPoint` still resolves to the target element. Never pay, buy, delete, send money or password actions; `pending_safety_checks` go to the human. UIPI blocks elevated windows: say so ("This window is protected").

## Verify before saying done

`mm check`, `mm test`, `cargo check --workspace`; for S1/S3 the manual checks above on 100 % and 150 % scaling and, if available, a monitor at negative X. Agents cannot test any of this (no desktop): give them `mm-core` and replay tests instead.
