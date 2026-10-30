"""Check Tauri commands, arguments, events, locale keys and serde shapes offline.

Uses mm_map's offset-preserving scanner and event extraction. Dynamic command/event
names and interpolated templates cannot be checked statically. --verbose reports
invoke argument expressions that cannot be compared; these notes do not fail the check.
The explicit serde pairs compare keys and tags, not Rust/TS value assignability.
Unsupported mapped shapes (including flatten) fail as unknown instead of silently
passing. default is input compatibility; it does not omit output fields.
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

# Explicit wire boundaries, never a repository-wide same-name guess. A dotted TS
# name selects an inline property union; @ selects an anonymous function return.
# Session and the selftest/s3 reports are disk/probe data, not WebView contracts.
# MateHome has no TS model: its containing Settings.mateHome is checked below.
SHAPE_PAIRS = {
    ('settings.rs', 'Settings'): ('src/settings/model.ts', 'Settings'),
    ('settings.rs', 'Language'): ('src/settings/model.ts', 'Language'),
    ('settings.rs', 'Model'): ('src/settings/model.ts', 'ModelChoice'),
    ('settings.rs', 'Ptt'): ('src/settings/model.ts', 'PttMode'),
    ('settings.rs', 'PttKey'): ('src/settings/model.ts', 'PttKey'),
    ('settings.rs', 'Sound'): ('src/settings/model.ts', 'SoundMode'),
    ('settings.rs', 'RobotSize'): ('src/settings/model.ts', 'RobotSize'),
    ('answer_stream.rs', 'AnswerMsg'): ('src/ipc/answer-stream.ts', 'AnswerMsg'),
    ('card.rs', 'LessonCard'): ('src/card.ts', 'LessonCard'),
    ('card.rs', 'WordsBy'): ('src/card.ts', 'CardWordsBy'),
    ('auth.rs', 'AuthStatus'): ('src/settings/account.ts', 'AuthStatus'),
    ('chat.rs', 'Message'): ('src/chat/model.ts', 'Message'),
    ('chat.rs', 'Role'): ('src/chat/model.ts', 'Message.role'),
    ('highlight.rs', 'HighlightMsg'): ('src/highlight/geometry.ts', 'HighlightMsg'),
    ('highlight.rs', 'Rect'): ('src/robot/types.ts', 'CssRect'),
    ('highlight.rs', 'Point'): ('src/robot/types.ts', 'CssPoint'),
    ('highlight.rs', 'Step'): ('src/highlight/geometry.ts', 'HighlightStep'),
    ('pointing.rs', 'StepNumber'): ('src/highlight/geometry.ts', 'HighlightStep'),
    ('pointing.rs', 'MateRect'): ('src/ipc/answer-stream.ts', 'MateRect'),
    ('debug_view.rs', 'DebugAnswerMsg'): ('src/debug/view.ts', 'DebugAnswerMsg'),
    ('debug_view.rs', 'DebugMsg'): ('src/debug/view.ts', 'DebugMsg'),
    ('proxy.rs', 'GuestUsage'): ('src/mate-menu.ts', '@guestUsageOf'),
}
# (Rust file, type, variant, diagnostic kind, member): one reason per exception.
SHAPE_ALLOWLIST = {
    ('settings.rs', 'Settings', '', 'Rust-only field', 'mateHome'):
        'Rust owns the resting position; save_settings preserves it when the preferences UI saves.',
    ('card.rs', 'LessonCard', '', 'Rust-only field', 'locale'):
        'The card page follows the shared settings locale instead of reading payload.locale.',
    ('proxy.rs', 'GuestUsage', '', 'Rust-only field', 'reset'):
        'The guest UI projects only left and limit; the reset countdown stays in Rust.',
    ('debug_view.rs', 'DebugAnswerMsg', 'heard', 'Rust-only enum member', 'heard'):
        'The debug view displays timeline and audio diagnostics and discards transcript character counts.',
}


def _spans(bare, start, stop, separators):
    """Split only at the outer level, including generic arguments."""
    first, k = start, start
    while k < stop:
        if bare[k] in '([{<':
            if bare[k] == '<':
                depth = 1
                k += 1
                while k < stop and depth:
                    depth += (bare[k] == '<') - (bare[k] == '>')
                    k += 1
                continue
            k = _end(bare, k)
            continue
        if bare[k] in separators:
            if bare[first:k].strip():
                yield first, k
            first = k + 1
        k += 1
    if bare[first:stop].strip():
        yield first, stop


def _attrs(src, start, stop):
    """Serde attributes only; comments and string contents cannot invent flags."""
    values = {}
    cursor = start
    while cursor < stop:
        prefix = re.match(r'\s*#\s*\[', src.bare[cursor:stop])
        if not prefix:
            break
        bracket = cursor + prefix.end() - 1
        cursor = _end(src.bare, bracket)
        match = re.match(r'\s*serde\s*\(', src.bare[bracket + 1:cursor])
        if not match:
            continue
        paren = bracket + match.end()
        end = _end(src.bare, paren)
        for a, b in _parts(src.bare, paren + 1, end - 1):
            flag = KEY.search(src.bare, a, b)
            if flag:
                literals = [v for p, v in src.strings.items() if a <= p < b]
                values[flag[0]] = literals[0] if literals else True
    return values


def _serde_name(name, rule, *, variant=False):
    # Serde splits enum variants at every uppercase character (including acronyms),
    # while struct fields start as snake_case. camelCase only lowers a variant's head.
    if rule == 'lowercase':
        return name.lower() if variant else name
    if rule == 'UPPERCASE':
        return name.upper()
    if rule in ('PascalCase', 'camelCase'):
        pascal = name if variant else ''.join(s[:1].upper() + s[1:] for s in name.split('_'))
        return pascal[:1].lower() + pascal[1:] if rule == 'camelCase' else pascal
    snake = re.sub(r'(?<!^)([A-Z])', r'_\1', name).lower() if variant else name
    if rule in ('snake_case', 'SCREAMING_SNAKE_CASE', 'kebab-case', 'SCREAMING-KEBAB-CASE'):
        if rule.startswith('SCREAMING'):
            snake = snake.upper()
        return snake.replace('_', '-') if 'KEBAB' in rule or 'kebab' in rule else snake
    return name


def _rust_fields(src, start, stop, rule):
    fields = {}
    for a, b in _parts(src.bare, start, stop, rust=True):
        attrs = _attrs(src, a, b)
        if 'flatten' in attrs:
            raise ValueError('serde flatten has unknown fields')
        match = re.search(r'(?:pub(?:\([^)]*\))?\s+)?(?:r#)?(\w+)\s*:\s*', src.bare[a:b])
        if not match or 'skip' in attrs or 'skip_serializing' in attrs:
            continue
        name = attrs.get('rename', _serde_name(match[1], rule))
        kind = src.bare[a + match.end():b]
        # default affects deserialization only, never removes a serialized field.
        optional = 'skip_serializing_if' in attrs or bool(re.match(r'(?:\w+::)*Option\s*<', kind))
        fields[name] = (src.line(a + match.start(1)), optional)
    return fields


def _rust_shape(src, name):
    pattern = r'((?:#\s*\[[^\]]*\]\s*)*)(?:pub(?:\([^)]*\))?\s+)?(struct|enum)\s+' + re.escape(name) + r'\b'
    for match in re.finditer(pattern, src.bare):
        if src.in_test(match.start()) or not re.search(r'\b(?:Serialize|Deserialize)\b', match[1]):
            continue
        attrs = _attrs(src, match.start(), match.end())
        start = src.bare.find('{', match.end())
        if start < 0 or src.bare[match.end():start].strip():
            raise ValueError('tuple/newtype shape is unknown')
        stop = _end(src.bare, start) - 1
        if match[2] == 'struct':
            line = src.line(match.end() - len(name))
            return line, None, {'': (line, _rust_fields(src, start + 1, stop, attrs.get('rename_all')))}
        variants = {}
        tag = attrs.get('tag')
        for a, b in _parts(src.bare, start + 1, stop, rust=True):
            options = _attrs(src, a, b)
            clean = re.sub(r'#\s*\[[^\]]*\]', lambda m: ' ' * len(m[0]), src.bare[a:b])
            variant = KEY.search(clean)
            if not variant or 'skip' in options or 'skip_serializing' in options:
                continue
            key = options.get('rename', _serde_name(variant[0], attrs.get('rename_all'), variant=True))
            body = a + variant.end()
            while body < b and src.bare[body].isspace():
                body += 1
            fields = {}
            if body < b:
                if src.bare[body] != '{' or not tag or 'content' in attrs or 'untagged' in attrs:
                    raise ValueError('enum payload representation is unknown')
                fields = _rust_fields(src, body + 1, _end(src.bare, body) - 1,
                                      options.get('rename_all', attrs.get('rename_all_fields')))
            if tag:
                fields[tag] = (src.line(a + variant.start()), False)
            variants[key] = (src.line(a + variant.start()), fields)
        return src.line(match.end() - len(name)), tag, variants
    return None


def _ts_definition(src, name):
    match = re.search(r'\b(interface|type)\s+' + re.escape(name) + r'\b\s*(?:=\s*)?', src.bare)
    if not match:
        return None
    start = match.end()
    if match[1] == 'interface':
        if src.bare[start] != '{':
            raise ValueError('TypeScript interface inheritance is unknown')
        return start, _end(src.bare, start)
    end = next(_spans(src.bare, start, len(src.bare), ';'), (start, len(src.bare)))[1]
    return start, end


def _ts_objects(project, src, start, stop, seen):
    """Resolve literal objects, unions, local aliases and explicit relative imports."""
    objects, members = [], {}
    for a, b in _spans(src.bare, start, stop, '|'):
        while a < b and src.bare[a].isspace():
            a += 1
        token = src.bare[a:b].strip()
        if token in ('null', 'undefined'):
            continue
        if src.bare[a] == '{':
            fields = {}
            for c, d in _spans(src.bare, a + 1, _end(src.bare, a) - 1, ';,'):
                prop = re.match(r'''\s*(?:readonly\s+)?([\w$]+|"[^"]*"|'[^']*')\s*(\?)?\s*:''', src.code[c:d])
                if not prop:
                    raise ValueError('TypeScript property shape is unknown')
                key = prop[1].strip('"\x27')
                value_start = c + prop.end()
                literals = {v: src.line(p) for p, v in src.strings.items() if value_start <= p < d}
                fields[key] = (src.line(c + prop.start(1)), bool(prop[2]), literals, (src, value_start, d))
            objects.append(fields)
        elif a in src.strings:
            members[src.strings[a]] = (src.rel, src.line(a))
        elif token.startswith('Extract<'):
            args = list(_parts(src.bare, a + len('Extract<'), a + len(src.bare[a:b].rstrip()) - 1, rust=True))
            if len(args) != 2:
                raise ValueError('TypeScript Extract shape is unknown')
            base, _ = _ts_objects(project, src, *args[0], seen)
            filters, _ = _ts_objects(project, src, *args[1], seen)
            objects.extend(obj for obj in base if all(k in obj and obj[k][2].keys() == v[2].keys()
                                                      for k, v in filters[0].items()))
        elif match := re.fullmatch(r'\(typeof\s+(\w+)\)\[number\]', token):
            const = re.search(r'\bconst\s+' + re.escape(match[1]) + r'\s*=\s*\[', src.bare)
            if not const:
                raise ValueError('TypeScript const union is unknown')
            end = _end(src.bare, const.end() - 1)
            members.update({v: (src.rel, src.line(p)) for p, v in src.strings.items() if const.end() <= p < end})
        elif KEY.fullmatch(token):
            identity = (src.rel, token)
            if identity in seen:
                raise ValueError('TypeScript alias cycle')
            target, definition = src, _ts_definition(src, token)
            if definition is None:
                for imp in re.finditer(r'\bimport\s+(?:type\s+)?\{([^}]+)\}\s+from\s*("[^"]*"|\x27[^\x27]*\x27)', src.code):
                    if not any(re.fullmatch(r'\s*' + re.escape(token) + r'\s*', item) for item in imp[1].split(',')):
                        continue
                    path = imp[2][1:-1]
                    if path.startswith('.'):
                        rel = (Path(src.rel).parent / path).as_posix()
                        target = project.source(rel)
                        definition = _ts_definition(target, token)
                        break
            if definition is None:
                raise ValueError(f'TypeScript type {token} is unknown')
            nested, strings = _ts_objects(project, target, *definition, seen | {identity})
            objects.extend(nested)
            members.update(strings)
        else:
            raise ValueError('TypeScript expression is unknown')
    return objects, members


def _shape_findings(project, report):
    for (file, name), (ts_rel, ts_name) in SHAPE_PAIRS.items():
        rel = 'src-tauri/src/' + file
        if not (project.root / rel).is_file():
            continue
        src = project.source(rel)
        line, ts_line = 1, 1

        def diagnostic(variant, kind, member, rs_line, other_line, other_rel=ts_rel):
            if (file, name, variant, kind, member) not in SHAPE_ALLOWLIST:
                report(rel, rs_line, f'serde {name}{"." + variant if variant else ""}: {kind} {member!r}; {other_rel}:{other_line} ({ts_name})')

        try:
            declaration = re.search(r'\b(?:struct|enum)\s+' + re.escape(name) + r'\b', src.bare)
            if declaration:
                line = src.line(declaration.start())
            shape = _rust_shape(src, name)
            if shape is None:
                continue
            line, tag, variants = shape
            ts = project.source(ts_rel)
            if ts_name.startswith('@'):
                match = re.search(r'\bfunction\s+' + re.escape(ts_name[1:]) + r'\([^)]*\)\s*:\s*', ts.bare)
                if not match:
                    raise ValueError('TypeScript function return is unknown')
                span = (match.end(), _end(ts.bare, match.end()))
            else:
                span = _ts_definition(ts, ts_name.split('.')[0])
            if span is None:
                raise ValueError('TypeScript definition is missing')
            ts_line = ts.line(span[0])
            objects, members = _ts_objects(project, ts, *span, set())
            if '.' in ts_name:
                field = objects[0][ts_name.split('.')[1]]
                objects, members = _ts_objects(project, field[3][0], *field[3][1:], set())
            if tag:
                ts_variants = {}
                for obj in objects:
                    if tag not in obj or not obj[tag][2]:
                        raise ValueError('TypeScript discriminant is unknown')
                    for key, at in obj[tag][2].items():
                        ts_variants[key] = (at, obj, obj[tag][3][0].rel)
            elif members:
                ts_variants = {key: (at, {}, path) for key, (path, at) in members.items()}
            elif len(objects) == 1:
                ts_variants = {'': (ts_line, objects[0], ts_rel)}
            else:
                raise ValueError('TypeScript object union without a tag is unknown')
            for key in sorted(variants.keys() - ts_variants.keys()):
                diagnostic(key, 'Rust-only enum member', key, variants[key][0], ts_line)
            for key in sorted(ts_variants.keys() - variants.keys()):
                diagnostic(key, 'TS-only enum member', key, line, ts_variants[key][0], ts_variants[key][2])
            for key in variants.keys() & ts_variants.keys():
                rs_at, fields = variants[key]
                ts_at, other, other_rel = ts_variants[key]
                for field in sorted(fields.keys() - other.keys()):
                    diagnostic(key, 'Rust-only field', field, fields[field][0], ts_at, other_rel)
                for field in sorted(other.keys() - fields.keys()):
                    if not other[field][1]:
                        diagnostic(key, 'required TS field never sent', field, rs_at, other[field][0], other[field][3][0].rel)
        except (ValueError, OSError, KeyError) as error:
            report(rel, line, f'serde {name}: unknown shape ({error}); {ts_rel}:{ts_line} ({ts_name})')


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
    _shape_findings(project, report)
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
