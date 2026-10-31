"""mm shot: render MetaMate's WebView windows to PNG without Tauri.

    mm shot [PAGE ...] [--theme light|dark|both] [--lang en|ru|both]
            [--build] [--wait-ms N]

Pages (default: all): home card chat settings mate highlight signin.

Serves dist/ on 127.0.0.1 with a mock of Tauri's IPC: window.__TAURI_INTERNALS__
answers invoke() from tools/shot_fixtures/<page>.json ({command: result}; unknown
commands get null), replays fixture events after load ({events: [{event, payload,
at_ms}]}) and reports the page name through metadata for getCurrentWebviewWindow.
Headless Edge screenshots each page into shots/<page>-<theme>-<lang>.png.

Runs `npm run build` only when dist/ is older than src/, the root *.html pages or
locales/ (or with --build). Safe for `mm agent shot`: npm build, the loopback
server and Edge only — no network, no secrets. Exit codes: 0 ok; 1 a page, the
build or Edge failed (the reason is printed); 2 usage.
"""

import argparse
import ctypes
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import threading
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import parse_qs, unquote, urlsplit

ROOT = Path(__file__).absolute().parent.parent
TOOLS = Path(__file__).absolute().parent
FIXTURES = TOOLS / 'shot_fixtures'
SHOTS = ROOT / 'shots'
DIST = ROOT / 'dist'

# The same lookup table as tools/mm.py; kept as a fallback copy so importing this
# module never imports the CLI (mm.py loads the code tools lazily to avoid a cycle).
EDGE_PATHS = (
    Path(os.environ.get('ProgramFiles(x86)', r'C:\Program Files (x86)')) / 'Microsoft/Edge/Application/msedge.exe',
    Path(os.environ.get('ProgramFiles', r'C:\Program Files')) / 'Microsoft/Edge/Application/msedge.exe',
)

MOCK_ROUTE = '/__mm_mock.js'
PAGES = {
    'home': (1100, 800),
    'card': (900, 1200),
    'chat': (480, 720),
    'settings': (900, 700),
    'mate': (260, 260),
    'highlight': (800, 600),   # no size was specified for highlight.html
    'signin': (480, 640),
}
THEMES = ('light', 'dark')
LANGS = ('en', 'ru')

MOCK_TEMPLATE = """// Mock of Tauri's window.__TAURI_INTERNALS__ for `mm shot` (tools/mm_shot.py).
// Served at /__mm_mock.js with the page's fixture baked in by the local server;
// injected into the served copy of each page — never written into dist/.
(function () {
  'use strict';

  var commands = /*__MM_COMMANDS__*/;
  var events = /*__MM_EVENTS__*/;
  var label = /*__MM_LABEL__*/;

  // The settings payload is what the real app serves; apply its theme and
  // language straight away so the page is styled before any module script runs.
  var settings = commands && commands.load_settings;
  if (settings && typeof settings === 'object') {
    if (settings.theme) {
      document.documentElement.setAttribute('data-theme', settings.theme);
      document.documentElement.style.colorScheme = settings.theme;
    }
    if (settings.language) {
      document.documentElement.setAttribute('lang', settings.language);
    }
  }

  var listeners = Object.create(null);
  var registry = Object.create(null);
  var nextId = 0;

  function addListener(event, handler) {
    var id = ++nextId;
    (listeners[event] || (listeners[event] = [])).push({ id: id, handler: handler });
    return id;
  }

  function removeListener(event, id) {
    var list = listeners[event] || [];
    listeners[event] = list.filter(function (entry) { return entry.id !== id; });
  }

  function runCallback(id, value) {
    var entry = registry[id];
    if (!entry) return;
    if (entry.once) delete registry[id];
    entry.callback(value);
  }

  function dispatch(event, payload) {
    var list = (listeners[event] || []).slice();
    for (var i = 0; i < list.length; i += 1) {
      try {
        list[i].handler({ event: event, id: list[i].id, payload: payload });
      } catch (error) {
        console.error('[mm shot mock] listener for "' + event + '" failed:', error);
      }
    }
  }

  function transformCallback(callback, once) {
    var id = ++nextId;
    registry[id] = { callback: callback, once: once === true };
    return id;
  }

  var internals = {
    invoke: function (cmd, args) {
      args = args || {};
      if (cmd === 'plugin:event|listen') {
        return Promise.resolve(addListener(args.event, function (event) {
          runCallback(args.handler, event);
        }));
      }
      if (cmd === 'plugin:event|unlisten') {
        removeListener(args.event, args.eventId);
        return Promise.resolve();
      }
      if (cmd === 'plugin:event|emit' || cmd === 'plugin:event|emit_to') {
        dispatch(args.event, args.payload);
        return Promise.resolve();
      }
      if (cmd === 'chat_subscribe') {
        var channel = args.channel;
        var id = typeof channel === 'string' ? Number(channel.split(':')[1]) : channel.id;
        // Tauri v2 Channel callbacks receive ordered {message, index} envelopes.
        runCallback(id, { message: commands.chat_subscribe || [], index: 0 });
        return Promise.resolve();
      }
      if (Object.prototype.hasOwnProperty.call(commands, cmd)) {
        return Promise.resolve(commands[cmd]);
      }
      return Promise.resolve(null);   // the default for unknown commands
    },
    transformCallback: transformCallback,
    runCallback: runCallback,
    unregisterCallback: function (id) {
      delete registry[id];
    },
    convertFileSrc: function (filePath) {
      return filePath;   // fixtures use plain relative paths
    },
    metadata: {
      currentWindow: { label: label },
      currentWebview: { label: label },
      currentWebviewWindow: { label: label }
    }
  };

  window.__TAURI_INTERNALS__ = internals;
  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: removeListener };

  // Replay the fixture's scripted events after load; a listener registered later
  // still receives every event whose at_ms has not passed.
  window.addEventListener('load', function () {
    events.forEach(function (item) {
      if (item && typeof item.event === 'string') {
        setTimeout(function () { dispatch(item.event, item.payload); }, item.at_ms || 0);
      }
    });
  });
})();
"""

SAFE_NAME = re.compile(r'[A-Za-z0-9_-]{1,40}')
STATIC_SUFFIXES = {'.html', '.js', '.css', '.png', '.svg', '.jpg', '.jpeg', '.webp',
                   '.avif', '.ico', '.woff', '.woff2', '.ttf', '.otf', '.ogg', '.wav',
                   '.mp3', '.mp4', '.webm'}


def expand(value, choices):
    return list(choices) if value == 'both' else [value]


def last_line(text):
    lines = [line for line in text.strip().splitlines() if line.strip()]
    return lines[-1] if lines else ''


# --- build staleness ---------------------------------------------------------

def source_files(root=ROOT):
    """What makes dist/ stale: src/, the root *.html pages and locales/."""
    files = []
    for name in ('src', 'locales'):
        folder = root / name
        if folder.is_dir():
            files.extend(p for p in folder.rglob('*') if p.is_file())
    files.extend(p for p in root.glob('*.html') if p.is_file())
    return files


def dist_stale(dist, root=ROOT):
    if not dist.is_dir() or not any(dist.rglob('*.html')):
        return True
    oldest_page = min(p.stat().st_mtime_ns for p in dist.rglob('*.html') if p.is_file())
    return any(p.stat().st_mtime_ns > oldest_page for p in source_files(root))


def npm_cmd():
    return 'npm.cmd' if os.name == 'nt' else 'npm'


def build_dist(root=ROOT):
    npm = shutil.which(npm_cmd())
    if not npm:
        return False, 'npm not found (docs/setup-windows.md)'
    try:
        proc = subprocess.run([npm, 'run', 'build'], cwd=root, capture_output=True,
                              text=True, encoding='utf-8', errors='replace',
                              timeout=900, stdin=subprocess.DEVNULL)
    except subprocess.TimeoutExpired:
        return False, 'npm run build timed out after 900 s'
    except OSError as error:
        return False, f'npm run build could not start: {error}'
    if proc.returncode != 0:
        tail = '\n'.join((proc.stdout + proc.stderr).splitlines()[-15:])
        return False, f'npm run build exited {proc.returncode}:\n{tail}'
    return True, ''


# --- fixtures and the mock ----------------------------------------------------

def load_fixture(fixtures, page):
    if not SAFE_NAME.fullmatch(page):
        raise ValueError('bad fixture page')
    path = (fixtures / f'{page}.json').resolve()
    if not path.is_relative_to(fixtures.resolve()):
        raise ValueError('fixture is outside the fixtures directory')
    if not path.is_file():
        return {}
    data = json.loads(path.read_text(encoding='utf-8-sig'))
    if not isinstance(data, dict):
        raise ValueError(f'{path.name}: top level must be a JSON object')
    return data


def mock_context(fixture, theme, lang):
    """Split a fixture into (commands, events) and apply the --theme/--lang overrides."""
    commands = {k: v for k, v in fixture.items() if k != 'events'}
    events = fixture.get('events')
    if not isinstance(events, list):
        events = []
    settings = commands.get('load_settings')
    if not isinstance(settings, dict):
        settings = {}
    settings = dict(settings)
    settings.setdefault('profile', {'name': '', 'about': ''})
    settings.setdefault('profileAsked', True)
    settings['theme'] = theme
    settings['language'] = lang
    commands['load_settings'] = settings
    return commands, events


def _embedded(obj):
    # `</` inside a string would close the script block early; `<\/` is the same
    # string in JS and safe inside HTML.
    return json.dumps(obj, ensure_ascii=False).replace('</', '<\\/')


def render_mock_js(commands, events, label):
    if not SAFE_NAME.fullmatch(label):
        raise ValueError('bad label')
    values = {'COMMANDS': commands, 'EVENTS': events, 'LABEL': label}
    return re.sub(r'/\*__MM_(COMMANDS|EVENTS|LABEL)__\*/',
                  lambda match: _embedded(values[match[1]]), MOCK_TEMPLATE)


def inject_mock(html, page, theme, lang):
    """Insert the mock script tag at the start of <head> in the served copy only."""
    if MOCK_ROUTE in html:
        return html   # never injected twice
    if not SAFE_NAME.fullmatch(page):
        raise ValueError('bad page name')
    if theme not in THEMES or lang not in LANGS:
        raise ValueError('bad theme or language')
    tag = f'<script src="{MOCK_ROUTE}?page={page}&theme={theme}&lang={lang}"></script>'
    match = re.search(r'<head[^>]*>', html, re.I)
    if match:
        return html[:match.end()] + tag + html[match.end():]
    match = re.search(r'<html[^>]*>', html, re.I)
    if match:
        return html[:match.end()] + tag + html[match.end():]
    return tag + html


# --- the loopback server -------------------------------------------------------

def make_handler(dist, fixtures):
    dist = dist.resolve()

    class Handler(SimpleHTTPRequestHandler):
        server_version = 'mm-shot/1.0'

        def __init__(self, *args, **kwargs):
            super().__init__(*args, directory=str(dist), **kwargs)

        def log_message(self, fmt, *args):
            pass

        def _send(self, body, content_type):
            self.send_response(200)
            self.send_header('Content-Type', content_type)
            self.send_header('Content-Length', str(len(body)))
            self.send_header('Cache-Control', 'no-store')
            self.send_header('Content-Security-Policy',
                             "default-src 'self' data: blob:; script-src 'self'; "
                             "style-src 'self' 'unsafe-inline'; object-src 'none'")
            self.end_headers()
            if self.command != 'HEAD':
                self.wfile.write(body)

        def _target(self, parsed):
            path = unquote(parsed.path)
            parts = path.lstrip('/').split('/')
            if path == '/':
                parts = ['home.html']
            if (any(not part or part.startswith('.') or ':' in part or '\\' in part
                    or '%' in part or part.lower().endswith(('.env', '.pem', '.key'))
                    for part in parts)):
                self.send_error(403)
                return None
            try:
                target = dist.joinpath(*parts).resolve()
                if not target.is_relative_to(dist) or target.suffix.lower() not in STATIC_SUFFIXES:
                    self.send_error(403)
                    return None
                if not target.is_file():
                    self.send_error(404)
                    return None
            except (OSError, ValueError):
                self.send_error(403)
                return None
            return target

        def do_HEAD(self):
            self.do_GET()

        def do_GET(self):
            parsed = urlsplit(self.path)
            if parsed.path == MOCK_ROUTE:
                self._mock(parsed)
            else:
                target = self._target(parsed)
                if target is None:
                    return
                if target.suffix.lower() == '.html':
                    self._page(parsed, target)
                else:
                    self._send(target.read_bytes(), self.guess_type(str(target)))

        def _page(self, parsed, target):
            if not SAFE_NAME.fullmatch(target.stem):
                self.send_error(403)
                return
            query = parse_qs(parsed.query)
            theme = (query.get('__mm_theme') or ['light'])[0]
            lang = (query.get('__mm_lang') or ['en'])[0]
            if theme not in THEMES or lang not in LANGS:
                self.send_error(400)
                return
            body = inject_mock(target.read_text(encoding='utf-8'), target.stem,
                               theme, lang).encode('utf-8')
            self._send(body, 'text/html; charset=utf-8')

        def _mock(self, parsed):
            query = parse_qs(parsed.query)
            page = (query.get('page') or ['main'])[0]
            theme = (query.get('theme') or ['light'])[0]
            lang = (query.get('lang') or ['en'])[0]
            if not SAFE_NAME.fullmatch(page) or theme not in THEMES or lang not in LANGS:
                self.send_error(400)
                return
            try:
                fixture = load_fixture(fixtures, page)
            except (OSError, ValueError):
                self.send_error(500, 'Invalid fixture')
                return
            commands, events = mock_context(fixture, theme, lang)
            self._send(render_mock_js(commands, events, page).encode('utf-8'),
                       'application/javascript; charset=utf-8')

    return Handler


def start_server(dist, fixtures):
    """Serve dist/ on 127.0.0.1 with a random free port — never 0.0.0.0."""
    server = ThreadingHTTPServer(('127.0.0.1', 0), make_handler(dist, fixtures))
    threading.Thread(target=server.serve_forever, daemon=True).start()
    return server


# --- screenshots ---------------------------------------------------------------

def find_edge():
    paths = EDGE_PATHS
    try:
        sys.path.insert(0, str(TOOLS))
        from mm import EDGE_PATHS as mm_paths   # reuse the CLI's lookup table
        paths = mm_paths
    except Exception:
        pass
    return next((p for p in paths if p.is_file()), None)


def png_size(path):
    with open(path, 'rb') as stream:
        head = stream.read(24)
    if len(head) != 24 or head[:8] != b'\x89PNG\r\n\x1a\n' or head[12:16] != b'IHDR':
        return None
    return int.from_bytes(head[16:20], 'big'), int.from_bytes(head[20:24], 'big')


def run_edge(cmd):
    """Own Edge's process tree on Windows, including on timeout or interruption."""
    job = None
    if os.name == 'nt':
        from ctypes import wintypes

        class BasicLimits(ctypes.Structure):
            _fields_ = [('process_time', ctypes.c_int64), ('job_time', ctypes.c_int64),
                        ('flags', wintypes.DWORD), ('min_ws', ctypes.c_size_t),
                        ('max_ws', ctypes.c_size_t), ('active', wintypes.DWORD),
                        ('affinity', ctypes.c_size_t), ('priority', wintypes.DWORD),
                        ('scheduling', wintypes.DWORD)]

        class ExtendedLimits(ctypes.Structure):
            _fields_ = [('basic', BasicLimits), ('io', ctypes.c_uint64 * 6),
                        ('process_memory', ctypes.c_size_t), ('job_memory', ctypes.c_size_t),
                        ('peak_process', ctypes.c_size_t), ('peak_job', ctypes.c_size_t)]

        kernel = ctypes.WinDLL('kernel32', use_last_error=True)
        kernel.CreateJobObjectW.argtypes = [ctypes.c_void_p, wintypes.LPCWSTR]
        kernel.CreateJobObjectW.restype = wintypes.HANDLE
        kernel.SetInformationJobObject.argtypes = [wintypes.HANDLE, ctypes.c_int,
                                                   ctypes.c_void_p, wintypes.DWORD]
        kernel.AssignProcessToJobObject.argtypes = [wintypes.HANDLE, wintypes.HANDLE]
        kernel.CloseHandle.argtypes = [wintypes.HANDLE]
        resume = ctypes.WinDLL('ntdll').NtResumeProcess
        resume.argtypes = [wintypes.HANDLE]
        resume.restype = wintypes.LONG
        job = kernel.CreateJobObjectW(None, None)
        if not job:
            raise ctypes.WinError(ctypes.get_last_error())
        limits = ExtendedLimits()
        limits.basic.flags = 0x2000  # JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
        if not kernel.SetInformationJobObject(job, 9, ctypes.byref(limits), ctypes.sizeof(limits)):
            error = ctypes.WinError(ctypes.get_last_error())
            kernel.CloseHandle(job)
            raise error
    try:
        with subprocess.Popen(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                              text=True, encoding='utf-8', errors='replace',
                              stdin=subprocess.DEVNULL,
                              # Assign the job before Edge can spawn any children.
                              creationflags=0x4 if job else 0) as proc:  # CREATE_SUSPENDED
            try:
                if job and not kernel.AssignProcessToJobObject(job, int(proc._handle)):
                    raise ctypes.WinError(ctypes.get_last_error())
                if job and resume(int(proc._handle)) < 0:
                    raise OSError('could not resume Edge')
                stdout, stderr = proc.communicate(timeout=120)
                return subprocess.CompletedProcess(cmd, proc.returncode, stdout, stderr)
            finally:
                if job:
                    kernel.CloseHandle(job)
                    job = None
                if proc.poll() is None:
                    proc.kill()
                proc.wait()
    finally:
        if job:
            kernel.CloseHandle(job)


def shoot_page(edge, base, page, theme, lang, wait_ms, out_dir, profile):
    """Screenshot one page; returns a failure reason or None."""
    width, height = PAGES[page]
    if theme not in THEMES or lang not in LANGS or not isinstance(wait_ms, int) or wait_ms < 0:
        return f'{page}: invalid screenshot options'
    out = out_dir / f'{page}-{theme}-{lang}.png'
    # A previous successful capture must not hide a failed capture on this run.
    out.unlink(missing_ok=True)
    url = f'{base}/{page}.html?__mm_theme={theme}&__mm_lang={lang}'
    cmd = [str(edge), '--headless=new', '--disable-gpu', '--hide-scrollbars', '--no-first-run',
           '--disable-extensions', '--disable-background-networking', '--disable-component-update',
           f'--user-data-dir={profile}', '--force-device-scale-factor=1',
           '--run-all-compositor-stages-before-draw', f'--window-size={width},{height}',
           f'--virtual-time-budget={wait_ms}', f'--lang={lang}']
    if theme == 'dark':
        cmd.append('--force-dark-mode')   # makes prefers-color-scheme: dark match
    cmd += [f'--screenshot={out}', url]
    try:
        proc = run_edge(cmd)
    except subprocess.TimeoutExpired:
        return f'{page}: Edge timed out after 120 s'
    except OSError as error:
        return f'{page}: Edge could not start: {error}'
    if proc.returncode != 0:
        return f'{page}: Edge exited {proc.returncode}: {last_line(proc.stdout + proc.stderr)}'
    size = png_size(out) if out.exists() else None
    if size is None:
        return f'{page}: no PNG produced'
    if size != (width, height):
        return f'{page}: PNG is {size[0]}x{size[1]}, expected {width}x{height}'
    return None


# --- entry point ---------------------------------------------------------------

def build_parser():
    def page_name(value):
        if value not in PAGES:
            raise argparse.ArgumentTypeError('unknown page: ' + value)
        return value

    parser = argparse.ArgumentParser(prog='mm shot', description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('pages', nargs='*', type=page_name, metavar='PAGE',
                        help='pages to shoot (default: all)')
    parser.add_argument('--theme', choices=('light', 'dark', 'both'), default='light')
    parser.add_argument('--lang', choices=('en', 'ru', 'both'), default='en')
    parser.add_argument('--build', action='store_true', help='run npm run build even if dist/ is fresh')
    parser.add_argument('--wait-ms', type=int, default=1500,
                        help='virtual time budget for fonts/animations (default: 1500)')
    return parser


def main(argv=None):
    parser = build_parser()
    args = parser.parse_args(argv)
    if args.wait_ms < 0:
        parser.error('--wait-ms must be >= 0')
    pages = args.pages or list(PAGES)
    themes = expand(args.theme, THEMES)
    langs = expand(args.lang, LANGS)

    edge = find_edge()
    if not edge:
        print('mm shot: Microsoft Edge not found (needed for headless screenshots).')
        return 1
    if dist_stale(DIST) and not shutil.which(npm_cmd()):
        print('mm shot: dist/ is stale and npm is not installed.')
        return 1
    if args.build or dist_stale(DIST):
        print('mm shot: building dist/ (npm run build)...', flush=True)
        ok, why = build_dist()
        if not ok:
            print(f'mm shot: {why}')
            return 1
    if not any(DIST.rglob('*.html')):
        print(f'mm shot: {DIST.relative_to(ROOT)} has no pages to serve.')
        return 1
    missing = [page for page in pages if not (DIST / f'{page}.html').is_file()]
    if missing:
        print('mm shot: dist/ is missing pages: ' + ', '.join(missing))
        return 1

    for page in pages:
        try:
            load_fixture(FIXTURES, page)
        except (OSError, ValueError) as error:
            print(f'mm shot: fixture {error}')
            return 1

    server = start_server(DIST, FIXTURES)
    failures = 0
    try:
        host, port = server.server_address[:2]
        base = f'http://{host}:{port}'
        print(f'mm shot: serving dist/ at {base} (loopback only)')
        SHOTS.mkdir(exist_ok=True)
        with tempfile.TemporaryDirectory(prefix='mm-shot-') as profile:
            for page in pages:
                for theme in themes:
                    for lang in langs:
                        name = f'{page}-{theme}-{lang}'
                        error = shoot_page(edge, base, page, theme, lang,
                                           args.wait_ms, SHOTS, profile)
                        if error:
                            failures += 1
                            print(f'FAIL  {error}')
                        else:
                            print(f'OK    {(SHOTS / (name + ".png")).relative_to(ROOT)}')
    except OSError as error:
        print(f'mm shot: {error}')
        return 1
    finally:
        server.shutdown()
        server.server_close()
    if failures:
        print(f'mm shot: {failures} of {len(pages) * len(themes) * len(langs)} shot(s) failed.')
        return 1
    return 0


if __name__ == '__main__':
    sys.exit(main())
