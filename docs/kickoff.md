# Kickoff runbook (Day 1, 2026-10-26)

What happens from the end of the welcome stream (T0, about 23:00 Tashkent: the kickoff is 22:00, D38) to the S1/S3 checkpoint on 27.10 at 15:00, so nobody waits for anybody. Timings and fallbacks: [`edge.md`](edge.md) §1; the plan: [`plan.md`](plan.md) Day 1.

## Before T0 (until 21:00, no product code)

- Owner: record the UIA fixtures on the Demo user (`mm uia --save NAME`, list in [`project-structure.md`](project-structure.md)); the login form twice, hidden and revealed (tree only, D37).
- Lead: `mm doctor`, `mm check`, `mm test` green; the event monitor and the `mm-check` watcher running.

## T0 + 0:30 — the rules

1. The owner shares the rules (link or text). The lead saves them to the scratchpad, never into the repo (they are the organisers' text).
2. Gemini, one part per account (`team.py fan gemini ask --attach rules.txt`):
   - eligibility and pre-event work: is our pre-kickoff material allowed, what must be disclosed (README "Prepared before the kickoff", `CHANGELOG.md`), "Meta" in the product name, licences (MMS-TTS is CC-BY-NC-4.0);
   - submission requirements against `plan.md` and `video.md`: video length, public repo, demo build, deadlines and time zone;
   - judging criteria against `edge.md` §4: which moves gain or lose weight.
3. The lead logs the answers as new decisions in `decisions.md`, updates `plan.md`, and fixes the product name in `APP_NAME` if needed (invariant 6).

## T0 + 1:00 — first commit

`git status` (nothing ignored or secret staged), `mm check`, then the first commit: docs, tooling, art and sounds, with an honest message such as `Pre-kickoff preparation: docs, tooling, art, sounds (see README)` (D31). From here on `team.py ... review` works, because there is a commit to diff against.

## T0 + 1:00 to 2:00 — scaffold (lead)

- Workspace `Cargo.toml` with `crates/mm-core`, `crates/mm-platform`, `crates/mm-win32`, `src-tauri`; Vite + TypeScript in `src/` from the v2 template (`@tauri-apps/cli@2`, `tauri@2`, D34; the warm copy is in `C:\mmw\tauri-warm`).
- `.cargo/config.toml`: `rustc-wrapper = "sccache"`, short `target-dir`; the dev profile from `edge.md` §5.
- `team.py watch -C . --name cargo --ext rs,toml -- cargo check --workspace`.
- `mm check` and `mm test` green → second commit. Then the team starts.

## T0 + 1:30 — the team's night jobs (S1/S3 follow in the morning)

| Agent | Task | Done when |
|---|---|---|
| Codex `edit` | MateScript parser in `mm-core` (prompt below) | the 25 cases of `matescript-cases.md` and the chunk fuzz pass in `mm agent test` |
| Codex `edit` | `PhysPx` / `ShotPx` / `CssPx` newtypes and `FrameTransform` in `mm-core` (convention 2), tests for 100/125/150 % and a negative-X monitor | mixing the types does not compile; tests pass |
| Claude `edit` | `mm-platform`: the five traits, `MockBackend`, `ReplayBackend` reading `fixtures/` | a test replays the Settings fixture |
| Copilot `edit` | `locales/en.json` and `ru.json` from `copy.md`, plus the key/placeholder test (convention 8) | the test passes |
| Gemini `fan ask` | S1, S3 and S4 checklists: exact Tauri v2 / `windows` 0.62 / `uiautomation` 0.25 calls, pitfalls (`edge.md` §5 field notes), how to test each | three checklists, every API marked verified or [verify] |
| Mistral | third review of every patch above | — |

In the morning the lead reviews the night's patches, then does S1 (overlay, DPI, no focus steal) and S3 (UIA from Rust) in `mm-win32`; **checkpoint 27.10 at 15:00** (`plan.md`).

### Prompt: MateScript parser (Codex)

> In `crates/mm-core`, add a `matescript` module (one concept per file, convention 12): the streaming parser of `docs/matescript.md` §4 (sentence segmentation, the TEXT / LT / TAG / DISCARD state machine and its length limits) with the failure rules of §5. Input arrives in arbitrary chunks; output is **one ordered timeline** of items (say, sfx, mood, point, step, wait, pause, ambience, lang). No I/O, no async, no `windows` crate (the `mm-core` rule). Tests: every case in `docs/matescript-cases.md` as a golden test (`insta` snapshots), and a `proptest` that splits each case into random 1–20-character chunks and must produce the identical timeline. Run `python tools/mm.py agent test`.

### Prompt: coordinates (Codex)

> In `crates/mm-core`, add `PhysPx`, `ShotPx` and `CssPx` as distinct newtypes (points and rects) and a `FrameTransform` built per captured frame (`docs/architecture.md`, coordinates section): physical virtual-screen pixels ↔ screenshot pixels after downscaling ↔ CSS pixels of one monitor's scale. Conversions only through the transform; mixing types must not compile (add a `compile_fail` doc test). Tests: 100/125/150 % scaling, a monitor at negative X, a downscaled screenshot, rounding at the edges. Run `python tools/mm.py agent test`.

## When the credits arrive — S4

`mm probe` (skill `metamate-meta-api`; tested on 2026-10-26 against a development fallback): `mm probe models`, `mm probe stream "..."`, then `mm probe click FIXTURE "..." --mode computer` on a recorded Settings fixture; note the batch format, `pending_safety_checks`, whether text comes next to `computer_call`, and the cost per request (it sizes `PROXY_DAILY_BUDGET_USD` for the $150, `edge.md` §7). Credits late → S4 the moment they arrive, S2 meanwhile (PTT hook with the `GetAsyncKeyState` release watchdog).
