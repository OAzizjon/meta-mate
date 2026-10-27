# Cached phrases

Mate's short lines (the keys in `keys.txt`, the text in `locales/en.json` and `locales/ru.json`), synthesized with Meta's MMS-TTS (`facebook/mms-tts-eng`, `facebook/mms-tts-rus`) by `mm phrases build` (`tools/phrases.py`, `tts/mms.py`). The model weights are licensed **CC-BY-NC 4.0**, so these recordings are for non-commercial use, like the rest of the hackathon build (`docs/architecture.md` section 8).

`manifest.json` records each file with the SHA-256 of its text; `mm phrases check` fails when a locale line changed and its recording was not rebuilt.
