"""mm difflint [PATCH|-]

Lint added unified-diff lines for patch leftovers (standard library only).

Rule names carry their severity. Header and summary diagnostics use line 0;
ordinary findings use the new-file line number. Rust test context deliberately
extends from #[cfg(test)] to EOF, matching the lightweight patch-review rule.
"""

import argparse
import ast
from collections import Counter
from dataclasses import dataclass, field
from pathlib import Path, PurePosixPath
import re
import subprocess
import sys

from mm_checks import ASSIGNMENT, SECRET_PATTERN


ROOT = Path(__file__).absolute().parent.parent
HUNK = re.compile(r'^@@ -\d+(?:,(\d+))? \+(\d+)(?:,(\d+))? @@')
CFG_TEST = re.compile(r'^\s*#\s*\[\s*cfg\s*\(\s*test\s*\)\s*\]')
TOKEN = r'"(?:\\.|[^"\\])*"|\S+'


@dataclass
class AddedLine:
    number: int
    text: str
    test: bool = False


@dataclass
class FileDiff:
    path: str
    headers: list[str] = field(default_factory=list)
    added: list[AddedLine] = field(default_factory=list)


@dataclass
class Finding:
    path: str
    line: int
    rule: str
    text: str

    def __str__(self):
        return f'{self.path}:{self.line}  {self.rule}  {self.text}'


def _path(value, prefix=False):
    value = value.split('\t', 1)[0].strip()
    if value.startswith('"'):
        # Git quotes paths as C strings, with UTF-8 bytes in octal escapes.
        value = ast.literal_eval(value)
        try:
            value = value.encode('latin-1').decode('utf-8')
        except (UnicodeError, ValueError):
            pass
    if prefix and value.startswith(('a/', 'b/')):
        value = value[2:]
    return value


SELF = {'tools/tests/test_mm_difflint.py'}


def is_secret(path):
    parts = PurePosixPath(path.replace('\\', '/')).parts
    return any(p.lower().rstrip('. ') == '.env' or
               p.lower().rstrip('. ').startswith('.env.') for p in parts)


def parse_diff(text):
    """Read Git or plain unified diffs, including renames and CRLF input."""
    files = []
    current = None
    remaining = 0
    old_remaining = 0
    number = 0
    test = False
    old_header = False
    for line in text.splitlines():
        if line.startswith('diff --git '):
            raw = line[len('diff --git '):]
            names = re.findall(TOKEN, raw)
            if len(names) != 2 and raw.startswith('a/') and ' b/' in raw:
                before, after = raw.rsplit(' b/', 1)
                names = [before, 'b/' + after]
            # +++/rename headers below supply the authoritative path with spaces.
            headers = [_path(n, prefix=True) for n in names] if len(names) == 2 else []
            current = FileDiff(headers[-1] if headers else '', headers)
            files.append(current)
            remaining, old_remaining, test, old_header = 0, 0, False, False
            continue
        match = HUNK.match(line)
        if match and current is not None:
            old_remaining = int(match[1]) if match[1] is not None else 1
            number = int(match[2])
            remaining = int(match[3]) if match[3] is not None else 1
            continue
        # Within a hunk, +++ is source text, not a file header.
        if (remaining or old_remaining) and current is not None:
            if line.startswith((' ', '+')):
                content = line[1:]
                test = test or bool(CFG_TEST.search(content))
                if line.startswith('+'):
                    current.added.append(AddedLine(number, content, test))
                number += 1
                remaining -= 1
            if line.startswith((' ', '-')):
                old_remaining -= 1
            continue
        if line.startswith('--- '):
            name = _path(line[4:], prefix=True)
            if current is None or old_header:
                current = FileDiff(name)
                files.append(current)
                test = False
            current.headers.append(name)
            old_header = True
        elif line.startswith('+++ ') and current is not None:
            name = _path(line[4:], prefix=True)
            current.headers.append(name)
            if name != '/dev/null':
                current.path = name
            old_header = True
        elif line.startswith(('rename from ', 'rename to ')) and current is not None:
            name = _path(line.split(' ', 2)[2])
            current.headers.append(name)
            if line.startswith('rename to '):
                current.path = name
    return [f for f in files if f.path and f.path != '/dev/null']


def _read_file(root, name):
    """Diff paths are untrusted; never follow them outside the repository."""
    if is_secret(name):
        return None
    path = (root / name).resolve()
    if not path.is_relative_to(root.resolve()) or is_secret(path.as_posix()):
        return None
    try:
        data = path.read_bytes()
    except (OSError, ValueError):
        return None
    if b'\0' in data:
        return None
    return data.decode('utf-8', errors='replace')


def _is_test(path):
    p = PurePosixPath(path)
    return ('tests' in p.parts or '__tests__' in p.parts or
            bool(re.search(r'\.(?:test|spec)\.[^.]+$', p.name)))


def _secret_value(text):
    return bool(SECRET_PATTERN.search(text) or ASSIGNMENT.search(text))


def _code_part(text):
    """A Rust or TS line without its comment: a doc or block-comment line is empty, and a
    trailing `//` comment goes (one outside quotes: `"https://..."` stays)."""
    if text.lstrip().startswith(('//', '/*', '*')):
        return ''
    quote = None
    for index, char in enumerate(text):
        if quote:
            if char == quote and text[index - 1] != '\\':
                quote = None
        elif char in '"\'`':
            quote = char
        elif text.startswith('//', index):
            return text[:index]
    return text


def _dom_literal(text):
    # Match literals at the DOM sink, so a lookup elsewhere on the line cannot
    # hide an inline label. Empty strings clear the UI and need no locale key.
    literal = r'''(?:"[^"\n]+"|'[^'\n]+'|`[^`\n]+`)'''
    assignment = r'\b(?:textContent|innerText|title|placeholder)\s*=\s*'
    attribute = r'''["'](?:aria-label|placeholder)["']\s*[:,]\s*'''
    return bool(re.search(r'(?:' + assignment + '|' + attribute + ')' + literal, text))


def lint(files, root):
    """Return findings, then per-file and per-area added-line summaries."""
    findings = []
    areas = Counter()
    summaries = []
    for file in files:
        path = file.path
        for header in dict.fromkeys(file.headers):
            if is_secret(header):
                findings.append(Finding(header, 0, 'ERROR_SECRETS_FILE', '[secrets file]'))
        if path in SELF:
            # This tool's own tests hold every leftover on purpose, as fixtures.
            continue
        disk = _read_file(root, path)
        disk_lines = disk.splitlines() if disk is not None else []
        test_start = next((i for i, line in enumerate(disk_lines, 1)
                           if CFG_TEST.search(line)), None)
        rust = path.endswith('.rs')
        ts = path.endswith(('.ts', '.tsx'))
        for added in file.added:
            text = added.text
            test = (_is_test(path) or added.test or
                    (test_start is not None and added.number >= test_start))
            secret = _secret_value(text)
            display = '[redacted secret]' if secret or is_secret(path) else text.strip()

            def emit(rule):
                findings.append(Finding(path, added.number, rule, display))

            # The code rules look at code only: "// never println!() here" is advice, not a leftover.
            code = _code_part(text) if rust or ts else text

            if re.match(r'^\s*(?:<{7}|={7}|>{7}|\|{7})(?:\s|$)', text):
                emit('ERROR_CONFLICT')
            if path.startswith('crates/mm-core/') and re.search(r'\b(?:dbg|e?println)\s*!\s*\(', code):
                emit('ERROR_CORE_DEBUG')
            if path.startswith('src/') and not test and re.search(r'\bconsole\s*\.\s*log\s*\(', code):
                emit('ERROR_CONSOLE_LOG')
            if test and re.search(r'\.\s*only\s*\(|\btest\s*\.\s*only\b', text):
                emit('ERROR_TEST_ONLY')
            if re.search(r'#\s*\[\s*ignore\s*(?:\]|=)', text):
                emit('ERROR_IGNORED_TEST')
            if secret:
                emit('ERROR_SECRET')
            if rust and path.startswith(('src-tauri/', 'crates/')) and not test:
                if re.search(r'\.\s*(?:unwrap\s*\(\s*\)|expect\s*\()', code):
                    emit('WARN_RUST_PANIC')
            if re.search(r'\b(?:todo|unimplemented)\s*!\s*\(', code):
                emit('WARN_UNIMPLEMENTED')
            if re.search(r'\b(?:TODO|FIXME|XXX)\b', text):
                emit('WARN_TODO')
            if ts and re.search(r'\bas\s+any\b|//\s*@ts-ignore\b', text):
                emit('WARN_TS_ESCAPE')
            if ts and _dom_literal(code):
                emit('WARN_UI_LITERAL')
            if re.search(r'\bunsafe\s*\{', code) and not path.startswith('crates/mm-win32/'):
                emit('WARN_UNSAFE')
        size = len(disk_lines) if disk is not None else max((a.number for a in file.added), default=0)
        if size > 800:
            qualifier = '' if disk is not None else 'at least '
            findings.append(Finding(path, 0, 'WARN_FILE_SIZE', f'{qualifier}{size} lines after change'))
        summaries.append(Finding(path, 0, 'INFO_ADDED', f'{len(file.added)} added lines'))
        parts = PurePosixPath(path).parts
        area = '/'.join(parts[:2]) if parts[0] == 'crates' else parts[0] if len(parts) > 1 else '.'
        areas[area] += len(file.added)
    return findings + summaries + [Finding(area, 0, 'INFO_AREA', f'{count} added lines')
                                    for area, count in sorted(areas.items())]


def _git(root, args):
    result = subprocess.run(['git', *args], cwd=root, capture_output=True, timeout=60)
    if result.returncode:
        raise ValueError('git command failed')
    return result.stdout.decode('utf-8', errors='replace')


def working_diff(root):
    files = parse_diff(_git(root, ['diff', '--no-ext-diff', '--no-textconv', '--no-color',
                                   '--src-prefix=a/', '--dst-prefix=b/', 'HEAD', '--']))
    for path in _git(root, ['ls-files', '--others', '--exclude-standard', '-z']).split('\0'):
        if not path:
            continue
        file = FileDiff(path, [path])
        # Secrets files are reported by name without opening them.
        text = _read_file(root, path)
        if text is not None:
            test = False
            for number, line in enumerate(text.splitlines(), 1):
                test = test or bool(CFG_TEST.search(line))
                file.added.append(AddedLine(number, line, test))
        files.append(file)
    return files


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('patch', nargs='?', help='unified diff file, or - for stdin; default: git diff HEAD + untracked')
    args = parser.parse_args(argv)
    try:
        if args.patch == '-':
            files = parse_diff(sys.stdin.read())
        elif args.patch:
            patch_path = Path(args.patch)
            if is_secret(args.patch) or is_secret(patch_path.resolve().as_posix()):
                raise ValueError('secrets file cannot be read as a patch')
            files = parse_diff(patch_path.read_text(encoding='utf-8-sig'))
        else:
            files = working_diff(ROOT)
        findings = lint(files, ROOT)
    except (OSError, ValueError, SyntaxError, subprocess.SubprocessError) as error:
        # Exceptions can embed patch content or secret values.
        print(f'mm difflint: cannot read diff ({type(error).__name__})', file=sys.stderr)
        return 2
    for finding in findings:
        print(finding)
    return int(any(f.rule.startswith('ERROR_') for f in findings))


if __name__ == '__main__':
    sys.exit(main())
