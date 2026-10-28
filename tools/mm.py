"""mm: the MetaMate project CLI, for the lead (owner and Claude) and, in agent mode, for sandboxed AI agents.

    mm doctor                      build tools with versions (Rust, MSVC linker, WebView2, Node, ffmpeg, uv, ...)
    mm status                      one screen: git, deadline, today's plan, agent queue
    mm plan [today|N]              the plan for today (or day N) from docs/plan.md
    mm check [--full] [--only a,b] fast checks (tools/mm_checks.py)
    mm test                        unit tests (tools/tests; cargo and npm tests once they exist)
    mm release [--allow-dirty] [--dry-run]
                                   check and build the Windows NSIS installer; print size and SHA-256
    mm uia [--delay S] [--depth N] [--save NAME]
                                   UI Automation tree of the foreground window; --save writes
                                   fixtures/uia/NAME.json and NAME.png (record only the Demo user's windows)
    mm art [NAME ...]              render assets/art/src/*.html|*.svg to PNG with headless Edge
    mm sounds [check|normalize]    loudness-normalize assets/sounds/src with ffmpeg; every file needs a LICENSES.md line
    mm phrases [build|check]       build or verify cached English and Russian MMS-TTS phrases
    mm agents [status|stats]       the drafting team's queue (team.py)
    mm keys SOURCE_DIR [--apply]   copy the keys named in env_keys_names.txt from another project's
                                   secrets file into this one; prints names only, never values
    mm probe models|stream|click|ledger
                                   the Meta Model API from this machine: stream an answer with timings and
                                   cost, or one computer-use turn on a fixture with the element it hit
                                   (tools/mm_probe.py; reads the keys, never prints them)
    mm agent check|test            the restricted set for sandboxed agents: offline, no secrets, no GUI

Agent mode is forced when MM_AGENT=1, when the project has no .git folder (the drafting agents'
copies never contain one) or when it runs inside ~/.cache/draft-agents/sandboxes: every command
outside `mm agent ...` is refused there. Standard library only. Exit code 1 when something fails.
"""

import argparse
import datetime as dt
import hashlib
import json
import os
import re
import shutil
import stat
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).absolute().parent.parent
TOOLS = ROOT / 'tools'

OFFICIAL_DEADLINE = dt.datetime(2026, 11, 5, 2, 0, tzinfo=dt.timezone.utc)   # 2026-11-04 21:00 ET
INTERNAL_DEADLINE = dt.datetime(2026, 11, 4, 18, 0, tzinfo=dt.timezone.utc)
KICKOFF = dt.date(2026, 10, 26)

TEAM_PY = Path.home() / '.claude' / 'skills' / 'draft-agents' / 'scripts' / 'team.py'
EDGE_PATHS = (
    Path(os.environ.get('ProgramFiles(x86)', r'C:\Program Files (x86)')) / 'Microsoft/Edge/Application/msedge.exe',
    Path(os.environ.get('ProgramFiles', r'C:\Program Files')) / 'Microsoft/Edge/Application/msedge.exe',
)
OBS_PATH = Path(os.environ.get('ProgramFiles', r'C:\Program Files')) / 'obs-studio/bin/64bit/obs64.exe'
VSWHERE = Path(os.environ.get('ProgramFiles(x86)', r'C:\Program Files (x86)')) / 'Microsoft Visual Studio/Installer/vswhere.exe'
WEBVIEW2_KEY = r'SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}'

AGENT_COMMANDS = {'agent'}
SOUND_EXTS = {'.wav', '.ogg', '.mp3', '.flac', '.opus', '.m4a'}


# --- helpers -----------------------------------------------------------------

def _extend_path():
    """Tools installed earlier in this session are not on the inherited PATH yet."""
    extra = [
        Path.home() / '.cargo' / 'bin',
        Path(os.environ.get('LOCALAPPDATA', '')) / 'Microsoft' / 'WinGet' / 'Links',
        Path.home() / '.local' / 'bin',
    ]
    if os.name == 'nt':
        import winreg
        for hive, key_name in ((winreg.HKEY_LOCAL_MACHINE, r'SYSTEM\CurrentControlSet\Control\Session Manager\Environment'),
                               (winreg.HKEY_CURRENT_USER, r'Environment')):
            try:
                with winreg.OpenKey(hive, key_name) as key:
                    extra.extend(Path(os.path.expandvars(p)) for p in winreg.QueryValueEx(key, 'Path')[0].split(os.pathsep) if p)
            except OSError:
                pass
    parts = os.environ.get('PATH', '').split(os.pathsep)
    for p in extra:
        if p.is_dir() and str(p) not in parts:
            parts.append(str(p))
    os.environ['PATH'] = os.pathsep.join(parts)


def run(cmd, *, cwd=ROOT, timeout=60, env=None):
    """Run a command; return (exit code, combined output). Missing program -> (127, '')."""
    try:
        cmd = [shutil.which(str(cmd[0])) or str(cmd[0]), *cmd[1:]]
        proc = subprocess.run(
            cmd, cwd=cwd, capture_output=True, text=True, encoding='utf-8', errors='replace',
            timeout=timeout, env=env, stdin=subprocess.DEVNULL,
        )
    except FileNotFoundError:
        return 127, ''
    except subprocess.TimeoutExpired:
        return 124, f'timed out after {timeout} s'
    return proc.returncode, (proc.stdout + proc.stderr).strip()


def first_line(text):
    return text.splitlines()[0].strip() if text else ''


def agent_mode_reason():
    if os.environ.get('MM_AGENT', '').strip().lower() not in ('', '0', 'false', 'no', 'off'):
        return 'MM_AGENT is enabled'
    for path in (ROOT.absolute(), ROOT.resolve()):
        parts = [part.lower() for part in path.parts]
        if 'draft-agents' in parts and 'sandboxes' in parts:
            return 'running in a draft-agents sandbox'
    if not (ROOT / '.git').is_dir() or _is_link(ROOT / '.git'):
        return 'no .git folder (an agent copy)'
    return ''


def say(ok, label, detail=''):
    mark = {True: 'OK  ', False: 'FAIL', None: 'SKIP'}[ok]
    print(f'{mark}  {label:<22} {detail}')


# --- doctor ------------------------------------------------------------------

def _msvc():
    if not VSWHERE.exists():
        return False, 'vswhere not found: install Build Tools (docs/setup-windows.md)'
    code, out = run([str(VSWHERE), '-latest', '-products', '*', '-requires',
        'Microsoft.VisualStudio.Component.VC.Tools.x86.x64', '-property', 'installationPath'])
    if code == 0 and out:
        return True, first_line(out)
    return False, 'C++ build tools (VC.Tools.x86.x64) not installed'


def _webview2():
    try:
        import winreg
    except ImportError:
        return None, 'not Windows'
    for hive in (winreg.HKEY_LOCAL_MACHINE, winreg.HKEY_CURRENT_USER):
        try:
            with winreg.OpenKey(hive, WEBVIEW2_KEY) as key:
                return True, winreg.QueryValueEx(key, 'pv')[0]
        except OSError:
            continue
    return False, 'WebView2 runtime not found'


def _windows_sdk():
    inc = Path(os.environ.get('ProgramFiles(x86)', r'C:\Program Files (x86)')) / 'Windows Kits/10/Include'
    versions = sorted(p.name for p in inc.glob('10.*')) if inc.is_dir() else []
    return (True, versions[-1]) if versions else (False, 'Windows 10/11 SDK not found')


def cmd_doctor(args):
    _extend_path()
    required = [
        ('python', [sys.executable, '--version']),
        ('git', ['git', '--version']),
        ('node', ['node', '--version']),
        ('npm', ['npm.cmd' if os.name == 'nt' else 'npm', '--version']),
        ('rustc', ['rustc', '--version']),
        ('cargo', ['cargo', '--version']),
    ]
    optional = [
        ('rustup toolchain', ['rustup', 'show', 'active-toolchain']),
        ('cargo-nextest', ['cargo', 'nextest', '--version']),
        ('sccache', ['sccache', '--version']),
        ('ffmpeg', ['ffmpeg', '-hide_banner', '-version']),
        ('uv', ['uv', '--version']),
        ('typos', ['typos', '--version']),
        ('hyperfine', ['hyperfine', '--version']),
        ('gh', ['gh', '--version']),
        ('cargo-insta', ['cargo', 'insta', '--version']),
        ('jq', ['jq', '--version']),
        ('modal (TTS deploy)', ['modal', '--version']),
    ]
    failed = False
    for label, cmd in required:
        code, out = run(cmd)
        ok = code == 0
        failed |= not ok
        say(ok, label, first_line(out) if ok else 'missing (required)')
    for label, fn in (('MSVC build tools', _msvc), ('Windows SDK', _windows_sdk), ('WebView2', _webview2)):
        ok, detail = fn()
        failed |= ok is False
        say(ok, label, detail)
    for label, cmd in optional:
        code, out = run(cmd)
        say(True if code == 0 else None, label, first_line(out) if code == 0 else 'not installed (optional)')
    edge = next((p for p in EDGE_PATHS if p.exists()), None)
    say(True if edge else None, 'Edge (mm art)', str(edge) if edge else 'not found')
    say(True if OBS_PATH.exists() else None, 'OBS (backup video)', str(OBS_PATH) if OBS_PATH.exists() else 'not installed (optional)')
    modal_login = Path.home() / '.modal.toml'
    say(True if modal_login.is_file() else None, 'modal sign-in', '~/.modal.toml' if modal_login.is_file() else 'not yet: run `modal setup`')
    say(TEAM_PY.exists() or None, 'drafting team', str(TEAM_PY) if TEAM_PY.exists() else 'team.py not found')
    if failed:
        print('\nInstall commands: docs/setup-windows.md')
    return 1 if failed else 0


# --- status, plan ------------------------------------------------------------

def _countdown(now):
    def left(when):
        secs = int((when - now).total_seconds())
        if secs <= 0:
            return 'passed'
        return f'{secs // 86400} d {secs % 86400 // 3600} h'
    return f'internal {left(INTERNAL_DEADLINE)} / official {left(OFFICIAL_DEADLINE)}'


def _plan_file():
    path = ROOT / 'docs' / 'plan.md'
    return path if path.exists() else None


DAY_HEADING = re.compile(r'^## (?:Day|День) (\d+)\s*[—-]\s*(\d{2})\.(\d{2})')


def plan_sections(text):
    """Split the plan into {day number: (date, lines)} using '## Day N — DD.MM ...' headings."""
    sections, current = {}, None
    for line in text.splitlines():
        m = DAY_HEADING.match(line)
        if m:
            day, dd, mm_ = int(m.group(1)), int(m.group(2)), int(m.group(3))
            current = day
            sections[day] = (dt.date(2026, mm_, dd), [line])
        elif line.startswith('## '):
            current = None
        elif current is not None:
            sections[current][1].append(line)
    return sections


def _today_section(sections, today):
    for day, (date, lines) in sections.items():
        if date == today:
            return day, lines
    return None, None


def cmd_plan(args):
    path = _plan_file()
    if not path:
        print('No plan file (docs/plan.md).')
        return 1
    sections = plan_sections(path.read_text(encoding='utf-8'))
    if args.day and args.day != 'today':
        if not args.day.isdecimal():
            print('Day must be today or a day number.')
            return 1
        lines = sections.get(int(args.day), (None, None))[1]
    else:
        today = dt.date.today()
        _, lines = _today_section(sections, today)
        if lines is None:
            if today < KICKOFF:
                print(f'Before the kickoff ({KICKOFF}): see the "Before kickoff" table in {path.relative_to(ROOT)}.')
            else:
                print(f'No day in the plan for {today}.')
            return 0
    if not lines:
        print('No such day in the plan.')
        return 1
    print('\n'.join(lines).strip())
    return 0


def cmd_status(args):
    now = dt.datetime.now(dt.timezone.utc)
    print(f'MetaMate  {now.astimezone():%Y-%m-%d %H:%M}   deadline: {_countdown(now)}')
    code, branch = run(['git', 'branch', '--show-current'])
    status_code, changes = run(['git', 'status', '--porcelain'])
    last_code, last = run(['git', 'log', '-1', '--format=%h %s (%cr)'])
    n = len([line for line in changes.splitlines() if line.strip()]) if status_code == 0 else 0
    print(f'git       {branch if code == 0 and branch else "(no branch)"}, {n if status_code == 0 else "unknown"} changed/untracked, last: {last if last_code == 0 and last else "no commits"}')
    path = _plan_file()
    if path:
        sections = plan_sections(path.read_text(encoding='utf-8'))
        day, lines = _today_section(sections, dt.date.today())
        print(f'plan      {lines[0][3:] if lines else ("before the kickoff" if dt.date.today() < KICKOFF else "no day today")}')
    if TEAM_PY.exists():
        code, out = run([sys.executable, str(TEAM_PY), 'status'], timeout=30)
        running = [line for line in out.splitlines() if 'running' in line or 'queued' in line]
        print(f'agents    {len(running)} running/queued' + ''.join(f'\n          {line.strip()}' for line in running[:6]))
    return 0


# --- check, test -------------------------------------------------------------

def _load_checks():
    sys.path.insert(0, str(TOOLS))
    try:
        import mm_checks
    except ImportError as e:
        print(f'tools/mm_checks.py is missing or broken: {e}')
        return None
    return mm_checks


AGENT_SAFE_CHECKS = {'secrets', 'env-names', 'md-links', 'typos'}


def _print_results(results):
    failed = False
    for r in results:
        state = None if r.skipped else r.ok
        say(state, r.name, f'{r.seconds:4.1f}s  {r.summary}')
        failed |= not r.ok and not r.skipped
    for r in results:
        if not r.ok and not r.skipped and r.details:
            print(f'\n--- {r.name}')
            print('\n'.join(r.details[:50]))
    return 1 if failed else 0


def cmd_check(args, agent=False):
    _extend_path()
    checks = _load_checks()
    if not checks:
        return 1
    only = set(args.only.split(',')) if getattr(args, 'only', None) else None
    if agent:
        if only and only - AGENT_SAFE_CHECKS:
            print('Agent checks must be offline checks: ' + ', '.join(sorted(AGENT_SAFE_CHECKS)))
            return 2
        only = only or AGENT_SAFE_CHECKS
    return _print_results(checks.run_checks(ROOT, full=getattr(args, 'full', False), only=only))


def _offline_env():
    env = dict(os.environ)
    env.update({'CARGO_NET_OFFLINE': 'true', 'NPM_CONFIG_OFFLINE': 'true', 'MM_AGENT': '1',
                # Agents' sandboxes may not reach the sccache server; an empty wrapper turns it off.
                'RUSTC_WRAPPER': '', 'CARGO_INCREMENTAL': '0'})
    return env


def cmd_test(args, agent=False):
    _extend_path()
    env = _offline_env() if agent else None
    steps = [('tools', [sys.executable, '-m', 'unittest', 'discover', '-s', 'tools/tests', '-v'])]
    if (ROOT / 'server' / 'tests').is_dir():
        # The proxy's pure policy code: standard library only, so the same interpreter runs it.
        steps.append(('server', [sys.executable, '-m', 'unittest', 'discover', '-s', 'server/tests', '-t', 'server']))
    if (ROOT / 'tts' / 'tests').is_dir():
        # Text normalization needs no model dependencies.
        steps.append(('tts', [sys.executable, '-m', 'unittest', 'discover', '-s', 'tts/tests', '-t', 'tts']))
    if (ROOT / 'Cargo.toml').exists():
        has_nextest = run(['cargo', 'nextest', '--version'], env=env)[0] == 0
        offline = ['--offline'] if agent else []
        steps.append(('rust', ['cargo', 'nextest', 'run', *offline] if has_nextest else ['cargo', 'test', *offline, '-q']))
        if has_nextest:
            # nextest skips doctests, and the compile_fail ones guard the coordinate types.
            steps.append(('rust doc', ['cargo', 'test', '--doc', *offline, '-q']))
    pkg = ROOT / 'package.json'
    if not agent and pkg.exists() and 'test' in json.loads(pkg.read_text(encoding='utf-8-sig')).get('scripts', {}):
        steps.append(('web', ['npm.cmd' if os.name == 'nt' else 'npm', 'test', '--silent']))
    failed = False
    for label, cmd in steps:
        print(f'\n=== {label}: {" ".join(cmd[1:] if cmd[0] == sys.executable else cmd)}', flush=True)
        try:
            cmd[0] = shutil.which(cmd[0]) or cmd[0]
            code = subprocess.call(cmd, cwd=ROOT, env=env, stdin=subprocess.DEVNULL, timeout=600)
        except (OSError, subprocess.TimeoutExpired):
            code = 1
        failed |= code != 0
    print('\nmm test: ' + ('FAILED' if failed else 'OK'))
    return 1 if failed else 0


def cmd_agent(args):
    if args.what == 'check':
        return cmd_check(args, agent=True)
    return cmd_test(args, agent=True)


# --- release -----------------------------------------------------------------

def cmd_release(args):
    target = 'x86_64-pc-windows-msvc'
    npm = 'npm.cmd' if os.name == 'nt' else 'npm'
    npx = 'npx.cmd' if os.name == 'nt' else 'npx'
    metadata = ['cargo', 'metadata', '--offline', '--no-deps', '--format-version', '1']
    builds = [
        [npm, 'run', 'build'],
        # The frontend was built above; override Tauri's duplicate beforeBuildCommand.
        [npx, 'tauri', 'build', '--bundles', 'nsis', '--target', target,
         '--config', json.dumps({'build': {'beforeBuildCommand': ''}})],
    ]
    if args.dry_run:
        print('Dry run: no checks or commands are executed.')
        print('mm check')
        print('git status --porcelain --untracked-files=all'
              + (' (dirty tree allowed)' if args.allow_dirty else ' (must be clean)'))
        for cmd in [metadata, *builds]:
            print(subprocess.list2cmdline(cmd))
        print(f'Verify a new or updated <Cargo target directory>/{target}/release/bundle/nsis/*.exe;'
              ' print installer path, size in bytes and SHA-256.')
        return 0

    if cmd_check(argparse.Namespace(full=False, only=None)) != 0:
        print('Release refused: mm check failed.')
        return 1
    code, out = run(['git', 'status', '--porcelain', '--untracked-files=all'], cwd=ROOT)
    if code != 0 or (out and not args.allow_dirty):
        print('Release refused: cannot read git status or tree is dirty (use --allow-dirty).')
        return 1
    code, out = run(metadata, cwd=ROOT)
    try:
        if code != 0:
            raise ValueError('cargo metadata failed')
        bundle = Path(json.loads(out)['target_directory']) / target / 'release/bundle/nsis'
        before = {p: (p.stat().st_mtime_ns, p.stat().st_size) for p in bundle.glob('*.exe')}
    except (OSError, ValueError, KeyError, TypeError):
        print('Release failed: cannot locate Cargo output directory.')
        return 1
    for cmd in builds:
        print(subprocess.list2cmdline(cmd), flush=True)
        code, out = run(cmd, cwd=ROOT, timeout=1800)
        if out:
            print(out)
        if code != 0:
            print('Release failed: build command failed.')
            return 1
    try:
        installers = [p for p in sorted(bundle.glob('*.exe'))
                      if p.is_file() and (p.stat().st_mtime_ns, p.stat().st_size) != before.get(p)]
        if not installers:
            print(f'Release failed: produced NSIS installer missing in {bundle}.')
            return 1
        for installer in installers:
            with installer.open('rb') as stream:
                digest = hashlib.file_digest(stream, 'sha256').hexdigest()
            print(f'Installer: {installer}\nSize: {installer.stat().st_size} bytes\nSHA-256: {digest}')
    except OSError:
        print('Release failed: cannot read produced NSIS installer.')
        return 1
    return 0


# --- uia ---------------------------------------------------------------------

def cmd_uia(args):
    script = TOOLS / 'uia_dump.ps1'
    cmd = ['powershell.exe', '-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', str(script),
        '-Delay', str(args.delay), '-Depth', str(args.depth)]
    out_json = None
    if args.save:
        if not re.fullmatch(r'[A-Za-z0-9_-]{1,60}', args.save):
            print('--save takes a simple name: letters, digits, - and _')
            return 1
        folder = ROOT / 'fixtures' / 'uia'
        folder.mkdir(parents=True, exist_ok=True)
        out_json = folder / f'{args.save}.json'
        cmd += ['-JsonPath', str(out_json), '-ShotPath', str(folder / f'{args.save}.png')]
    if args.delay:
        print(f'Focus the target window: capturing in {args.delay} s...', flush=True)
    code = subprocess.call(cmd, cwd=ROOT, stdin=subprocess.DEVNULL)
    if code == 0 and out_json:
        print(f'saved {out_json.relative_to(ROOT)} (+ .png). Check it for private data before committing.')
    return code


# --- art ---------------------------------------------------------------------

SIZE_HINT = re.compile(r'<!--\s*size:\s*(\d+)x(\d+)\s*(transparent)?\s*-->')


def _png_size(path):
    with open(path, 'rb') as f:
        head = f.read(24)
    if len(head) != 24 or head[:8] != b'\x89PNG\r\n\x1a\n' or head[12:16] != b'IHDR':
        return None
    return int.from_bytes(head[16:20], 'big'), int.from_bytes(head[20:24], 'big')


def cmd_art(args):
    edge = next((p for p in EDGE_PATHS if p.exists()), None)
    if not edge:
        print('Microsoft Edge not found.')
        return 1
    src = ROOT / 'assets' / 'art' / 'src'
    files = sorted(p for p in src.glob('*') if p.suffix in ('.html', '.svg'))
    if args.names:
        files = [p for p in files if p.stem in args.names]
    if not files:
        print(f'Nothing to render in {src.relative_to(ROOT)}.')
        return 1
    failed = False
    with tempfile.TemporaryDirectory(prefix='mm-art-') as profile:
        for path in files:
            m = SIZE_HINT.search(path.read_text(encoding='utf-8'))
            w, h = (int(m.group(1)), int(m.group(2))) if m else (1080, 1080)
            out = path.parent.parent / f'{path.stem}.png'
            cmd = [str(edge), '--headless=new', '--disable-gpu', '--hide-scrollbars', '--no-first-run',
                '--disable-extensions', f'--user-data-dir={profile}', '--force-device-scale-factor=1',
                f'--window-size={w},{h}', f'--screenshot={out}', path.as_uri()]
            if m and m.group(3):
                cmd.insert(1, '--default-background-color=00000000')
            if out.exists():
                out.unlink()
            code, text = run(cmd, timeout=90)
            size = _png_size(out) if out.exists() else None
            ok = code == 0 and size == (w, h)
            failed |= not ok
            say(ok, path.name, f'{out.relative_to(ROOT)} {size[0]}x{size[1]}' if ok else first_line(text) or f'exit {code}')
    return 1 if failed else 0


# --- sounds ------------------------------------------------------------------

def _licensed_names():
    lic = ROOT / 'assets' / 'sounds' / 'LICENSES.md'
    if not lic.exists():
        return set()
    return set(re.findall(r'`([\w.-]+)`', lic.read_text(encoding='utf-8')))


def _sound_kind(path, src):
    """src/amb/* -> ambience (stereo, -28 LUFS), src/cues/* -> cues (levels kept), everything else -> sfx."""
    parts = path.relative_to(src).parts
    return 'amb' if 'amb' in parts else 'cues' if 'cues' in parts else 'sfx'


def cmd_sounds(args):
    _extend_path()
    base = ROOT / 'assets' / 'sounds'
    src = base / 'src'
    sources = sorted(p for p in src.rglob('*') if p.suffix.lower() in SOUND_EXTS) if src.is_dir() else []
    licensed = _licensed_names()
    missing = [p for p in sources if p.name not in licensed]
    for p in missing:
        say(False, p.name, 'no line in assets/sounds/LICENSES.md (`file name` in backticks)')
    if args.action == 'check' or not sources:
        if not sources:
            print('No sources in assets/sounds/src.')
        else:
            say(not missing, 'licences', f'{len(sources) - len(missing)}/{len(sources)} listed')
        return 1 if missing else 0
    if not shutil.which('ffmpeg'):
        print('ffmpeg is not installed (docs/setup-windows.md).')
        return 1
    failed = bool(missing)
    destinations = [(_sound_kind(p, src), p.stem.lower()) for p in sources]
    if len(set(destinations)) != len(destinations):
        print('Multiple sound sources map to the same output name; rename them first.')
        return 1
    for p in sources:
        kind = _sound_kind(p, src)
        out = base / kind / f'{p.stem}.ogg'
        if not getattr(args, 'force', False) and out.exists() and out.stat().st_mtime_ns >= p.stat().st_mtime_ns:
            say(None, p.name, 'up to date')
            continue
        out.parent.mkdir(parents=True, exist_ok=True)
        layout = 'stereo' if kind == 'amb' else 'mono'
        # Cues are synthesized at deliberate levels below speech (tools/synth_cues.py): convert only.
        loud = {'amb': ',loudnorm=I=-28:TP=-3:LRA=11', 'sfx': ',loudnorm=I=-16:TP=-1.5:LRA=11', 'cues': ''}[kind]
        cmd = ['ffmpeg', '-nostdin', '-hide_banner', '-loglevel', 'error', '-y', '-i', str(p),
            '-map', '0:a:0', '-vn', '-af', f'aformat=channel_layouts={layout}{loud}',
            '-ar', '48000', '-ac', '2' if kind == 'amb' else '1', '-c:a', 'libvorbis', '-q:a', '5', str(out)]
        code, text = run(cmd, timeout=120)
        failed |= code != 0
        say(code == 0, p.name, str(out.relative_to(ROOT)) if code == 0 else first_line(text))
    return 1 if failed else 0


def cmd_phrases(args):
    sys.path.insert(0, str(TOOLS))
    import phrases
    argv = [args.action]
    if args.action == 'build':
        argv += ['--tts-python', args.tts_python]
        if args.force:
            argv.append('--force')
    return phrases.main(argv)


# --- agents ------------------------------------------------------------------

def cmd_agents(args):
    if not TEAM_PY.exists():
        print('team.py not found (~/.claude/skills/draft-agents).')
        return 1
    return subprocess.call([sys.executable, str(TEAM_PY), args.action], stdin=subprocess.DEVNULL)


# --- keys --------------------------------------------------------------------

SECRETS_FILE = '.env'
ENV_LINE = re.compile(r'^\s*(?:export\s+)?([A-Za-z_][A-Za-z0-9_]*)\s*=(.*)$')


def wanted_key_names():
    text = (ROOT / 'env_keys_names.txt').read_text(encoding='utf-8')
    names = [line.strip() for line in text.splitlines() if line.strip() and not line.lstrip().startswith('#')]
    if any(not re.fullmatch(r'[A-Za-z_][A-Za-z0-9_]*', name) for name in names):
        raise ValueError('Invalid key name')
    return list(dict.fromkeys(names))


def parse_env(text):
    values = {}
    for line in text.lstrip('\ufeff').splitlines():
        m = ENV_LINE.match(line)
        if m:
            value = m.group(2).strip()
            # Refuse multiline or ambiguous quoting rather than copy a fragment.
            if value.startswith(('"', "'")) and not re.fullmatch(r'''("(?:[^"\\]|\\.)*"|'[^']*')\s*(?:#.*)?''', value):
                raise ValueError('Unsupported quoting')
            name = m.group(1)
            if name in values and values[name] != value:
                raise ValueError('Conflicting duplicate assignment')
            values[name] = value
        elif line.strip() and not line.lstrip().startswith('#'):
            raise ValueError('Unsupported secrets file syntax')
    return values


def _is_link(path):
    return path.is_symlink() or path.is_junction()


def _target_bytes(path):
    """Open the entry itself, never a symlink or Windows reparse target."""
    try:
        if os.name == 'nt':
            import ctypes
            import msvcrt
            from ctypes import wintypes
            create = ctypes.WinDLL('kernel32', use_last_error=True).CreateFileW
            create.argtypes = [wintypes.LPCWSTR, wintypes.DWORD, wintypes.DWORD,
                               wintypes.LPVOID, wintypes.DWORD, wintypes.DWORD, wintypes.HANDLE]
            create.restype = wintypes.HANDLE
            handle = create(str(path), 0x80000000, 7, None, 3, 0x00200000, None)
            if handle == wintypes.HANDLE(-1).value:
                raise ctypes.WinError(ctypes.get_last_error())
            try:
                fd = msvcrt.open_osfhandle(handle, os.O_RDONLY | os.O_BINARY)
            except Exception:
                close = ctypes.WinDLL('kernel32').CloseHandle
                close.argtypes = [wintypes.HANDLE]
                close(handle)
                raise
        else:
            fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW)
    except FileNotFoundError:
        return b''
    with os.fdopen(fd, 'rb') as stream:
        info = os.fstat(stream.fileno())
        if (not stat.S_ISREG(info.st_mode)
                or getattr(info, 'st_file_attributes', 0) & stat.FILE_ATTRIBUTE_REPARSE_POINT):
            raise ValueError('Unsafe target')
        return stream.read()


def cmd_keys(args):
    # Never include exception text: decoding errors and filenames can contain secrets.
    try:
        return _copy_keys(args)
    except Exception:
        print('Key import failed; no secret values are shown.')
        return 1


def _copy_keys(args):
    source = Path(args.source).expanduser().resolve() / SECRETS_FILE
    root = ROOT.resolve(strict=True)
    target = root / SECRETS_FILE
    if _is_link(target) or target.resolve().parent != root:
        raise ValueError('Unsafe target')
    if source == target:
        print('Source and target are the same file.')
        return 1
    if not source.is_file():
        print('Source secrets file not found.')
        return 1
    src = parse_env(source.read_text(encoding='utf-8-sig'))
    original = _target_bytes(target)
    dst_text = original.decode('utf-8-sig')
    dst = parse_env(dst_text)
    to_copy, kept, absent = [], [], []
    for name in wanted_key_names():
        if name in dst and dst[name]:
            kept.append(name)
        elif src.get(name):
            to_copy.append(name)
        else:
            absent.append(name)
    print(f'copy ({len(to_copy)}): ' + (', '.join(to_copy) or '-'))
    print(f'already set here ({len(kept)}): ' + (', '.join(kept) or '-'))
    print(f'not in the source ({len(absent)}): ' + (', '.join(absent) or '-'))
    if not args.apply:
        print('\nDry run: nothing written. Add --apply to copy.')
        return 0
    if not to_copy:
        return 0
    newline = '\r\n' if b'\r\n' in original else '\n'
    pending = set(to_copy)
    lines = []
    for line in dst_text.splitlines(keepends=True):
        m = ENV_LINE.match(line.rstrip('\r\n'))
        if m and m.group(1) in to_copy:
            name = m.group(1)
            ending = '\r\n' if line.endswith('\r\n') else ('\n' if line.endswith('\n') else '')
            lines.append(f'{name}={src[name]}{ending}')
            pending.discard(name)
        else:
            lines.append(line)
    text = ''.join(lines)
    if pending:
        if text and not text.endswith(('\r', '\n')):
            text += newline
        text += ''.join(f'{name}={src[name]}{newline}' for name in to_copy if name in pending)
    data = (b'\xef\xbb\xbf' if original.startswith(b'\xef\xbb\xbf') else b'') + text.encode('utf-8')
    temporary = None
    try:
        # Same-directory replacement is atomic; the original survives any write failure.
        with tempfile.NamedTemporaryFile(dir=root, prefix='.mm-keys-', delete=False) as stream:
            temporary = Path(stream.name)
            stream.write(data)
            stream.flush()
            os.fsync(stream.fileno())
        if _is_link(target) or _target_bytes(target) != original:
            raise ValueError('Target changed during import')
        os.replace(temporary, target)
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)
    print(f'\nWrote {len(to_copy)} key(s) to {SECRETS_FILE} (values not shown).')
    return 0


# --- main --------------------------------------------------------------------

def build_parser():
    p = argparse.ArgumentParser(prog='mm', description='MetaMate project CLI (see the module docstring).')
    sub = p.add_subparsers(dest='command', required=True)
    sub.add_parser('doctor', help='build tools with versions')
    sub.add_parser('status', help='git, deadline, plan, agents')
    sp = sub.add_parser('plan', help='the plan for today or day N')
    sp.add_argument('day', nargs='?', default='today')
    sp = sub.add_parser('check', help='fast checks')
    sp.add_argument('--full', action='store_true')
    sp.add_argument('--only', help='comma-separated check names')
    sub.add_parser('test', help='unit tests')
    sp = sub.add_parser('release', help='check and build the Windows NSIS installer')
    sp.add_argument('--allow-dirty', action='store_true', help='allow uncommitted or untracked files')
    sp.add_argument('--dry-run', action='store_true', help='print steps without executing them')
    sp = sub.add_parser('uia', help='UI Automation tree of the foreground window')
    sp.add_argument('--delay', type=int, default=3)
    sp.add_argument('--depth', type=int, default=12)
    sp.add_argument('--save', help='fixture name: fixtures/uia/NAME.json + .png')
    sp = sub.add_parser('art', help='render assets/art/src to PNG')
    sp.add_argument('names', nargs='*')
    sp = sub.add_parser('sounds', help='check licences / normalize loudness')
    sp.add_argument('action', nargs='?', choices=('check', 'normalize'), default='check')
    sp.add_argument('--force', action='store_true', help='convert all sources even if outputs are up to date')
    sp = sub.add_parser('phrases', help='build / check cached MMS-TTS phrases (lead only)')
    sp.add_argument('action', nargs='?', choices=('build', 'check'), default='check')
    sp.add_argument('--tts-python', default='C:/mmw/py-tts/Scripts/python.exe')
    sp.add_argument('--force', action='store_true')
    sp = sub.add_parser('agents', help="the drafting team's queue")
    sp.add_argument('action', nargs='?', choices=('status', 'stats'), default='status')
    sp = sub.add_parser('keys', help='copy key values from another project (names printed only)')
    sp.add_argument('source', help='folder of the other project')
    sp.add_argument('--apply', action='store_true')
    sp = sub.add_parser('agent', help='restricted commands for sandboxed agents')
    sp.add_argument('what', choices=('check', 'test'))
    sp.add_argument('--only', help='comma-separated check names')
    sp = sub.add_parser('probe', help='Meta API probe: models | stream | click FIXTURE | ledger (uses keys)',
                        add_help=False)
    sp.add_argument('rest', nargs=argparse.REMAINDER)
    return p


def cmd_probe(args):
    """The Meta Model API from this machine: tools/mm_probe.py (lead only: it reads the keys)."""
    _extend_path()  # ffmpeg (winget) for downscaling
    sys.path.insert(0, str(TOOLS))
    import mm_probe
    return mm_probe.main(args.rest or ['--help'])


COMMANDS = {
    'doctor': cmd_doctor, 'status': cmd_status, 'plan': cmd_plan, 'check': cmd_check, 'test': cmd_test,
    'uia': cmd_uia, 'art': cmd_art, 'sounds': cmd_sounds, 'agents': cmd_agents, 'keys': cmd_keys,
    'agent': cmd_agent, 'probe': cmd_probe, 'phrases': cmd_phrases, 'release': cmd_release,
}


def main(argv=None):
    if hasattr(sys.stdout, 'reconfigure'):
        sys.stdout.reconfigure(encoding='utf-8', errors='replace')
    parser = build_parser()
    args, extra = parser.parse_known_args(argv)
    # `mm probe` has its own parser: a leading option such as --help is not part of REMAINDER.
    if args.command == 'probe':
        args.rest = extra + args.rest
    elif extra:
        parser.error('unrecognized arguments: ' + ' '.join(extra))
    reason = agent_mode_reason()
    if reason and args.command not in AGENT_COMMANDS:
        print(f'mm {args.command}: refused in agent mode ({reason}). Allowed: mm agent check | mm agent test')
        return 2
    return COMMANDS[args.command](args)


if __name__ == '__main__':
    sys.exit(main())
