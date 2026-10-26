# Video: storyboard, interview with an older adult tester, consent

The video length will be limited by the rules; plan for **2:30**, maximum 3:00. Language: English, tester's speech as-is, English subtitles (just in case; Zoom audio can be poor).

## Storyboard (2:30)

| Time | Shot | Audio | Meta shown |
|---|---|---|---|
| 0:00–0:20 | **Hook.** Tester (on Zoom, with their screen beside them) holds down the talk key: “How do I make the text bigger?” The robot flies over, points, the person clicks it themselves, “Well done!”, next step | Tester’s voice → robot’s voice, soft ping | — |
| 0:20–0:35 | The problem: “I call my grandson every time.” Montage of confusing interfaces | Voiceover (you) | — |
| 0:35–1:05 | **How it works.** Screen + debug window: MateScript stream, UIA tree, computer use click intercepted → robot points; SAM outline around an icon with no UIA | Voiceover | Muse Spark, computer use, SAM 3.1, Voice Transcribe |
| 1:05–1:30 | Scenario 2 (video call/browser), step by step, incorrect click → “Almost” | Live audio | — |
| 1:30–1:45 | Fun mode: “Tell me a short story” → forest, thunder, birds | Sounds | MateScript |
| 1:45–2:10 | **Honest tester feedback** (what they liked and didn't like) | Tester | — |
| 2:10–2:30 | Privacy (active window only, passwords masked), installer, open source, Meta stack, title card | Voiceover | Model list |

## Recording over Zoom: how to make it reliable

1. **In advance (1–2 days before):** 10-minute test call. Check the microphone, lighting, and that Zoom is recording.
2. **Install for the tester (during the 31.10 pilot):** installer + invite code. Windows will show SmartScreen (“Windows protected your PC”) if the build isn't signed: send an image in advance showing “More info → Run anyway.” Ideally, have a family member help install it, or help over Zoom using “remote control.”
3. **Conversation mode for the tester:** “press — talk — pause” (no need to hold a key): older adults may find it hard to hold a key and speak at the same time. Enable it in settings in advance.
4. **During recording:**
   - Zoom → Settings → Recording: **Record a separate audio file for each participant**, **Optimize for 3rd party video editor**, record **locally** (better quality).
   - Tester shares the **entire screen (Screen / Desktop), not a single window**: otherwise the robot and highlighting won't appear in the stream. This is the main footage; face is a small window.
   - In parallel, ask a family member to film **over the tester's shoulder** on a phone (screen + hands) — it's the most natural shot.
   - Tester wears headphones, or you mute your audio so the robot doesn't cause Zoom echo.
5. **Don't give them a script.** Just give the task (“Try making the text bigger”) and stay quiet. Real pauses and mistakes show that it's genuine.
6. Record a longer session (20–30 minutes); use 30–40 seconds in the video.

## Plan B

- **Backup video** of all scenarios recorded on the demo machine on the evening of 01.11 (frozen build v0.9): we'll have a video to submit even if the Zoom recording fails.
- **Backup tester** (a second older adult) available on 02.11 or 03.11.
- If Zoom audio is poor: ask the tester to record on a phone nearby and send you the file.

## Questions for the tester (in English)

1. What on your computer makes you feel stuck most often?
2. When you get stuck, what do you usually do?
3. *(Task)* Please ask the robot to help you make the text bigger. *(stay quiet and watch)*
4. Was the robot's pointing clearer than someone explaining on the phone? Why?
5. Was anything confusing, too fast, or annoying? The sounds, the voice, the robot?
6. Would you use it alone, when nobody is around to help? What would make you trust it more?
7. If you could change one thing, what would it be?

After recording: ask if it's okay to show their face and name (or just their name / only their hands and screen).

## Consent form (for the tester, in English)

> This is a template, not legal advice. Send it in advance and get a written “I agree” (by email/message) or a signature. If the tester lives in the EU, face and voice recordings count as personal data, so written consent is especially important.

---

**Recording consent — MetaMate test session**

Hello {first name},

Thank you for helping test MetaMate, a free helper for using a computer. I would like to record our video call so I can show it in my entry for the Meta Global AI Developer Hackathon.

What will be recorded: your voice, your face (only if you agree below) and your computer screen during the test. Please close any private windows (bank, e-mail, messages) before we start.

How it will be used: short parts (about one minute in total) may appear in the hackathon submission video, on the project website and on social media about the project. The full recording stays private and will not be shared.

Your choices (please reply with your choices):
- [ ] You may show my face.
- [ ] Use only my voice and my screen, not my face.
- [ ] You may show my first name / [ ] Please do not show my name.

You can change your mind: tell me before **{date, e.g. 3 November 2026}** and I will not use the recording. There is no payment, and you can stop the session at any time.

If you agree, please reply "I agree" with your choices.

Thank you!
{your name}, {contact}
