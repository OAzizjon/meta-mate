"""The proxy on Modal (D57): the same FastAPI app as `server/run_dev.py`, served by Modal's
ASGI runner over https, WebSockets included (/v1/asr).

    python server/deploy/deploy_modal_proxy.py    # owner: secrets into Modal, deploy, link

Settings come from the Modal secret `metamate-proxy` (written by that script from the local
secrets file; never on a command line). One container, always warm; the ledger survives
container replacement in a Modal Volume. Multiple concurrent writers are not supported.
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
    .add_local_dir(str(SERVER.parent / 'site'), '/root/site')
)

app = modal.App(APP_NAME)
ledger_volume = modal.Volume.from_name('metamate-proxy-ledger', create_if_missing=True)


@app.function(
    image=image,
    secrets=[modal.Secret.from_name(SECRET_NAME),
             modal.Secret.from_dict({'PROXY_LEDGER_PATH': '/data/ledger.json',
                                     'PROXY_SITE_DIR': '/root/site'})],
    volumes={'/data': ledger_volume},
    min_containers=1,
    # One ledger: never a second container.
    max_containers=1,
    timeout=900,
)
@modal.concurrent(max_inputs=64)
@modal.asgi_app()
def proxy():
    from metamate_proxy.app import app_from_env

    return app_from_env(ledger_commit=background_commit())


def background_commit():
    """Volume.commit is a network call: never on the event loop, at most every 5 s. A
    restart loses at most the last 5 s of counts."""
    import threading
    import time

    lock = threading.Lock()
    pending = [False]

    def run():
        time.sleep(5)
        with lock:
            pending[0] = False
        try:
            ledger_volume.commit()
        except Exception as error:  # noqa: BLE001 - the next save tries again
            print(f'ledger commit failed: {type(error).__name__}', flush=True)

    def request():
        with lock:
            if pending[0]:
                return
            pending[0] = True
        threading.Thread(target=run, daemon=True).start()

    return request
