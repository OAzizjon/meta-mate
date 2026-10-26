---
name: metamate-meta-api
description: Use when MetaMate talks to the Meta Model API or measures it: spike S4, the Responses API with streaming and the computer-use tool, Muse Spark / Muse Wonder / Muse Voice Transcribe / SAM model choice, the $150 credit budget and PROXY_DAILY_BUDGET_USD, grounding accuracy on recorded fixtures, and the `mm probe` tool (models, stream, click, ledger) with its replay recordings.
---

# Meta Model API for MetaMate

Facts as of 2026-10-24 (re-verify at the kickoff): `docs/decisions.md` "Facts found on 2026-10-24", computer-use flow: `docs/architecture.md` §3 (A "where is X", B teacher). Keys live only in the lead's secrets file and on the proxy, never in the client (CLAUDE.md "Secrets"); non-Meta models are development fallbacks only (invariant 8).

## Models

- `muse-spark-1.3`: default for answers and computer use (weak on audio input). `muse-spark-1.2`: better with audio input. `muse-wonder-1.0`: preview, $4 / $20 per 1M tokens: escalation only after two failed steps, if at all (`docs/edge.md` §6).
- `muse-voice-transcribe-1.0`: speech to text, realtime `wss://api.meta.ai/v1/asr/realtime`, 25 languages. `sam-3.1`: boxes and masks from text, points or boxes (no-UIA fallback). No TTS in the API: MMS-TTS on Modal (CC-BY-NC-4.0).

## Computer use (teacher)

- Responses API, `tools: [{"type": "computer"}]`. The model returns a `computer_call` with an `actions` batch (click, double_click, drag, keypress, move, scroll, type, screenshot, wait) in pixels of the **last screenshot sent**. We never execute it: each action becomes a sub-step the human performs; then a new screenshot of the same size goes back as `computer_call_output`.
- `pending_safety_checks` are shown to the human, never auto-acknowledged (invariant 4). Ask for one action per call; split a batch into ordered sub-steps.
- The spoken line comes from the turn's text if the API returns one next to the call [verify in S4], else from a fast parallel Spark call; a pre-cached phrase covers the wait.

## `mm probe` (lead only; agents are refused)

```
mm probe models [--provider meta]                 # confirm model ids after the kickoff
mm probe stream "question"                        # live text, time to first token, total, tokens, cost
mm probe click FIXTURE "question" [--max-edge 1280] [--mode computer|point] [--dry-run]
mm probe ledger                                   # spend so far per provider
```

- `click` sends `fixtures/uia/FIXTURE.png` (refused if the fixture has a password-like field, a truncated tree, no per-monitor DPI, a PNG of the wrong size, or a link), maps the model's click back to physical pixels and names the UIA element: `HIT` (on an interactive element or within 8 px), `NEAR` (within 40 px: the teacher's "Almost"), `MISS`.
- `--mode point` asks for a MateScript `<point eNN>` answer (mode A) and works with any vision model; `computer` uses the computer-use tool (Meta).
- Every exchange is recorded (key scrubbed, image by path) in `fixtures/replay/<case>/probe-*.json` for offline replay tests. `fixtures/uia/synthetic-text-size` is a made-up page for tests.
- Budget: `--budget` dollars per provider, `--max-unpriced` calls of unknown price (pass `--price IN OUT` once Spark's price is known); the ledger is `~/.cache/metamate/probe-ledger.jsonl`. `META_API_BASE` in the secrets file overrides the guessed base URL (must be https).
- Dev fallback that works from Tashkent: `--provider openrouter --model meta-llama/llama-4-maverick` (Groq is blocked by Cloudflare here; the OpenAI key has no credits).

## S4 checklist (evening of Day 1)

1. `mm probe models` → note the exact ids; `mm probe stream` twice → TTFT and total.
2. `mm probe click` on the Settings and Edge fixtures with `--mode computer` → the `computer_call` JSON shape (`actions` vs `action`), safety checks, text next to the call; then `--mode point` for mode A.
3. Cost per request at `--max-edge 1280` and `960` (image tokens dominate) → size `PROXY_DAILY_BUDGET_USD` so development, testers and judges fit in $150 (`docs/edge.md` §7).
4. Log the findings in `docs/decisions.md`; keep two good replays per scenario for the agents.
