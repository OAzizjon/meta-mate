"""Changed-file test plans and the lightweight agent Rust configuration (stdlib only)."""

import json
import os
from pathlib import Path, PurePosixPath
import shutil
import subprocess
import sys
import tempfile


def agent_rust_env(env):
    """Avoid the protected project target and keep MSVC output paths short."""
    env = dict(env)
    env['CARGO_BUILD_JOBS'] = '2'
    # The agent launcher supplies a writable TEMP. Do not build under the deeper
    # project tree, or use a global C:/ folder that the sandbox cannot write.
    env.setdefault('CARGO_TARGET_DIR', str(Path(tempfile.gettempdir()) / 'r'))
    return env


def workspace_graph(metadata):
    """Return package -> workspace path dependencies, including dev/build edges."""
    members = set(metadata['workspace_members'])
    packages = [p for p in metadata['packages'] if p['id'] in members]
    paths = {Path(p['manifest_path']).parent.resolve(): p['name'] for p in packages}
    graph = {p['name']: set() for p in packages}
    for package in packages:
        for dependency in package.get('dependencies', []):
            path = dependency.get('path')
            name = paths.get(Path(path).resolve()) if path else None
            if name:
                graph[package['name']].add(name)
    return graph


def reverse_closure(changed, graph):
    """Include changed packages and every transitive workspace dependent."""
    selected = set(changed)
    reverse = {}
    for package, dependencies in graph.items():
        for dependency in dependencies:
            reverse.setdefault(dependency, set()).add(package)
    pending = list(selected)
    while pending:
        for dependent in reverse.get(pending.pop(), ()):
            if dependent not in selected:
                selected.add(dependent)
                pending.append(dependent)
    return selected


def affected_steps(files, graph):
    """Map repo-relative paths to logical steps; Rust steps are rust:<package>."""
    steps, packages = set(), set()
    for filename in files:
        path = PurePosixPath(filename.replace('\\', '/'))
        parts = path.parts
        if not parts:
            continue
        top = parts[0]
        # Documentation inside a crate or tools does not change executable code.
        if top == 'docs' or path.suffix.lower() == '.md':
            steps.add('docs')
        elif top == 'crates' and len(parts) > 1:
            packages.add(parts[1])
        elif top == 'src-tauri':
            packages.add('metamate')
        elif top in ('src', 'locales') or path.suffix == '.html' or str(path) == 'package.json':
            steps.add('web')
            if top == 'locales':
                steps.add('locales')
        elif top in ('tools', 'server', 'tts'):
            steps.add(top)
        elif str(path) in ('Cargo.toml', 'Cargo.lock') or top == '.cargo':
            packages.update(graph)
    steps.update('rust:' + name for name in reverse_closure(packages, graph))
    # Full tools discovery already includes the locale tests.
    if 'tools' in steps:
        steps.discard('locales')
    return steps


def changed_files(base, run):
    """Include staged, unstaged, deleted and untracked paths, without Git quoting."""
    files = set()
    for command in (['git', 'diff', '--name-only', '--no-renames', '-z', '--end-of-options', base, '--'],
                    ['git', 'ls-files', '--others', '--exclude-standard', '-z']):
        code, output = run(command)
        if code:
            raise ValueError(f'{" ".join(command)} failed: {output}')
        files.update(p for p in output.split('\0') if p)
    return sorted(files)


def build_plan(files, graph):
    """Return ordered (label, argv) steps; no subprocesses or filesystem access."""
    selected = affected_steps(files, graph)
    plan = []
    if 'docs' in selected:
        plan.append(('docs', [sys.executable, 'tools/mm.py', 'check', '--only', 'md-links,typos']))
    for label, directory in (('tools', 'tools'), ('locales', 'tools'), ('server', 'server'), ('tts', 'tts')):
        if label not in selected:
            continue
        command = [sys.executable, '-m', 'unittest', 'discover', '-s', directory + '/tests']
        command += ['-p', 'test_locales.py', '-v'] if label == 'locales' else ['-v']
        if directory in ('server', 'tts'):
            command += ['-t', directory]
        plan.append((label, command))
    for name in sorted(s[5:] for s in selected if s.startswith('rust:')):
        plan.append(('rust:' + name, ['cargo', 'test', '-p', name]))
    if 'web' in selected:
        plan.append(('web', ['npm.cmd' if os.name == 'nt' else 'npm', 'test']))
    return plan


def cmd_changed(args, root, run):
    """Discover and print a plan before executing it; discovery is read-only."""
    if getattr(args, 'only', None):
        print('--only cannot be combined with --changed')
        return 2
    try:
        files = changed_files(getattr(args, 'base', 'HEAD'), run)
        graph = {}
        if any(s.startswith('rust:') for s in affected_steps(files, {})) or any(
                p in ('Cargo.toml', 'Cargo.lock') or p.startswith('.cargo/') for p in files):
            code, output = run(['cargo', 'metadata', '--no-deps', '--format-version', '1'])
            if code:
                raise ValueError('cargo metadata failed: ' + output)
            graph = workspace_graph(json.loads(output))
        plan = build_plan(files, graph)
    except (OSError, ValueError, KeyError, TypeError) as error:
        print(f'Changed-file plan failed: {error}')
        return 1
    print('will run: ' + ('; '.join(subprocess.list2cmdline(cmd) for _, cmd in plan) or 'nothing'), flush=True)
    if getattr(args, 'dry_run', False):
        return 0
    failed = False
    for label, command in plan:
        print(f'\n=== {label}', flush=True)
        command = [shutil.which(command[0]) or command[0], *command[1:]]
        try:
            code = subprocess.call(command, cwd=root, stdin=subprocess.DEVNULL, timeout=600)
        except (OSError, subprocess.TimeoutExpired) as error:
            print(f'{label}: {error}')
            code = 1
        failed |= code != 0
    return int(failed)
