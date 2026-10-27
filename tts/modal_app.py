"""MMS-TTS on a Modal GPU (spike S6): the same `POST /synthesize` contract as
tts/serve_dev.py, behind the shared secret the proxy sends (`X-MMS-Secret`).

    python tts/deploy_modal.py          # lead only: secret, deploy, server/.dev-tts.json

The weights are baked into the image, so a cold start never downloads. Idle containers
scale to zero; `MMS_TTS_WARM=1` at deploy time keeps one warm for demo and judging days.
MMS-TTS weights are CC-BY-NC 4.0 (docs/architecture.md section 8).
"""

import os
from pathlib import Path

import modal

HERE = Path(__file__).resolve().parent
APP_NAME = 'metamate-tts'
SECRET_NAME = 'metamate-tts'
CHECKPOINTS = ('facebook/mms-tts-eng', 'facebook/mms-tts-rus')
MAX_TEXT = 300
MAX_BODY = 4096
WARM = os.environ.get('MMS_TTS_WARM') == '1'


def download_checkpoints() -> None:
    """Build step: both checkpoints into the image's Hugging Face cache."""
    from transformers import AutoTokenizer, VitsModel

    for name in CHECKPOINTS:
        VitsModel.from_pretrained(name)
        AutoTokenizer.from_pretrained(name)


image = (
    modal.Image.debian_slim(python_version='3.12')
    # PyPI's Linux torch wheel brings its CUDA 12 runtime; versions as tested locally.
    .uv_pip_install('torch==2.14.1', 'transformers==5.20.2', 'numpy==2.5.4', 'fastapi[standard]==0.162.0')
    .env({'HF_HOME': '/models'})
    .run_function(download_checkpoints)
    .env({'HF_HUB_OFFLINE': '1'})
    # Our code last, so editing it never rebuilds torch.
    .add_local_file(HERE / 'mms.py', '/root/mms.py')
    .add_local_file(HERE / 'text_norm.py', '/root/text_norm.py')
)
app = modal.App(APP_NAME, image=image)


@app.cls(
    gpu='T4',
    secrets=[modal.Secret.from_name(SECRET_NAME)],
    scaledown_window=300,
    min_containers=1 if WARM else 0,
    # A leaked secret or a burst cannot run up more than two GPUs at once.
    max_containers=2,
    timeout=60,
)
@modal.concurrent(max_inputs=4)
class Tts:
    @modal.enter()
    def load(self) -> None:
        import mms

        secret = os.environ.get('MMS_TTS_SECRET', '')
        if len(secret) < 16:
            raise RuntimeError('MMS_TTS_SECRET (16+ characters) is required')
        self.secret = secret.encode('utf-8')
        for lang in mms.CHECKPOINTS:
            mms.load(lang)
            mms.synthesize(lang, 'Ready.' if lang == 'en' else 'Готово.')  # warms the CUDA kernels

    @modal.asgi_app()
    def web(self):
        import asyncio
        import hmac
        import json
        import time

        import mms
        from fastapi import FastAPI, Request
        from fastapi.responses import Response

        api = FastAPI(docs_url=None, redoc_url=None, openapi_url=None)

        def empty(status: int) -> Response:
            return Response(b'{}', status_code=status, media_type='application/json')

        @api.get('/healthz')
        async def healthz() -> Response:
            return empty(200)

        @api.post('/synthesize')
        async def synthesize(request: Request) -> Response:
            given = request.headers.get('x-mms-secret', '').encode('utf-8', errors='replace')
            if not hmac.compare_digest(given, self.secret):
                return empty(401)
            body = b''
            async for chunk in request.stream():
                body += chunk
                if len(body) > MAX_BODY:
                    return empty(413)
            try:
                payload = json.loads(body)
                lang, text = payload['lang'], payload['text']
                if lang not in mms.CHECKPOINTS or not isinstance(text, str) or not 0 < len(text) <= MAX_TEXT:
                    raise ValueError
            except (ValueError, KeyError, TypeError, RecursionError):
                return empty(400)
            started = time.perf_counter()
            try:
                # The GPU call off the event loop: other requests keep flowing meanwhile.
                samples, rate = await asyncio.to_thread(mms.synthesize, lang, text)
            except Exception:  # nothing speakable after normalization ("..."), or a model error
                return empty(422)
            wav = mms.to_wav(samples, rate)
            elapsed = round(1000 * (time.perf_counter() - started))
            return Response(wav, media_type='audio/wav', headers={'X-Synth-Ms': str(elapsed)})

        return api
