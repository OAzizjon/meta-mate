"""Owner: put the proxy on Modal for the judges' build (D57).

    python server/deploy/deploy_modal_proxy.py [--budget USD] [--new-invite]

1. The Modal secret `metamate-proxy` is written from the local secrets file (the Meta key,
   the OAuth clients, the judges' e-mails) plus the MMS-TTS link (server/.dev-tts.json), a
   fresh session secret (tokens signed before stop working) and the judge invite of
   server/.prod-proxy.json (made once; --new-invite replaces it). It travels through a
   temporary JSON file only this run sees, never a command line, and nothing is printed.
2. `modal deploy server/deploy/modal_proxy.py`.
3. The proxy's URL and the judge invite stay in server/.prod-proxy.json (git-ignored), where
   `mm release` finds them (D51).

Then register the OAuth redirect URIs it prints with Google and GitHub.
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

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'tools'))
import mm_probe  # noqa: E402 - the secrets file reader, standard library only

LINK_FILE = ROOT / 'server' / '.prod-proxy.json'
TTS_LINK = ROOT / 'server' / '.dev-tts.json'
SECRET_NAME = 'metamate-proxy'
APP_FILE = ROOT / 'server' / 'deploy' / 'modal_proxy.py'
GLIMMER_URL = 'https://obdazizjon--metamate-glimmer-serve.us-east.modal.run'
URL = re.compile(r'https://[A-Za-z0-9.-]+\.modal\.run')
# Copied from the secrets file when present (the Meta key under either name).
FROM_SECRETS = ('META_API_KEY', 'META_KEY', 'META_API_BASE', 'GOOGLE_CLIENT_ID', 'GOOGLE_CLIENT_SECRET',
                'GITHUB_CLIENT_ID', 'GITHUB_CLIENT_SECRET', 'PROXY_JUDGE_EMAILS', 'PROXY_PRICES')


def load(path: Path) -> dict:
    try:
        value = json.loads(path.read_text(encoding='utf-8'))
    except (OSError, ValueError):
        return {}
    return value if isinstance(value, dict) else {}


def save_link(link: dict) -> None:
    LINK_FILE.write_bytes((json.dumps(link, indent=2, sort_keys=True) + '\n').encode('utf-8'))


def store_secret(values: dict) -> None:
    handle, path = tempfile.mkstemp(suffix='.json')
    try:
        with os.fdopen(handle, 'w', encoding='utf-8') as out:
            json.dump(values, out)
        subprocess.run(['modal', 'secret', 'create', '--force', SECRET_NAME, '--from-json', path],
                       check=True, stdout=subprocess.DEVNULL)
    finally:
        Path(path).unlink(missing_ok=True)


def restart_containers() -> None:
    """Stops this app's running containers; Modal starts a fresh one (min_containers=1)."""
    listed = subprocess.run(['modal', 'container', 'list', '--json'], capture_output=True, text=True,
                            encoding='utf-8', errors='replace')
    try:
        containers = json.loads(listed.stdout)
    except ValueError:
        containers = []
    for container in containers if isinstance(containers, list) else []:
        if isinstance(container, dict) and container.get('app_name') == 'metamate-proxy':
            subprocess.run(['modal', 'container', 'stop', '-y', str(container.get('container_id'))],
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument('--budget', default='15', help="the proxy's daily dollar cap (default 15)")
    parser.add_argument('--new-invite', action='store_true', help='replace the judge invite')
    args = parser.parse_args()
    link = load(LINK_FILE)
    tts = load(TTS_LINK)
    values = {name: value for name in FROM_SECRETS if (value := mm_probe.read_secret(name))}
    if not (values.get('META_API_KEY') or values.get('META_KEY')):
        print('no Meta key in the secrets file: nothing deployed')
        return 2
    if not (tts.get('url') and tts.get('secret')):
        print(f'no MMS-TTS link in {TTS_LINK.relative_to(ROOT).as_posix()}: run tts/deploy_modal.py first')
        return 2
    if args.new_invite or not link.get('invite'):
        # Kept before the secret changes: the invite in Modal is never one we lost.
        link['invite'] = secrets.token_urlsafe(24)
        link.pop('url', None)
        save_link(link)
    # The public URL is Modal's name for this app's function, known before the deploy; when
    # Modal names it otherwise, the secret gets the real one and the app is deployed again.
    url = link.get('url') or 'https://obdazizjon--metamate-proxy-proxy.modal.run'
    values.update({
        'PROXY_INVITE_CODES': f"{link['invite']}:judge",
        'PROXY_DAILY_BUDGET_USD': args.budget,
        'PROXY_SESSION_SECRET': secrets.token_urlsafe(48),
        'MMS_TTS_URL': tts['url'],
        'MMS_TTS_SECRET': tts['secret'],
        'GLIMMER_URL': GLIMMER_URL,
    })
    for _ in range(2):
        values['PROXY_PUBLIC_URL'] = url
        store_secret(values)
        deployed = subprocess.run(['modal', 'deploy', str(APP_FILE)], capture_output=True, text=True,
                                  encoding='utf-8', errors='replace')
        if deployed.returncode != 0:
            print('modal deploy failed; its last lines:')
            print(deployed.stderr[-2000:])
            return deployed.returncode
        found = sorted(set(URL.findall(deployed.stdout)))
        if len(found) != 1:
            print('modal deploy printed no single URL: nothing linked for mm release')
            return 1
        if found[0] == url:
            break
        url = found[0]
    else:
        print('the deployed URL keeps changing: nothing linked for mm release')
        return 1
    # The always-warm container keeps the secret it started with (2026-10-29: it went on
    # sending Google the old URL); a fresh one reads the new secret.
    restart_containers()
    # Only a deployment whose secret names its own URL is linked for `mm release`.
    link['url'] = url
    save_link(link)
    print(f'proxy: {url} (invite in {LINK_FILE.relative_to(ROOT).as_posix()})')
    print('OAuth redirect URIs to register:')
    for provider in ('google', 'github'):
        print(f'  {url}/auth/{provider}/callback')
    return 0


if __name__ == '__main__':
    sys.exit(main())
