"""Lead only: deploy MMS-TTS to Modal (tts/modal_app.py) and link it to the dev proxy.

    python tts/deploy_modal.py [--warm] [--new-secret]

1. The shared secret lives in server/.dev-tts.json (git-ignored, left out of agent copies,
   never printed). A new one is made when that file has none (or with --new-secret) and is
   stored in the Modal secret `metamate-tts` through a temporary JSON file, never on a
   command line.
2. `modal deploy tts/modal_app.py`, with MMS_TTS_WARM=1 under --warm (one warm container:
   demo and judging days only, it costs GPU time while idle).
3. The endpoint URL Modal prints goes into server/.dev-tts.json for
   `server/run_dev.py --tts-modal`.
"""

import argparse
import json
import os
import re
import secrets
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
LINK_FILE = ROOT / 'server' / '.dev-tts.json'
SECRET_NAME = 'metamate-tts'
URL = re.compile(r'https://[A-Za-z0-9.-]+\.modal\.run')


def load_link() -> dict:
    try:
        link = json.loads(LINK_FILE.read_text(encoding='utf-8'))
    except (OSError, ValueError):
        return {}
    return link if isinstance(link, dict) else {}


def save_link(link: dict) -> None:
    LINK_FILE.write_bytes((json.dumps(link, indent=2, sort_keys=True) + '\n').encode('utf-8'))


def store_secret(secret: str) -> None:
    """The secret goes to Modal through a file only this run sees, deleted at once."""
    handle, path = tempfile.mkstemp(suffix='.json')
    try:
        with os.fdopen(handle, 'w', encoding='utf-8') as out:
            json.dump({'MMS_TTS_SECRET': secret}, out)
        subprocess.run(['modal', 'secret', 'create', '--force', SECRET_NAME, '--from-json', path],
                       check=True, stdout=subprocess.DEVNULL)
    finally:
        Path(path).unlink(missing_ok=True)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument('--warm', action='store_true', help='keep one container warm (costs GPU time)')
    parser.add_argument('--new-secret', action='store_true', help='rotate the shared secret')
    args = parser.parse_args()
    link = load_link()
    if args.new_secret or len(str(link.get('secret', ''))) < 16:
        link['secret'] = secrets.token_urlsafe(32)
        store_secret(link['secret'])
        save_link(link)
        print(f'new shared secret stored in Modal ({SECRET_NAME}) and {LINK_FILE.relative_to(ROOT).as_posix()}')
    env = dict(os.environ, MMS_TTS_WARM='1' if args.warm else '0', PYTHONIOENCODING='utf-8')
    deployed = subprocess.run(['modal', 'deploy', str(ROOT / 'tts' / 'modal_app.py')], env=env,
                              capture_output=True, text=True, encoding='utf-8', errors='replace')
    output = deployed.stdout + deployed.stderr
    if deployed.returncode != 0:
        print(output[-4000:])
        return deployed.returncode
    found = URL.findall(output)
    if not found:
        print('deployed, but no endpoint URL in the output:\n' + output[-2000:])
        return 1
    link['url'] = found[-1]
    save_link(link)
    print(f'MMS-TTS on Modal: {link["url"]} (linked in {LINK_FILE.relative_to(ROOT).as_posix()})')
    return 0


if __name__ == '__main__':
    sys.exit(main())
