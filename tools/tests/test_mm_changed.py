"""Changed-file plans and sandbox Rust settings, without Git or a Rust toolchain."""

import contextlib
import io
import os
from pathlib import Path
import sys
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import mm
import mm_changed


class MappingTests(unittest.TestCase):
    graph = {'mm-core': set(), 'mm-platform': {'mm-core'},
             'mm-win32': {'mm-platform'}, 'metamate': {'mm-core', 'mm-win32'}}

    def test_reverse_closure_includes_transitive_dependents(self):
        self.assertEqual(mm_changed.reverse_closure({'mm-core'}, self.graph), set(self.graph))
        self.assertEqual(mm_changed.reverse_closure({'mm-platform'}, self.graph),
                         {'mm-platform', 'mm-win32', 'metamate'})
        self.assertEqual(mm_changed.reverse_closure(set(), self.graph), set())

    def test_closure_handles_cycles_diamonds_and_unknown_packages(self):
        graph = {'a': {'b'}, 'b': {'a'}, 'c': {'a', 'b'}, 'unrelated': set()}
        self.assertEqual(mm_changed.reverse_closure({'a'}, graph), {'a', 'b', 'c'})
        self.assertEqual(mm_changed.reverse_closure({'new'}, graph), {'new'})

    def test_workspace_graph_uses_paths_and_excludes_external_packages(self):
        root = Path(__file__).resolve().parent
        metadata = {'workspace_members': ['core-id', 'app-id'], 'packages': [
            {'id': 'core-id', 'name': 'core', 'manifest_path': str(root / 'core/Cargo.toml'),
             'dependencies': []},
            {'id': 'app-id', 'name': 'app', 'manifest_path': str(root / 'app/Cargo.toml'),
             'dependencies': [
                 {'name': 'core', 'rename': 'alias', 'path': str(root / 'core'), 'kind': 'dev'},
                 {'name': 'external', 'path': str(root / 'external'), 'kind': 'build'},
                 {'name': 'serde', 'path': None}]},
            {'id': 'external-id', 'name': 'external',
             'manifest_path': str(root / 'external/Cargo.toml'), 'dependencies': []}]}
        self.assertEqual(mm_changed.workspace_graph(metadata), {'core': set(), 'app': {'core'}})

    def test_file_mapping(self):
        cases = {
            'crates/mm-core/src/lib.rs': {'rust:' + name for name in self.graph},
            'src-tauri/tauri.conf.json': {'rust:metamate'},
            'src/player.ts': {'web'}, 'mate.html': {'web'}, 'package.json': {'web'},
            'locales/ru.json': {'web', 'locales'}, 'tools/mm.py': {'tools'},
            'server/metamate_proxy/policy.py': {'server'}, 'tts/text_norm.py': {'tts'},
            'docs/architecture.md': {'docs'}, 'README.md': {'docs'},
            'crates/mm-core/README.md': {'docs'}, 'docs/example.json': {'docs'},
            'Cargo.lock': {'rust:' + name for name in self.graph},
        }
        for filename, expected in cases.items():
            with self.subTest(filename=filename):
                self.assertEqual(mm_changed.affected_steps([filename], self.graph), expected)

    def test_plan_deduplicates_and_includes_locale_tests(self):
        plan = mm_changed.build_plan(['locales/en.json', 'locales/ru.json'], {})
        self.assertEqual([name for name, _ in plan], ['locales', 'web'])
        self.assertIn('test_locales.py', plan[0][1])
        self.assertEqual(mm_changed.affected_steps(['locales/en.json', 'tools/mm.py'], {}),
                         {'tools', 'web'})

    def test_docs_only_and_empty_plans(self):
        plan = mm_changed.build_plan(['README.md', 'docs/test.md'], {})
        self.assertEqual(len(plan), 1)
        self.assertEqual(plan[0][1][-3:], ['check', '--only', 'md-links,typos'])
        self.assertEqual(mm_changed.build_plan([], {}), [])

    def test_rust_plan_uses_each_affected_package_once(self):
        plan = mm_changed.build_plan(['crates/mm-core/src/lib.rs', 'crates/mm-platform/src/lib.rs'],
                                     self.graph)
        self.assertEqual([cmd for _, cmd in plan],
                         [['cargo', 'test', '-p', name] for name in sorted(self.graph)])


class CommandTests(unittest.TestCase):
    def test_changed_files_includes_untracked_and_uses_requested_base(self):
        with patch.object(mm, 'run', side_effect=[(0, 'src/a.ts\0deleted file.py\0'),
                                                  (0, 'src/a.ts\0tools/new.py\0')]) as run:
            files = mm_changed.changed_files('main', run)
        self.assertEqual(files, ['deleted file.py', 'src/a.ts', 'tools/new.py'])
        self.assertIn('main', run.call_args_list[0].args[0])
        self.assertIn('--no-renames', run.call_args_list[0].args[0])
        self.assertEqual(run.call_args_list[1].args[0],
                         ['git', 'ls-files', '--others', '--exclude-standard', '-z'])

    def test_dry_run_does_not_execute_plan_or_query_unneeded_metadata(self):
        args = mm.build_parser().parse_args(['test', '--changed', '--dry-run'])
        output = io.StringIO()
        with patch.object(mm, 'run', side_effect=[(0, 'tools/new.py\0'), (0, '')]) as run, \
                patch.object(mm_changed.subprocess, 'call') as call, \
                contextlib.redirect_stdout(output):
            self.assertEqual(mm_changed.cmd_changed(args, mm.ROOT, run), 0)
        call.assert_not_called()
        self.assertEqual(run.call_count, 2)
        self.assertIn('will run:', output.getvalue())
        self.assertIn('tools/tests', output.getvalue())

    def test_dry_run_without_changed_still_only_prints_a_plan(self):
        for command in ('check', 'test'):
            args = mm.build_parser().parse_args([command, '--dry-run'])
            with self.subTest(command=command), patch.object(mm, '_extend_path'), \
                    patch.object(mm, 'run', side_effect=[(0, 'README.md\0'), (0, '')]), \
                    patch.object(mm_changed.subprocess, 'call') as call, \
                    contextlib.redirect_stdout(io.StringIO()) as output:
                self.assertEqual(getattr(mm, 'cmd_' + command)(args), 0)
                call.assert_not_called()
                self.assertIn('md-links,typos', output.getvalue())

    def test_metadata_and_dependency_plan_are_used_for_both_commands(self):
        metadata = '{"workspace_members": ["id"], "packages": [' \
                   '{"id": "id", "name": "mm-core", "manifest_path": "/core/Cargo.toml"}]}'
        for command in ('check', 'test'):
            args = mm.build_parser().parse_args([command, '--changed', '--base', 'main', '--dry-run'])
            with self.subTest(command=command), patch.object(mm, '_extend_path'), \
                    patch.object(mm, 'run', side_effect=[(0, 'crates/mm-core/src/lib.rs\0'),
                                                        (0, ''), (0, metadata)]) as run, \
                    contextlib.redirect_stdout(io.StringIO()) as output:
                self.assertEqual(getattr(mm, 'cmd_' + command)(args), 0)
                self.assertIn('cargo test -p mm-core', output.getvalue())
                self.assertEqual(run.call_args.args[0],
                                 ['cargo', 'metadata', '--no-deps', '--format-version', '1'])

    def test_discovery_failures_do_not_run_tests(self):
        args = mm.build_parser().parse_args(['test', '--changed'])
        for results in ([(1, 'bad ref')], [(0, 'crates/mm-core/x\0'), (0, ''), (1, 'metadata error')]):
            with self.subTest(results=results), patch.object(mm, 'run', side_effect=results) as run, \
                    patch.object(mm_changed.subprocess, 'call') as call, \
                    contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(mm_changed.cmd_changed(args, mm.ROOT, run), 1)
                call.assert_not_called()

    def test_execution_preserves_build_jobs_and_reports_failure(self):
        args = mm.build_parser().parse_args(['test', '--changed'])
        with patch.object(mm, 'run', side_effect=[(0, 'crates/mm-core/x\0'), (0, ''),
                                                 (0, '{"workspace_members": [], "packages": []}')]) as run, \
                patch.dict(os.environ, {'CARGO_BUILD_JOBS': '7'}), \
                patch.object(mm_changed.subprocess, 'call', return_value=1) as call, \
                contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(mm_changed.cmd_changed(args, mm.ROOT, run), 1)
            self.assertEqual(os.environ['CARGO_BUILD_JOBS'], '7')
            self.assertNotIn('env', call.call_args.kwargs)

    def test_agent_target_and_explicit_override(self):
        with patch.object(mm_changed.tempfile, 'gettempdir', return_value='/writable'):
            env = mm_changed.agent_rust_env({'CARGO_BUILD_JOBS': '9'})
            self.assertEqual(env['CARGO_TARGET_DIR'], str(Path('/writable/r')))
            self.assertEqual(env['CARGO_BUILD_JOBS'], '2')
            env = mm_changed.agent_rust_env({'CARGO_TARGET_DIR': '/custom'})
            self.assertEqual(env['CARGO_TARGET_DIR'], '/custom')


if __name__ == '__main__':
    unittest.main()
