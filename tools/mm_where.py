"""mm where TERM: where a name is defined and used, across Rust, TypeScript, Python, locales and docs.

    mm where sign_out            the definition, then the uses grouped by area (ui, app, core, ...)
    mm where robot.sign_in_again a locale key: its English and Russian text, then the code that says it
    mm where SessionRefused --all     every hit, not only the first few per area
    mm where "Написать Mate"     words from a window: the locale key that shows them, then its code

One search finds the spellings a name takes across the languages here: `sign_out` also finds
`signOut` and `sign-out`. A Tauri command is marked as such, and its `invoke("...")` callers
show under `ui`. Paths are `path:line` so `mm cite` and editors can jump to them. Standard
library only; it reads the files itself (no ripgrep needed), never a secrets file.
"""

import argparse
import json
import os
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).absolute().parent.parent

SKIP_DIRS = {'.git', 'target', 'node_modules', 'dist', '__pycache__', '.cache', '.venv', 'venv'}
TEXT_EXTS = {'.rs', '.ts', '.tsx', '.js', '.mjs', '.py', '.html', '.css', '.json', '.md', '.toml',
             '.ps1', '.sh', '.txt', '.yml', '.yaml'}
SKIP_FILES = {'package-lock.json', 'Cargo.lock'}
MAX_BYTES = 1_000_000
SELF = {'tools/mm_where.py', 'tools/tests/test_mm_where_cite.py', 'docs/code-map.md'}

# Order of the groups on screen: where a reader looks first.
AREAS = (
    ('ui', ('src/', 'home.html', 'settings.html', 'chat.html', 'signin.html', 'mate.html',
            'highlight.html', 's3.html', 'index.html')),
    ('app', ('src-tauri/',)),
    ('core', ('crates/mm-core/',)),
    ('win32', ('crates/mm-win32/',)),
    ('platform', ('crates/mm-platform/',)),
    ('server', ('server/',)),
    ('tts', ('tts/',)),
    ('tools', ('tools/',)),
    ('locales', ('locales/',)),
    ('site', ('site/',)),
    ('docs', ('docs/', 'README.md', 'CHANGELOG.md', 'CLAUDE.md')),
)

DEFINITION = (
    # Rust
    r'\b(?:fn|struct|enum|trait|type|const|static|mod|macro_rules!)\s+{name}\b',
    r'^\s*{name}\s*[({{,]',  # an enum variant or struct field on its own line
    # TypeScript / JavaScript
    r'\b(?:function|class|interface|type|enum)\s+{name}\b',
    r'\b(?:const|let|var)\s+{name}\s*[=:]',
    # Python
    r'^\s*(?:async\s+)?def\s+{name}\b',
    r'^\s*class\s+{name}\b',
    r'^{name}\s*=',
)


def is_secret(path):
    """The secrets files are never opened (CLAUDE.md, Secrets), in any case: Windows opens
    `.ENV` as `.env`."""
    name = Path(path).name.lower().rstrip('. ')
    return name == '.env' or name.startswith('.env.')


def variants(term):
    """The spellings one name takes here: sign_out, signOut, SignOut, sign-out, SIGN_OUT."""
    if not re.fullmatch(r'[A-Za-z_][A-Za-z0-9_-]*', term):
        return [term]
    words = [w for w in re.split(r'[_-]|(?<=[a-z0-9])(?=[A-Z])', term) if w]
    if len(words) < 2:
        return [term]
    lower = [w.lower() for w in words]
    spellings = [
        '_'.join(lower),
        lower[0] + ''.join(w.capitalize() for w in lower[1:]),
        ''.join(w.capitalize() for w in lower),
        '-'.join(lower),
        '_'.join(lower).upper(),
    ]
    out = [term]
    for spelling in spellings:
        if spelling not in out:
            out.append(spelling)
    return out


def area_of(path):
    for area, prefixes in AREAS:
        if any(path == p or path.startswith(p) for p in prefixes):
            return area
    return 'other'


def is_test_path(path):
    return ('/tests/' in path or path.startswith('tests/') or path.endswith(('.test.ts', '_test.py'))
            or Path(path).name.startswith('test_'))


def tracked_files(root):
    """The files git knows (tracked and untracked but not ignored); a walk without git."""
    try:
        out = subprocess.run(['git', 'ls-files', '--cached', '--others', '--exclude-standard'],
                             cwd=root, capture_output=True, text=True, encoding='utf-8', timeout=30)
        if out.returncode == 0 and out.stdout.strip():
            return sorted(set(out.stdout.splitlines()))
    except (OSError, subprocess.TimeoutExpired):
        pass
    files = []
    for folder, dirs, names in os.walk(root):
        dirs[:] = [d for d in dirs if d not in SKIP_DIRS and not d.startswith('.')]
        for name in names:
            files.append(Path(folder, name).relative_to(root).as_posix())
    return sorted(files)


def searchable(path):
    p = Path(path)
    return (p.suffix in TEXT_EXTS and p.name not in SKIP_FILES and not is_secret(path)
            and not any(part in SKIP_DIRS for part in p.parts))


def read_lines(root, path):
    full = Path(root, path)
    try:
        if full.stat().st_size > MAX_BYTES:
            return []
        return full.read_text(encoding='utf-8', errors='replace').splitlines()
    except OSError:
        return []


def first_test_line(lines):
    """The line where a Rust file's unit tests start (`#[cfg(test)]`), else None."""
    for number, line in enumerate(lines, 1):
        if line.strip().startswith('#[cfg(test)]'):
            return number
    return None


def search(root, term, files=None, exact=False):
    """Every hit of the term's spellings (only the term itself when `exact`): dicts with path,
    line, text, area, kind."""
    names = [term] if exact else variants(term)
    # `accountStore.signOut()` is a use: a dot before the name is fine.
    word = re.compile('|'.join(rf'(?<![\w]){re.escape(n)}(?![\w])' for n in names))
    definition = re.compile('|'.join(f'(?:{d.format(name=alt)})' for d in DEFINITION
                                     for alt in (f'(?:{"|".join(map(re.escape, names))})',)))
    hits = []
    for path in files if files is not None else tracked_files(root):
        # This tool's own docs and tests are full of example names.
        if not searchable(path) or path in SELF:
            continue
        lines = read_lines(root, path)
        if not lines:
            continue
        tests_from = first_test_line(lines) if path.endswith('.rs') else None
        for number, line in enumerate(lines, 1):
            if not word.search(line):
                continue
            kind = 'use'
            if path.endswith(('.rs', '.ts', '.tsx', '.js', '.py')) and definition.search(line):
                kind = 'def'
            if kind == 'def' and path.endswith('.rs') and any(
                    '#[tauri::command]' in lines[i] for i in range(max(0, number - 4), number - 1)):
                kind = 'command'
            if path.startswith('locales/') and re.search(rf'^\s*"{re.escape(term)}"\s*:', line):
                kind = 'key'
            in_tests = is_test_path(path) or (tests_from is not None and number > tests_from)
            hits.append({'path': path, 'line': number, 'text': line.strip(), 'kind': kind,
                         'area': 'tests' if in_tests and kind != 'key' else area_of(path)})
    return hits


def is_comment(text):
    return text.startswith(('//', '#', '*', '/*', '<!--', '"""', '|', '- '))


def constants_holding(term, hits):
    """(name, path, exported) for each constant whose value is the term: `const EVENT: &str =
    "highlight"` in Rust (used in its own file) or `export const LESSON_CARD_EVENT = "lesson-card"`
    in TypeScript (imported anywhere). Event names and keys often travel this way."""
    found = []
    pattern = re.compile(r'\b(export\s+)?(?:pub(?:\([^)]*\))?\s+)?(?:const|static)\s+([A-Za-z_]\w*)'
                         r'\s*(?::[^=]+)?=\s*["\']' + re.escape(term) + r'["\']')
    for hit in hits:
        match = pattern.search(hit['text'])
        if match:
            exported = bool(match.group(1)) or hit['text'].startswith('pub ')
            found.append((match.group(2), hit['path'], exported))
    return found


def via_constants(root, term, hits, files=None):
    """The uses of the constants that hold the term, as hits marked with `via`."""
    extra = []
    seen = {(h['path'], h['line']) for h in hits}
    for name, path, exported in constants_holding(term, hits):
        scope = None if exported else [path]
        for hit in search(root, name, files=scope if scope else files, exact=True):
            if (hit['path'], hit['line']) in seen or hit['kind'] != 'use':
                continue
            seen.add((hit['path'], hit['line']))
            extra.append({**hit, 'via': name})
    return extra


def trimmed(text, width=110):
    text = re.sub(r'\s+', ' ', text)
    return text if len(text) <= width else text[:width - 1] + '…'


def report(term, hits, limit=6, show_all=False):
    """The screen: definitions first, then uses by area, each capped at `limit`."""
    names = variants(term)
    lines = [f'where {term}' + (f'  (also {", ".join(names[1:])})' if len(names) > 1 else '')]
    if not hits:
        lines.append('  no hits')
        return '\n'.join(lines)
    # The lines that define a constant holding the term are definitions too.
    held = [h for h in hits if not h.get('via') and constants_holding(term, [h])]
    defs = [h for h in hits if (h['kind'] in ('def', 'command', 'key') or h in held)
            and h['area'] != 'tests' and not h.get('via')]
    if defs:
        lines.append('defined')
        for h in defs[:limit * 2 if not show_all else None]:
            mark = {'command': '  [tauri command]', 'key': '  [locale key]'}.get(h['kind'], '')
            if h in held:
                mark = '  [constant]'
            lines.append(f"  {h['path']}:{h['line']}  {trimmed(h['text'])}{mark}")
    commands = {re.search(r'fn\s+(\w+)', h['text']).group(1) for h in defs
                if h['kind'] == 'command' and re.search(r'fn\s+(\w+)', h['text'])}
    uses = [h for h in hits if h not in defs]
    # The constants' uses first: there the name travels where a search for the string misses it.
    groups = [(f"via {name}", [h for h in uses if h.get('via') == name])
              for name in dict.fromkeys(h['via'] for h in uses if h.get('via'))]
    groups += [(area, [h for h in uses if h['area'] == area and not h.get('via')])
               for area in [a for a, _ in AREAS] + ['tests', 'other']]
    for area, group in groups:
        # Code before comments and docs strings: "sign-out" in prose is the least likely target.
        group = sorted(group, key=lambda h: is_comment(h['text']))
        if not group:
            continue
        lines.append(f'{area} ({len(group)})')
        shown = group if show_all else group[:limit]
        for h in shown:
            mark = ''
            if commands and re.search(r'invoke\s*(?:<[^>]*>)?\s*\(\s*["\'](' + '|'.join(commands) + r')["\']', h['text']):
                mark = '  [invoke]'
            lines.append(f"  {h['path']}:{h['line']}  {trimmed(h['text'])}{mark}")
        if len(group) > len(shown):
            lines.append(f'  +{len(group) - len(shown)} more (--all)')
    return '\n'.join(lines)


def locale_text(root, key):
    """A locale key's English and Russian text, or None."""
    out = []
    for lang in ('en', 'ru'):
        try:
            data = json.loads(Path(root, 'locales', f'{lang}.json').read_text(encoding='utf-8-sig'))
        except (OSError, ValueError):
            return None
        if key not in data:
            return None
        out.append(f'  {lang}: {data[key]}')
    return '\n'.join(out)


def keys_with_text(root, text):
    """The locale keys whose English or Russian text contains `text` (any case): a phrase the
    owner quotes from a window ("Написать Mate") leads to the key, and the key to the code."""
    needle = text.casefold().strip(' "«»')
    if len(needle) < 3:
        return []
    keys = []
    for lang in ('en', 'ru'):
        try:
            data = json.loads(Path(root, 'locales', f'{lang}.json').read_text(encoding='utf-8-sig'))
        except (OSError, ValueError):
            continue
        keys += [key for key, value in data.items()
                 if isinstance(value, str) and needle in value.casefold() and key not in keys]
    return keys


def main(argv=None, root=ROOT):
    parser = argparse.ArgumentParser(prog='mm where', description=__doc__.split('\n')[0])
    parser.add_argument('term')
    parser.add_argument('--all', action='store_true', help='every hit, not the first few per area')
    parser.add_argument('--limit', type=int, default=6, help='hits per area (default 6)')
    parser.add_argument('--json', action='store_true', help='the hits as JSON')
    args = parser.parse_args(argv)
    hits = search(root, args.term)
    hits += via_constants(root, args.term, hits)
    if args.json:
        print(json.dumps(hits, ensure_ascii=False, indent=1))
        return 0 if hits else 1
    text = locale_text(root, args.term)
    if text:
        print(f'locale {args.term}\n{text}')
    # Words from a window: the keys that show them, then where each key is used.
    keys = [] if text else keys_with_text(root, args.term)
    if keys and (' ' in args.term.strip() or not any(h['kind'] != 'use' for h in hits)):
        print(f'text "{args.term}" is shown by {len(keys)} locale key(s)')
        for key in keys[:5]:
            key_hits = [h for h in search(root, key, exact=True) if h['kind'] != 'key']
            key_hits += via_constants(root, key, key_hits)
            print(f'\n{key}\n{locale_text(root, key) or ""}')
            body = report(key, key_hits, limit=args.limit, show_all=args.all).splitlines()[1:]
            print('\n'.join(body) if body and body != ['  no hits'] else '  used nowhere in code')
        if len(keys) > 5:
            print(f'\n+{len(keys) - 5} more keys: ' + ', '.join(keys[5:15]))
        return 0
    print(report(args.term, hits, limit=args.limit, show_all=args.all))
    return 0 if hits else 1


if __name__ == '__main__':
    if hasattr(sys.stdout, 'reconfigure'):
        sys.stdout.reconfigure(encoding='utf-8', errors='replace')
    sys.exit(main())
