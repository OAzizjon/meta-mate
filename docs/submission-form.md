# Cerebral Valley submission

**Project name**  
MetaMate

**Tagline**  
Mate lights the button. You click it yourself.

**One-sentence pitch**  
MetaMate is a Windows desktop helper that listens to questions in English or Russian, answers aloud, and guides people to the right control in the app they are using without taking over.

**Description**  
MetaMate is a Windows desktop helper for older people and anyone navigating an unfamiliar interface. Ask aloud in English or Russian: Mate answers with his voice, shines a beam on the relevant control, and teaches tasks one step at a time while the person clicks. He never presses Pay, Delete, money-transfer, or password buttons; optional actions require a spoken yes. For “Is this safe?”, he checks the active window, names visible red flags, and never advises clicking, calling, or paying. Muse Voice Transcribe recognizes speech; Muse Spark 1.3 answers, points, teaches, and checks for scams; Muse Wonder 1.0 provides thorough answers and plans a lesson's next step after two clicks away from the beam. SAM 3.1 outlines controls when the accessibility tree is empty. MMS-TTS speaks, and Muse Glimmer 30B plus Muse Image create printable lesson cards. In tests on 40 fictional pages, both Spark and Wonder called every scam a scam; results are from fictional pages, not a real-world rate. Built with Rust, Tauri v2, a TypeScript WebView, and a FastAPI proxy. Next, we want to learn from real sessions and improve safety checks for less common window types.

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
