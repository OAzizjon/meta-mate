"""mm gate [PATCH|-]

Read-only triage of an agent's patch before the lead applies it (standard
library only). The patch is never applied and .env* is never opened. Each
check prints a section with OK / WARN / FAIL rows:

  1. git apply --check   applies to the working tree, or not (WARN when there
                         is no .git, as in an agent copy; draft.sh's
                         timestamped '+++ b/path<TAB>date' headers are stripped
                         first, like mm_locales does)
  2. difflint leftovers  the existing tools/mm_difflint.py rules, imported and
                         run against the patch (ERROR_* -> FAIL, WARN_* -> WARN)
  3. invariants          touched paths -> the project invariants the lead must
                         re-check (a small static table from docs/agent-brief.md)
  4. tests to run        the mm test --changed plan (mm_changed, imported:
                         reverse dependencies + embedded inputs) plus the exact
                         python unittest files for touched tools/*.py
  5. file size           files the patch pushes over 800 lines

Exit code 0 when no section is FAIL, 1 otherwise; --json prints one object.
"""

import argparse
import json
import re
import subprocess
import sys
from pathlib import Path

import mm_changed
import mm_difflint

ROOT = Path(__file__).absolute().parent.parent

MARK = {'OK': 'OK  ', 'WARN': 'WARN', 'FAIL': 'FAIL'}


def new_section(name):
    return {'name': name, 'rows': []}


def add(section, status, text=''):
    section['rows'].append((status, text))


def section_status(section):
    statuses = [status for status, _ in section['rows']]
    if 'FAIL' in statuses:
        return 'FAIL'
    if 'WARN' in statuses:
        return 'WARN'
    return 'OK'


def _run(cmd, cwd, timeout=60):
    try:
        proc = subprocess.run(cmd, cwd=cwd, capture_output=True, text=True, encoding='utf-8',
                              errors='replace', timeout=timeout, stdin=subprocess.DEVNULL)
    except FileNotFoundError:
        return 127, ''
    except subprocess.TimeoutExpired:
        return 124, f'timed out after {timeout} s'
    return proc.returncode, (proc.stdout + proc.stderr).strip()


def _strip_header_dates(text):
    """draft.sh headers end in '<TAB>timestamp': git apply rejects them."""
    lines = []
    for line in text.splitlines():
        if line.startswith(('--- ', '+++ ')) and '\t' in line:
            line = line.split('\t', 1)[0]
        lines.append(line)
    return '\n'.join(lines) + '\n'


def check_apply(text, root):
    """Section 1: git apply --check against the working tree (never applies)."""
    section = new_section('git apply --check')
    if not (Path(root) / '.git').exists():
        add(section, 'WARN', 'no .git here (an agent copy): not checked')
        return section
    try:
        # Bytes, not text: a text-mode pipe on Windows writes CRLF line ends and
        # every hunk stops matching.
        proc = subprocess.run(['git', 'apply', '--check'], cwd=root,
                              input=text.encode('utf-8'), capture_output=True, timeout=60)
        proc.stdout = proc.stdout.decode('utf-8', 'replace')
        proc.stderr = proc.stderr.decode('utf-8', 'replace')
    except FileNotFoundError:
        add(section, 'WARN', 'git not installed: not checked')
        return section
    except subprocess.TimeoutExpired:
        add(section, 'FAIL', 'git apply --check timed out')
        return section
    # First line only: later lines can quote the patch (and any secret in it).
    detail = (proc.stderr or proc.stdout or '').strip().splitlines()
    if proc.returncode == 0:
        add(section, 'WARN' if detail else 'OK',
            detail[0] if detail else 'applies cleanly to the working tree')
    else:
        add(section, 'FAIL', detail[0] if detail else f'git apply --check exited {proc.returncode}')
    return section


def difflint_section(findings):
    """Section 2: the mm_difflint rules, imported. File-size findings are section 5."""
    section = new_section('difflint leftovers')
    for finding in findings:
        if finding.rule.startswith('ERROR_'):
            add(section, 'FAIL', str(finding))
        elif finding.rule.startswith('WARN_') and finding.rule != 'WARN_FILE_SIZE':
            add(section, 'WARN', str(finding))
    if not section['rows']:
        add(section, 'OK', 'no leftovers in the added lines')
    return section


_FOCUS_DPI = re.compile(r'src-tauri/src/(?:overlay|highlight|flight|mate)[\w.-]*\.rs$')
_LEAD_ONLY = ('auth', 'payment', 'migration')


def invariant_rows(path):
    """One touched path -> [(status, row)] for the invariants it endangers."""
    clean = path.replace('\\', '/')
    lower = clean.lower()
    if mm_difflint.is_secret(clean):
        return [('FAIL', f'{clean}: lead only, agents review only (secrets files are never opened)')]
    rows = []
    if lower.startswith('crates/mm-win32/') or _FOCUS_DPI.fullmatch(lower):
        rows.append(('WARN', f'{clean}: invariants 1+2 (no focus steal, per-monitor DPI v2)'))
    if any(key in lower for key in ('capture', 'screen', 'redact', 'privacy')):
        rows.append(('WARN', f'{clean}: invariant 3 (screenshots: consent flow, redaction, fail closed)'))
    if any(key in lower for key in ('input', 'do-it', 'do_it', 'explain', 'safety')):
        rows.append(('WARN', f'{clean}: invariant 4 (never auto-click payment/purchase/delete/password)'))
    if 'pointing' in lower or 'matescript' in lower:
        rows.append(('WARN', f"{clean}: invariant 5 ('I can't find it' beats pointing at a guess)"))
    if lower.startswith('locales/'):
        rows.append(('WARN', f'{clean}: invariant 7 (user-facing strings via locales/en.json + ru.json)'))
    if lower.startswith('server/'):
        rows.append(('WARN', f'{clean}: invariant 8 (Meta models by default); proxy: owner redeploy needed)'))
    if any(key in lower for key in _LEAD_ONLY):
        rows.append(('FAIL', f'{clean}: lead only, agents review only'))
    return rows


def check_invariants(paths):
    """Section 3: touched paths -> project invariants (docs/agent-brief.md)."""
    section = new_section('invariants')
    seen = set()
    for path in paths:
        clean = path.replace('\\', '/')
        if not clean or clean == '/dev/null' or clean in seen:
            continue
        seen.add(clean)
        for status, row in invariant_rows(clean):
            add(section, status, row)
    if not section['rows']:
        add(section, 'OK', 'no invariant hotspots in the touched paths')
    return section


def check_tests(paths, root):
    """Section 4: the mm test --changed plan plus per-file unittest commands."""
    section = new_section('tests to run')
    names = sorted({p.replace('\\', '/') for p in paths})
    inputs = mm_changed.embedded_inputs(root)
    graph = {}
    steps = mm_changed.affected_steps(names, graph, inputs)
    if any(s.startswith('rust:') for s in steps) or any(
            p in ('Cargo.toml', 'Cargo.lock') or p.startswith('.cargo/') for p in names):
        code, output = _run(['cargo', 'metadata', '--no-deps', '--format-version', '1'], root)
        if code == 0:
            try:
                graph = mm_changed.workspace_graph(json.loads(output))
            except (ValueError, KeyError, TypeError):
                graph = {}
        if not graph:
            add(section, 'WARN', 'cargo metadata failed: rust reverse-dependencies incomplete')
    for label, command in mm_changed.build_plan(names, graph, inputs):
        if label == 'docs':
            continue
        add(section, 'OK', f'{label}: ' + subprocess.list2cmdline(command))
    for name in names:
        match = re.fullmatch(r'tools/(\w+)\.py', name)
        if match and (Path(root) / 'tools' / 'tests' / f'test_{match.group(1)}.py').exists():
            add(section, 'OK', f'{name}: python -m unittest discover -s tools/tests '
                               f'-p test_{match.group(1)}.py -v')
    if not section['rows']:
        add(section, 'OK', 'nothing the test suite covers')
    return section


def check_sizes(findings):
    """Section 5: files the patch would push over 800 lines (mm_difflint sizes them)."""
    section = new_section('file size')
    for finding in findings:
        if finding.rule == 'WARN_FILE_SIZE':
            add(section, 'WARN', str(finding))
    if not section['rows']:
        add(section, 'OK', 'no file over 800 lines')
    return section


def sections_for(text, root=ROOT):
    """Parse the patch and run all five sections against root."""
    root = Path(root)
    files = mm_difflint.parse_diff(text)
    findings = mm_difflint.lint(files, root)
    paths = []
    for file in files:
        for name in [*(file.headers or []), file.path]:
            if name and name not in paths:
                paths.append(name)
    return [check_apply(_strip_header_dates(text), root),
            difflint_section(findings),
            check_invariants(paths),
            check_tests(paths, root),
            check_sizes(findings)]


def _read_patch(arg):
    if arg == '-':
        return sys.stdin.read(), '(stdin)'
    if mm_difflint.is_secret(arg) or mm_difflint.is_secret(Path(arg).resolve().as_posix()):
        raise ValueError('refused: secrets files are never opened')
    try:
        return Path(arg).read_text(encoding='utf-8-sig'), arg
    except OSError:
        raise ValueError(f'cannot read patch: {arg}') from None


def _print_text(sections, label):
    print(f'mm gate: {label}')
    for section in sections:
        print(f'\n== {section["name"]}')
        for status, row in section['rows']:
            print(f'{MARK[status]}  {row}' if row else MARK[status])


def _print_json(sections, label, failed):
    payload = {'patch': label, 'ok': not failed,
               'sections': [{'name': section['name'], 'status': section_status(section),
                             'rows': [{'status': status, 'text': text}
                                      for status, text in section['rows']]}
                            for section in sections]}
    print(json.dumps(payload, indent=2, ensure_ascii=False))


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('patch', nargs='?', default='-',
                        help='unified diff file, or - for stdin (default: -)')
    parser.add_argument('--json', action='store_true', help='print one JSON object')
    args = parser.parse_args(argv)
    try:
        text, label = _read_patch(args.patch)
    except ValueError as error:
        print(f'mm gate: {error}', file=sys.stderr)
        return 2
    sections = sections_for(text, ROOT)
    failed = any(section_status(section) == 'FAIL' for section in sections)
    if args.json:
        _print_json(sections, label, failed)
    else:
        _print_text(sections, label)
    return 1 if failed else 0


if __name__ == '__main__':
    sys.exit(main())
