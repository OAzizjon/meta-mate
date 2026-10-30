"""Check Tauri commands, invoke arguments, events and locale keys offline.

Uses mm_map's offset-preserving scanner and event extraction. Dynamic command/event
names and interpolated templates cannot be checked statically. --verbose reports
invoke argument expressions that cannot be compared; these notes do not fail the check.
"""

import argparse
import json
from pathlib import Path
import re

import mm_map

ROOT = Path(__file__).resolve().parent.parent
# Intentional events without a counterpart, with a reason beside each future entry.
EVENT_ALLOWLIST = set()
INJECTED_TYPES = {'AppHandle', 'State', 'Window', 'WebviewWindow', 'Webview', 'Request',
                  'CommandScope', 'GlobalScope'}
INVOKE = re.compile(r'(?<![\w$])invoke\s*(?:<[^;()]*?>\s*)?\(')
HANDLER = re.compile(r'\btauri\s*::\s*generate_handler\s*!\s*\[')
KEY = re.compile(r'[A-Za-z_$][\w$]*')
LOCALE_KEY = re.compile(r'[A-Za-z_]\w*(?:\.[A-Za-z_]\w*)+')


def _end(bare, start):
    """Offset just after a balanced bracket, ignoring already masked literals."""
    depth = 0
    for k in range(start, len(bare)):
        if bare[k] in '([{':
            depth += 1
        elif bare[k] in ')]}':
            depth -= 1
            if depth == 0:
                return k + 1
    return len(bare)


def _parts(bare, start, end, rust=False):
    """Top-level comma-separated spans, including Rust generic type arguments."""
    depth, angle, first = 0, 0, start
    for k in range(start, end):
        c = bare[k]
        if c in '([{':
            depth += 1
        elif c in ')]}':
            depth -= 1
        elif rust and c == '<':
            angle += 1
        elif rust and c == '>' and angle:
            angle -= 1
        elif c == ',' and depth == angle == 0:
            if bare[first:k].strip():
                yield first, k
            first = k + 1
    if bare[first:end].strip():
        yield first, end


def _camel(name):
    return re.sub(r'_([a-zA-Z0-9])', lambda m: m[1].upper(), name)


def _parameters(src, match):
    paren = src.bare.find('(', match.end())
    end = _end(src.bare, paren)
    # Tauri also supports an explicit snake_case argument naming convention.
    snake = bool(re.search(r'rename_all\s*=\s*"snake_case"',
                           src.code[match.start():match.end()]))
    required, accepted = set(), set()
    for start, stop in _parts(src.bare, paren + 1, end - 1, rust=True):
        param = src.bare[start:stop].strip()
        found = re.fullmatch(r'(?:mut\s+)?(r#)?(\w+)\s*:\s*(.*)', param, re.S)
        if not found:
            continue
        name, kind = found[2], found[3]
        # Match the outer type, never State/AppHandle nested inside a payload type.
        outer = re.match(r"(?:&\s*(?:'\w+\s*)?(?:mut\s+)?)?"
                         r'(?:(?:\w+)\s*::\s*)*(\w+)', kind)
        type_name = outer[1] if outer else ''
        if type_name in INJECTED_TYPES:
            continue
        key = name if snake else _camel(name)
        accepted.add(key)
        # Tauri deserializes an absent Option argument as None.
        if type_name != 'Option':
            required.add(key)
    return required, accepted


def _object_keys(src, span):
    start, stop = span
    while start < stop and src.bare[start].isspace():
        start += 1
    if start == stop or src.bare[start] != '{' or _end(src.bare, start) != start + len(
            src.bare[start:stop].rstrip()):
        return None
    keys, complete = set(), True
    for first, end in _parts(src.bare, start + 1, _end(src.bare, start) - 1):
        while src.bare[first].isspace():
            first += 1
        token = src.bare[first:end].strip()
        if first in src.strings:
            close = src.bare.find('"', first + 1)
            if src.bare[close + 1:end].lstrip().startswith(':'):
                keys.add(src.strings[first])
                continue
        match = KEY.match(token)
        if match and (token[match.end():].lstrip().startswith(':') or match.end() == len(token)):
            keys.add(match[0])
        else:
            # A spread or computed property can supply any missing key.
            complete = False
    return keys, complete


def _file_literal(project, src, offset, value):
    """Exclude paths that happen to share a locale prefix (chat.json, card.html)."""
    # Existing project assets are unambiguous. Never open the referenced file.
    if (project.root / value).is_file():
        return True
    before = src.bare[max(0, offset - 512):offset]
    if re.search(r'(?:WebviewUrl\s*::\s*App|\.\s*(?:join|with_file_name))\s*\(\s*$', before):
        return True
    # Runtime data files need not exist in the repository; these are file constants.
    if not re.search(r'\.(?:html|json|json\.tmp)$', value):
        return False
    return bool(re.search(r'\b(?:const|static)\s+[A-Z_]*(?:FILE|PAGE|PATH|TEMP_NAME)[A-Z_]*'
                          r'\s*:\s*[^=;]+\s*=\s*$', before))


def findings(root=ROOT, *, verbose=False, notes=None):
    """Sorted path:line diagnostics; optional verbose notes are non-failing."""
    project = mm_map.Project(root)
    rust = project.files('src-tauri/src/**/*.rs', 'crates/*/src/**/*.rs')
    ts = [rel for rel in project.files('src/**/*.ts') if not rel.endswith('.test.ts')]
    details = []

    def report(rel, line, message):
        details.append(f'{rel}:{line}  {message}')

    commands = {}
    for rel in rust:
        src = project.source(rel)
        for match in mm_map.TAURI_COMMAND.finditer(src.bare):
            if not src.in_test(match.start()):
                commands[match[1]] = (rel, src.line(match.start(1)), _parameters(src, match))
    registered = {}
    lib = 'src-tauri/src/lib.rs'
    if (Path(root) / lib).is_file():
        src = project.source(lib)
        for match in HANDLER.finditer(src.bare):
            if src.in_test(match.start()):
                continue
            start, end = match.end(), _end(src.bare, match.end() - 1) - 1
            for first, stop in _parts(src.bare, start, end):
                token = src.bare[first:stop].strip()
                if mm_map.IDENT_PATH.fullmatch(token):
                    name = token.split('::')[-1]
                    registered[name] = src.line(first + len(src.bare[first:stop])
                                                - len(src.bare[first:stop].lstrip()))
    for name, (rel, line, _) in commands.items():
        if name not in registered:
            report(rel, line, f'command {name!r} is not registered in generate_handler!')
    for name, line in registered.items():
        if name not in commands:
            report(lib, line, f'registered command {name!r} has no #[tauri::command] function')
    for rel in ts:
        src = project.source(rel)
        for match in INVOKE.finditer(src.bare):
            args = mm_map._call_args(src.bare, match.end() - 1)
            if not args:
                continue
            name, _ = mm_map._arg_value(src, args[0], {})
            if name is None:
                continue
            line = src.line(match.start())
            if name not in registered:
                report(rel, line, f'invoke command {name!r} is not registered')
            if name not in commands:
                continue
            obj = _object_keys(src, args[1]) if len(args) > 1 else (set(), True)
            if obj is None or not obj[1]:
                if verbose and notes is not None:
                    notes.append(f'{rel}:{line}  skipped dynamic invoke arguments for {name!r}')
                if obj is None:
                    continue
            keys, complete = obj
            required, accepted = commands[name][2]
            for key in sorted(required - keys if complete else set()):
                report(rel, line, f'invoke {name!r}: missing argument {key!r}')
            for key in sorted(keys - accepted):
                report(rel, line, f'invoke {name!r}: unknown argument {key!r}')

    for name, sites in mm_map._event_sites(project, rust, ts).items():
        if name in EVENT_ALLOWLIST:
            continue
        if sites.get('rs emit') and not sites.get('ts listen'):
            for rel, line in sites['rs emit']:
                report(rel, line, f'Rust event {name!r} has no TypeScript listen/once')
        if sites.get('ts listen') and not (sites.get('rs emit') or sites.get('ts emit')):
            for rel, line in sites['ts listen']:
                report(rel, line, f'listened event {name!r} has no Rust or TypeScript emitter')

    locales = {}
    for lang in ('en', 'ru'):
        rel = f'locales/{lang}.json'
        try:
            locales[lang] = json.loads(project.text(rel))
            if not isinstance(locales[lang], dict):
                raise ValueError('expected a locale object')
        except (OSError, ValueError):
            report(rel, 1, 'locale file is missing or is not a JSON object')
            locales[lang] = {}
    prefixes = {key.split('.')[0] for key in locales['en'] if '.' in key}
    for rel in rust + ts:
        src = project.source(rel)
        for offset, value in src.strings.items():
            if (not src.in_test(offset) and LOCALE_KEY.fullmatch(value)
                    and value.split('.')[0] in prefixes
                    and not _file_literal(project, src, offset, value)):
                for lang in ('en', 'ru'):
                    if value not in locales[lang]:
                        report(rel, src.line(offset), f'locale key {value!r} missing from locales/{lang}.json')
    return sorted(set(details))


def main(argv=None, root=ROOT):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--verbose', action='store_true')
    args = parser.parse_args(argv)
    notes = []
    details = findings(root, verbose=args.verbose, notes=notes)
    for detail in details + notes:
        print(detail)
    return int(bool(details))


if __name__ == '__main__':
    raise SystemExit(main())
