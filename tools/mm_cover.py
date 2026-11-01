"""mm cover PATH: the tests that plausibly cover one source file, ranked.

    mm cover crates/mm-core/src/coords.rs
                                   the inline #[cfg(test)] mod first, then crates/*/tests and
                                   src-tauri tests that 'use' its module path, then the ones
                                   mentioning its pub items; every hit prints its cargo command
    mm cover src/stores/account.ts the src/**/*.test.ts importing it, directly or one hop
                                   through an index.ts; every hit prints its npm test command
    mm cover tools/mm_where.py     tools/tests/test_mm_where.py first, then tests importing
                                   the module; every hit prints its python -m unittest command
    mm cover src/plain.ts          NO TESTS, loudly, when nothing plausibly covers the file
    mm cover PATH --json           the same findings as JSON

The file walking, the secrets-file refusal and the spelling variants come from mm_where.
Standard library only; a hit is a plausibility, run the printed command to be sure.
"""

import argparse
import json
import os
import re
import sys
from pathlib import Path

import mm_where

ROOT = Path(__file__).absolute().parent.parent

RUST_CFG_TEST = re.compile(r'^\s*#\[\s*cfg\s*\(\s*test\s*\)\s*\]', re.M)
RUST_TEST = re.compile(r'^\s*#\[\s*(?:\w+\s*::\s*)*test\s*\]', re.M)
RUST_PUB = re.compile(r'^\s*pub(?:\s*\([^)]*\))?\s+(?:(?:async|unsafe|extern)\s+)*'
                      r'(?:fn|struct|enum|trait|const|static|type|mod)\s+(\w+)')
TS_IMPORT = re.compile(r"(?:\bimport\b|\bexport\b)[^'\";]*?\bfrom\s*['\"]([^'\"]+)['\"]"
                       r"|\bimport\s*['\"]([^'\"]+)['\"]"
                       r"|\brequire\(\s*['\"]([^'\"]+)['\"]\s*\)")
PY_IMPORT = re.compile(r'^\s*(?:from\s+([A-Za-z_]\w*(?:\.\w+)*)\s+import\s+([^#\n]+)'
                       r'|import\s+([^#\n]+))', re.M)
MIN_STEM = 3  # shorter stems are too noisy as free-text mentions


def entry(rank, group, path, command, detail):
    return {'rank': rank, 'group': group, 'path': path, 'command': command, 'detail': detail}


def read(root, rel):
    return '\n'.join(mm_where.read_lines(root, rel))


def rel_of(path, root):
    """The argument as a project-relative path ('\\' accepted), or None when outside the tree."""
    text = str(path).replace('\\', '/')
    p = Path(text)
    if p.is_absolute():
        try:
            p = p.relative_to(root)
        except ValueError:
            return None
    else:
        p = Path(os.path.normpath(text))
    if p.is_absolute() or any(part == '..' for part in p.parts):
        return None
    return p.as_posix()


# --- Rust --------------------------------------------------------------------

def tauri_names(root):
    """(package name, lib name) of src-tauri from [package] in its Cargo.toml; ('app', 'app') fallback."""
    try:
        section, pkg = None, None
        for line in (root / 'src-tauri' / 'Cargo.toml').read_text(encoding='utf-8').splitlines():
            text = line.strip()
            if text.startswith('['):
                if pkg:
                    break
                section = text
            elif section == '[package]':
                m = re.match(r'name\s*=\s*"([^"]+)"', text)
                if m:
                    pkg = m.group(1)
        if pkg:
            return pkg, pkg.replace('-', '_')
    except OSError:
        pass
    return 'app', 'app'


def rust_location(rel, root):
    """(package, lib, module path) for a Rust source under crates/*/src or src-tauri/src."""
    parts = rel.split('/')
    if not rel.endswith('.rs'):
        return None
    if len(parts) > 3 and parts[0] == 'crates' and parts[2] == 'src':
        pkg, lib, rest = parts[1], parts[1].replace('-', '_'), parts[3:]
    elif len(parts) > 2 and parts[0] == 'src-tauri' and parts[1] == 'src':
        pkg, lib = tauri_names(root)
        rest = parts[2:]
    else:
        return None
    stem = rest[-1][:-3] if rest[-1].endswith('.rs') else rest[-1]
    if stem in ('lib', 'main') and len(rest) == 1:
        return pkg, lib, ''
    modules = [part[:-3] if part.endswith('.rs') else part for part in rest]
    if modules and modules[-1] in ('mod', 'lib', 'main'):
        modules.pop()
    return pkg, lib, '::'.join(modules)


def as_rust_test(rel, root):
    """(package, test name) when rel is itself an integration test file."""
    parts = rel.split('/')
    if not rel.endswith('.rs'):
        return None
    if len(parts) > 3 and parts[0] == 'crates' and parts[2] == 'tests':
        return parts[1], Path(rel).stem
    if len(parts) > 2 and parts[0] == 'src-tauri' and parts[1] == 'tests':
        return tauri_names(root)[0], Path(rel).stem
    return None


def rust_test_files(root):
    out = []
    for f in mm_where.tracked_files(root):
        parts = f.split('/')
        if (f.endswith('.rs') and mm_where.searchable(f)
                and (len(parts) > 3 and parts[0] == 'crates' and parts[2] == 'tests'
                     or len(parts) > 2 and parts[0] == 'src-tauri' and parts[1] == 'tests')):
            out.append(f)
    return sorted(out)


def rust_pub_items(text):
    """Names worth mentioning from `pub` items (short names like `new` match too much)."""
    items = []
    for line in text.splitlines():
        m = RUST_PUB.match(line)
        if m and (len(m.group(1)) >= 4 or '_' in m.group(1)):
            items.append(m.group(1))
    return list(dict.fromkeys(items))


def rust_uses_module(text, lib, module):
    pattern = re.compile(r'\buse\s+' + re.escape(lib) + r'::([^;]+);')
    needle = re.compile(r'(?:^|[\s,{])' + re.escape(module) + r'(?![\w])')
    return any(needle.search(m.group(1)) for m in pattern.finditer(text))


def rust_entries(rel, root):
    located = rust_location(rel, root)
    if located is None:
        as_test = as_rust_test(rel, root)
        if not as_test:
            return []
        pkg, name = as_test
        n = len(RUST_TEST.findall(read(root, rel)))
        detail = f'{n} #[test]; it is itself a test file' if n else 'it is itself a test file'
        return [entry(0, 'itself', rel, f'cargo test -p {pkg} --test {name}', detail)]
    pkg, lib, module = located
    entries = []
    text = read(root, rel)
    if RUST_CFG_TEST.search(text):
        n = len(RUST_TEST.findall(text))
        command = f'cargo test -p {pkg} --lib' + (f' {module}' if module else '')
        detail = f'{n} #[test] in this file' if n else 'a #[cfg(test)] module in this file'
        entries.append(entry(0, 'inline tests', rel, command, detail))
    items = rust_pub_items(text.split('#[cfg(test)]', 1)[0])
    for test_rel in rust_test_files(root):
        t = read(root, test_rel)
        command = f'cargo test -p {as_rust_test(test_rel, root)[0]} --test {Path(test_rel).stem}'
        if module and rust_uses_module(t, lib, module):
            entries.append(entry(1, "'use' of the module", test_rel, command, f'use {lib}::{module}'))
        else:
            mentioned = [name for name in items
                         if re.search(r'(?<![\w])' + re.escape(name) + r'(?![\w])', t)]
            if mentioned:
                extra = f', +{len(mentioned) - 6}' if len(mentioned) > 6 else ''
                entries.append(entry(2, 'mentions pub items', test_rel, command,
                                     'mentions ' + ', '.join(mentioned[:6]) + extra))
    return entries


# --- TypeScript ---------------------------------------------------------------

def ts_specs(text):
    return [m.group(1) or m.group(2) or m.group(3) for m in TS_IMPORT.finditer(text)]


def resolve_spec(spec, importer, root):
    """Resolve a relative import spec from 'importer' to a project file, or None."""
    if not spec.startswith('.'):
        return None
    base = os.path.normpath(str(Path(importer).parent / spec)).replace('\\', '/')
    if (root / base).is_file():
        return base
    for candidate in (base + '.ts', base + '.tsx', base + '/index.ts', base + '/index.tsx'):
        if (root / candidate).is_file():
            return candidate
    return None


def ts_reexports(index_rel, root):
    """The project files an index.ts/index.tsx re-exports."""
    return [r for spec in ts_specs(read(root, index_rel))
            if (r := resolve_spec(spec, index_rel, root))]


def mention_pattern(stem):
    names = [re.escape(v) for v in mm_where.variants(stem) if len(v) >= MIN_STEM]
    return re.compile(r'(?<![\w])' + '|'.join(names) + r'(?![\w])') if names else None


def ts_entries(rel, root):
    if not rel.startswith('src/') or not rel.endswith(('.ts', '.tsx')):
        return []
    if rel.endswith('.test.ts'):
        return [entry(0, 'itself', rel, f'npm test -- {rel}', 'it is itself a test file')]
    stem = Path(rel).stem
    tests = sorted(f for f in mm_where.tracked_files(root)
                   if f.startswith('src/') and f.endswith('.test.ts') and mm_where.searchable(f))
    entries, seen = [], set()
    for test_rel in tests:
        specs = ts_specs(read(root, test_rel))
        hit = next((s for s in specs if resolve_spec(s, test_rel, root) == rel), None)
        if hit is not None:
            entries.append(entry(1, 'imports it', test_rel, f'npm test -- {test_rel}', f"from '{hit}'"))
            seen.add(test_rel)
    for test_rel in tests:
        if test_rel in seen:
            continue
        specs = ts_specs(read(root, test_rel))
        via = next((r for s in specs if (r := resolve_spec(s, test_rel, root))
                    and Path(r).stem == 'index' and rel in ts_reexports(r, root)), None)
        if via:
            entries.append(entry(2, 'one hop through an index.ts', test_rel,
                                 f'npm test -- {test_rel}', f'{via} re-exports it'))
            seen.add(test_rel)
    needle = mention_pattern(stem)
    if needle:
        for test_rel in tests:
            if test_rel not in seen and needle.search(read(root, test_rel)):
                entries.append(entry(3, 'mentions it', test_rel, f'npm test -- {test_rel}',
                                     f'the name {stem} appears in it'))
    return entries


# --- Python -------------------------------------------------------------------

def is_py_test(rel):
    parts = rel.split('/')
    return len(parts) > 1 and 'tests' in parts[:-1] and parts[-1].startswith('test_')


def dotted(rel):
    return rel[:-3].replace('/', '.')  # tools/tests/test_x.py -> tools.tests.test_x


def py_imports_module(text, stem, dotted_name, parent):
    """True when the file imports the module (as stem, dotted, or from its parent)."""
    for m in PY_IMPORT.finditer(text):
        if m.group(1) is not None:  # from X import a, b as c
            module, names = m.group(1), m.group(2)
            if module == dotted_name or module == stem or module.endswith('.' + stem):
                return True
            imported = {name.strip().split(' as ')[0].strip() for name in names.split(',')}
            if parent and module == parent and stem in imported:
                return True
        else:  # import a, b.c as d
            for token in m.group(3).split(','):
                token = token.strip().split(' as ')[0].strip()
                if token == stem or token == dotted_name or token.endswith('.' + stem):
                    return True
    return False


def py_entries(rel, root):
    if not rel.endswith('.py'):
        return []
    if is_py_test(rel):
        return [entry(0, 'itself', rel, f'python -m unittest {dotted(rel)}',
                      'it is itself a test file')]
    stem = Path(rel).stem
    dotted_name = dotted(rel)
    parent = dotted_name.rsplit('.', 1)[0] if '.' in dotted_name else ''
    wanted = {'test_' + v for v in mm_where.variants(stem)}
    entries = []
    for test_rel in sorted(f for f in mm_where.tracked_files(root)
                           if f.endswith('.py') and mm_where.searchable(f) and is_py_test(f)):
        named = Path(test_rel).stem in wanted
        import_hit = py_imports_module(read(root, test_rel), stem, dotted_name, parent)
        if not (named or import_hit):
            continue
        rank = 1 if named else 2
        details = [f'test_{stem}.py naming convention'] if named else []
        if import_hit:
            details.append(f'imports {dotted_name}')
        group = 'named after it' if named else 'imports it'
        entries.append(entry(rank, group, test_rel,
                             f'python -m unittest {dotted(test_rel)}', '; '.join(details)))
    return entries


# --- dispatch, output, main ---------------------------------------------------

def cover(path, root):
    """((rel, language, entries), None) or (None, error message)."""
    rel = rel_of(path, root)
    if rel is None or not (root / rel).is_file():
        return None, f'mm cover: no such file in the project: {path}'
    if mm_where.is_secret(rel):
        return None, f'mm cover: never opens a secrets file: {rel}'
    suffix = Path(rel).suffix
    if suffix == '.rs':
        language, entries = 'rust', rust_entries(rel, root)
    elif suffix in ('.ts', '.tsx'):
        language, entries = 'ts', ts_entries(rel, root)
    elif suffix == '.py':
        language, entries = 'python', py_entries(rel, root)
    else:
        return None, f'mm cover: not a source file: {rel} (expected .rs, .ts, .tsx or .py)'
    entries.sort(key=lambda item: (item['rank'], item['path']))
    return (rel, language, entries), None


def render(rel, language, entries):
    lines = [f'cover {rel} ({language})']
    for e in entries:
        detail = f"  — {e['detail']}" if e['detail'] else ''
        lines.append(f"  [{e['group']}] {e['path']}{detail}")
        lines.append(f"      {e['command']}")
    return '\n'.join(lines)


def no_tests_text(rel):
    return '\n'.join([
        f'!!!! NO TESTS: nothing plausibly covers {rel} !!!!',
        '     checked Rust (an inline #[cfg(test)] mod, crates/*/tests, src-tauri/tests),',
        '           TypeScript (src/**/*.test.ts importing it, one hop through an index.ts),',
        '           Python (tests/test_<name>.py and tests importing the module)',
    ])


def main(argv=None, root=ROOT):
    parser = argparse.ArgumentParser(prog='mm cover',
                                     description='The tests that plausibly cover one source file, ranked.')
    parser.add_argument('path', help='a Rust, TypeScript or Python source file')
    parser.add_argument('--json', action='store_true', help='the findings as JSON')
    args = parser.parse_args(argv)
    root = Path(root)
    found, error = cover(args.path, root)
    if error:
        print(error)
        return 2
    rel, language, entries = found
    if args.json:
        print(json.dumps({'path': rel, 'language': language, 'tests': entries},
                         ensure_ascii=False, indent=1))
        if not entries:
            print(no_tests_text(rel), file=sys.stderr)
    elif entries:
        print(render(rel, language, entries))
    else:
        print(no_tests_text(rel))
    return 0 if entries else 1


if __name__ == '__main__':
    if hasattr(sys.stdout, 'reconfigure'):
        sys.stdout.reconfigure(encoding='utf-8', errors='replace')
    sys.exit(main())
