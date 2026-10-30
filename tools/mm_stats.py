"""A standard-library-only project dashboard; mm.py owns command dispatch.

Fallback LOC means nonblank physical source lines (including comments). Test
co-changes are a naming/crate heuristic, not a claim about coverage.
"""

import argparse
import ast
from collections import Counter, defaultdict
import datetime as dt
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys


ROOT = Path(__file__).resolve().parents[1]
AREAS = ('crates/mm-core', 'crates/mm-platform', 'crates/mm-win32', 'src-tauri/src', 'src',
         'tools', 'server', 'tts')
LANGUAGES = {'.rs': 'Rust', '.ts': 'TypeScript', '.tsx': 'TypeScript',
             '.js': 'JavaScript', '.mjs': 'JavaScript', '.cjs': 'JavaScript',
             '.py': 'Python', '.html': 'HTML', '.css': 'CSS', '.scss': 'Sass',
             '.ps1': 'PowerShell', '.sh': 'Shell', '.c': 'C', '.h': 'C',
             '.cpp': 'C++', '.go': 'Go', '.sql': 'SQL', '.svg': 'SVG'}
SKIP = {'.git', '.cache', '.draft-input', 'node_modules', 'target', 'dist',
        '__pycache__', '.venv', 'venv', 'gen'}


def area(path):
    path = path.replace('\\', '/').removeprefix('./')
    return next((a for a in AREAS if path.startswith(a + '/')), 'other')


def crate(path):
    parts = path.split('/')
    if len(parts) > 2 and parts[0] == 'crates':
        return '/'.join(parts[:2])
    return 'src-tauri' if path.startswith('src-tauri/') else None


def source_files(root):
    """Walk source areas without needing git, pruning generated directories."""
    for base in ('crates', 'src-tauri', 'src', 'tools', 'server', 'tts'):
        for folder, dirs, files in os.walk(root / base, followlinks=False):
            dirs[:] = sorted(d for d in dirs if d not in SKIP and not (Path(folder) / d).is_symlink())
            for name in sorted(files):
                path = Path(folder) / name
                if path.suffix in LANGUAGES and not path.is_symlink():
                    yield path


def read(path):
    try:
        return path.read_text(encoding='utf-8-sig')
    except (OSError, UnicodeError):
        return None


def run(command, root):
    try:
        result = subprocess.run(command, cwd=root, capture_output=True, text=True,
                                encoding='utf-8', errors='replace', timeout=60,
                                stdin=subprocess.DEVNULL)
        return result.stdout if result.returncode == 0 else None
    except (OSError, subprocess.TimeoutExpired):
        return None


def code_summary(files, method):
    languages, areas = Counter(), Counter({a: 0 for a in AREAS})
    for item in files:
        languages[item['language']] += item['loc']
        areas[area(item['path'])] += item['loc']
    return {'method': method, 'loc': sum(languages.values()),
            'languages': dict(sorted(languages.items())), 'areas': dict(areas),
            'largest': sorted(files, key=lambda f: (-f['loc'], f['path']))[:10],
            'complex': (sorted(files, key=lambda f: (-f['complexity'], f['path']))[:10]
                        if method == 'scc' else None)}


def parse_scc(text, root=None):
    """Parse scc --by-file --format json (language totals contain Files)."""
    files = []
    for language in json.loads(text):
        for item in language.get('Files', []):
            name = item.get('Location') or item['Name']
            if root is not None and Path(name).is_absolute():
                name = Path(name).relative_to(root).as_posix()
            name = str(name).replace('\\', '/').removeprefix('./')
            files.append({'path': name, 'language': language['Name'],
                          'loc': int(item['Code']), 'complexity': int(item.get('Complexity', 0))})
    return code_summary(files, 'scc')


def collect_code(root):
    if shutil.which('scc'):
        output = run(['scc', '--by-file', '--format', 'json', '.'], root)
        if output is not None:
            try:
                return parse_scc(output, root)
            except (ValueError, KeyError, TypeError):
                pass
    tracked = run(['git', 'ls-files', '-z'], root)
    if tracked is None:
        return None
    files = []
    for name in tracked.split('\0'):
        path = root / name
        if path.suffix not in LANGUAGES or path.is_symlink():
            continue
        text = read(path)
        if text is not None:
            files.append({'path': name, 'language': LANGUAGES[path.suffix],
                          'loc': sum(bool(line.strip()) for line in text.splitlines()),
                          'complexity': None})
    return code_summary(files, 'nonblank lines (git tracked)')


def mask_code(text):
    """Mask C/Rust/TS comments and literals, preserving offsets and newlines."""
    token = re.compile(r'//[^\n]*|/\*|r(#+)?"|"(?:\\.|[^"\\])*"|'
                       r"'(?:\\.|[^'\\\n])'|`(?:\\.|[^`\\])*`", re.S)
    chars = list(text)
    end = 0
    while match := token.search(text, end):
        start, end = match.span()
        if match.group() == '/*':
            depth = 1
            while depth and end < len(text):
                next_token = re.search(r'/\*|\*/', text[end:])
                if not next_token:
                    end = len(text)
                    break
                end += next_token.end()
                depth += 1 if next_token.group() == '/*' else -1
        elif match.group().startswith('r'):
            closing = '"' + (match.group(1) or '')
            finish = text.find(closing, end)
            end = len(text) if finish < 0 else finish + len(closing)
        chars[start:end] = ['\n' if c == '\n' else ' ' for c in text[start:end]]
    return ''.join(chars)


def count_tests(text, language):
    """Count declarations, ignoring comments and strings; no code execution."""
    if language == 'Python':
        try:
            return sum(isinstance(n, (ast.FunctionDef, ast.AsyncFunctionDef))
                       and n.name.startswith('test_') for n in ast.walk(ast.parse(text)))
        except SyntaxError:
            return 0
    clean = mask_code(text)
    if language == 'Rust':
        return len(re.findall(r'#\s*\[\s*test\s*\]\s*(?:#\[[^]]*\]\s*)*'
                              r'(?:pub\s+)?(?:async\s+)?(?:unsafe\s+)?fn\s+\w+', clean))
    return len(re.findall(r'\b(?:test|it)\s*\(', clean))


def production_rust(text):
    """Remove #[cfg(test)] items and #[test] functions before counting unwraps."""
    clean = mask_code(text)
    if re.search(r'#!\s*\[\s*cfg\s*\(\s*test\s*\)\s*\]', clean):
        return ''
    pattern = re.compile(r'#\s*\[\s*(?:cfg\s*\(\s*test\s*\)|test)\s*\]')
    chars = list(clean)
    offset = 0
    while match := pattern.search(clean, offset):
        body = re.search(r'[;{]', clean[match.end():])
        if body is None:
            break
        end = match.end() + body.end()
        if body.group() == '{':
            depth = 1
            while depth and end < len(clean):
                depth += (clean[end] == '{') - (clean[end] == '}')
                end += 1
        chars[match.start():end] = ' ' * (end - match.start())
        offset = end
    return ''.join(chars)


def is_test(path):
    p = Path(path)
    return ('tests' in p.parts or p.stem in {'test', 'tests'}
            or p.name.startswith('test_') or '.test.' in p.name or '.spec.' in p.name)


def related_test(source, test):
    """Match adjacent/name-based tests and Rust crate integration tests."""
    if source == test:
        return True
    if crate(source) != crate(test):
        return False
    if crate(source) and crate(source) == crate(test):
        if '/tests/' in test and not '/src/' in test:
            return True
        if test.endswith('/tests.rs') and Path(test).parent == Path(source).parent:
            return True
        if test.endswith('/tests.rs') and Path(test).parent.as_posix() == source[:-3]:
            return True
    s, t = Path(source), Path(test)
    stem = t.stem.removeprefix('test_').removesuffix('.test').removesuffix('.spec')
    return area(source) == area(test) and s.stem == stem


def parse_numstat(text):
    """Parse log --numstat -z --format=%x1e%H%x00, preserving odd paths/renames."""
    commits = []
    for block in text.split('\x1e')[1:]:
        identifier, _, body = block.partition('\0')
        changes = []
        fields = iter(body.split('\0'))
        for field in fields:
            parts = field.lstrip('\n').split('\t', 2)
            if len(parts) != 3:
                continue
            added, removed, path = parts
            if not path:  # -z rename: empty path, old path, new path
                next(fields, '')
                path = next(fields, '')
            if not path:
                continue
            changes.append({'path': path, 'added': int(added) if added.isdigit() else 0,
                            'removed': int(removed) if removed.isdigit() else 0,
                            'binary': added == '-' or removed == '-'})
        commits.append({'commit': identifier.strip(), 'changes': changes})
    return commits


def churn_summary(commits, inline_tests=()):
    files = defaultdict(lambda: {'added': 0, 'removed': 0, 'changes': 0,
                                 'with_tests': 0, 'without_tests': 0, 'test_files': Counter()})
    inline_tests = set(inline_tests)
    for commit in commits:
        paths = {c['path'] for c in commit['changes']}
        tests = {p for p in paths if is_test(p) or p in inline_tests}
        for path in paths:
            row = files[path]
            row['changes'] += 1
            for change in commit['changes']:
                if change['path'] == path:
                    row['added'] += change['added']
                    row['removed'] += change['removed']
            matches = {t for t in tests if related_test(path, t)}
            row['with_tests' if matches else 'without_tests'] += 1
            row['test_files'].update(matches)
    rows = [dict(path=p, **v, lines=v['added'] + v['removed']) for p, v in files.items()]
    production = [r for r in rows if not is_test(r['path'])]
    return {'commits': len(commits), 'added': sum(r['added'] for r in rows),
            'removed': sum(r['removed'] for r in rows),
            'most_changed': sorted(rows, key=lambda r: (-r['lines'], r['path']))[:10],
            'cochanges': sorted(production, key=lambda r: (-r['changes'], r['path']))[:10],
            'hot_spots': sorted((r for r in production if r['changes'] >= 3 and r['with_tests'] == 0),
                                key=lambda r: (-r['changes'], r['path']))[:10]}


def locale_stats(root, texts):
    counts, english = {}, None
    for lang in ('en', 'ru'):
        try:
            data = json.loads((root / 'locales' / f'{lang}.json').read_text(encoding='utf-8-sig'))
            counts[lang] = len(data)
            if lang == 'en':
                english = data
        except (OSError, ValueError):
            counts[lang] = None
    # Literal matching intentionally includes comments, per the CLI's string heuristic.
    literals = set()
    for path, text in texts.items():
        if path.startswith(('src/', 'src-tauri/', 'crates/')):
            literals.update(value for _, value in re.findall(r'''(["'`])([^"'`\n]+)\1''', text))
    # A key built at run time (`home.where_${key}`, format!("robot.{}")) counts as used by its
    # literal prefix.
    prefixes = tuple({re.split(r'\$\{|\{', value)[0] for value in literals
                      if ('${' in value or '{' in value) and re.match(r'[a-z]+\.', value)})
    counts['unused'] = sorted(key for key in set(english) - literals
                              if not (prefixes and key.startswith(prefixes))) if english is not None else None
    return counts


def first_table(text):
    """Keep the first Markdown, box, or separator-delimited text table verbatim."""
    lines = text.splitlines(keepends=True)
    start = None
    for i, line in enumerate(lines):
        stripped = line.lstrip()
        table = stripped.startswith(('|', '+', '\u250c', '\u2502', '\u251c', '\u2514', '\u2554', '\u2551', '\u2560', '\u255a'))
        if table and start is None:
            start = i
        elif not table and start is not None:
            return ''.join(lines[start:i])
    if start is not None:
        return ''.join(lines[start:])
    for i, line in enumerate(lines):
        if i and re.fullmatch(r'[\s|+:=-]+', line) and '---' in line and lines[i - 1].strip():
            end = i + 1
            while end < len(lines) and lines[end].strip():
                end += 1
            return ''.join(lines[i - 1:end])
    return None


def agents_report(root):
    # Use the same gate as mm.py, including sandbox detection and symlinked .git.
    import mm
    if mm.agent_mode_reason():
        return None
    team = Path.home() / '.claude/skills/draft-agents/scripts/team.py'
    if not team.is_file():
        return None
    report = run([sys.executable, str(team), 'report'], root)
    if not report:
        return None
    # team.py prints a space-aligned table under an "agent  mode ..." header.
    lines = report.splitlines()
    start = next((i for i, line in enumerate(lines) if line.split()[:2] == ['agent', 'mode']), None)
    if start is not None:
        end = next((i for i in range(start, len(lines)) if not lines[i].strip()), len(lines))
        return '\n'.join(lines[start:end]) + '\n'
    return first_table(report)


def collect(root, days):
    texts = {}
    for path in source_files(root):
        text = read(path)
        if text is not None:
            texts[path.relative_to(root).as_posix()] = text
    rust, python = Counter(), Counter({a: 0 for a in ('tools', 'server', 'tts')})
    node = 0
    todos = Counter({a: 0 for a in AREAS})
    unwraps = Counter()
    inline = set()
    for path, text in texts.items():
        todos[area(path)] += len(re.findall(r'\b(?:TODO|FIXME|XXX)\b', text))
        if path.endswith('.rs'):
            name = crate(path)
            rust[name] += count_tests(text, 'Rust')
            if count_tests(text, 'Rust'):
                inline.add(path)
            unwraps[name] += (0 if is_test(path) else
                              len(re.findall(r'\.\s*unwrap\s*\(\s*\)', production_rust(text))))
        elif path.startswith('src/') and path.endswith('.test.ts'):
            node += count_tests(text, 'Node')
        elif path.endswith('.py') and any(path.startswith(a + '/tests/') for a in python):
            python[area(path)] += count_tests(text, 'Python')
    log = run(['git', 'log', f'--since={days}.days', '--numstat', '-z', '--format=%x1e%H%x00'], root)
    return {'created': dt.datetime.now().isoformat(timespec='seconds'), 'since_days': days,
            'code': collect_code(root),
            'tests': {'rust': dict(rust), 'node': node, 'python': dict(python),
                      'total': sum(rust.values()) + node + sum(python.values())},
            'churn': churn_summary(parse_numstat(log), inline) if log is not None else None,
            'hygiene': {'todos': dict(todos), 'unwraps': dict(unwraps),
                        'locales': locale_stats(root, texts)},
            'agents': agents_report(root)}


def totals(snapshot):
    return {'loc': snapshot['code']['loc'] if snapshot['code'] else None,
            'tests': snapshot['tests']['total'],
            'todos': sum(snapshot['hygiene']['todos'].values()),
            'unwraps': sum(snapshot['hygiene']['unwraps'].values())}


def save_snapshot(snapshot, root):
    folder = root / '.cache/mm-stats'
    folder.mkdir(parents=True, exist_ok=True)
    previous = None
    for path in sorted(folder.glob('*.json'), reverse=True):
        try:
            candidate = json.loads(path.read_text(encoding='utf-8'))
            totals(candidate)
            previous = candidate
            break
        except (OSError, ValueError, KeyError, TypeError):
            continue
    current = totals(snapshot)
    old = totals(previous) if previous else {}
    snapshot['deltas'] = {key: value - old[key] if value is not None and old.get(key) is not None else None
                          for key, value in current.items()}
    if previous and (snapshot['code'] or {}).get('method') != (previous['code'] or {}).get('method'):
        snapshot['deltas']['loc'] = None
    path = folder / (dt.datetime.now().strftime('%Y-%m-%dT%H-%M') + '.json')
    # Repeated saves in one minute compare to the file before replacing it.
    path.write_text(json.dumps(snapshot, indent=2, ensure_ascii=False) + '\n', encoding='utf-8')
    return path


def render(snapshot):
    def delta(key):
        value = snapshot.get('deltas', {}).get(key)
        return f' ({value:+d})' if value is not None else ''

    def mapping(values):
        return ', '.join(f'{k}={v if v is not None else "n/a"}' for k, v in values.items()) or 'none'

    def ranked(rows, key):
        return '; '.join(f'{r["path"]}={r[key]}' for r in rows) or 'none'

    code, tests, churn, hygiene = (snapshot[k] for k in ('code', 'tests', 'churn', 'hygiene'))
    lines = [f'MetaMate stats | last {snapshot["since_days"]} days']
    if code is None:
        lines.append('Code: n/a (scc and git unavailable)')
    else:
        lines.extend([f'Code: {code["loc"]} LOC{delta("loc")} | {code["method"]}',
                      '  Languages: ' + mapping(code['languages']), '  Areas: ' + mapping(code['areas']),
                      '  Largest: ' + ranked(code['largest'], 'loc'),
                      '  Complexity: ' + (ranked(code['complex'], 'complexity') if code['complex'] is not None else 'n/a')])
    lines.extend([f'Tests: {tests["total"]}{delta("tests")} | Rust: {mapping(tests["rust"])}',
                  f'  Node: {tests["node"]} | Python: {mapping(tests["python"])}'])
    if churn is None:
        lines.append('Churn: n/a (git unavailable)')
    else:
        lines.extend([f'Churn: {churn["commits"]} commits, +{churn["added"]}/-{churn["removed"]}',
                      '  Most changed (lines): ' + ranked(churn['most_changed'], 'lines'),
                      '  With/without related tests (heuristic): ' + ('; '.join(
                          f'{r["path"]}={r["with_tests"]}/{r["without_tests"]}' for r in churn['cochanges']) or 'none'),
                      '  Hot spots without tests: ' + ranked(churn['hot_spots'], 'changes')])
    locales = hygiene['locales']
    lines.extend([f'Hygiene: TODO/FIXME/XXX={sum(hygiene["todos"].values())}{delta("todos")} | ' + mapping(hygiene['todos']),
                  f'  Production unwraps={sum(hygiene["unwraps"].values())}{delta("unwraps")} | ' + mapping(hygiene['unwraps']),
                  '  Locale keys: ' + mapping({k: locales[k] for k in ('en', 'ru')}),
                  '  Unused locale keys: ' + (', '.join(locales['unused']) or 'none' if locales['unused'] is not None else 'n/a')])
    lines.append('Agents:\n' + snapshot['agents'] if snapshot['agents'] else 'agents: n/a')
    return '\n'.join(lines)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--json', action='store_true', help='print the full snapshot as JSON')
    parser.add_argument('--save', action='store_true', help='save snapshot and compare to the previous one')
    parser.add_argument('--since', type=int, default=7, metavar='DAYS')
    args = parser.parse_args(argv)
    if args.since < 0:
        parser.error('--since must be nonnegative')
    snapshot = collect(ROOT, args.since)
    if args.save:
        try:
            save_snapshot(snapshot, ROOT)
        except OSError as exc:
            print(f'mm stats: cannot save snapshot: {exc}', file=sys.stderr)
            return 1
    print(json.dumps(snapshot, indent=2, ensure_ascii=False) if args.json else render(snapshot))
    return 0
