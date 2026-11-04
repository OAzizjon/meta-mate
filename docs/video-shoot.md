# Video shoot: final night (4-5 November 2026)

The final plan for the video, 2:56 long (the limit is 3:00). It replaces the older cut in
[video-edit.md](video-edit.md); the recording checklist stays in [video.md](video.md). Built from
four Muse Wonder judge reviews (Impact, Use of Meta AI tooling, Technical depth, Demo) and checked
against the README and the code. Motion graphics: `node tools/render_clips.mjs` renders
`video/clips/` to `video/out/`.

## The rule for every take

Never stage or composite a result. A take that fails is retaken or cut. Every number keeps its
scope: "40 made-up pages", "610 answers with the button list". Mate speaks English and Russian today.

Before rolling: Windows at 100 % or 125 %, a clean desktop, notifications off, speakers on, OBS
recording the full screen at 1920x1080 and 30 fps, MetaMate running, and one warm-up question
asked (it wakes the voice GPU). The model chip is on every answer; Developer mode is needed only for take F.

## 1. Screen takes (OBS), in recording order

| Take | Before | Ask | Must be visible | If it fails | Length |
|---|---|---|---|---|---|
| A | Edge with `fixtures/demo/scam-popup.html` | Hold Ctrl: "Is this safe?" | The voice bar with your words, Mate sad, the beam on the phone number, the SCAM ALERT stamp | Use `fixtures/demo/scam-email.html`; ask once more | 10 s |
| B | First film 3 s of yourself typing your own note on Home, under Edit next to your name ("Proud of you. Call me when you're done."): the note must be real and visibly written by a person. Then Windows Settings | Hold Ctrl: "How do I make the text bigger?" | The beam on Accessibility, your click, badge 1/2 then 2/2, one click just off target with the dotted trail and "Almost", the slider, "All done", the note, the keepsake card, Print | If the card takes over 20 s, cut to the card saved by the warm-up lesson in `Pictures\MetaMate` | 47 s |
| D | Notepad or a plain window | Hold Ctrl: "Where is the Pay now button?" | Mate saying he can't see it; no beam on a guess | Retake once, else cut | 6 s |
| E | A fictional page from `fixtures/eval/safety/en-legit` | "What is going on on this page?" then "Say that again, slower" | Mate's short summary in the bubble, then the slower repeat | Keep only the first question | 7 s |
| F | Settings, For judges, Open Under the hood, then ask one question | "Where is the search box?" | Model, timings, the MateScript timeline, fields blacked out | Skip the question, show the last answer | 16 s |
| G | Home, "Mate's brain"; Chat | Pick Muse Glimmer 30B, then ask "What is the cloud?" | The three model cards, the model chip under the answer | If it falls back to Spark, keep it: the chip shows the truth, and do not say "our own GPU" over it | 9 s |

Optional, only if the night allows: Home, Practice: spot the scam (`SPOTTED!`); a photo of the
printed card on a fridge for the last seconds of B and for the X post.

## 2. Camera pieces (laptop camera), read from the teleprompter

Look into the lens, short sentences, a pause after each. Record each piece 3 times.

**TH1 (19 s).** Every family has this phone call: "Where do I click?" You are far away. You can't
see their screen. They are scared to press the wrong thing. So I built Mate: a small light bulb on
their Windows PC. They ask out loud. He shines a beam on the exact button. They click it themselves.

The personal version replaces the first sentence only if it is your own story: "My parents live far
from me. When something breaks on their computer, I can't help."

**TH2 (14 s).** He never presses Pay. Never Delete. Nothing with a password. If you ask, he presses
a normal button, and only after you say yes. And passwords are painted black before anything
leaves the PC.

**TH3 (12 s).** One installer. No account to try. English and Russian today. Try it at meta-mate dot
stream. Mate lights the button. Your parent clicks it.

## 3. Voice-over: Mate narrates himself

Generated, not read aloud: Mate's own MMS-TTS voice through the live proxy (`video/voice/V1.wav` to `V6.wav`). The voice in the video is the model named in it.

**V1, over the card (10 s).** After each lesson, a card to keep. Muse Glimmer, Meta's open model on our own GPU, writes the steps. Muse Image draws the picture.

**V2, over take D (3 s).** And when it isn't there, I say so.

**V3, over take F (20 s).** This is how I really work. Muse Voice Transcribe hears you while you talk. Muse Spark finds the button in the list Windows gives us. MMS-TTS speaks. That is this voice. Every answer shows its model, its timing, and what was blacked out.

**V4, over `seven-models` and take G (9 s).** Seven Meta models. Each one has a job. You choose my brain: quick, careful, or open weights on our own GPU.

**V5, over `scam-stamp` and `pointing-610` (18 s).** We measured it. On forty made-up pages, I caught every scam. When the button was on the screen and Windows listed it, I was right all six hundred ten times. Without that list, less than half. We report both numbers.

**V6, over `scale` (11 s).** A son or daughter installs me once. A new app needs no new code where Windows lists its buttons. Where it doesn't, I say so.

## 4. The cut

Music: one quiet track, -12 dB under every voice, out at 2:42.

| Time | Picture | Caption | Sound |
|---|---|---|---|
| 0:00-0:10 | Take A | "Is this safe?" | Mate's verdict, no voice-over |
| 0:10-0:29 | TH1 | name lower third | TH1 |
| 0:29-1:03 | Take B, lesson | "Hold Ctrl. Ask out loud." / "You click. Not Mate." / "Wrong click? No red. Just 'Almost'." | Mate's steps, "All done" |
| 1:03-1:16 | 3 s of the note being typed on Home, then take B, note and card | "Written by whoever set Mate up" / "Steps: Muse Glimmer 30B" / "Picture: Muse Image" | Mate reads the note, then V1 |
| 1:16-1:22 | Take D | "Not on this screen, so he says so." | Mate, V2 |
| 1:22-1:29 | Take E | "What is going on on this page?" | Mate |
| 1:29-1:43 | TH2 | | TH2 |
| 1:43-1:46 | clip `passwords-black` | (in the clip) | music |
| 1:46-2:02 | Take F | "Under the hood" | V3 |
| 2:02-2:15 | clip `seven-models`, then take G | `lower-third-model` | V4 |
| 2:15-2:32 | clips `scam-stamp`, `pointing-610` | (in the clips) | V5 |
| 2:32-2:42 | clip `scale` | (in the clip) | V6 |
| 2:42-2:56 | TH3, `outro` over the last 2 s | meta-mate.stream | TH3, music out |

If it runs long, cut take E first, then the second half of V5. English subtitles on everything.

Left out on purpose: the `screamer` clip breaks the trust the lesson builds, so it stays for vertical
social posts (`screamer-vertical`, `cold-open-vertical`), and `problem` and `three-steps` delay the
real product.
