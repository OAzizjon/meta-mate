---
name: metamate-submission
description: Use when preparing what the judges see: the demo video and its first 10 seconds, the backup video, the README above the fold, the landing page and installer, the judge's first 60 seconds, the Day-7 freeze, the tester recording and the Day-10 submission checklist (Days 6–10 of docs/plan.md).
---

# MetaMate submission

Sources: `docs/plan.md` Days 6–10 and "What to cut first", `docs/video.md`, `docs/edge.md` §4 and §7, `docs/strategy.md`, `docs/pointing-risks.md` (test matrix), `docs/copy.md` (landing taglines), `README.md`, `CHANGELOG.md`. Draft by Mistral (#410), checked and corrected.

## Never cut

The main scenario, step-by-step mode with click verification, screenshot privacy, a working installer, the backup video (`docs/plan.md`).

## Day 6 — landing, installer, judge path, pilot

- [ ] Proxy with invite codes and the daily budget; Modal warm on demo days.
- [ ] Per-user NSIS installer, icon, first launch with a pre-filled judge invite code; a SmartScreen "More info → Run anyway" picture next to the download (or signing).
- [ ] Landing page: one tagline for the adult child from `docs/copy.md`, three links (try it, download, video).
- [ ] Judge script, three lines: hold Right Ctrl, ask to make the text bigger, click where Mate shines.
- [ ] Evening pilot with the tester on the installed build; note what is unclear or annoying.
- [ ] JustLog3: shut down only with the owner's explicit yes (`docs/edge.md` §6 proposes keeping it until after 04.11).

## Day 7 — freeze and backup video

- [ ] Fix the pilot's issues first; then SAM fallback, "do it for me" and scenario 2 only if time remains.
- [ ] Tag `v0.9`, save the installer; from now on changes go to a branch and reach the demo build only after the 12-point matrix passes.
- [ ] Record the backup video of every scenario on the demo machine (OBS is installed).

## Day 8 — tester recording

- [ ] Tech check 1–2 days before; the backup tester on standby.
- [ ] Zoom: share the full screen (a single window hides Mate), "Record a separate audio file for each participant", "Optimize for 3rd party video editor", record locally; phone footage over the shoulder.
- [ ] 20–30 minutes recorded, 30–40 seconds used; keep one honest criticism; ask about face and name afterwards.

## Day 9 — video and README

- [ ] First 10 seconds: a real hand, "How do I make the text bigger?", Mate shines his beam, a wrong click, "Almost", the real click, the success chime; no logo before 0:10 (`docs/edge.md` §4 move 1).
- [ ] 0:35–1:05 the debug view: transcript, MateScript stream, redacted screenshot with UIA box vs SAM contour, TTS latency (move 5).
- [ ] README above the fold: an 8-second loop from the real build, the three links, the "clicks for you vs shows you where" meme; the open MateScript spec; a limits table (elevated windows, what is and is not redacted, the MMS-TTS licence) (moves 10, 11, 14).
- [ ] Linux: decided at 12:00 only if the whole Windows matrix is green.

## Day 10 — submit by 18:00 UTC

- [ ] Video 2:30 (max 3:00), English subtitles; final build; GitHub release; links on the site.
- [ ] Test matrix on a clean machine; Modal warm; proxy budget checked.
- [ ] Pre-kickoff disclosure in the README and `CHANGELOG.md`; MMS-TTS weights CC-BY-NC-4.0 stated.
- [ ] Submit by 18:00 UTC (23:00 Tashkent); the official deadline is 05.11 02:00 UTC.

## Honesty rules

Show privacy, don't claim it: the password goes black in the debug view before anything leaves the PC (D37). Never claim "works in any app". Never show Muse Wonder unless it was really used and logged.
