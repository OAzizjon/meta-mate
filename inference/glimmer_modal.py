"""Muse Glimmer (Meta's 30B open-weight model, distilled from Muse Spark) on a Modal GPU:
llama.cpp's OpenAI-compatible server behind the proxy's shared secret.

    modal run inference/glimmer_modal.py::download     # once: the GGUF weights into a Volume
    modal deploy inference/glimmer_modal.py             # lead only (D53)

The Q4_K_M build (~17 GB) and its vision projector fit an L4's 24 GB. Idle containers scale
to zero; `GLIMMER_WARM=1` at deploy time keeps one warm for demo and judging days. The
endpoint answers only requests carrying the secret the proxy holds (the `metamate-tts`
Modal secret, reused so no new secret has to be stored).
"""

import os
import subprocess

import modal

APP_NAME = 'metamate-glimmer'
SECRET_NAME = 'metamate-tts'
REPO = 'meta-models/Muse-Glimmer-30B-GGUF'
MODEL_FILE = 'Muse-Glimmer-30B-KQuant-17GB-Q4_K_M.gguf'
PROJECTOR_FILE = 'mmproj-Muse-Glimmer-30B-Q4_K_M.gguf'
WEIGHTS = '/weights'
PORT = 8080
WARM = os.environ.get('GLIMMER_WARM') == '1'

volume = modal.Volume.from_name('metamate-glimmer-weights', create_if_missing=True)

# llama.cpp's own CUDA server image; Python is added for Modal's runner.
server_image = modal.Image.from_registry('ghcr.io/ggml-org/llama.cpp:server-cuda', add_python='3.12').entrypoint([])
download_image = modal.Image.debian_slim(python_version='3.12').uv_pip_install('huggingface_hub>=0.26')

app = modal.App(APP_NAME)


@app.function(image=download_image, volumes={WEIGHTS: volume}, timeout=3600)
def download() -> None:
    """The weights into the Volume (public repo, no token needed)."""
    from huggingface_hub import hf_hub_download

    for name in (MODEL_FILE, PROJECTOR_FILE):
        hf_hub_download(REPO, name, local_dir=WEIGHTS)
    volume.commit()


@app.function(
    image=server_image,
    gpu='L4',
    volumes={WEIGHTS: volume},
    secrets=[modal.Secret.from_name(SECRET_NAME)],
    scaledown_window=300,
    min_containers=1 if WARM else 0,
    # One GPU at most: a leaked secret cannot run up more.
    max_containers=1,
    timeout=3600,
)
@modal.concurrent(max_inputs=4)
@modal.web_server(port=PORT, startup_timeout=900)
def serve() -> None:
    subprocess.Popen([
        '/app/llama-server',
        '--model', f'{WEIGHTS}/{MODEL_FILE}',
        '--mmproj', f'{WEIGHTS}/{PROJECTOR_FILE}',
        '--alias', 'muse-glimmer-30b',
        '--n-gpu-layers', '999',
        '--ctx-size', '8192',
        '--parallel', '4',
        # Glimmer reasons before it answers (like Spark); MetaMate's uses are short and fast,
        # and at ~15 tokens/s on an L4 the reasoning alone would take seconds.
        '--reasoning-budget', '0',
        '--host', '0.0.0.0',
        '--port', str(PORT),
        '--api-key', os.environ['MMS_TTS_SECRET'],
    ])
