"""Fetch CC0 and public-domain sounds from Wikimedia Commons."""

import argparse
from datetime import date
from html.parser import HTMLParser
import json
from pathlib import Path
import re
import sys
import time
from urllib.error import HTTPError
from urllib.parse import quote, urlencode, urlparse
from urllib.request import Request, urlopen


ROOT = Path(__file__).resolve().parents[1]
API = 'https://commons.wikimedia.org/w/api.php'
USER_AGENT = 'MetaMate-sound-fetch/0.1 (hackathon project; contact via repo)'
MAX_BYTES = 15 * 1024 * 1024
MIMES = ['audio/ogg', 'audio/x-wav', 'audio/wav', 'audio/flac', 'audio/mpeg']
EXTENSIONS = dict(zip(MIMES, ['ogg', 'wav', 'wav', 'flac', 'mp3']))
EXTENSIONS.update({'application/ogg': 'ogg', 'audio/webm': 'webm',
                   'audio/opus': 'opus', 'audio/aac': 'aac'})
_last_request = None


class PlainText(HTMLParser):
    def __init__(self):
        super().__init__()
        self.parts = []

    def handle_data(self, data):
        self.parts.append(data)


def plain(value):
    parser = PlainText()
    parser.feed(str(value))
    return ' '.join(''.join(parser.parts).split())


def metadata(info, key):
    return plain(info.get('extmetadata', {}).get(key, {}).get('value', ''))


def licence(info):
    # Require every supplied licence label to be explicitly allowed.
    labels = [metadata(info, key) for key in ('LicenseShortName', 'License')]
    labels = [label for label in labels if label]
    if not labels:
        return None
    for label in labels:
        normalized = label.lower().strip()
        if not (re.fullmatch(r'(?:cc0|cc-zero)(?:[ -]1\.0)?(?: universal)?', normalized)
                or re.fullmatch(r'public[ -]domain(?: mark)?(?:[ -]1\.0)?', normalized)
                or re.fullmatch(r'pd(?:-[a-z0-9]+)*', normalized)):
            return None
    return labels[0]


def http(url, limit):
    """Read bounded bytes with request spacing and at most four retries."""
    global _last_request
    for attempt in range(5):
        if _last_request is not None:
            time.sleep(max(0, 2.5 - (time.monotonic() - _last_request)))
        delay = min(60, 2.5 * 2 ** attempt)
        try:
            with urlopen(Request(url, headers={'User-Agent': USER_AGENT}), timeout=20) as response:
                length = response.headers.get('Content-Length')
                if length and int(length) > limit:
                    raise ValueError('response exceeds size limit')
                data = response.read(limit + 1)
                if len(data) > limit:
                    raise ValueError('response exceeds size limit')
            if url.startswith(API + '?'):
                result = json.loads(data)
                if result.get('error', {}).get('code') == 'maxlag':
                    if attempt == 4:
                        raise ValueError('Commons API error: maxlag')
                else:
                    return data
            else:
                return data
        except HTTPError as error:
            try:
                if error.code not in (429, 503) or attempt == 4:
                    raise
                try:
                    delay = max(delay, float(error.headers.get('Retry-After', '0')))
                except (TypeError, ValueError):
                    pass
            finally:
                error.close()
        finally:
            _last_request = time.monotonic()
        time.sleep(delay)


def measured_duration(info):
    for key in ('Duration', 'Length'):
        value = metadata(info, key)
        if value:
            try:
                parts = value.split(':')
                seconds = 0.0
                for part in parts:
                    seconds = seconds * 60 + float(part)
                if seconds > 0:
                    return seconds
            except ValueError:
                pass
    return None


def duration(info):
    seconds = measured_duration(info)
    if seconds is not None:
        return seconds
    # Approximate seconds from typical bitrates when Commons omits duration.
    rates = {'audio/x-wav': 1536000, 'audio/wav': 1536000,
             'audio/flac': 768000, 'audio/mpeg': 128000}
    return info['size'] * 8 / rates.get(info['mime'], 128000)


def candidates(entry):
    params = {'action': 'query', 'format': 'json', 'prop': 'imageinfo',
              'iiprop': 'url|size|mime|extmetadata', 'maxlag': 5}
    if entry.get('file'):
        params['titles'] = entry['file']
    else:
        params.update(generator='search', gsrnamespace=6,
                      gsrsearch='filetype:audio ' + entry['query'], gsrlimit=20)
    result = json.loads(http(API + '?' + urlencode(params), 4 * 1024 * 1024))
    if 'error' in result:
        raise ValueError('Commons API error: ' + str(result['error'].get('code')))
    candidates = []
    for page in result.get('query', {}).get('pages', {}).values():
        title = page['title']
        if (re.match(r'^(File:)?[A-Za-z]{2,3}(-[A-Za-z]{2,4})?-.*\.(oga|ogg|wav|mp3)$',
                     title, re.IGNORECASE)
                or re.search(r'pronunciation|sine|square-wave|sawtooth|white noise|test tone|Hz',
                             title, re.IGNORECASE)):
            continue
        for info in page.get('imageinfo', []):
            label = licence(info)
            size = info.get('size', 0)
            if not label or not 0 < size <= MAX_BYTES or info.get('mime') not in EXTENSIONS:
                continue
            url = urlparse(info.get('url', ''))
            if url.scheme != 'https' or url.hostname != 'upload.wikimedia.org':
                continue
            info = dict(info, title=page['title'], licence=label)
            seconds = duration(info)
            preference = MIMES.index(info['mime']) if info['mime'] in MIMES else len(MIMES)
            rank = ((0, seconds, preference, size) if seconds <= entry['max_seconds']
                    else (1, size, preference, seconds))
            candidates.append((rank, info))
    return [info for rank, info in sorted(candidates, key=lambda item: item[0])]


def choose(entry):
    options = candidates(entry)
    return options[0] if options else None


def target_path(root, entry, extension):
    name = entry['name']
    if (not isinstance(name, str) or not re.fullmatch(r'[a-zA-Z0-9_-]+', name)
            or name.upper() in {'CON', 'PRN', 'AUX', 'NUL',
                               *(f'COM{i}' for i in range(1, 10)),
                               *(f'LPT{i}' for i in range(1, 10))}):
        raise ValueError('unsafe sound name: ' + str(name))
    if entry['kind'] not in ('sfx', 'amb') or extension not in EXTENSIONS.values():
        raise ValueError('invalid sound kind or extension')
    base = root.resolve() / 'assets/sounds/src'
    if base.resolve() != base:
        raise ValueError('sound source directory must not redirect outside src')
    target = base / entry['kind'] / (name + '.' + extension)
    if not target.resolve().is_relative_to(base):
        raise ValueError('sound path escapes assets/sounds/src')
    return target


def table_cell(value):
    return str(value).replace('|', '&#124;').replace('`', '&#96;').replace('\n', ' ')


def update_licences(path, filename, info):
    source = 'https://commons.wikimedia.org/wiki/' + info['title'].replace(' ', '_')
    # Encode titles without allowing Markdown metacharacters into the table.
    source = quote(source, safe=':/_')
    row = (f'| `{filename}` | {source} | {table_cell(info["licence"])} | '
           f'{table_cell(metadata(info, "Artist"))} | '
           f'fetched {date.today().isoformat()} by tools/sounds_commons.py |')
    lines = path.read_text(encoding='utf-8').splitlines()
    pattern = re.compile(r'^\|\s*`' + re.escape(filename) + r'`\s*\|')
    updated = []
    found = False
    for line in lines:
        if pattern.match(line):
            if not found:
                updated.append(row)
                found = True
        else:
            updated.append(line)
    if not found:
        while updated and not updated[-1].strip():
            updated.pop()
        updated.append(row)
    path.write_bytes(('\n'.join(updated) + '\n').encode('utf-8'))


def main(argv=None, root=ROOT):
    if hasattr(sys.stdout, 'reconfigure'):
        sys.stdout.reconfigure(encoding='utf-8', errors='replace')
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--dry-run', action='store_true')
    parser.add_argument('--candidates', type=int, default=5)
    parser.add_argument('--pin', action='append', default=[], metavar='NAME=File:Title.ogg',
                        help='save a file pin and exit without downloading')
    parser.add_argument('--only', help='comma-separated sound names')
    args = parser.parse_args(argv)
    try:
        if args.candidates <= 0:
            raise ValueError('candidates must be positive')
        sources = root / 'assets/sounds/sources.json'
        entries = json.loads(sources.read_text(encoding='utf-8'))
        names = set()
        for entry in entries:
            target_path(root, entry, 'ogg')
            if entry['name'] in names:
                raise ValueError('duplicate sound name: ' + entry['name'])
            names.add(entry['name'])
            if not isinstance(entry['max_seconds'], (int, float)) or entry['max_seconds'] <= 0:
                raise ValueError('invalid max_seconds')
            if entry.get('file') and not entry['file'].startswith('File:'):
                raise ValueError('pinned title must start with File:')
        only = set(args.only.split(',')) if args.only is not None else names
        if only - names:
            raise ValueError('unknown sound names: ' + ', '.join(sorted(only - names)))
        for pin in args.pin:
            name, separator, title = pin.partition('=')
            if not separator or not title.startswith('File:') or not title[5:].strip():
                raise ValueError('pin must be NAME=File:Title.ogg')
            if name not in names:
                raise ValueError('unknown sound name: ' + name)
            next(entry for entry in entries if entry['name'] == name)['file'] = title
        if args.pin:
            sources.write_bytes((json.dumps(entries, indent=2, ensure_ascii=False) + '\n').encode('utf-8'))
            return 0
    except (OSError, ValueError, KeyError, TypeError) as error:
        print(f'Invalid sources: {error}')
        return 1
    failed = False
    for entry in entries:
        if entry['name'] not in only:
            continue
        if not args.dry_run and not entry.get('file'):
            print(f'{entry["name"]}: not pinned (run --dry-run and pin a file)')
            continue
        try:
            if args.dry_run:
                options = candidates(entry)[:args.candidates]
                if not options:
                    raise ValueError('no acceptable file')
                for info in options:
                    estimated = '~' if measured_duration(info) is None else ''
                    source = quote('https://commons.wikimedia.org/wiki/'
                                   + info['title'].replace(' ', '_'), safe=':/_')
                    print(f'{entry["name"]}: {info["title"]} | {info["licence"]} | '
                          f'{estimated}{duration(info):g} s | {info["size"]} bytes | '
                          f'{info["mime"]} | {source}')
                continue
            info = choose(entry)
            if info is None:
                raise ValueError('no acceptable file')
            target = target_path(root, entry, EXTENSIONS[info['mime']])
            print(f'{entry["name"]}: {info["title"]} | {info["licence"]} | {info["size"]} bytes')
            if not args.dry_run:
                data = http(info['url'], MAX_BYTES)
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_bytes(data)
                update_licences(root / 'assets/sounds/LICENSES.md', target.name, info)
        except (OSError, ValueError, KeyError, TypeError) as error:
            print(f'{entry["name"]}: {error}')
            failed = True
    return int(failed)


if __name__ == '__main__':
    raise SystemExit(main())
