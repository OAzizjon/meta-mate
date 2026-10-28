"""The proxy on Modal (D57): the same FastAPI app as `server/run_dev.py`, served by Modal's
ASGI runner over https, WebSockets included (/v1/asr).

    python server/deploy/deploy_modal_proxy.py    # owner: secrets into Modal, deploy, link

Settings come from the Modal secret `metamate-proxy` (written by that script from the local
secrets file; never on a command line). One container, always warm: the ledger of the
day's spend lives in its memory (policy.Ledger), so a second container or a cold start
would count from zero again.
"""

from pathlib import Path

import modal

APP_NAME = 'metamate-proxy'
SECRET_NAME = 'metamate-proxy'
SERVER = Path(__file__).resolve().parents[1]

image = (
    modal.Image.debian_slim(python_version='3.12')
    .pip_install_from_requirements(str(SERVER / 'requirements.txt'))
    .add_local_dir(str(SERVER / 'metamate_proxy'), '/root/metamate_proxy')
)

app = modal.App(APP_NAME)


@app.function(
    image=image,
    secrets=[modal.Secret.from_name(SECRET_NAME)],
    min_containers=1,
    # One ledger: never a second container.
    max_containers=1,
    timeout=900,
)
@modal.concurrent(max_inputs=64)
@modal.asgi_app()
def proxy():
    from metamate_proxy.app import app_from_env

    return app_from_env()
