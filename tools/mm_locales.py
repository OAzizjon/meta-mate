"""mm_locales: safe edits to the locale files, for the lead and for agents.

    mm locales merge PATCH [--dry-run] [--theirs] [--locales-only]
    mm locales add KEY EN RU

`merge` lifts the locale keys a unified diff adds out of the patch — two agents
appending keys at the end of locales/en.json and locales/ru.json no longer
make each other's patch fail to apply — then applies the rest of the patch
with git apply, excluding the two locale files. `add` inserts one key into
both files at once.

Both commands keep the files' formatting: 2-space indent, one key per line,
the new key after the last key with the same prefix before the first dot (else
at the end), UTF-8 without BOM, LF. The rewritten text is validated with
json.loads before it is written atomically (temp file + os.replace), and keys
outside [a-z0-9_.]+ are refused. `mm agent locales add` is allowed in agent
mode because it writes nothing outside locales/.
"""

import argparse
import json
import os
import re
import subprocess
import tempfile
from pathlib import Path

ROOT = Path(__file__).absolute().parent.parent

LOCALE_NAMES = ('locales/en.json', 'locales/ru.json')
KEY_NAME = re.compile(r'[a-z0-9_.]+')
KEY_LINE = re.compile(r'^\s*"((?:[^"\\]|\\.)*)"\s*:\s*"(?:[^"\\]|\\.)*"\s*,?\s*$')
PAIR_LINE = re.compile(r'^\s*"((?:[^"\\]|\\.)*)"\s*:\s*"((?:[^"\\]|\\.)*)"\s*,?\s*$')
DIFF_TARGET = re.compile(r'^\+\+\+\s+"?(.*?)"?\s*$')
TRAILING_COMMA = re.compile(r',\s*$')


# --- diff parsing -------------------------------------------------------------

def _unjson(inner):
    """The string a JSON string body (text between the quotes) stands for."""
    try:
        value = json.loads(f'"{inner}"')
    except ValueError:
        return None
    return value if isinstance(value, str) else None


def _norm_target(raw):
    path = raw.split('\t', 1)[0].strip().strip('"')
    if path in ('/dev/null', ''):
        return None
    for prefix in ('a/', 'b/'):
        if path.startswith(prefix):
            return path[len(prefix):]
    return path


def parse_patch(text):
    """{locale name: [(key, value), ...]} added by the patch, in hunk order.

    Only added lines of the exact shape "key": "value" inside hunks whose
    target is locales/en.json or locales/ru.json are collected.
    """
    found = {name: [] for name in LOCALE_NAMES}
    target = None
    for line in text.splitlines():
        m = DIFF_TARGET.match(line)
        if m:
            target = _norm_target(m.group(1))
            continue
        if target not in found or not line.startswith('+') or line.startswith('+++'):
            continue
        pair = PAIR_LINE.match(line[1:])
        if pair:
            key = _unjson(pair.group(1))
            value = _unjson(pair.group(2))
            if key is not None and value is not None:
                found[target].append((key, value))
    return found


# --- text editing -------------------------------------------------------------

def _key_line(key, value):
    return f'  {json.dumps(key, ensure_ascii=False)}: {json.dumps(value, ensure_ascii=False)}'


def _key_rows(lines):
    rows = []
    for i, line in enumerate(lines):
        m = KEY_LINE.match(line)
        if m:
            rows.append((i, _unjson(m.group(1))))
    return rows


def _fix_commas(lines):
    rows = [i for i, _ in _key_rows(lines)]
    for i in rows[:-1]:
        if not TRAILING_COMMA.search(lines[i]):
            lines[i] = lines[i].rstrip() + ','
    if rows:
        lines[rows[-1]] = TRAILING_COMMA.sub('', lines[rows[-1]].rstrip())


def insert_key(text, key, value, replace=False):
    """Return text with key inserted (or its value replaced), formatting kept."""
    lines = text.replace('\r\n', '\n').split('\n')
    if replace:
        for i, line in enumerate(lines):
            m = KEY_LINE.match(line)
            if m and _unjson(m.group(1)) == key:
                newline = _key_line(key, value)
                if TRAILING_COMMA.search(line):
                    newline += ','
                lines[i] = newline
                return '\n'.join(lines)
        raise ValueError(f'key not found: {key}')
    rows = _key_rows(lines)
    prefix = key.split('.', 1)[0]
    same = [i for i, k in rows if k is not None and k.split('.', 1)[0] == prefix]
    if same:
        at = same[-1] + 1
    elif rows:
        at = rows[-1][0] + 1
    else:
        at = 1 if lines and lines[0].strip() == '{' else 0
    lines.insert(at, _key_line(key, value))
    _fix_commas(lines)
    return '\n'.join(lines)


def _write_atomic(path, text):
    data = text.encode('utf-8')  # no BOM; text already uses LF
    fd, tmp_name = tempfile.mkstemp(prefix=path.name + '.', suffix='.tmp', dir=str(path.parent))
    try:
        with os.fdopen(fd, 'wb') as stream:
            stream.write(data)
            stream.flush()
            os.fsync(stream.fileno())
        try:
            os.chmod(tmp_name, path.stat().st_mode & 0o777)
        except OSError:
            pass
        os.replace(tmp_name, path)
    except BaseException:
        try:
            os.unlink(tmp_name)
        except OSError:
            pass
        raise


def _rewrite(path, edits):
    """Apply [(key, value, replace)] to path; validate before writing."""
    text = path.read_text(encoding='utf-8-sig')
    json.loads(text)  # the file must be valid before we touch it
    for key, value, replace in edits:
        text = insert_key(text, key, value, replace=replace)
    parsed = json.loads(text)
    for key, value, _ in edits:
        if parsed.get(key) != value:
            raise ValueError(f'{path.name}: {key} did not round-trip')
    if not text.endswith('\n'):
        text += '\n'
    _write_atomic(path, text)


# --- commands -----------------------------------------------------------------

def _short(text, limit=48):
    return text if len(text) <= limit else text[:limit - 1] + '…'


def _git_apply(patch, root, dry_run):
    cmd = ['git', 'apply', '--exclude=locales/en.json', '--exclude=locales/ru.json', str(patch)]
    if dry_run:
        print(f'dry-run: would run {subprocess.list2cmdline(cmd)}')
        return 0
    print(f'rest of the patch: {subprocess.list2cmdline(cmd)}')
    try:
        proc = subprocess.run(cmd, cwd=root, capture_output=True, text=True,
                              encoding='utf-8', errors='replace', stdin=subprocess.DEVNULL)
    except FileNotFoundError:
        print('mm locales merge: git not found')
        return 1
    if proc.returncode != 0:
        output = (proc.stdout + proc.stderr).strip()
        if output:
            print(output)
        print('mm locales merge: git apply failed')
        return 1
    return 0


def cmd_merge(args, root=ROOT):
    patch = Path(args.patch)
    if not patch.is_file():
        print(f'mm locales merge: patch not found: {patch}')
        return 2
    try:
        wanted = parse_patch(patch.read_text(encoding='utf-8-sig'))
    except OSError as error:
        print(f'mm locales merge: cannot read {patch}: {error}')
        return 2

    edits = {}
    added, skipped, conflicts = [], [], []
    for name in LOCALE_NAMES:
        path = root / name
        try:
            current = json.loads(path.read_text(encoding='utf-8-sig'))
        except (OSError, ValueError) as error:
            print(f'mm locales merge: {name}: cannot read the current file: {error}')
            return 1
        todo = []
        latest = {}
        for key, value in wanted.get(name, []):
            latest[key] = value  # one key may appear in several hunks; the last wins
        for key, value in latest.items():
            if not KEY_NAME.fullmatch(key):
                skipped.append((name, key, 'not [a-z0-9_.]+'))
            elif key not in current:
                todo.append((key, value, False))
                added.append((name, key))
            elif current[key] == value:
                skipped.append((name, key, 'same value'))
            elif args.theirs:
                todo.append((key, value, True))
                skipped.append((name, key, 'conflict, took theirs'))
            else:
                conflicts.append((name, key, current[key], value))
        if todo:
            edits[name] = todo

    for name, key in added:
        print(f'  + {name}: {key}')
    for name, key, why in skipped:
        print(f'  = {name}: {key} ({why})')
    for name, key, ours, theirs in conflicts:
        print(f'  ! {name}: {key}: file has "{_short(ours)}", patch has "{_short(theirs)}";'
              ' rerun with --theirs to take the patch value')
    if not (added or skipped or conflicts):
        print('mm locales merge: the patch adds no locale keys.')

    if edits and not args.dry_run:
        try:
            for name, todo in edits.items():
                _rewrite(root / name, todo)
        except (OSError, ValueError) as error:
            print(f'mm locales merge: write failed: {error}')
            return 1
        for name, todo in edits.items():
            print(f'wrote {len(todo)} key(s) to {name}')
    elif edits:
        for name, todo in edits.items():
            print(f'dry-run: would write {len(todo)} key(s) to {name}')

    if args.locales_only:
        print('rest of the patch: skipped (--locales-only)')
    elif _git_apply(patch, root, args.dry_run) != 0:
        return 1

    if conflicts:
        print(f'mm locales merge: {len(conflicts)} conflict(s); those keys were left unchanged.')
        return 1
    return 0


def cmd_add(args, root=ROOT):
    if not KEY_NAME.fullmatch(args.key):
        print(f'mm locales add: keys must match [a-z0-9_.]+: {args.key!r}')
        return 2
    edits = []
    for name, value in zip(LOCALE_NAMES, (args.en, args.ru)):
        path = root / name
        try:
            current = json.loads(path.read_text(encoding='utf-8-sig'))
        except (OSError, ValueError) as error:
            print(f'mm locales add: {name}: cannot read the current file: {error}')
            return 1
        if args.key in current:
            if current[args.key] == value:
                print(f'{name}: {args.key} already has this value; unchanged')
                continue
            print(f'mm locales add: {name}: {args.key} already exists with a different value;'
                  ' refusing to change it')
            return 1
        edits.append((name, [(args.key, value, False)]))
    for name, todo in edits:
        try:
            _rewrite(root / name, todo)
        except (OSError, ValueError) as error:
            print(f'mm locales add: write failed: {error}')
            return 1
        print(f'wrote {args.key} to {name}')
    return 0


def build_parser():
    parser = argparse.ArgumentParser(prog='mm locales',
                                     description='Safe edits to the locale files.')
    sub = parser.add_subparsers(dest='command', required=True)
    sp = sub.add_parser('merge', help='add the locale keys a patch adds, then apply the rest of it')
    sp.add_argument('patch', help='unified diff file')
    sp.add_argument('--dry-run', action='store_true', help='report only; write nothing, run nothing')
    sp.add_argument('--theirs', action='store_true', help='on a conflicting key, take the patch value')
    sp.add_argument('--locales-only', action='store_true', help='do not git apply the rest of the patch')
    sp.set_defaults(func=cmd_merge)
    sp = sub.add_parser('add', help='add one key to locales/en.json and locales/ru.json')
    sp.add_argument('key', help='[a-z0-9_.]+')
    sp.add_argument('en', help='English value')
    sp.add_argument('ru', help='Russian value')
    sp.set_defaults(func=cmd_add)
    return parser


def main(argv=None):
    args = build_parser().parse_args(argv)
    return args.func(args)


if __name__ == '__main__':
    raise SystemExit(main())
