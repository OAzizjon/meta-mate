"""MMS-TTS (Meta's open weights, CC-BY-NC-4.0): one VITS checkpoint per language.

The core the Modal service wraps (docs/architecture.md section 8). Run locally to make
test speech or to time synthesis on this machine:

    C:/mmw/py-tts/Scripts/python.exe tts/mms.py --lang eng --text "How do I make the text bigger?" --out sample.wav

Uses only the Hugging Face cache (HF_HUB_OFFLINE=1): no download at run time.
"""

import argparse
import io
import os
import sys
import time
import wave

os.environ.setdefault('HF_HUB_OFFLINE', '1')

import numpy as np  # noqa: E402
import torch  # noqa: E402
from transformers import AutoTokenizer, VitsModel  # noqa: E402

# Language codes as MateScript's <lang> sends them (ISO 639-1) -> MMS checkpoints.
CHECKPOINTS = {'en': 'facebook/mms-tts-eng', 'ru': 'facebook/mms-tts-rus'}
_loaded: dict[str, tuple[VitsModel, AutoTokenizer]] = {}


def load(lang: str) -> tuple[VitsModel, AutoTokenizer]:
    if lang not in CHECKPOINTS:
        raise ValueError(f'no MMS-TTS checkpoint for {lang!r}')
    if lang not in _loaded:
        name = CHECKPOINTS[lang]
        _loaded[lang] = (VitsModel.from_pretrained(name).eval(), AutoTokenizer.from_pretrained(name))
    return _loaded[lang]


def synthesize(lang: str, text: str) -> tuple[np.ndarray, int]:
    """Mono float32 samples in [-1, 1] and the sample rate (16 kHz for MMS)."""
    model, tokenizer = load(lang)
    inputs = tokenizer(text, return_tensors='pt')
    with torch.no_grad():
        waveform = model(**inputs).waveform[0].numpy()
    return waveform.astype(np.float32), int(model.config.sampling_rate)


def to_wav(samples: np.ndarray, rate: int) -> bytes:
    pcm = (np.clip(samples, -1.0, 1.0) * 32767).astype('<i2').tobytes()
    out = io.BytesIO()
    with wave.open(out, 'wb') as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(rate)
        w.writeframes(pcm)
    return out.getvalue()


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument('--lang', default='en', choices=sorted(CHECKPOINTS))
    parser.add_argument('--text', required=True)
    parser.add_argument('--out', required=True)
    args = parser.parse_args(argv)
    started = time.perf_counter()
    load(args.lang)
    loaded = time.perf_counter()
    samples, rate = synthesize(args.lang, args.text)
    done = time.perf_counter()
    with open(args.out, 'wb') as f:
        f.write(to_wav(samples, rate))
    seconds = len(samples) / rate
    print(f'{args.lang}: load {1000 * (loaded - started):.0f} ms, synth {1000 * (done - loaded):.0f} ms '
          f'for {seconds:.2f} s of audio (real-time factor {(done - loaded) / seconds:.2f}), {rate} Hz')
    return 0


if __name__ == '__main__':
    sys.exit(main())
