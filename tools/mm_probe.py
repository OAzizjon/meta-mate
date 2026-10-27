"""mm probe: the Meta Model API from the lead's machine (never from the agents' copies).

    mm probe models [--provider meta|openai]
        the model ids the key can use (confirm the names at the kickoff)
    mm probe stream "QUESTION" [--model M] [--provider ...] [--max-tokens N]
        stream one answer: print it live, then time to first token, total time, tokens, cost
    mm probe click FIXTURE "QUESTION" [--max-edge PX] [--model M] [--provider ...] [--dry-run]
        one computer-use turn on fixtures/uia/FIXTURE.png; the model's click is mapped back to
        the fixture's UI Automation tree, and the element it hit is named
    mm probe ledger
        what the probes have spent so far (estimated from the token counts)

Every exchange is recorded in fixtures/replay/<case>/probe-*.json (no key; the image by path),
so the teacher loop can be built and tested offline. The keys (META_API_KEY with META_API_BASE,
or the development fallback CHATGPT_KEY, invariant 8) are read from the project's secrets file
by this process only and never printed. Fail closed: a fixture with a password-like field, a
truncated tree or no PNG is never sent. Standard library only (ffmpeg downscales the image).
"""

import argparse
import base64
import datetime as dt
import http.client
import io
import json
import math
import re
import secrets
import shutil
import stat
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request
import wave
from pathlib import Path

import mm_fixture

ROOT = Path(__file__).absolute().parent.parent
SECRETS_FILE = '.env'
LEDGER = Path.home() / '.cache' / 'metamate' / 'probe-ledger.jsonl'
REPLAY = ROOT / 'fixtures' / 'replay'

# The Meta base URL is a guess until the kickoff: META_API_BASE in the secrets file wins.
# 'key_aliases': other names the owner may have used for the same key (META_KEY, 2026-10-26).
PROVIDERS = {
    'meta': {'key': 'META_API_KEY', 'key_aliases': ('META_KEY',), 'base_key': 'META_API_BASE',
             'base': 'https://api.meta.ai/v1',
             'stream_model': 'muse-spark-1.3', 'click_model': 'muse-spark-1.3', 'computer': True},
    # Development fallbacks (invariant 8). Groq serves Meta's open Llama models.
    'groq': {'key': 'GROQ_KEY', 'base_key': None, 'base': 'https://api.groq.com/openai/v1',
             'stream_model': 'meta-llama/llama-4-scout-17b-16e-instruct',
             'click_model': 'meta-llama/llama-4-scout-17b-16e-instruct', 'computer': False},
    'openrouter': {'key': 'OPENROUTER_KEY', 'base_key': None, 'base': 'https://openrouter.ai/api/v1',
                   'stream_model': None, 'click_model': None, 'computer': False},
    'openai': {'key': 'CHATGPT_KEY', 'base_key': None, 'base': 'https://api.openai.com/v1',
               'stream_model': None, 'click_model': None, 'computer': True},
}
# USD per 1M tokens (input, output); unknown models need --price IN OUT.
PRICES = {'muse-wonder-1.0': (4.0, 20.0)}
DEFAULT_BUDGET_USD = 10.0
CLICK_INSTRUCTIONS = (
    'You guide an older person on a Windows PC. You never act yourself: the person clicks. '
    'Return exactly one computer action: a click on the element that answers the request. '
    'If that element is not visible in the screenshot, say so in one sentence and do not click.'
)
# Mode A of docs/architecture.md ("where is X"): any vision model, no computer tool.
POINT_INSTRUCTIONS = (
    'You guide an older person on a Windows PC. You never act yourself: the person clicks. '
    'Answer with exactly one tag first: <point eNN> with an element id from the list, or <point X,Y> '
    'in screenshot pixels if the target has no id, or <point none> if it is not visible. '
    'Then one short, kind sentence saying what to click.'
)
POINT_TAG = re.compile(r'<point\s+(?:(e\d+)|(\d+(?:\.\d+)?)\s*,\s*(\d+(?:\.\d+)?)|(none))\s*>', re.IGNORECASE)


class ProbeError(Exception):
    pass


# --- secrets ------------------------------------------------------------------

def _unquote(value: str) -> str:
    """A quoted value ends at its closing quote; escaped quotes are refused ('' = no key)."""
    value = value.strip()
    if value[:1] in ('"', "'"):
        end = value.find(value[0], 1)
        if end <= 0 or '\\' in value[1:end]:
            return ''
        return value[1:end]
    return re.split(r'\s+#', value, maxsplit=1)[0].strip()


def is_plain_file(path: Path) -> bool:
    """No symlink, junction or other reparse point, no hard link: a link could point the
    read (or an upload) at another file."""
    try:
        st = path.lstat()
    except OSError:
        return False
    reparse = getattr(st, 'st_file_attributes', 0) & getattr(stat, 'FILE_ATTRIBUTE_REPARSE_POINT', 0x400)
    return stat.S_ISREG(st.st_mode) and not reparse and st.st_nlink == 1


def read_secret(name: str) -> str:
    """One value from the project's secrets file; '' when absent. Never logged."""
    path = ROOT / SECRETS_FILE
    try:
        if not is_plain_file(path):
            return ''
        text = path.read_text(encoding='utf-8-sig', errors='replace')
    except OSError:
        return ''
    for line in text.splitlines():
        m = re.match(r'^\s*(?:export\s+)?([A-Za-z_][A-Za-z0-9_]*)\s*=(.*)$', line)
        if m and m.group(1) == name:
            return _unquote(m.group(2))
    return ''


def scrub(text: str, key: str) -> str:
    """Text from the API reaches the terminal and the recorded JSON: never with the key,
    a masked head or tail of it, or a token-looking string after a key-like word."""
    if key:
        text = text.replace(key, '[key]')
        if len(key) >= 12:
            text = text.replace(key[:8], '[key]').replace(key[-6:], '[key]')
    text = re.sub(r'(?i)\b(bearer\s+)\S+', r'\1[key]', text)
    text = re.sub(r'\b(?:sk|gsk|rk|pk|xai|sk-or|sk-ant)[-_][\w*.-]{4,}', '[key]', text)
    return re.sub(r'(?i)\b(api[ _-]?key|token|secret|authorization)\b([^\n]{0,40}?[:=]?\s*["\']?)([\w*.-]{12,})',
                  r'\1\2[key]', text)


def provider_config(name: str) -> tuple[dict, str, str]:
    cfg = PROVIDERS[name]
    names = (cfg['key'], *cfg.get('key_aliases', ()))
    key = next((value for value in map(read_secret, names) if value), '')
    if not key:
        raise ProbeError(f'no {" or ".join(names)} in the secrets file (names only: env_keys_names.txt)')
    base = ((read_secret(cfg['base_key']) if cfg['base_key'] else '') or cfg['base']).rstrip('/')
    # The key travels in a header: never over plain HTTP.
    if not base.startswith('https://'):
        raise ProbeError(f'{cfg["base_key"] or "the base URL"} must start with https://')
    if name != 'meta':
        print(f'note: {name} is a development fallback, not a Meta model (invariant 8)')
    return cfg, key, base


class _NoRedirect(urllib.request.HTTPRedirectHandler):
    """A redirect could carry the Authorization header to another host: refuse it."""
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        raise ProbeError(f'HTTP {code}: redirect refused (the key is never sent to another URL)')


_OPENER = urllib.request.build_opener(_NoRedirect)


# --- HTTP ---------------------------------------------------------------------

def _retry_after(error: urllib.error.HTTPError, attempt: int) -> float:
    try:
        wait = float(error.headers.get('Retry-After') or 0)
    except ValueError:
        wait = 0.0
    if not math.isfinite(wait) or wait <= 0:
        wait = 2.0 * 2 ** attempt
    return min(wait, 30.0)


def _request(url: str, key: str, payload: dict | None, timeout: float, stream: bool = False,
             body: bytes | None = None, content_type: str | None = None):
    """Only 429 and 503 are retried, and only before any byte of an answer arrived:
    a retry after that could pay twice. `body` + `content_type` send raw bytes instead of
    the JSON `payload` (the multipart ASR upload)."""
    data = body if body is not None else (None if payload is None else json.dumps(payload).encode('utf-8'))
    req = urllib.request.Request(url, data=data, method='GET' if data is None else 'POST')
    # Unredirected: even a followed redirect would not carry the key (the opener refuses them anyway).
    req.add_unredirected_header('Authorization', 'Bearer ' + key)
    req.add_header('Accept', 'text/event-stream' if stream else 'application/json')
    if data is not None:
        req.add_header('Content-Type', content_type or 'application/json')
    for attempt in range(3):
        try:
            return _open(req, timeout)
        except urllib.error.HTTPError as error:
            with error:
                if error.code == 401:
                    raise ProbeError('HTTP 401: the API key was rejected') from None
                if error.code in (429, 503) and attempt < 2:
                    wait = _retry_after(error, attempt)
                    print(f'HTTP {error.code}: retrying in {wait:.0f} s')
                    time.sleep(wait)
                    continue
                try:
                    body = scrub(error.read(20000).decode('utf-8', errors='replace'), key)[:400]
                except OSError:
                    body = '(no body)'
            raise ProbeError(f'HTTP {error.code}: {body}') from None
        except (urllib.error.URLError, TimeoutError, OSError, http.client.HTTPException) as error:
            raise ProbeError(f'network error: {type(error).__name__}') from None
    raise ProbeError('still rate limited or unavailable after 3 attempts')


def _open(req: urllib.request.Request, timeout: float):
    return _OPENER.open(req, timeout=timeout)


def iter_sse(lines):
    """(event, data) pairs of a server-sent-events stream; a blank line ends an event."""
    event, data = None, []
    for raw in lines:
        line = raw.decode('utf-8', errors='replace').rstrip('\r\n') if isinstance(raw, bytes) else raw.rstrip('\r\n')
        if not line:
            if data:
                yield event, '\n'.join(data)
            event, data = None, []
        elif line.startswith(':'):
            continue
        elif line.startswith('event:'):
            event = line[6:].strip()
        elif line.startswith('data:'):
            data.append(line[5:].lstrip())
    if data:
        yield event, '\n'.join(data)


def output_text(response: dict) -> str:
    parts = []
    for item in response.get('output') or []:
        if isinstance(item, dict) and item.get('type') == 'message':
            for c in item.get('content') or []:
                if isinstance(c, dict) and c.get('type') in ('output_text', 'text') and isinstance(c.get('text'), str):
                    parts.append(c['text'])
    return ''.join(parts)


def computer_actions(response: dict) -> list[dict]:
    """Every action of every computer_call: Meta batches them (`actions`), OpenAI sends one (`action`)."""
    found = []
    for item in response.get('output') or []:
        if isinstance(item, dict) and item.get('type') == 'computer_call':
            actions = item.get('actions') or ([item['action']] if isinstance(item.get('action'), dict) else [])
            for a in actions:
                if isinstance(a, dict):
                    found.append({**a, '_call_id': item.get('call_id'),
                                  '_safety': item.get('pending_safety_checks') or []})
    return found


def _visible_rect(fx, node: dict) -> tuple[float, float, float, float] | None:
    """The node's rect clipped to the screenshot, as the element list showed it; None when
    hidden, disabled or outside."""
    rect = node.get('rect')
    if node.get('enabled') is not True or node.get('offscreen') is not False or not rect:
        return None
    x, y, w, h = fx.shot_frame
    left, top = max(x, rect[0]), max(y, rect[1])
    right, bottom = min(x + w, rect[0] + rect[2]), min(y + h, rect[1] + rect[3])
    return (left, top, right - left, bottom - top) if right > left and bottom > top else None


def point_actions(text: str, fx, fmap) -> list[dict]:
    """The first <point ...> tag of a mode-A answer as a click. An element id becomes the
    centre of its visible rect, kept in physical pixels (no rounding through the sent image);
    an id that is unknown or not visible is reported as such; <point none> or no tag: no click."""
    m = POINT_TAG.search(text or '')
    if not m or m.group(4):
        return []
    if m.group(1):
        ident = m.group(1).lower()
        node = next((n for n in fx.nodes if n.get('id') == ident), None)
        rect = _visible_rect(fx, node) if node else None
        if rect is None:
            return [{'type': 'point', '_safety': [], '_unknown_id': ident}]
        px, py = rect[0] + rect[2] / 2, rect[1] + rect[3] / 2
        sx, sy = fmap.to_sent(px, py)
        return [{'type': 'point', 'x': sx, 'y': sy, '_phys': (px, py), '_safety': [], '_id': ident}]
    return [{'type': 'point', 'x': float(m.group(2)), 'y': float(m.group(3)), '_safety': []}]


def _number(v) -> bool:
    return type(v) in (int, float) and math.isfinite(v)


def action_points(action: dict) -> list[tuple[float, float]]:
    if _number(action.get('x')) and _number(action.get('y')):
        return [(action['x'], action['y'])]
    path = action.get('path')
    if isinstance(path, list):
        return [(p['x'], p['y']) for p in path[:1] + path[-1:]
                if isinstance(p, dict) and _number(p.get('x')) and _number(p.get('y'))]
    return []


# --- cost and ledger ----------------------------------------------------------

def usage_tokens(response: dict) -> tuple[int, int]:
    u = response.get('usage') or {}
    return int(u.get('input_tokens') or u.get('prompt_tokens') or 0), int(u.get('output_tokens') or u.get('completion_tokens') or 0)


def cost_usd(model: str, tokens_in: int, tokens_out: int, price: tuple[float, float] | None,
             reported=None) -> float | None:
    """--price wins, then the price table, then the cost the provider reported (OpenRouter
    returns usage.cost); None when nothing is known."""
    p = price or PRICES.get(model)
    if p is not None:
        return round(tokens_in / 1e6 * p[0] + tokens_out / 1e6 * p[1], 6)
    return round(float(reported), 6) if _number(reported) and reported >= 0 else None


def _ledger_rows(provider: str) -> list[dict]:
    """A corrupt line must not zero the budget: it counts as a call of unknown cost."""
    rows = []
    try:
        lines = LEDGER.read_text(encoding='utf-8').splitlines()
    except OSError:
        return rows
    for line in lines:
        if not line.strip():
            continue
        try:
            row = json.loads(line)
        except ValueError:
            row = {'provider': provider, 'usd': None, 'corrupt': True}
        if isinstance(row, dict) and row.get('provider') == provider:
            rows.append(row)
    return rows


def ledger_total(provider: str) -> float:
    return sum(r['usd'] for r in _ledger_rows(provider) if _number(r.get('usd')) and r['usd'] > 0)


def ledger_unpriced(provider: str) -> int:
    return sum(1 for r in _ledger_rows(provider) if not _number(r.get('usd')))


def ledger_add(row: dict) -> None:
    LEDGER.parent.mkdir(parents=True, exist_ok=True)
    with LEDGER.open('a', encoding='utf-8') as f:
        f.write(json.dumps(row) + '\n')


def guard_budget(provider: str, budget: float, max_unpriced: int = 30) -> None:
    """Dollars when the price is known; calls of unknown price are capped by count, so an
    unpriced model (Muse Spark until its price is known) cannot run without limit."""
    spent = ledger_total(provider)
    if spent >= budget:
        raise ProbeError(f'budget reached: ${spent:.2f} of ${budget:.2f} for {provider} (see `mm probe ledger`, raise --budget)')
    unpriced = ledger_unpriced(provider)
    if unpriced >= max_unpriced:
        raise ProbeError(f'{unpriced} calls of unknown price for {provider}: pass --price IN OUT (USD per 1M tokens) '
                         'or raise --max-unpriced')


def record(case: str, payload: dict, key: str = '') -> Path:
    """fixtures/ is committed: everything recorded passes through scrub()."""
    folder = REPLAY / re.sub(r'[^A-Za-z0-9_.-]', '_', case)
    folder.mkdir(parents=True, exist_ok=True)
    stamp = dt.datetime.now(dt.timezone.utc).strftime('%Y%m%dT%H%M%S%fZ')  # microseconds: no overwrite
    path = folder / f'probe-{stamp}-{payload["provider"]}.json'
    path.write_bytes((scrub(json.dumps(payload, ensure_ascii=False, indent=1), key) + '\n').encode('utf-8'))
    return path


def report(model: str, t_first: float | None, t_total: float, tokens: tuple[int, int], usd: float | None) -> None:
    first = f'first token {t_first * 1000:.0f} ms, ' if t_first is not None else ''
    cost = f'${usd:.4f}' if usd is not None else 'cost unknown (pass --price IN OUT, USD per 1M tokens)'
    print(f'\n{model}: {first}total {t_total * 1000:.0f} ms, tokens in {tokens[0]} / out {tokens[1]}, {cost}')


ASR_MODEL = 'muse-voice-transcribe-1.0'
ASR_MAX_BYTES = 32 * 1024 * 1024


def multipart(fields: list[tuple[str, str | None, bytes, str]]) -> tuple[bytes, str]:
    """(name, filename, data, content type) parts as multipart/form-data."""
    boundary = 'mm-' + secrets.token_hex(12)
    out = []
    for name, filename, data, ctype in fields:
        disposition = f'form-data; name="{name}"' + (f'; filename="{filename}"' if filename else '')
        head = f'--{boundary}\r\nContent-Disposition: {disposition}\r\nContent-Type: {ctype}\r\n\r\n'
        out.append(head.encode() + data + b'\r\n')
    out.append(f'--{boundary}--\r\n'.encode())
    return b''.join(out), f'multipart/form-data; boundary={boundary}'


def wav_info(data: bytes) -> tuple[int, int, float]:
    """(channels, sample rate, seconds); refuses what the ASR file endpoint does not take."""
    try:
        with wave.open(io.BytesIO(data)) as w:
            channels, width, rate, frames = w.getnchannels(), w.getsampwidth(), w.getframerate(), w.getnframes()
    except (wave.Error, EOFError) as error:
        raise ProbeError(f'not a WAV file: {error}') from None
    if channels != 1 or width != 2 or rate not in (16000, 24000):
        raise ProbeError(f'need mono 16-bit PCM at 16 or 24 kHz, got {channels} ch, {8 * width}-bit, {rate} Hz')
    return channels, rate, frames / rate


def cmd_asr(args) -> int:
    """Muse Voice Transcribe, file endpoint (S5): one WAV in, the transcript and the time out."""
    if args.provider != 'meta':
        raise ProbeError('speech recognition is Muse Voice Transcribe: --provider meta only')
    audio = Path(args.audio)
    if not is_plain_file(audio):
        raise ProbeError(f'{audio}: not a plain file')
    data = audio.read_bytes()
    if len(data) > ASR_MAX_BYTES:
        raise ProbeError('the file endpoint takes at most 32 MB')
    _, rate, seconds = wav_info(data)
    _, key, base = provider_config('meta')
    guard_budget('meta', args.budget, args.max_unpriced)
    model = args.model or ASR_MODEL
    request = {'mode': args.mode, 'model': model, 'audioEncoding': 'WAV'}
    body, ctype = multipart([('request', None, json.dumps(request).encode('utf-8'), 'application/json'),
                             ('audio', audio.name, data, 'audio/wav')])
    started = time.perf_counter()
    with _request(base + '/asr/transcribe', key, None, args.timeout, body=body, content_type=ctype) as resp:
        result = json.loads(resp.read().decode('utf-8'))
    total = time.perf_counter() - started
    transcript = result.get('transcript') if isinstance(result, dict) else None
    print(scrub(str(transcript), key))
    print(f'\n{model}: {seconds:.2f} s of {rate // 1000} kHz audio, answered in {total * 1000:.0f} ms '
          f'(real-time factor {total / seconds:.2f}), cost unknown')
    ledger_add({'ts': time.time(), 'provider': 'meta', 'model': model, 'kind': 'asr',
                'audio_s': round(seconds, 3), 'usd': None})
    path = record('asr', {'provider': 'meta', 'model': model, 'kind': 'asr', 'audio': audio.name,
                          'audio_s': round(seconds, 3), 'total_ms': round(total * 1000), 'request': request,
                          'response': result}, key)
    print(f'recorded: {path.relative_to(ROOT).as_posix()}')
    return 0


# --- commands -----------------------------------------------------------------

def cmd_models(args) -> int:
    _, key, base = provider_config(args.provider)
    with _request(base + '/models', key, None, 30) as resp:
        data = json.loads(resp.read().decode('utf-8'))
    print(' '.join(sorted(str(m.get('id')) for m in data.get('data') or [] if isinstance(m, dict))))
    return 0


def add_reasoning(payload: dict, args) -> None:
    """Muse Spark reasons before it answers (S4, 2026-10-26: first token after 7-10 s):
    --reasoning sets the Responses API's reasoning effort."""
    if getattr(args, 'reasoning', None):
        payload['reasoning'] = {'effort': args.reasoning}


def cmd_stream(args) -> int:
    cfg, key, base = provider_config(args.provider)
    model = args.model or cfg['stream_model']
    if not model:
        raise ProbeError(f'pass --model for {args.provider} (`mm probe models --provider {args.provider}`)')
    guard_budget(args.provider, args.budget, args.max_unpriced)
    payload = {'model': model, 'input': args.question, 'stream': True, 'max_output_tokens': args.max_tokens}
    add_reasoning(payload, args)
    started = time.perf_counter()
    first, text, response, events, completed, failure = None, [], {}, [], False, None
    try:
        with _request(base + '/responses', key, payload, args.timeout, stream=True) as resp:
            for event, data in iter_sse(resp):
                if data.strip() == '[DONE]':
                    # Some proxies end with [DONE] and no response.completed: fine if text came.
                    completed = completed or (bool(text) and failure is None)
                    break
                try:
                    d = json.loads(data)
                except ValueError:
                    continue
                kind = d.get('type') or event or ''
                events.append(kind)
                if kind.endswith('output_text.delta') and isinstance(d.get('delta'), str):
                    if first is None:
                        first = time.perf_counter() - started
                    text.append(d['delta'])
                    print(scrub(d['delta'], key), end='', flush=True)
                elif kind.endswith('output_text.done') and not text and isinstance(d.get('text'), str):
                    text.append(d['text'])
                elif kind in ('response.completed', 'response.done') and isinstance(d.get('response'), dict):
                    response = d['response']
                    completed = response.get('status') in (None, 'completed') and not response.get('error')
                    if not completed:
                        failure = f'status {response.get("status")}'
                elif kind in ('error', 'response.failed', 'response.incomplete') or d.get('error'):
                    if isinstance(d.get('response'), dict):
                        response = d['response']  # its usage still goes to the ledger
                    failure = scrub(json.dumps(d, ensure_ascii=False), key)[:400]
                    break
    except (OSError, http.client.HTTPException) as error:
        # The connection dropped mid-answer: no retry (it could pay twice), nothing recorded.
        failure = f'connection lost mid-stream ({type(error).__name__})'
    total = time.perf_counter() - started
    if not text and response:
        print(scrub(output_text(response), key), end='')
    tokens = usage_tokens(response)
    usd = cost_usd(model, *tokens, args.price, (response.get('usage') or {}).get('cost'))
    report(model, first, total, tokens, usd)
    ledger_add({'ts': time.time(), 'provider': args.provider, 'model': model, 'kind': 'stream',
                'in': tokens[0], 'out': tokens[1], 'usd': usd})
    if failure or not completed:
        # A failed or cut stream is not a valid replay: report it, record nothing.
        raise ProbeError(f'stream not completed: {failure or "no response.completed event"} ({len(text)} chunks received)')
    path = record('stream', {'provider': args.provider, 'model': model, 'kind': 'stream', 'question': args.question,
                             'first_token_ms': None if first is None else round(first * 1000),
                             'total_ms': round(total * 1000), 'text': ''.join(text) or output_text(response),
                             'event_types': sorted(set(events)), 'response': response}, key)
    print(f'recorded: {path.relative_to(ROOT).as_posix()}')
    return 0


def _downscaled_png(png: Path, size: tuple[int, int], original: tuple[int, int]) -> bytes:
    if size == original:
        return png.read_bytes()
    ffmpeg = shutil.which('ffmpeg')
    if not ffmpeg:
        raise ProbeError('ffmpeg is needed to downscale the screenshot (or pass --max-edge 0)')
    with tempfile.TemporaryDirectory() as tmp:
        out = Path(tmp) / 'sent.png'
        subprocess.run([ffmpeg, '-v', 'error', '-y', '-i', str(png), '-vf', f'scale={size[0]}:{size[1]}:flags=lanczos', str(out)],
                       check=True, stdin=subprocess.DEVNULL, timeout=60)
        return out.read_bytes()


def verdict(hit) -> str:
    """The teacher's own bands (docs/architecture.md): a click within 8 px of an interactive
    element counts, within 40 px is "Almost", farther is a miss."""
    if hit.node is not None and hit.interactive:
        return 'HIT'
    distance = hit.distance_px
    if distance is None:
        return 'MISS'
    return 'HIT' if distance <= 8 else 'NEAR' if distance <= 40 else 'MISS'


def computer_tool(provider: str, size: tuple[int, int]) -> dict:
    if provider == 'openai':
        return {'type': 'computer_use_preview', 'display_width': size[0], 'display_height': size[1], 'environment': 'windows'}
    return {'type': 'computer'}


def png_size(path: Path) -> tuple[int, int] | None:
    try:
        with path.open('rb') as f:
            head = f.read(24)
    except OSError:
        return None
    if head[:8] != b'\x89PNG\r\n\x1a\n' or head[12:16] != b'IHDR':
        return None
    return int.from_bytes(head[16:20], 'big'), int.from_bytes(head[20:24], 'big')


def cmd_click(args) -> int:
    fx = mm_fixture.load_fixture(ROOT, args.fixture)
    reason = mm_fixture.unsafe_reason(fx)
    if reason:
        raise ProbeError(f'refused: {reason} (fail closed, invariant 3)')
    # Only plain files under fixtures/uia: a link could upload some other image.
    for path in (fx.json_path, fx.png_path):
        if not is_plain_file(path) or path.resolve().parent != (ROOT / 'fixtures' / 'uia').resolve():
            raise ProbeError(f'refused: {path.name} is a link or lies outside fixtures/uia')
    original = (int(fx.shot_frame[2]), int(fx.shot_frame[3]))
    if png_size(fx.png_path) != original:
        raise ProbeError(f'refused: {fx.png_path.name} is not {original[0]}x{original[1]} (the shot_frame size)')
    if args.max_edge < 0:
        raise ProbeError('--max-edge must be 0 (send as is) or positive')
    size = mm_fixture.FrameMap(fx.shot_frame, original).fit(args.max_edge) if args.max_edge else original
    fmap = mm_fixture.FrameMap(fx.shot_frame, size)
    elements = mm_fixture.element_list(fx, fmap)
    text = f'{args.question}\n\nInteractive elements (id, type, name, [x, y, w, h] in screenshot pixels):\n' + '\n'.join(elements)
    print(f'{fx.name}: shot {original[0]}x{original[1]} -> sent {size[0]}x{size[1]}, {len(elements)} elements')
    if args.dry_run:
        print(text)
        return 0
    cfg, key, base = provider_config(args.provider)
    model = args.model or cfg['click_model']
    if not model:
        raise ProbeError(f'pass --model for {args.provider} (`mm probe models --provider {args.provider}`)')
    mode = args.mode or ('computer' if cfg['computer'] else 'point')
    guard_budget(args.provider, args.budget, args.max_unpriced)
    image = base64.b64encode(_downscaled_png(fx.png_path, size, original)).decode('ascii')
    payload = {'model': model, 'instructions': CLICK_INSTRUCTIONS if mode == 'computer' else POINT_INSTRUCTIONS,
               'input': [{'type': 'message', 'role': 'user', 'content': [{'type': 'input_text', 'text': text},
                                                      {'type': 'input_image', 'image_url': 'data:image/png;base64,' + image}]}]}
    if mode == 'computer':
        payload['tools'] = [computer_tool(args.provider, size)]
        if args.provider == 'openai':
            payload['truncation'] = 'auto'
    add_reasoning(payload, args)
    started = time.perf_counter()
    try:
        with _request(base + '/responses', key, payload, args.timeout) as resp:
            response = json.loads(resp.read().decode('utf-8'))
    except (OSError, http.client.HTTPException) as error:
        raise ProbeError(f'connection lost while reading the answer ({type(error).__name__})') from None
    total = time.perf_counter() - started
    if not isinstance(response, dict):
        raise ProbeError('the answer is not a JSON object')
    # The call is paid once the answer is here: the ledger row comes before anything that can fail.
    tokens = usage_tokens(response)
    usd = cost_usd(model, *tokens, args.price, (response.get('usage') or {}).get('cost'))
    ledger_add({'ts': time.time(), 'provider': args.provider, 'model': model, 'kind': 'click',
                'in': tokens[0], 'out': tokens[1], 'usd': usd})
    if response.get('error'):
        raise ProbeError(f'API error: {scrub(json.dumps(response["error"], ensure_ascii=False)[:400], key)}')
    said = output_text(response)
    if said:
        print(f'said: {scrub(said.strip(), key)}')
    hits = []
    actions = computer_actions(response) if mode == 'computer' else point_actions(said, fx, fmap)
    if not actions:
        print('no click (the model may have said the element is not visible)')
    for a in actions:
        if a['_safety']:
            print(f'pending_safety_checks: {json.dumps(a["_safety"], ensure_ascii=False)} (a human must confirm)')
        if a.get('_unknown_id'):
            line = f'MISS <point {a["_unknown_id"]}>: no visible element with this id'
            print(line)
            hits.append(line)
            continue
        points = action_points(a)
        if not points:
            # keypress, type, wait, screenshot...: the teacher shows these, nothing to hit-test.
            line = f'ACTION {a.get("type", "?")} without coordinates: {json.dumps({k: v for k, v in a.items() if not k.startswith("_")}, ensure_ascii=False)[:200]}'
            print(line)
            hits.append(line)
        for sx, sy in points:
            inside = 0 <= sx < size[0] and 0 <= sy < size[1]
            px, py = a['_phys'] if '_phys' in a else fmap.to_phys(sx, sy)
            hit = mm_fixture.hit_test(fx, px, py)
            line = (f'{verdict(hit)} {a.get("type", "?")} at sent ({sx:.0f}, {sy:.0f}) -> physical ({px:.0f}, {py:.0f}): '
                    f'{mm_fixture.describe(hit)}')
            print(line + ('' if inside else '  [outside the screenshot]'))
            hits.append(line)
    report(model, None, total, tokens, usd)
    sent = {**payload, 'input': [{'role': 'user', 'content': [
        {'type': 'input_text', 'text': text},
        {'type': 'input_image', 'image_path': fx.png_path.relative_to(ROOT).as_posix(), 'sent_size': list(size)}]}]}
    path = record(fx.name, {'provider': args.provider, 'model': model, 'kind': 'click', 'question': args.question,
                            'shot_frame': list(fx.shot_frame), 'sent_size': list(size), 'total_ms': round(total * 1000),
                            'request': sent, 'response': response, 'hits': hits}, key)
    print(f'recorded: {path.relative_to(ROOT).as_posix()}')
    return 0


SAM_MODEL = 'sam-3.1'
SAM_BOX = re.compile(r'<\|box;x1=(\d+);y1=(\d+);x2=(\d+);y2=(\d+);w=(\d+);h=(\d+)\|>')


def cmd_sam(args) -> int:
    """SAM 3.1 on a fixture: a point prompt at (x, y) in the PNG's pixels, the boxes it
    returns (masks are not printed), and the element under each box's center. The image
    goes as it is (no downscale): SAM answers in the sent image's pixels."""
    fx = mm_fixture.load_fixture(ROOT, args.fixture)
    reason = mm_fixture.unsafe_reason(fx)
    if reason:
        raise ProbeError(f'refused: {reason} (fail closed, invariant 3)')
    for path in (fx.json_path, fx.png_path):
        if not is_plain_file(path) or path.resolve().parent != (ROOT / 'fixtures' / 'uia').resolve():
            raise ProbeError(f'refused: {path.name} is a link or lies outside fixtures/uia')
    size = png_size(fx.png_path)
    if size is None or size != (int(fx.shot_frame[2]), int(fx.shot_frame[3])):
        raise ProbeError(f'refused: {fx.png_path.name} does not match its shot_frame')
    width, height = size
    if not (0 <= args.x < width and 0 <= args.y < height):
        raise ProbeError(f'the point must lie inside the {width}x{height} image')
    prompt = args.text or f'<0f>1<|point;x={args.x};y={args.y};w={width};h={height}|>'
    _, key, base = provider_config(args.provider)
    guard_budget(args.provider, args.budget, args.max_unpriced)
    image = base64.b64encode(fx.png_path.read_bytes()).decode('ascii')
    payload = {'model': args.model or SAM_MODEL, 'stream': True, 'input': [{
        'type': 'message', 'role': 'user', 'content': [
            {'type': 'input_text', 'text': prompt},
            {'type': 'input_image', 'image_url': 'data:image/png;base64,' + image}]}]}
    started = time.perf_counter()
    text, final, t_first = [], None, None
    try:
        with _request(base + '/responses', key, payload, args.timeout, stream=True) as resp:
            for event, data in iter_sse(resp):
                try:
                    message = json.loads(data)
                except ValueError:
                    continue
                kind = event or message.get('type')
                if kind == 'response.output_text.delta':
                    t_first = t_first or time.perf_counter() - started
                    text.append(str(message.get('delta', '')))
                elif kind in ('response.completed', 'response.incomplete', 'response.failed'):
                    final = message.get('response') or message
                    break
    except (OSError, http.client.HTTPException) as error:
        raise ProbeError(f'connection lost while reading the answer ({type(error).__name__})') from None
    total = time.perf_counter() - started
    tokens = usage_tokens(final) if isinstance(final, dict) else (0, 0)
    usd = cost_usd(payload['model'], *tokens, args.price, ((final or {}).get('usage') or {}).get('cost'))
    ledger_add({'ts': time.time(), 'provider': args.provider, 'model': payload['model'], 'kind': 'sam',
                'in': tokens[0], 'out': tokens[1], 'usd': usd})
    answer = ''.join(text)
    boxes = [tuple(map(int, m.groups())) for m in SAM_BOX.finditer(answer)]
    print(f'{fx.name}: {width}x{height}, prompt {prompt!r}, status {(final or {}).get("status")}, '
          f'{len(answer)} characters, {len(boxes)} box(es)')
    lines = []
    for x1, y1, x2, y2, w, h in boxes:
        fmap = mm_fixture.FrameMap(fx.shot_frame, size)
        px, py = fmap.to_phys((x1 + x2) / 2, (y1 + y2) / 2)
        hit = mm_fixture.hit_test(fx, px, py)
        line = (f'box ({x1}, {y1})-({x2}, {y2}) of {w}x{h}, {x2 - x1 + 1}x{y2 - y1 + 1} px, '
                f'point inside: {x1 <= args.x <= x2 and y1 <= args.y <= y2}; center on {mm_fixture.describe(hit)}')
        print(line)
        lines.append(line)
    report(payload['model'], t_first, total, tokens, usd)
    sent = {**payload, 'input': [{'role': 'user', 'content': [
        {'type': 'input_text', 'text': prompt},
        {'type': 'input_image', 'image_path': fx.png_path.relative_to(ROOT).as_posix(), 'sent_size': list(size)}]}]}
    path = record(fx.name, {'provider': args.provider, 'model': payload['model'], 'kind': 'sam',
                            'total_ms': round(total * 1000), 'first_ms': round((t_first or 0) * 1000),
                            'request': sent, 'answer_chars': len(answer), 'answer_head': answer[:400],
                            'status': (final or {}).get('status'), 'usage': (final or {}).get('usage'), 'boxes': lines}, key)
    print(f'recorded: {path.relative_to(ROOT).as_posix()}')
    return 0


def cmd_ledger(args) -> int:
    rows = []
    try:
        for line in LEDGER.read_text(encoding='utf-8').splitlines():
            if line.strip():
                try:
                    rows.append(json.loads(line))
                except ValueError:
                    rows.append({'provider': '(corrupt line)', 'usd': None})
    except OSError:
        pass
    if not rows:
        print('no probe calls yet')
        return 0
    for provider in sorted({r.get('provider') for r in rows}):
        mine = [r for r in rows if r.get('provider') == provider]
        known = sum(r['usd'] for r in mine if isinstance(r.get('usd'), (int, float)))
        unknown = sum(1 for r in mine if not isinstance(r.get('usd'), (int, float)))
        print(f'{provider}: {len(mine)} calls, ${known:.4f} estimated' + (f', {unknown} at unknown prices' if unknown else ''))
    return 0


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog='mm probe', description='The Meta Model API from the lead machine')
    sub = parser.add_subparsers(dest='cmd', required=True)
    common = argparse.ArgumentParser(add_help=False)
    common.add_argument('--provider', choices=sorted(PROVIDERS), default='meta')
    common.add_argument('--model')
    common.add_argument('--timeout', type=float, default=120)
    common.add_argument('--budget', type=float, default=DEFAULT_BUDGET_USD, help='USD cap for this provider in the ledger')
    common.add_argument('--max-unpriced', type=int, default=30, help='cap on calls of unknown price for this provider')
    common.add_argument('--price', type=float, nargs=2, metavar=('IN', 'OUT'), help='USD per 1M input/output tokens')
    common.add_argument('--reasoning', metavar='EFFORT', help='reasoning effort sent to the API (none, minimal, low, ...)')
    sp = sub.add_parser('models', parents=[common])
    sp.set_defaults(func=cmd_models)
    sp = sub.add_parser('stream', parents=[common])
    sp.add_argument('question')
    sp.add_argument('--max-tokens', type=int, default=400)
    sp.set_defaults(func=cmd_stream)
    sp = sub.add_parser('click', parents=[common])
    sp.add_argument('fixture')
    sp.add_argument('question')
    sp.add_argument('--max-edge', type=int, default=1280, help='long edge of the sent image; 0 sends it as is')
    sp.add_argument('--dry-run', action='store_true', help='build the request and print the element list; send nothing')
    sp.add_argument('--mode', choices=('computer', 'point'),
                    help='computer: the computer-use tool (Meta); point: a <point eNN> answer, any vision model')
    sp.set_defaults(func=cmd_click)
    sp = sub.add_parser('asr', parents=[common], help='Muse Voice Transcribe on a WAV file (file endpoint)')
    sp.add_argument('audio')
    sp.add_argument('--mode', default='PUSH_TO_TALK', choices=('PUSH_TO_TALK', 'ENDPOINTING'))
    sp.set_defaults(func=cmd_asr)
    sp = sub.add_parser('sam', parents=[common], help='SAM 3.1: a point prompt on a fixture, the boxes it returns')
    sp.add_argument('fixture')
    sp.add_argument('x', type=int, help='pixels of the fixture PNG')
    sp.add_argument('y', type=int)
    sp.add_argument('--text', help='a concept prompt instead of the point (e.g. "send button")')
    sp.set_defaults(func=cmd_sam)
    sp = sub.add_parser('ledger')
    sp.set_defaults(func=cmd_ledger)
    return parser


def main(argv: list[str]) -> int:
    if hasattr(sys.stdout, 'reconfigure'):
        sys.stdout.reconfigure(encoding='utf-8', errors='replace')
    # Run directly too, never in an agent's copy (mm.py refuses `mm probe` there as well).
    from mm import agent_mode_reason
    reason = agent_mode_reason()
    if reason:
        print(f'mm probe: refused in agent mode ({reason})')
        return 2
    args = build_parser().parse_args(argv)
    try:
        return args.func(args)
    except (ProbeError, ValueError) as error:
        print(f'mm probe: {error}')
        return 1
    except subprocess.SubprocessError as error:
        print(f'mm probe: ffmpeg failed ({type(error).__name__})')
        return 1


if __name__ == '__main__':
    sys.exit(main(sys.argv[1:]))
