# Changelog

All notable changes to MetaMate are recorded in this file.

This changelog follows [Keep a Changelog 1.1.0](https://keepachangelog.com/en/1.1.0/) and [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

<!-- Hackathon build: 2026-10-26 .. 2026-11-04 -->

- Rust workspace in `Cargo.toml` with `crates/mm-core` for pure core names and coordinate types, and `crates/mm-platform` for OS service traits.
- `mm_core::geom` physical, screenshot, and CSS pixel types, per-frame `FrameTransform`, per-monitor `MonitorTransform`, tests, and a compile-fail doctest.
- English and Russian locale files in `locales/en.json` and `locales/ru.json`, based on `docs/copy.md`, with a model-choice setting, plus `tools/tests/test_locales.py`.
- `mm probe --reasoning EFFORT` for Muse Spark reasoning before answering; `minimal` reduces first-token latency from about 10 seconds to about 2 seconds, and `META_KEY` aliases the Meta API key.
- Spike S4 Meta API replays in `fixtures/replay/`.

### Changed

- README status now says the hackathon build is in progress and Windows-only; Muse Wonder is an optional choice in settings.
- Linux X11 support was cut, and the JustLog3 shutdown checklist in `docs/server.md` was extended (decisions D39-D42 in `docs/decisions.md`).
- `mm test` now also runs Rust doctests; agent Rust builds run without sccache and incremental compilation.

## Prepared before the kickoff (2026-10-24 .. the morning of 2026-10-26)

- Design documents in `docs/`: [strategy](docs/strategy.md), [decisions](docs/decisions.md), [plan](docs/plan.md), [edge](docs/edge.md), [architecture](docs/architecture.md), [MateScript](docs/matescript.md) and its [test cases](docs/matescript-cases.md), [pointing risks](docs/pointing-risks.md), [sounds](docs/sounds.md), [robot](docs/robot.md), [prompts](docs/prompts.md), [copy](docs/copy.md), [video](docs/video.md), [Reddit research](docs/research-reddit.md), [server](docs/server.md), [Windows setup](docs/setup-windows.md), and [project structure](docs/project-structure.md).
- The `mm` CLI and checks in `tools/`: doctor, status, plan, check, test, uia, art, sounds, agents, keys, and the restricted agent mode.
- The UI Automation dump tool (`tools/uia_dump.ps1`): owned popups, black gaps between frames, refusal of screenshots with a password-like field (including revealed passwords), `-SelfTest`.
- `mm probe` (`tools/mm_probe.py`, `tools/mm_fixture.py`): the Meta Model API from the lead's machine (stream timings, one computer-use or `<point>` turn on a fixture with a hit test, replay recordings, a dollar ledger), tested on a development fallback with a synthetic fixture.
- Claude Code project skills in `.claude/skills/`: Windows core, Meta API, submission.
- Synthesized cue sounds (our own, CC0), CC0/public-domain sound effects and ambience from Wikimedia Commons with a licence-checked fetcher, every file listed in `assets/sounds/LICENSES.md`.
- Mate concept art in `assets/art/`.
- Secret protection: a Claude Code hook and deny rules that keep secrets files out of reach, and the variable names (never values) in `env_keys_names.txt`.

No application code existed before the kickoff, as noted in the [README](README.md#prepared-before-the-kickoff).
