"""Run the proxy on this machine for development: 127.0.0.1 only.

The Meta key is read from the project's secrets file (the same reader as `mm probe`,
values never printed); a fresh invite code is generated per run and written to
`server/.dev-invite` (git-ignored) for the app to use. `--tts` also starts the MMS-TTS
dev server (tts/serve_dev.py, its own Python with torch) with a per-run shared secret;
`--tts-modal` uses the GPU service on Modal instead, linked by tts/deploy_modal.py in
`server/.dev-tts.json` (git-ignored; the secret is never printed).

    C:/mmw/py-proxy/Scripts/python.exe server/run_dev.py [--port 8787] [--tts | --tts-modal]
"""

import argparse
import json
import os
import secrets
import subprocess
import sys
from pathlib import Path

import uvicorn

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'tools'))
sys.path.insert(0, str(ROOT / 'server'))
import mm_probe  # noqa: E402
from metamate_proxy.app import Settings, create_app  # noqa: E402

INVITE_FILE = ROOT / 'server' / '.dev-invite'
MODAL_LINK = ROOT / 'server' / '.dev-tts.json'
# The key that signs dev session tokens (sign-in, D54): kept so a restart keeps you signed in.
SESSION_FILE = ROOT / 'server' / '.dev-session'
# Muse Glimmer on Modal (inference/glimmer_modal.py), behind the same secret as the TTS.
GLIMMER_DEV_URL = 'https://obdazizjon--metamate-glimmer-serve.us-east.modal.run'
OAUTH_NAMES = ('GOOGLE_CLIENT_ID', 'GOOGLE_CLIENT_SECRET', 'GITHUB_CLIENT_ID', 'GITHUB_CLIENT_SECRET')
TTS_PYTHON = Path('C:/mmw/py-tts/Scripts/python.exe')
TTS_PORT = 8788
# An 80 ms PCM frame is under 4 KB: a socket never needs more (uvicorn's default is 16 MiB).
WS_MAX_SIZE = 1 << 16
WS_MAX_QUEUE = 4


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument('--port', type=int, default=8787)
    speech = parser.add_mutually_exclusive_group()
    speech.add_argument('--tts', action='store_true', help='also run MMS-TTS on 127.0.0.1:8788')
    speech.add_argument('--tts-modal', action='store_true', help='use MMS-TTS on Modal (tts/deploy_modal.py)')
    parser.add_argument('--tts-python', type=Path, default=TTS_PYTHON)
    args = parser.parse_args()
    _, key, base = mm_probe.provider_config('meta')
    invite = 'dev-' + secrets.token_urlsafe(18)
    INVITE_FILE.write_text(invite + '\n', encoding='utf-8')
    env = dict(os.environ, META_API_KEY=key, META_API_BASE=base, PROXY_INVITE_CODES=f'{invite}:judge')
    # Sign-in: the OAuth clients from the secrets file (names only ever printed) and a
    # persistent dev signing key.
    for name in OAUTH_NAMES:
        value = mm_probe.read_secret(name)
        if value:
            env[name] = value
    if not SESSION_FILE.exists():
        SESSION_FILE.write_text(secrets.token_urlsafe(48) + '
', encoding='utf-8')
    env['PROXY_SESSION_SECRET'] = SESSION_FILE.read_text(encoding='utf-8').strip()
    tts_secret = secrets.token_urlsafe(24)
    if args.tts:
        env.update(MMS_TTS_URL=f'http://127.0.0.1:{TTS_PORT}', MMS_TTS_SECRET=tts_secret)
    if args.tts_modal:
        try:
            link = json.loads(MODAL_LINK.read_text(encoding='utf-8'))
            env.update(MMS_TTS_URL=str(link['url']), MMS_TTS_SECRET=str(link['secret']))
            env['GLIMMER_URL'] = GLIMMER_DEV_URL
        except (OSError, ValueError, KeyError, TypeError):
            print(f'no Modal link in {MODAL_LINK.relative_to(ROOT).as_posix()}: run tts/deploy_modal.py first')
            return 2
    app = create_app(Settings.from_env(env))  # a bad setting stops here, before any child starts
    tts = None
    if args.tts:
        # The TTS process gets its secret, never the Meta key.
        tts_env = {k: v for k, v in os.environ.items() if not k.startswith(('META_', 'PROXY_'))}
        tts = subprocess.Popen([str(args.tts_python), str(ROOT / 'tts' / 'serve_dev.py'), '--port', str(TTS_PORT)],
                               env=dict(tts_env, MMS_TTS_SECRET=tts_secret, HF_HUB_OFFLINE='1'))
    print(f'proxy on http://127.0.0.1:{args.port} (invite code in {INVITE_FILE.relative_to(ROOT).as_posix()})',
          flush=True)
    try:
        uvicorn.run(app, host='127.0.0.1', port=args.port, log_level='warning',
                    ws_max_size=WS_MAX_SIZE, ws_max_queue=WS_MAX_QUEUE)
    finally:
        if tts is not None:
            tts.terminate()
            tts.wait(timeout=10)
    return 0


if __name__ == '__main__':
    sys.exit(main())
