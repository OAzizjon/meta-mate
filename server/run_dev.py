"""Run the proxy on this machine for development: 127.0.0.1 only.

The Meta key is read from the project's secrets file (the same reader as `mm probe`,
values never printed); a fresh invite code is generated per run and written to
`server/.dev-invite` (git-ignored) for the app to use.

    C:/mmw/py-proxy/Scripts/python.exe server/run_dev.py [--port 8787]
"""

import argparse
import os
import secrets
import sys
from pathlib import Path

import uvicorn

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'tools'))
sys.path.insert(0, str(ROOT / 'server'))
import mm_probe  # noqa: E402
from metamate_proxy.app import Settings, create_app  # noqa: E402

INVITE_FILE = ROOT / 'server' / '.dev-invite'


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument('--port', type=int, default=8787)
    args = parser.parse_args()
    _, key, base = mm_probe.provider_config('meta')
    invite = 'dev-' + secrets.token_urlsafe(18)
    INVITE_FILE.write_text(invite + '\n', encoding='utf-8')
    env = dict(os.environ, META_API_KEY=key, META_API_BASE=base, PROXY_INVITE_CODES=f'{invite}:judge')
    app = create_app(Settings.from_env(env))
    print(f'proxy on http://127.0.0.1:{args.port} (invite code in {INVITE_FILE.relative_to(ROOT).as_posix()})')
    uvicorn.run(app, host='127.0.0.1', port=args.port, log_level='warning')
    return 0


if __name__ == '__main__':
    sys.exit(main())
