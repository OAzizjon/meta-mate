# Cerebral Valley submission

**Project name**  
MetaMate

**Tagline**  
Mate lights the button. Your parent clicks it.

**One-sentence pitch**  
MetaMate is a Windows desktop helper that listens to questions in English or Russian, answers aloud, and guides people to the right control in the app they are using without taking over.

**Description**
Your parent is stuck in an unfamiliar program and cannot find the right button, and you are far away. Mate is a small light bulb on the Windows desktop. Hold Ctrl, ask out loud in English or Russian, and he answers aloud, flies beside the right control and shines a beam on it. You click it yourself. For "how do I" questions he teaches one step at a time and checks each click. "Is this safe?" looks only at the active window and names the red flags it sees. After a lesson he leaves a printable card.

Meta's models each do one job: Muse Voice Transcribe hears you; Muse Spark 1.3 answers, points, teaches and runs the scam check; Muse Wonder 1.0 plans the next step when you click away from the beam twice; MMS-TTS speaks; Muse Glimmer 30B writes the card; Muse Image draws it; SAM 3.1 (experimental) outlines what lies under a point where Windows can't read the app.

Measured on our own 20 fictional windows and 71 questions: with the Windows element list, all 610 answers to a question whose button was on the window named the right control. Without it, under half landed on the control, so we use the list.

Only the window you are using is captured, only when the question needs the screen. Password fields are painted black first, and it fails closed. Mate never presses Pay, Delete, money-transfer or password buttons. "I can't find it" beats a guess.

Mate lights the button. Your parent clicks it.

**Meta models used**
- Muse Spark 1.3 — default quick answers, pointing, step-by-step lessons, and scam checks.
- Muse Wonder 1.0 — thorough answers and next-step planning after two clicks away from the beam on Quick.
- Muse Voice Transcribe — realtime push-to-talk speech recognition.
- MMS-TTS — spoken voice output (Meta open weights, served on Modal GPU).
- Muse Glimmer 30B — chat answers and plain-language keepsake lesson card text (Meta open weights on Modal GPU).
- Muse Image — custom illustrations for printable keepsake lesson cards.
- SAM 3.1 — experimental outlines of controls where the accessibility tree has nothing.

**Links**
- Video: <VIDEO_URL>
- Repository: https://github.com/OAzizjon/meta-mate
- Website: https://meta-mate.stream

**Prize categories**
- Best Agentic Use-Case — Mate teaches one step at a time, verifies each target was clicked, and only performs an optional action after the person says yes.
- Best Creative Build with Muse Image — Muse Image creates a custom, soft-gouache illustration for each printable lesson card.
- Best Build on Meta Open Weight Models with Muse Glimmer 30B — Muse Glimmer 30B provides chat answers and rewrites lesson steps in plain language for printable cards, running on our Modal GPU.

**X posts**
1. MetaMate helps you find the right button without taking over. Ask aloud in English or Russian; Mate answers, points, and guides one step at a time. Built with Meta's Muse models. https://meta-mate.stream
2. Every completed MetaMate lesson can become a printable keepsake card: Muse Glimmer 30B rewrites the steps, and Muse Image illustrates them. A little help you can keep. https://meta-mate.stream
3. Your mum can find the right button without guessing. Mate shines a beam on it on her screen; she clicks it herself. He won't press Pay, Delete or password buttons. https://meta-mate.stream
4. After a lesson, Mate reads the note the family left and prints it on a keepsake card for your parent. They hear your words and can hold them. https://meta-mate.stream
