# Strategy: how to reach the top 5

## In one sentence

**MetaMate is a small desktop robot that doesn't do things for you; it shows you what to do. It flies to the right button in any app, guides you step by step, and waits for you to click. Talk to it in any language.**

For the judges (English): *MetaMate turns Meta's computer-use model into a patient teacher: the model decides where to click, a friendly desktop robot shows it, and the human does the click.*

Gemini's "judge" alternative (emphasizes the technology): *Autonomous agents take control away from users; passive overlays just talk over the screen. MetaMate intercepts Meta's computer-use actions, grounds them with UI Automation and SAM 3.1, and checks every click at the OS level, so a friendly robot guides your hand until you can do it yourself.*

Gemini is right about the main point: the judges will be impressed not by the mascot, but by the **pattern**—intercepting computer-use actions and turning them into a tutorial that checks each click. The robot is the face of this pattern, not the whole idea.

## Who it's for

1. Older adults who are afraid they'll "break" their computer and call their grandchildren every time.
2. Anyone using an unfamiliar app (settings, banking, government services, Zoom).
3. A secondary audience: children and families (stories with sound effects, fun mode).

## How we're different

| Product | What it does | Our response |
|---|---|---|
| Microsoft Copilot Vision (Windows 10/11), Highlights feature: "Show me how" | Sees the screen and highlights where to click and what steps to take. Since June 2025, it's been available in the US, with "more countries outside Europe coming soon" (I couldn't find any news of expansion in 2026) | A character with a body and patience: he flies over, shines his beam, **checks** that you clicked correctly, and guides you to the next step. Works in any language and any country. A "do it for me" mode requires confirmation. Open source and free. |
| Gemini Live, ChatGPT desktop (screen sharing) | Gives verbal instructions like "click the gear in the top right" | It's hard for an older adult to translate words into a location on screen. We show the exact spot, with no spatial ambiguity. |
| Screen magnifiers, Narrator | Rigid, lots of keyboard shortcuts | Ordinary speech instead of hotkeys. |
| Desktop mascots (BonziBuddy, Desktop Goose) | Toys | Our robot is useful: every movement explains something. |

Risk: a judge might say, "Copilot already does this." So the first 20 seconds of the video must show what Copilot doesn't do: **the robot flies over, points, waits for a click, praises the user, and guides them to the next step**, while the older adult does everything themselves.

## Where Meta models appear (make sure to show the judges)

| Model | Role | How to show it in the video |
|---|---|---|
| Muse Spark 1.3 | The brain: understands the request, writes a response in **MateScript**, and guides the lesson through **computer use** | Debug window showing a MateScript stream with highlighted tags |
| Muse Wonder 1.0 | Complex multi-step tasks (escalation if Spark gets confused) | A line in the debug window: "escalated to Wonder" |
| Muse Voice Transcribe | Real-time speech recognition for any language and mixed speech | Live subtitles appear as the older adult speaks |
| SAM 3.1 | Precisely outlines a button when the app has no UIA tree | Comparison: a rough box and SAM's outline around an icon |
| Muse Image | The robot character: concepts and emotions | A "how we designed the robot" frame in the README |
| MMS-TTS (Meta open weights) on Modal | The robot's voice | — |
| Muse Code (optional) | Another agent on our development team | A line in the README |

## What it takes to reach the top 5 (Gemini "judge" assessment, checked and expanded)

**Double down on:**
- A reliable main scenario, rehearsed on a prepared machine (125% DPI, two monitors, clean Windows install).
- Step-by-step mode with click verification: this is the "wow," not just highlighting.
- Hybrid "vision": UIA → computer use → SAM. Show the fallback in an app without UIA.
- A real older adult and an honest assessment (including criticism).
- Speed: a pre-recorded reply ("One moment…") within ~0.3 s of releasing the key, the first sentence of the real answer within ~2–2.5 s; a thinking animation and a quiet sound cover the gap.
- An installer that actually works for the judges (invite code).

**Cut ruthlessly:** the compiler, Linux (cut at the kickoff, D39), and anything that won't make it into the video.

**Sounds (rethought):** for older adults, sound is a **cue**, not noise:
- A quiet "ping" from the side of the screen the robot is pointing to (stereo panning).
- A soft "correct" sound after a successful click.
- A calm background sound while the model thinks, so it's clear the app hasn't frozen.

Fun mode (stories, drum, thunder, explosion) is for everyone else and the grandchildren; turn it on by voice. Loud and startling sounds are not allowed in calm mode.

## The first 20 seconds' "wow" (draft)

0:00–0:05 — An older adult's hand holds down the talk key: "How do I make the text bigger?"
0:05–0:12 — Mate glides across the screen to "Settings," shines his beam on it, a soft glow appears around the button, and a quiet ping sounds: "Right here. Click this."
0:12–0:18 — The person clicks it themselves → "Well done!" → the robot flies to the next step.
0:18–0:20 — Title card: **MetaMate — shows you, doesn't do it for you.**

## Judges' criteria and how we meet them

| Criterion (Indie) | How we meet it |
|---|---|
| Creativity | A teacher instead of an agent; a character; MateScript; sound cues |
| Technical ambition | Computer use with click interception + UIA + SAM, our own streaming language, real-time voice, multi-monitor DPI-aware overlay |
| Does it work? | Installer + proxy for judges, rehearsed scenarios, honest video without editing tricks |
