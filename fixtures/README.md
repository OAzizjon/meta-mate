# Fixtures

Recorded screens for tests that run without a live desktop (see `docs/project-structure.md`).

- `uia/NAME.json` + `uia/NAME.png`: `mm uia --save NAME` (UI Automation tree + window screenshot, physical pixels).
- `uia/synthetic-*`: made-up pages rendered by headless Edge with a hand-written tree whose rects match exactly (not recordings; safe for any model).
- `replay/<case>/probe-*.json`: `mm probe` exchanges (request with the image by path, the model's answer, the hit test); the key is scrubbed.
- `replay/<case>/`: `capture.png`, `frame.json` (frame transform), `uia.json`, `scenario.yaml` (request, mocked model output, expected target or refusal).

Record only on the **Demo** Windows user: no private windows, mail, chats or bank pages. The pre-commit hook asks you to confirm new fixtures were checked (`MM_FIXTURES_REVIEWED=1`).
