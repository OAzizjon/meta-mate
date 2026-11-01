"""mm legacy [--only SECTIONS] [--limit N] [--json] [--strict]

One report of legacy / dead-code candidates so the lead and the drafting
agents can find what refactors left behind. Every section lists sorted
'path:line  reason' entries:

  rust     #[allow(dead_code)] / #[allow(unused*)] attributes, plus pub fn and
           pub struct under crates/ and src-tauri/src whose name appears
           nowhere else in the tree (word match outside its own definition
           line; target/ and node_modules/ are never scanned)
  ts       exported names in src/**/*.ts (not *.test.ts) never imported or
           referenced in any other .ts/.html file; .ts files under src/
           imported by nothing and not referenced by any *.html at the root
  py       top-level def/class in tools/ and server/ never referenced
           elsewhere; test_* files and names, cmd_* names and argparse
           set_defaults targets are skipped
  locales  keys present in locales/en.json but unused in code, reusing
           mm_stats' unused-locale-key heuristic (imported, not copied)
  markers  comments containing legacy, deprecated, obsolete, 'old ',
           'no longer', 'TODO remove' or 'unused'
  docs     backticked repo paths in docs/*.md and README.md that do not
           exist (tokens with a slash and an extension only)
  external cargo machete, vulture and ruff when on PATH, else one
           'skipped: X not installed' line per missing tool

Standard library only; .env* files are never opened. The exit code is 0
(this is a report, not a gate); --strict exits 1 when any section has
findings. Silence known false positives (Tauri commands, serde-only
structs, web entry points, test helpers) in tools/legacy_allow.txt: one
'path:name' entry or path glob per line, '#' starts a comment.
"""

import argparse
import ast
from fnmatch import fnmatchcase
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]

SECTION_NAMES = ('rust', 'ts', 'py', 'locales', 'markers', 'docs', 'external')
DEFAULT_LIMIT = 40

SKIP_DIRS = {'.git', '.cache', '.draft-input', 'node_modules', 'target', 'dist',
             '__pycache__', '.venv', 'venv', 'gen', 'coverage'}
TEXT_SUFFIXES = {'.rs', '.toml', '.ts', '.tsx', '.js', '.mjs', '.cjs', '.json',
                 '.html', '.css', '.scss', '.py', '.md', '.ps1', '.sh', '.c',
                 '.h', '.cpp', '.hpp', '.go', '.sql', '.svg', '.txt', '.yml',
                 '.yaml', '.cfg', '.ini'}
MAX_TEXT_BYTES = 2_000_000
CODE_SUFFIXES = {'.rs', '.ts', '.tsx', '.js', '.mjs', '.cjs', '.py', '.c',
                 '.h', '.cpp', '.hpp', '.go', '.java', '.kt', '.cs'}
COMMENT_PREFIXES = ('//', '/*', '*', '#')
MARKERS = ('todo remove', 'no longer', 'deprecated', 'obsolete', 'legacy',
           'unused', 'old ')

ALLOW_GROUP = re.compile(r'#\s*!?\s*\[\s*allow\s*\(([^)]*)\)')
PUB_ITEM = re.compile(r'^\s*pub\s+(?:(?:async|unsafe)\s+)?fn\s+([A-Za-z_]\w*)'
                      r'|^\s*pub\s+struct\s+([A-Za-z_]\w*)')
EXPORT_DECL = re.compile(r'^\s*export\s+(?:async\s+)?(?:function|class|interface|'
                         r'type|enum|const|let|var)\s+([A-Za-z_$][\w$]*)')
IMPORT_SPEC = re.compile(r"""from\s*['"](\.[^'"]+)['"]"""
                         r"""|import\s*['"](\.[^'"]+)['"]"""
                         r"""|import\s*\(\s*['"](\.[^'"]+)['"]\s*\)""")
MODULE_SUFFIXES = ('.d.ts', '.ts', '.tsx', '.js', '.jsx', '.mjs', '.cjs')
BACKTICK = re.compile(r'`([^`\n]+)`')
PATHISH = re.compile(r'^(?:\./)?[\w./-]+?\.[A-Za-z0-9]{1,10}$')
COLON_LINE = re.compile(r'^(?P<path>.+?):(?P<line>\d+)(?::\d+)?:\s*(?P<msg>.*)$')


def _entry(path, line, name, reason):
    return {'path': path, 'line': int(line), 'name': name, 'reason': reason}


def is_env_file(name):
    lower = name.lower()
    return lower == '.env' or lower.startswith('.env.')


def _iter_text_files(root):
    """Yield (relpath, text) for every text-ish file; build dirs pruned, .env* never opened."""
    for folder, dirs, files in os.walk(root):
        dirs[:] = sorted(d for d in dirs
                         if d not in SKIP_DIRS and not (Path(folder) / d).is_symlink())
        for name in sorted(files):
            if is_env_file(name):
                continue
            path = Path(folder) / name
            if path.suffix.lower() not in TEXT_SUFFIXES or path.is_symlink():
                continue
            try:
                if path.stat().st_size > MAX_TEXT_BYTES:
                    continue
                text = path.read_text(encoding='utf-8-sig', errors='replace')
            except OSError:
                continue
            yield path.relative_to(root).as_posix(), text


def _iter_under(root, prefixes, suffixes):
    """Files under root/prefix for each prefix (e.g. 'src-tauri/src'), same pruning."""
    for prefix in prefixes:
        base = root / prefix
        if not base.is_dir():
            continue
        for folder, dirs, files in os.walk(base):
            dirs[:] = sorted(d for d in dirs
                             if d not in SKIP_DIRS and not (Path(folder) / d).is_symlink())
            for name in sorted(files):
                if is_env_file(name):
                    continue
                path = Path(folder) / name
                if path.suffix.lower() not in suffixes or path.is_symlink():
                    continue
                try:
                    text = path.read_text(encoding='utf-8-sig', errors='replace')
                except OSError:
                    continue
                yield path.relative_to(root).as_posix(), text


def _word_re(name):
    return re.compile(r'(?<![\w$])' + re.escape(name) + r'(?![\w$])')


def _used_elsewhere(name, files, def_path, def_line):
    """Word match in every file; inside the defining file the definition line itself is ignored."""
    pattern = _word_re(name)
    for path, text in files.items():
        if path == def_path:
            for lineno, line in enumerate(text.splitlines(), 1):
                if lineno != def_line and pattern.search(line):
                    return True
        elif pattern.search(text):
            return True
    return False


def _allowed_names(line):
    for group in ALLOW_GROUP.findall(line):
        for name in group.split(','):
            name = name.strip()
            if name == 'dead_code' or name.startswith('unused'):
                yield name


def rust_findings(root):
    entries, defs = [], []
    for path, text in _iter_under(root, ('crates', 'src-tauri/src'), {'.rs'}):
        for lineno, line in enumerate(text.splitlines(), 1):
            for name in _allowed_names(line):
                entries.append(_entry(path, lineno, name, f'allow({name}) attribute'))
            match = PUB_ITEM.match(line)
            if match:
                name = match.group(1) or match.group(2)
                kind = 'fn' if match.group(1) else 'struct'
                defs.append((path, lineno, name, kind))
    corpus = {path: text for path, text in _iter_text_files(root)}
    for path, lineno, name, kind in defs:
        if _used_elsewhere(name, corpus, path, lineno):
            continue
        entries.append(_entry(path, lineno, name, f'pub {kind} referenced nowhere else'))
    return entries


def _strip_module_suffix(path):
    for suffix in MODULE_SUFFIXES:
        if path.endswith(suffix):
            return path[:-len(suffix)]
    return path


def _module_keys(base, spec):
    joined = os.path.normpath(os.path.join(base, spec)).replace(os.sep, '/')
    key = _strip_module_suffix(joined)
    keys = {key}
    if key.endswith('/index'):
        keys.add(key[:-len('/index')])
    return keys


def _import_specs(text):
    specs = set()
    for match in IMPORT_SPEC.finditer(text):
        specs.update(group for group in match.groups() if group)
    return specs


def _ts_name_used(name, ts_files, html_texts, def_path):
    pattern = _word_re(name)
    for path, text in ts_files.items():
        if path != def_path and pattern.search(text):
            return True
    return any(pattern.search(text) for text in html_texts)


def ts_findings(root):
    entries = []
    ts_files, html_texts = {}, []
    for path, text in _iter_text_files(root):
        if path.endswith('.ts'):
            ts_files[path] = text
        elif path.endswith('.html'):
            html_texts.append(text)
    for path, text in sorted(ts_files.items()):
        if not path.startswith('src/') or path.endswith('.test.ts'):
            continue
        for lineno, line in enumerate(text.splitlines(), 1):
            match = EXPORT_DECL.match(line)
            if not match:
                continue
            name = match.group(1)
            if _ts_name_used(name, ts_files, html_texts, path):
                continue
            own = len(re.findall(r'(?<![\w$])' + re.escape(name) + r'(?![\w$])', text))
            reason = ('export used only inside its own file (drop "export"?)' if own > 1
                      else 'exported name never used anywhere: dead?')
            entries.append(_entry(path, lineno, name, reason))
    imported = set()
    for path, text in ts_files.items():
        base = os.path.dirname(path)
        for spec in _import_specs(text):
            imported.update(_module_keys(base, spec))
    root_html = []
    for html in sorted(root.glob('*.html')):
        if html.is_file() and not html.is_symlink():
            try:
                root_html.append(html.read_text(encoding='utf-8-sig', errors='replace'))
            except OSError:
                pass
    for path in sorted(p for p in ts_files
                       if p.startswith('src/') and not p.endswith('.test.ts')):
        if _strip_module_suffix(path) in imported:
            continue
        name = Path(path).name
        if any(path in html or name in html for html in root_html):
            continue
        entries.append(_entry(path, 1, name,
                              'module imported by nothing and not referenced by root *.html'))
    return entries


def _is_test_file(path):
    parts = path.split('/')
    name = parts[-1]
    return 'tests' in parts or name.startswith('test_') or name.endswith('_test.py')


def _registered_via_set_defaults(name, py_files):
    pattern = re.compile(r'set_defaults\s*\([^)]*\b' + re.escape(name) + r'\b')
    return any(pattern.search(text) for text in py_files.values())


def py_findings(root):
    entries = []
    py_files = {}
    for path, text in _iter_text_files(root):
        if path.endswith('.py'):
            if path.startswith(('tools/', 'server/')):
                py_files[path] = text
    search_files = {path: text for path, text in _iter_text_files(root)
                    if path.endswith('.py')}
    for path, text in sorted(py_files.items()):
        if _is_test_file(path):
            continue
        try:
            tree = ast.parse(text)
        except SyntaxError:
            continue
        for node in tree.body:
            if not isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef, ast.ClassDef)):
                continue
            name = node.name
            if name.startswith(('cmd_', 'test_')) or _registered_via_set_defaults(name, py_files):
                continue
            if _used_elsewhere(name, search_files, path, node.lineno):
                continue
            kind = 'class' if isinstance(node, ast.ClassDef) else 'def'
            entries.append(_entry(path, node.lineno, name,
                                  f'top-level {kind} never referenced elsewhere'))
    return entries


def locale_findings(root):
    try:
        import mm_stats
    except ImportError:
        sys.path.insert(0, str(Path(__file__).resolve().parent))
        import mm_stats
    texts = {}
    for path in mm_stats.source_files(root):
        text = mm_stats.read(path)
        if text is not None:
            texts[path.relative_to(root).as_posix()] = text
    unused = mm_stats.locale_stats(root, texts).get('unused') or []
    try:
        en_lines = (root / 'locales' / 'en.json').read_text(encoding='utf-8-sig').splitlines()
    except (OSError, UnicodeError):
        en_lines = []
    entries = []
    for item in unused:
        key, _, suffix = item.partition(' (')
        suffix = f' ({suffix}' if suffix else ''
        line = next((i for i, text in enumerate(en_lines, 1) if f'"{key}"' in text), 1)
        entries.append(_entry('locales/en.json', line, key, 'unused locale key' + suffix))
    return entries


def marker_findings(root):
    entries = []
    for path, text in _iter_text_files(root):
        if Path(path).suffix not in CODE_SUFFIXES:
            continue
        for lineno, line in enumerate(text.splitlines(), 1):
            if not line.strip().startswith(COMMENT_PREFIXES):
                continue
            low = line.lower()
            hit = next((m for m in MARKERS
                        if re.search(r'\b' + re.escape(m.strip()) + r'\b', low)), None)
            if hit:
                entries.append(_entry(path, lineno, hit.strip(),
                                      f'comment marker: {hit.strip()}'))
    return entries


def docs_findings(root):
    entries = []
    md_files = []
    docs = root / 'docs'
    if docs.is_dir():
        md_files.extend(sorted(docs.glob('*.md')))
    readme = root / 'README.md'
    if readme.is_file():
        md_files.append(readme)
    for md in md_files:
        rel = md.relative_to(root).as_posix()
        try:
            lines = md.read_text(encoding='utf-8-sig').splitlines()
        except (OSError, UnicodeError):
            continue
        for lineno, line in enumerate(lines, 1):
            for token in BACKTICK.findall(line):
                pathish = token.split('#', 1)[0].split('?', 1)[0].strip()
                if pathish.startswith('~/') or '/' not in pathish or not PATHISH.match(pathish):
                    continue
                if not (root / pathish).exists():
                    entries.append(_entry(rel, lineno, token, f'dead docs path: {token}'))
    return entries


def _run(cmd, root, timeout=120):
    try:
        proc = subprocess.run(cmd, cwd=str(root), capture_output=True, text=True,
                              encoding='utf-8', errors='replace', timeout=timeout,
                              stdin=subprocess.DEVNULL)
    except (OSError, subprocess.TimeoutExpired):
        return None, ''
    return proc.returncode, (proc.stdout + proc.stderr).strip()


def _rel(path, root):
    path = path.replace('\\', '/')
    root_text = str(root).replace('\\', '/')
    if path.startswith(root_text + '/'):
        return path[len(root_text) + 1:]
    return path


def _parse_vulture_ruff(out, root):
    entries = []
    for raw in (out or '').splitlines():
        match = COLON_LINE.match(raw.strip())
        if not match or not match.group('msg'):
            continue
        msg = match.group('msg').strip()
        quoted = re.search(r"'([^']+)'|`([^`]+)`", msg)
        name = (quoted.group(1) or quoted.group(2)) if quoted else ''
        entries.append(_entry(_rel(match.group('path'), root), int(match.group('line')),
                              name, msg))
    return entries


def _parse_machete(out, root):
    entries = []
    for raw in (out or '').splitlines():
        line = raw.strip()
        if '.toml' not in line:
            continue
        original = line
        line = line.split(' -- ', 1)[-1]
        match = re.search(r'(.+?\.toml)', line.replace('\\', '/'))
        if not match:
            continue
        path = _rel(match.group(1), root)
        dep = original.split('--')[0].strip().strip(':')
        lineno = 1
        try:
            for i, text in enumerate((root / path).read_text(encoding='utf-8-sig').splitlines(), 1):
                if re.match(r'\s*' + re.escape(dep) + r'\s*=', text):
                    lineno = i
                    break
        except (OSError, UnicodeError):
            pass
        entries.append(_entry(path, lineno, dep, f'unused dependency: {dep}'))
    return entries


EXTERNAL_TOOLS = (
    ('cargo machete', ['cargo', 'machete'], 'cargo-machete', _parse_machete),
    ('vulture', ['vulture', 'tools', 'server', '--min-confidence', '80'], 'vulture',
     _parse_vulture_ruff),
    ('ruff', ['ruff', 'check', '--select', 'F401,F811,F841', '--output-format', 'concise',
              'tools', 'server'], 'ruff', _parse_vulture_ruff),
)


def external_findings(root, which=None, runner=None):
    if which is None:
        which = shutil.which
    if runner is None:
        runner = _run
    entries = []
    for label, cmd, helper, parse in EXTERNAL_TOOLS:
        if not which(helper):
            entries.append(_entry('', 0, label, f'skipped: {label} not installed'))
            continue
        code, out = runner(cmd, root)
        if code is None:
            entries.append(_entry('', 0, label, f'skipped: {label} could not run'))
            continue
        entries.extend(parse(out, root))
    return entries


def load_allowlist(path):
    try:
        lines = Path(path).read_text(encoding='utf-8-sig').splitlines()
    except OSError:
        return []
    entries = []
    for line in lines:
        line = line.split('#', 1)[0].strip()
        if line:
            entries.append(line)
    return entries


def is_allowlisted(allow, path, name):
    combined = f'{path}:{name}'
    for pattern in allow:
        if ':' in pattern:
            head, _, tail = pattern.rpartition(':')
            if fnmatchcase(path, head) and fnmatchcase(name, tail):
                return True
        elif fnmatchcase(path, pattern) or fnmatchcase(combined, pattern):
            return True
    return False


def collect(root, only=None, limit=DEFAULT_LIMIT, allowlist_path=None):
    root = Path(root)
    if allowlist_path is None:
        allowlist_path = root / 'tools' / 'legacy_allow.txt'
    allow = load_allowlist(allowlist_path)
    builders = {'rust': rust_findings, 'ts': ts_findings, 'py': py_findings,
                'locales': locale_findings, 'markers': marker_findings,
                'docs': docs_findings, 'external': external_findings}
    report = {}
    for name in SECTION_NAMES:
        if only and name not in only:
            continue
        try:
            found = builders[name](root)
        except Exception as exc:  # a report: one broken section must not sink the rest
            report[name] = {'entries': [], 'count': 0, 'truncated': 0,
                            'error': f'{type(exc).__name__}: {exc}'}
            continue
        found = [e for e in found if not is_allowlisted(allow, e['path'], e['name'])]
        skipped = [e for e in found if not e['path']]
        real = sorted((e for e in found if e['path']),
                      key=lambda e: (e['path'], e['line'], e['reason'], e['name']))
        report[name] = {'entries': real[:limit] + skipped, 'count': len(real),
                        'truncated': max(0, len(real) - limit)}
    return report


def render(report, root, limit):
    total = sum(section.get('count', 0) for section in report.values())
    lines = [f'mm legacy: legacy / dead-code candidates under {root}']
    for name in SECTION_NAMES:
        if name not in report:
            continue
        section = report[name]
        if 'error' in section:
            lines.append(f'== {name}: {section["error"]}')
            continue
        lines.append(f'== {name} ({section["count"]})')
        for entry in section['entries']:
            if entry['path']:
                lines.append(f"{entry['path']}:{entry['line']}  {entry['reason']}")
            else:
                lines.append(entry['reason'])
        if section['truncated']:
            lines.append(f'... and {section["truncated"]} more (raise --limit above {limit})')
    lines.append(f'mm legacy: {total} finding(s) after the allowlist;'
                 ' silence false positives in tools/legacy_allow.txt')
    return '\n'.join(lines)


def main(argv=None, root=None):
    parser = argparse.ArgumentParser(prog='mm legacy', description=__doc__)
    parser.add_argument('--json', action='store_true', help='print the report as JSON')
    parser.add_argument('--only', help='comma-separated sections: ' + ','.join(SECTION_NAMES))
    parser.add_argument('--limit', type=int, default=DEFAULT_LIMIT,
                        help=f'max findings per section (default {DEFAULT_LIMIT})')
    parser.add_argument('--strict', action='store_true',
                        help='exit 1 when any section has findings (default: always 0)')
    args = parser.parse_args(argv)
    if args.limit < 1:
        parser.error('--limit must be >= 1')
    root = Path(root).resolve() if root is not None else ROOT
    only = None
    if args.only:
        only = {part.strip() for part in args.only.split(',') if part.strip()}
        unknown = only - set(SECTION_NAMES)
        if unknown:
            parser.error('unknown section(s): ' + ', '.join(sorted(unknown))
                         + f' (choose from {",".join(SECTION_NAMES)})')
    report = collect(root, only, args.limit)
    if args.json:
        print(json.dumps({'root': str(root), 'limit': args.limit, 'sections': report},
                         indent=2, ensure_ascii=False))
    else:
        print(render(report, root, args.limit))
    if args.strict and any(section.get('count') for section in report.values()):
        return 1
    return 0
