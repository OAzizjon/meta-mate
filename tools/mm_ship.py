"""mm ship: the owner's night-of-submission steps, in order, each asked first.

    mm ship [STEP ...] [--dry-run] [--yes]

Steps: glimmer, proxy, site, release, repo, check (all of them when none is named).
Agents and the lead never run this: it deploys, and it makes the repository public.
Standard library only; every outside tool (modal, gh, npx wrangler) keeps its own login.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
REPO = 'OAzizjon/meta-mate'
STEPS = ('glimmer', 'proxy', 'site', 'release', 'repo', 'check')
TARGET = 'x86_64-pc-windows-msvc'
SITE_SKIP = {'README.md'}


def app_version(root: Path = ROOT) -> str:
    return json.loads((root / 'src-tauri/tauri.conf.json').read_text(encoding='utf-8'))['version']


def installer_path(root: Path = ROOT, version: str | None = None) -> Path:
    version = version or app_version(root)
    return root / 'target' / TARGET / 'release/bundle/nsis' / f'MetaMate_{version}_x64-setup.exe'


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open('rb') as stream:
        for block in iter(lambda: stream.read(1 << 20), b''):
            digest.update(block)
    return digest.hexdigest()


def build_site(root: Path, out: Path, installer: Path) -> str:
    """site/ plus the installer into out/, as the Worker serves them. Refuses an installer
    whose hash differs from site/version.json: the updater would reject it for every user."""
    manifest = json.loads((root / 'site/version.json').read_text(encoding='utf-8'))
    digest = sha256(installer)
    if manifest.get('sha256') != digest:
        raise SystemExit(f'site/version.json has sha256 {manifest.get("sha256")}, the installer has {digest}: '
                         'run mm release again and commit site/version.json')
    # In place: a copy that fails raises here, before anything is deployed.
    if out.exists():
        shutil.rmtree(out)
    shutil.copytree(root / 'site', out, ignore=lambda _dir, names: [n for n in names if n in SITE_SKIP])
    # The demo video lives outside git (video/out/, `mm cut --web`); the page plays it from here.
    demo = root / 'video/out/metamate-demo.mp4'
    if demo.is_file():
        (out / 'media').mkdir(exist_ok=True)
        shutil.copy2(demo, out / 'media' / demo.name)
    elif (root / 'site/video.html').is_file():
        raise SystemExit('site/video.html plays a video that is missing: run mm cut --web first')
    download = out / 'download'
    download.mkdir()
    shutil.copy2(installer, download / 'MetaMate-setup.exe')
    (download / 'SHA256.txt').write_text(f'{digest}  MetaMate-setup.exe\n', encoding='utf-8')
    return digest


def release_notes(root: Path, version: str) -> str:
    """The README's way in and this version's CHANGELOG section, when it has one."""
    changelog = (root / 'CHANGELOG.md').read_text(encoding='utf-8')
    found = re.search(rf'^## .*{re.escape(version)}.*?(?=^## |\Z)', changelog, flags=re.M | re.S)
    body = found.group(0).strip() if found else ''
    return ('Hold Ctrl, ask out loud, and Mate lights the button. Windows 10/11 x64. '
            'No account needed to try it (12 messages an hour).\n\n'
            'Windows may say "Windows protected your PC": the installer is not code-signed yet. '
            'Click More info, then Run anyway, and check the SHA-256 at '
            'https://meta-mate.stream/download/SHA256.txt\n\n' + body).strip() + '\n'


class Runner:
    def __init__(self, dry_run: bool, yes: bool):
        self.dry_run = dry_run
        self.yes = yes

    def ask(self, question: str) -> None:
        """A no ends the whole run, so a skipped step can never lead into a later, public one."""
        if self.dry_run or self.yes:
            print(f'  {question} yes')
            return
        if input(f'  {question} [y/N] ').strip().lower() not in ('y', 'yes', 'д', 'да'):
            raise SystemExit('Stopped here; nothing after this step ran. To skip a step, name the others, '
                             'for example: mm ship site release repo check')

    def run(self, cmd: list[str], cwd: Path = ROOT, env: dict[str, str] | None = None) -> None:
        print('  > ' + subprocess.list2cmdline(cmd), flush=True)
        if self.dry_run:
            return
        try:
            code = subprocess.run(cmd, cwd=cwd, env=env).returncode
        except OSError as error:
            raise SystemExit(f'Could not start {cmd[0]}: {error}. Nothing after it ran.') from error
        if code != 0:
            raise SystemExit(f'That command failed with exit {code}. Nothing after it ran. '
                             f'Fix it, then run mm ship again from this step.')


def tool(name: str) -> str:
    found = shutil.which(name)
    if not found:
        raise SystemExit(f'{name} is not on PATH. Nothing after this ran.')
    return found


def proxy_link() -> dict[str, str]:
    """What the installer was built against; only ever compared, never printed."""
    try:
        link = json.loads((ROOT / 'server/.prod-proxy.json').read_text(encoding='utf-8'))
    except (OSError, ValueError):
        return {}
    return {key: link.get(key, '') for key in ('url', 'invite')}


def step_glimmer(run: Runner) -> None:
    print('Glimmer: Meta open weights on our own Modal GPU (the weights are already on the volume).')
    run.ask('Deploy inference/glimmer_modal.py?')
    # Modal's console output breaks on a cp1251 console without UTF-8 (deploy_modal_proxy.py too).
    env = {**os.environ, 'PYTHONIOENCODING': 'utf-8', 'PYTHONUTF8': '1'}
    run.run([tool('modal'), 'deploy', 'inference/glimmer_modal.py'], env=env)


def step_proxy(run: Runner) -> None:
    print('Proxy: the judges\' server on Modal. Earlier sign-ins end; guests and the app recover by themselves.')
    run.ask('Deploy the proxy (server/deploy/deploy_modal_proxy.py)?')
    before = proxy_link()
    run.run([sys.executable, 'server/deploy/deploy_modal_proxy.py'])
    if not run.dry_run and before and proxy_link() != before:
        raise SystemExit('The proxy address or invite changed, so the installer points at the old one. '
                         'Run mm release again, commit site/version.json, then: mm ship site release repo check')


def step_site(run: Runner) -> None:
    installer = installer_path()
    print(f'Site: meta-mate.stream with {installer.name} and version.json.')
    if not installer.exists() and not run.dry_run:
        raise SystemExit(f'{installer} is not there: mm release first')
    out = ROOT / 'site-dist'
    run.ask('Deploy the site to Cloudflare (Worker meta-mate-site)?')
    if not run.dry_run:
        print(f'  SHA-256 {build_site(ROOT, out, installer)}')
    run.run([tool('npx'), '--yes', 'wrangler@4', 'deploy', '--config', 'wrangler.jsonc'], cwd=ROOT / 'server/deploy/site')


def step_release(run: Runner) -> None:
    version = app_version()
    tag = f'v{version}'
    installer = installer_path(version=version)
    print(f'GitHub release {tag} with the installer.')
    if not installer.exists() and not run.dry_run:
        raise SystemExit(f'{installer} is not there: mm release first')
    run.ask(f'Publish release {tag}?')
    gh = tool('gh')
    # A second run after a failure further on: replace the asset, do not fail on the tag.
    exists = not run.dry_run and subprocess.run([gh, 'release', 'view', tag, '--repo', REPO],
                                                capture_output=True, cwd=ROOT).returncode == 0
    if exists:
        run.run([gh, 'release', 'upload', tag, f'{installer}#MetaMate-setup.exe', '--repo', REPO, '--clobber'])
        return
    with tempfile.TemporaryDirectory() as folder:
        notes = Path(folder, 'notes.md')
        notes.write_text(release_notes(ROOT, version), encoding='utf-8')
        run.run([gh, 'release', 'create', tag, f'{installer}#MetaMate-setup.exe', '--repo', REPO,
                 '--title', f'MetaMate {version}', '--notes-file', str(notes), '--latest'])


def step_repo(run: Runner) -> None:
    print('Repository: public, with its whole history. The rules need this before you submit.')
    if not (ROOT / 'LICENSE').exists() and not run.dry_run:
        raise SystemExit('No LICENSE file yet: ask the lead to add it first.')
    run.run([sys.executable, 'tools/mm.py', 'check'])
    # The one step that cannot be undone: a person answers even under --yes.
    Runner(run.dry_run, False).ask(f'Make {REPO} PUBLIC now? This cannot be taken back: copies may stay.')
    run.run([tool('gh'), 'repo', 'edit', REPO, '--visibility', 'public', '--accept-visibility-change-consequences'])


def step_check(run: Runner) -> None:
    print('Check: can we submit right now?')
    run.run([sys.executable, 'tools/mm.py', 'preflight'])


ACTIONS = {'glimmer': step_glimmer, 'proxy': step_proxy, 'site': step_site, 'release': step_release,
           'repo': step_repo, 'check': step_check}


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(prog='mm ship', description="The owner's submission-night steps, each asked first.")
    parser.add_argument('steps', nargs='*', metavar='STEP', help=f'any of: {", ".join(STEPS)}; all when none is named')
    parser.add_argument('--dry-run', action='store_true', help='print what would run; change nothing')
    parser.add_argument('--yes', action='store_true', help='do not ask before each step')
    args = parser.parse_args(argv)
    unknown = [s for s in args.steps if s not in STEPS]
    if unknown:
        parser.error(f'unknown step {unknown[0]} (steps: {", ".join(STEPS)})')
    if os.environ.get('MM_AGENT'):
        print('mm ship is the owner\'s: agents never deploy.')
        return 2
    run = Runner(args.dry_run, args.yes)
    for name in args.steps or STEPS:
        print(f'\n== {name}')
        ACTIONS[name](run)
    return 0


if __name__ == '__main__':
    sys.exit(main())
