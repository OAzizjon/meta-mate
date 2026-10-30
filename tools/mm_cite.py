"""mm cite: show the code behind every `path:line` an agent's answer cites, next to the claim.

    mm cite answer.md            references in a file
    mm cite - < answer.md        from standard input
    mm cite '#1126'              the answer of drafting job 1126 (team.py result 1126)
    mm cite -C 3 answer.md       three lines of context instead of one

Agents' answers are untrusted (CLAUDE.md): this puts each claim beside the lines it names, so a
wrong line, a missing file or a made-up function shows at once. It understands `path:12`,
`path:12-20`, `path:12:5`, `path#L12-L20`, `file.rs:12` (a unique file name), Windows
backslashes, `:449, :516` after a path, and "line 12 of path". It flags MISSING files, lines
past the end and ambiguous file names. Standard library only; never opens a secrets file.
"""

import argparse
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).absolute().parent.parent
TEAM_PY = Path.home() / '.claude' / 'skills' / 'draft-agents' / 'scripts' / 'team.py'

EXTS = r'(?:rs|ts|tsx|js|mjs|py|html|css|json|md|toml|ps1|sh|yml|yaml|txt)'
# A path never starts inside a word, a URL ("https://host/a.rs:3") or another path. A
# secrets file is recognised too, so that it can be refused by name.
PATH = (r'(?<![\w/:.\-\\])(?P<path>(?:[A-Za-z]:)?[\w.\-]*(?:[\\/][\w.\-]+)*'
        r'(?:\.' + EXTS + r'|[\\/]?\.env(?:\.[\w.\-]+)?)(?![\w\-]))`?')
# path:12, path:12-20, path:12–20, path:12:5, path#L12, path#L12-L20, path (line 12), path line 12
REFERENCE = re.compile(
    PATH + r'(?:'
    r'#L(?P<a1>\d+)(?:-L?(?P<b1>\d+))?'
    r'|:(?P<a2>\d+)(?:\s*[-–]\s*(?P<b2>\d+))?(?::\d+)?'
    r'|\s*\(?\s*lines?\s+(?P<a3>\d+)(?:\s*[-–]\s*(?P<b3>\d+))?\)?'
    r')(?!\w)(?!\.\d)')  # a full stop may end the sentence; "1.3" is no line
# "line 12 of path", "lines 12-14 in path"
REVERSED = re.compile(r'\blines?\s+(?P<a>\d+)(?:\s*[-–]\s*(?P<b>\d+))?\s+(?:of|in)\s+`?' + PATH)
# ", :516" or " and :516" right after a reference: the same file
MORE = re.compile(r'^(?:`?\s*(?:,|and|и)\s*`?:(?P<a>\d+)(?:\s*[-–]\s*(?P<b>\d+))?`?)')


def is_secret(path):
    """`.env`, `.env.local`, ... in any case: Windows opens `.ENV` as `.env`."""
    name = Path(str(path).replace('\\', '/')).name.lower().rstrip('. ')
    return name == '.env' or name.startswith('.env.')


def references(text):
    """Every (path, start, end, claim) in the text, in order, without repeats."""
    found = []
    for line in text.splitlines():
        claim = line.strip()
        spans = []
        for match in REFERENCE.finditer(line):
            groups = match.groupdict()
            start = groups['a1'] or groups['a2'] or groups['a3']
            if not start:
                continue
            end = groups['b1'] or groups['b2'] or groups['b3'] or start
            path = match.group('path')
            spans.append((match.start(), (path, int(start), int(end))))
            rest = line[match.end():]
            while (more := MORE.match(rest)):
                spans.append((match.end(), (path, int(more.group('a')), int(more.group('b') or more.group('a')))))
                rest = rest[more.end():]
        for match in REVERSED.finditer(line):
            spans.append((match.start(), (match.group('path'), int(match.group('a')),
                                          int(match.group('b') or match.group('a')))))
        for _, (path, start, end) in sorted(spans, key=lambda s: s[0]):
            if path.lower().startswith(('http', 'www.')):
                continue
            if end < start:
                start, end = end, start
            key = (path, start, end)
            if key not in [(p, s, e) for p, s, e, _ in found]:
                found.append((path, start, end, claim))
    return found


def project_files(root):
    try:
        out = subprocess.run(['git', 'ls-files', '--cached', '--others', '--exclude-standard'],
                             cwd=root, capture_output=True, text=True, encoding='utf-8', timeout=30)
        if out.returncode == 0:
            return out.stdout.splitlines()
    except (OSError, subprocess.TimeoutExpired):
        pass
    skip = {'.git', 'target', 'node_modules', 'dist', '__pycache__'}
    return [p.relative_to(root).as_posix() for p in Path(root).rglob('*')
            if p.is_file() and not skip & set(p.relative_to(root).parts)]


def resolve(root, path, files):
    """(relative path, note): as written, else by a unique suffix or file name."""
    clean = path.replace('\\', '/')
    clean = re.sub(r'^(?:\./|[ab]/|/)+', '', clean)
    base = Path(root).resolve()
    target = Path(clean) if Path(clean).is_absolute() else base / clean
    try:
        inside = target.resolve().relative_to(base).as_posix()
    except (ValueError, OSError):
        inside = None
    if inside is not None and Path(base, inside).is_file():
        return inside, ''
    if '..' in clean.split('/'):
        return None, 'outside the project'
    # A tail of the path: "Swift/Desktop/meta-mate/src/a.rs" (a path with a space, cut there)
    # or "a.rs" (a bare name) both end in a project file.
    parts = [part for part in clean.split('/') if part not in ('', '.')]
    for first in range(len(parts)):
        tail = '/'.join(parts[first:])
        found = [f for f in files if f == tail or f.endswith('/' + tail)]
        if len(found) == 1:
            return found[0], f'(as {clean})'
        if len(found) > 1:
            return None, 'AMBIGUOUS: ' + ', '.join(found[:5])
    return None, 'MISSING'


def show(root, text, context=1):
    """The report: each claim, then the cited lines with numbers. Returns (report, problems)."""
    files = project_files(root)
    out, problems = [], 0
    refs = references(text)
    if not refs:
        return 'no path:line references found', 0
    last_claim = None
    for path, start, end, claim in refs:
        if claim != last_claim:
            out.append('')
            out.append('» ' + (claim if len(claim) <= 200 else claim[:199] + '…'))
            last_claim = claim
        rel, note = resolve(root, path, files)
        if rel is None:
            problems += 1
            out.append(f'  {path}:{start}  !! {note}')
            continue
        if is_secret(rel):
            out.append(f'  {rel}  (a secrets file: not opened)')
            continue
        try:
            lines = Path(root, rel).read_text(encoding='utf-8', errors='replace').splitlines()
        except OSError as error:
            problems += 1
            out.append(f'  {rel}  !! unreadable: {error}')
            continue
        span = f'{start}' if start == end else f'{start}-{end}'
        if not 1 <= start <= end <= len(lines):
            problems += 1
            out.append(f'  {rel}:{span}  !! not in the file ({len(lines)} lines) {note}'.rstrip())
            if not 1 <= start <= len(lines):
                continue
        out.append(f'  {rel}:{span} {note}'.rstrip())
        last = min(len(lines), end + context, start + 40 + context)
        for number in range(max(1, start - context), last + 1):
            mark = '>' if start <= number <= end else ' '
            out.append(f'   {mark}{number:5} {lines[number - 1]}')
    out.append('')
    out.append(f'{len(refs)} reference(s), {problems} problem(s)')
    return '\n'.join(out).lstrip('\n'), problems


def read_source(source):
    if source == '-':
        return sys.stdin.read()
    if re.fullmatch(r'#?\d+', source):
        result = subprocess.run([sys.executable, str(TEAM_PY), 'result', source.lstrip('#')],
                                capture_output=True, text=True, encoding='utf-8', errors='replace')
        # The sandbox clean-up noise is not part of the answer.
        return '\n'.join(line for line in result.stdout.splitlines()
                         if not line.startswith(('rm: ', '   target/', 'warning: in the working copy')))
    return Path(source).read_text(encoding='utf-8', errors='replace')


def main(argv=None, root=ROOT):
    parser = argparse.ArgumentParser(prog='mm cite', description=__doc__.split('\n')[0])
    parser.add_argument('source', help="a file, '-' for standard input, or a job number like '#1126'")
    parser.add_argument('-C', '--context', type=int, default=1, help='lines around each reference')
    args = parser.parse_args(argv)
    if is_secret(args.source):
        print('mm cite: a secrets file is never opened')
        return 2
    report, problems = show(root, read_source(args.source), args.context)
    print(report)
    return 1 if problems else 0


if __name__ == '__main__':
    if hasattr(sys.stdout, 'reconfigure'):
        sys.stdout.reconfigure(encoding='utf-8', errors='replace')
    sys.exit(main())
