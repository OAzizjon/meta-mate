"""Regression tests for the project CLI, using dummy secrets only."""

import argparse
import contextlib
import io
import json
import os
import shutil
from pathlib import Path
import struct
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import mm
import mm_checks


class KeyTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name).resolve()
        self.source = self.root / 'source'
        self.source.mkdir()
        self.target = self.root / '.env'
        self.secret = 'dummy-private-value'
        (self.source / '.env').write_text(f'KEY="{self.secret}"\nKEY_LONG=long\n', encoding='utf-8')
        (self.root / 'env_keys_names.txt').write_text('KEY\nKEY_LONG\nKEY\n', encoding='utf-8')
        self.root_patch = patch.object(mm, 'ROOT', self.root)
        self.root_patch.start()
        self.addCleanup(self.root_patch.stop)

    def invoke(self, apply=True):
        stdout, stderr = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(stderr):
            result = mm.cmd_keys(argparse.Namespace(source=str(self.source), apply=apply))
        for value in (self.secret, 'existing-private-value'):
            self.assertNotIn(value, stdout.getvalue() + stderr.getvalue())
        return result

    def test_bom_crlf_export_quotes_and_prefix_names(self):
        self.target.write_bytes(b'\xef\xbb\xbfexport KEY_LONG=existing-private-value\r\nexport KEY=\r\n# tail\r\n')
        self.assertEqual(self.invoke(), 0)
        self.assertEqual(self.target.read_bytes(), b'\xef\xbb\xbfexport KEY_LONG=existing-private-value\r\nKEY="dummy-private-value"\r\n# tail\r\n')
        self.assertEqual(self.invoke(), 0)

    def test_duplicate_empty_assignments_are_all_filled(self):
        self.target.write_bytes(b'KEY=\nexport KEY=\n')
        self.assertEqual(self.invoke(), 0)
        self.assertEqual(self.target.read_text().count('KEY="' + self.secret + '"'), 2)

    def test_conflicting_duplicates_fail_without_changing_target(self):
        original = b'KEY=existing-private-value\nKEY=\n'
        self.target.write_bytes(original)
        self.assertEqual(self.invoke(), 1)
        self.assertEqual(self.target.read_bytes(), original)

    def test_invalid_utf8_does_not_leak(self):
        (self.source / '.env').write_bytes(b'KEY=' + self.secret.encode() + b'\xff')
        self.assertEqual(self.invoke(), 1)
        self.assertFalse(self.target.exists())

    def test_failed_replace_keeps_original_and_removes_temp(self):
        original = b'# untouched\r\n'
        self.target.write_bytes(original)
        with patch.object(mm.os, 'replace', side_effect=OSError(self.secret)):
            self.assertEqual(self.invoke(), 1)
        self.assertEqual(self.target.read_bytes(), original)
        self.assertEqual(list(self.root.glob('.mm-keys-*')), [])

    def test_failed_write_keeps_original_and_removes_temp(self):
        self.target.write_bytes(b'# original\n')
        with patch.object(mm.os, 'fsync', side_effect=OSError(self.secret)):
            self.assertEqual(self.invoke(), 1)
        self.assertEqual(self.target.read_bytes(), b'# original\n')
        self.assertEqual(list(self.root.glob('.mm-keys-*')), [])

    def test_target_link_is_refused_without_reading(self):
        with patch.object(mm, '_is_link', return_value=True), patch.object(mm, '_target_bytes') as read:
            self.assertEqual(self.invoke(), 1)
        read.assert_not_called()

    def test_real_symlink_does_not_change_destination(self):
        outside = self.root / 'outside'
        outside.write_bytes(b'# original\n')
        try:
            self.target.symlink_to(outside)
        except OSError:
            self.skipTest('Symlink creation needs Windows Developer Mode')
        self.assertEqual(self.invoke(), 1)
        self.assertEqual(outside.read_bytes(), b'# original\n')

    def test_multiline_quote_and_invalid_name_are_refused(self):
        (self.source / '.env').write_text('KEY="' + self.secret + '\nrest"\n', encoding='utf-8')
        self.assertEqual(self.invoke(), 1)
        (self.root / 'env_keys_names.txt').write_text('KEY=bad\n', encoding='utf-8')
        self.assertEqual(self.invoke(), 1)
        self.assertFalse(self.target.exists())

    def test_dry_run_does_not_write(self):
        self.assertEqual(self.invoke(apply=False), 0)
        self.assertFalse(self.target.exists())

    def test_source_bom_and_existing_quotes_are_preserved(self):
        (self.source / '.env').write_bytes(b'\xef\xbb\xbfexport KEY="dummy-private-value" # comment\r\n')
        self.target.write_bytes(b'KEY_LONG=\'existing-private-value\'')
        self.assertEqual(self.invoke(), 0)
        self.assertEqual(self.target.read_bytes(), b'KEY_LONG=\'existing-private-value\'\nKEY="dummy-private-value" # comment\n')

    def test_changed_target_is_not_overwritten(self):
        self.target.write_bytes(b'# original\n')
        with patch.object(mm, '_target_bytes', side_effect=[b'# original\n', b'# changed\n']):
            with patch.object(mm.os, 'replace') as replace:
                self.assertEqual(self.invoke(), 1)
        replace.assert_not_called()
        self.assertEqual(list(self.root.glob('.mm-keys-*')), [])


class CliTests(unittest.TestCase):
    def test_tts_tests_use_same_interpreter_when_present(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            for agent in (False, True):
                for present in (False, True):
                    tests = root / 'tts' / 'tests'
                    if present:
                        tests.mkdir(parents=True, exist_ok=True)
                    elif tests.exists():
                        tests.rmdir()
                    with self.subTest(agent=agent, present=present), \
                            patch.object(mm, 'ROOT', root), patch.object(mm, '_extend_path'), \
                            patch.object(mm.shutil, 'which', side_effect=lambda name: name), \
                            patch.object(mm.subprocess, 'call', return_value=0) as call, \
                            contextlib.redirect_stdout(io.StringIO()):
                        self.assertEqual(mm.cmd_test(argparse.Namespace(), agent=agent), 0)
                        self.assertEqual(call.call_count, 2 if present else 1)
                        if present:
                            self.assertEqual(call.call_args_list[1].args[0],
                                             [sys.executable, '-m', 'unittest', 'discover',
                                              '-s', 'tts/tests', '-t', 'tts'])

    def test_all_lead_commands_are_refused(self):
        for argv in (['uia'], ['art'], ['keys', 'dummy'], ['sounds', 'normalize'], ['agents'], ['doctor'],
                     ['probe'], ['probe', '--help'], ['probe', 'stream', 'hi'],
                     ['release'], ['release', '--dry-run'], ['release', '--allow-dirty'],
                     ['where', 'x'], ['cite', '-'], ['map'], ['stats']):
            with self.subTest(argv=argv), patch.object(mm, 'agent_mode_reason', return_value='test'), contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(mm.main(argv), 2)

    def test_code_tools_run_with_their_own_flags_also_for_agents(self):
        for argv, name, rest in ((['where', 'sign_out', '--all'], 'where', ['sign_out', '--all']),
                                 (['cite', '-C', '3', 'answer.md'], 'cite', ['-C', '3', 'answer.md']),
                                 (['agent', 'where', '--limit', '2', 'x'], 'where', ['--limit', '2', 'x']),
                                 (['agent', 'map', '--check'], 'map', ['--check']),
                                 (['stats', '--json'], 'stats', ['--json'])):
            agent = argv[0] == 'agent'
            with self.subTest(argv=argv), patch.object(mm, 'agent_mode_reason', return_value='test' if agent else None), \
                    patch.object(mm, '_run_tool', return_value=0) as run_tool:
                self.assertEqual(mm.main(argv), 0)
                self.assertEqual(run_tool.call_args.args, (name, rest))

    def test_agent_values_and_git_file(self):
        with tempfile.TemporaryDirectory() as folder, patch.object(mm, 'ROOT', Path(folder)), patch.dict(os.environ, {'MM_AGENT': '0'}), patch.object(Path, 'absolute', return_value=Path('C:/project')), patch.object(Path, 'resolve', return_value=Path('C:/project')):
            root = Path(folder)
            (root / '.git').write_text('gitdir: elsewhere', encoding='utf-8')
            self.assertTrue(mm.agent_mode_reason())
            (root / '.git').unlink()
            (root / '.git').mkdir()
            self.assertFalse(mm.agent_mode_reason())
            for value in ('1', ' TRUE ', 'yes', 'unexpected'):
                with patch.dict(os.environ, {'MM_AGENT': value}):
                    self.assertTrue(mm.agent_mode_reason())

    def test_sandbox_case_and_resolved_junction_path(self):
        with patch.object(mm, 'ROOT', Path('C:/DRAFT-AGENTS/SANDBOXES/copy')), patch.dict(os.environ, {'MM_AGENT': '0'}):
            self.assertIn('sandbox', mm.agent_mode_reason())
        with patch.object(mm, 'ROOT') as root, patch.dict(os.environ, {'MM_AGENT': '0'}):
            root.absolute.return_value = Path('C:/project')
            root.resolve.return_value = Path('C:/draft-agents/sandboxes/copy')
            self.assertIn('sandbox', mm.agent_mode_reason())

    def test_agent_tests_skip_npm_and_pass_offline_to_cargo(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            (root / 'package.json').write_text('{"scripts":{"test":"curl example.invalid"}}', encoding='utf-8')
            (root / 'Cargo.toml').touch()
            with patch.object(mm, 'ROOT', root), patch.object(mm, '_extend_path'), patch.object(mm, 'run', return_value=(127, '')), patch.object(mm.subprocess, 'call', return_value=0) as call, contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(mm.cmd_test(argparse.Namespace(), agent=True), 0)
            self.assertEqual(call.call_count, 2)
            self.assertIn('--offline', call.call_args_list[1].args[0])
            for invocation in call.call_args_list:
                self.assertEqual(invocation.kwargs['env']['CARGO_NET_OFFLINE'], 'true')
                self.assertEqual(invocation.kwargs['env']['MM_AGENT'], '1')

    def test_cargo_tests_are_offline_only_for_agents(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            (root / 'Cargo.toml').touch()
            for agent in (False, True):
                for nextest in (False, True):
                    with self.subTest(agent=agent, nextest=nextest), \
                            patch.object(mm, 'ROOT', root), patch.object(mm, '_extend_path'), \
                            patch.object(mm, 'run', return_value=(0 if nextest else 127, '')) as probe, \
                            patch.object(mm.shutil, 'which', side_effect=lambda name: name), \
                            patch.object(mm.subprocess, 'call', return_value=0) as call, \
                            contextlib.redirect_stdout(io.StringIO()):
                        self.assertEqual(mm.cmd_test(argparse.Namespace(), agent=agent), 0)
                        if agent:
                            probe.assert_not_called()
                            self.assertEqual(call.call_count, 2)
                            self.assertEqual(call.call_args_list[1].args[0],
                                             ['cargo', 'test', '-p', 'mm-core', '-p', 'mm-platform', '--offline'])
                            self.assertEqual(call.call_args_list[1].kwargs['env']['CARGO_BUILD_JOBS'], '2')
                            self.assertTrue(call.call_args_list[1].kwargs['env']['CARGO_TARGET_DIR'])
                            continue
                        expected = ['cargo', 'nextest', 'run'] if nextest else ['cargo', 'test']
                        if agent:
                            expected.append('--offline')
                        if not nextest:
                            expected.append('-q')
                        # nextest skips doctests, so a `cargo test --doc` step follows it.
                        self.assertEqual(call.call_count, 3 if nextest else 2)
                        self.assertEqual(call.call_args_list[1].args[0], expected)
                        if nextest:
                            self.assertEqual(call.call_args_list[2].args[0],
                                             ['cargo', 'test', '--doc', *(['--offline'] if agent else []), '-q'])
                        for invocation in [probe.call_args, *call.call_args_list]:
                            env = invocation.kwargs['env']
                            if agent:
                                self.assertEqual(env['CARGO_NET_OFFLINE'], 'true')
                                self.assertEqual(env['NPM_CONFIG_OFFLINE'], 'true')
                                self.assertEqual(env['MM_AGENT'], '1')
                                self.assertEqual(env['RUSTC_WRAPPER'], '')
                            else:
                                self.assertIsNone(env)

    def test_agent_checks_exclude_package_and_build_scripts(self):
        self.assertTrue(mm.AGENT_SAFE_CHECKS.isdisjoint({'web', 'rust'}))
        with patch.object(mm, '_extend_path'), patch.object(mm, '_load_checks'), contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(mm.cmd_check(argparse.Namespace(only='web'), agent=True), 2)

    def test_plan_heading_and_invalid_day(self):
        sections = mm.plan_sections('## Day 1 — 26.10\nhello\n## Other\nignored\n')
        self.assertEqual(sections[1][0].day, 26)
        self.assertEqual(sections[1][1], ['## Day 1 — 26.10', 'hello'])
        with patch.object(mm, '_plan_file', return_value=Path(__file__)), contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(mm.cmd_plan(argparse.Namespace(day='oops')), 1)

    def test_png_requires_complete_ihdr(self):
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / 'art.png'
            path.write_bytes(b'\x89PNG\r\n\x1a\n')
            self.assertIsNone(mm._png_size(path))
            path.write_bytes(b'\x89PNG\r\n\x1a\n' + struct.pack('>I', 13) + b'IHDR' + struct.pack('>II', 200, 300))
            self.assertEqual(mm._png_size(path), (200, 300))

    def test_art_refuses_wrong_size_or_failed_edge(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            src = root / 'assets/art/src'
            src.mkdir(parents=True)
            (src / 'robot.html').write_text('<!-- size: 200x300 transparent -->', encoding='utf-8')
            edge = root / 'edge.exe'
            edge.touch()
            def render(cmd, **kwargs):
                self.assertIn('--headless=new', cmd)
                self.assertTrue(any(c.startswith('--user-data-dir=') for c in cmd))
                (src.parent / 'robot.png').touch()
                return 0, ''
            with patch.object(mm, 'ROOT', root), patch.object(mm, 'EDGE_PATHS', [edge]), patch.object(mm, 'run', side_effect=render), patch.object(mm, '_png_size', return_value=(1, 1)), contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(mm.cmd_art(argparse.Namespace(names=[])), 1)

    def test_run_resolves_executable_and_handles_timeout(self):
        with patch.object(mm.shutil, 'which', return_value='C:/bin/npm.cmd'), patch.object(mm.subprocess, 'run', side_effect=subprocess.TimeoutExpired('npm', 3)) as run:
            self.assertEqual(mm.run(['npm', '--version'], timeout=3)[0], 124)
        self.assertEqual(run.call_args.args[0][0], 'C:/bin/npm.cmd')
        self.assertEqual(run.call_args.kwargs['encoding'], 'utf-8')

    def test_allowed_rejects_junctions_and_case_variants(self):
        root = Path('C:/project')
        with patch.object(Path, 'is_junction', return_value=True):
            self.assertFalse(mm_checks._allowed(root, root / 'linked/file.py'))
        self.assertFalse(mm_checks._allowed(root, root / '.DRAFT-INPUT/file.py'))
        self.assertFalse(mm_checks._allowed(root, root / '../outside.py'))

    def test_sound_normalize_only_converts_missing_or_older_outputs(self):
        for kind in ('cues', 'sfx', 'amb'):
            for state, output_time, force, converted in (
                ('missing', None, False, True),
                ('newer', 200, False, False),
                ('equal', 100, False, False),
                ('older', 50, False, True),
                ('forced', 200, True, True),
            ):
                with self.subTest(kind=kind, state=state), tempfile.TemporaryDirectory() as folder:
                    root = Path(folder)
                    base = root / 'assets/sounds'
                    source = base / 'src' / kind / 'beep.wav'
                    source.parent.mkdir(parents=True)
                    source.touch()
                    os.utime(source, (100, 100))
                    output = base / kind / 'beep.ogg'
                    if output_time is not None:
                        output.parent.mkdir(parents=True)
                        output.write_bytes(b'existing audio')
                        os.utime(output, (output_time, output_time))
                    argv = ['sounds', 'normalize'] + (['--force'] if force else [])
                    args = mm.build_parser().parse_args(argv)
                    stdout = io.StringIO()
                    with patch.object(mm, 'ROOT', root), patch.object(mm, '_extend_path'), patch.object(mm, '_licensed_names', return_value={'beep.wav'}), patch.object(mm.shutil, 'which', return_value='ffmpeg'), patch.object(mm, 'run', return_value=(0, '')) as run, contextlib.redirect_stdout(stdout):
                        self.assertEqual(mm.cmd_sounds(args), 0)
                    if converted:
                        run.assert_called_once()
                        cmd = run.call_args.args[0]
                        self.assertEqual(cmd[cmd.index('-i') + 1], str(source))
                        self.assertEqual(cmd[-1], str(output))
                        self.assertNotIn('SKIP', stdout.getvalue())
                    else:
                        run.assert_not_called()
                        self.assertEqual(stdout.getvalue(), f'SKIP  {"beep.wav":<22} up to date\n')
                        self.assertEqual(output.read_bytes(), b'existing audio')
                        self.assertEqual(output.stat().st_mtime, output_time)

    def test_sound_downmix_precedes_loudnorm_and_output_collisions_fail(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            src = root / 'assets/sounds/src'
            src.mkdir(parents=True)
            (src / 'beep.wav').touch()
            with patch.object(mm, 'ROOT', root), patch.object(mm, '_extend_path'), patch.object(mm, '_licensed_names', return_value={'beep.wav', 'beep.mp3'}), patch.object(mm.shutil, 'which', return_value='ffmpeg'), patch.object(mm, 'run', return_value=(0, '')) as run, contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(mm.cmd_sounds(argparse.Namespace(action='normalize')), 0)
                cmd = run.call_args.args[0]
                self.assertIn('-nostdin', cmd)
                self.assertIn('-vn', cmd)
                self.assertEqual(cmd[cmd.index('-af') + 1], 'aformat=channel_layouts=mono,loudnorm=I=-16:TP=-1.5:LRA=11')
                (src / 'beep.mp3').touch()
                run.reset_mock()
                self.assertEqual(mm.cmd_sounds(argparse.Namespace(action='normalize')), 1)
                run.assert_not_called()


class ReleaseTests(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.root = Path(directory.name)
        self.target_dir = self.root / 'custom target'
        self.installer = (self.target_dir / 'x86_64-pc-windows-msvc/release/bundle/nsis'
                          / 'MetaMate_0.1.0_x64-setup.exe')
        self.args = argparse.Namespace(allow_dirty=False, dry_run=False)
        self.output = io.StringIO()
        release_env = {'METAMATE_PROXY_URL': 'https://proxy.example', 'METAMATE_INVITE': 'demo-code-123'}
        for patcher in (patch.object(mm, 'ROOT', self.root),
                        patch.dict(mm.os.environ, release_env),
                        patch.object(mm, 'cmd_check', return_value=0),
                        patch.object(mm, 'run', side_effect=self.command),
                        contextlib.redirect_stdout(self.output)):
            result = patcher.__enter__()
            self.addCleanup(patcher.__exit__, None, None, None)
            if hasattr(result, 'assert_called_once'):
                if patcher.attribute == 'run':
                    self.run = result
                else:
                    self.check = result
        self.git = (0, '')
        self.build_code = 0
        self.produce = True

    def command(self, cmd, **kwargs):
        self.assertEqual(kwargs['cwd'], self.root)
        if cmd[0] == 'git':
            return self.git
        if cmd[0] == 'cargo':
            return 0, json.dumps({'target_directory': str(self.target_dir)})
        if 'tauri' in cmd and self.produce and self.build_code == 0:
            self.installer.parent.mkdir(parents=True, exist_ok=True)
            self.installer.write_bytes(b'abc')
        return self.build_code, ''

    def write_prod_proxy(self, contents):
        server = self.root / 'server'
        server.mkdir(exist_ok=True)
        (server / '.prod-proxy.json').write_text(contents, encoding='utf-8')

    def test_release_needs_the_built_in_proxy_and_invite(self):
        for env in ({'METAMATE_PROXY_URL': '', 'METAMATE_INVITE': 'demo-code-123'},
                    {'METAMATE_PROXY_URL': 'http://proxy.example', 'METAMATE_INVITE': 'demo-code-123'},
                    {'METAMATE_PROXY_URL': 'https://proxy.example', 'METAMATE_INVITE': ' '}):
            with self.subTest(env=env), patch.dict(mm.os.environ, env):
                self.assertEqual(mm.cmd_release(self.args), 1)
        self.assertNotIn('demo-code-123', self.output.getvalue())
        self.run.assert_not_called()

    def test_release_uses_proxy_file_when_environment_is_unset(self):
        invite = 'file-only-invite'
        self.write_prod_proxy(json.dumps({'url': 'https://file-proxy.example', 'invite': invite}))
        with patch.dict(mm.os.environ, {}, clear=True):
            self.assertEqual(mm.cmd_release(self.args), 0)
            self.assertEqual(mm.os.environ['METAMATE_PROXY_URL'], 'https://file-proxy.example')
            self.assertEqual(mm.os.environ['METAMATE_INVITE'], invite)
        self.assertNotIn('Release refused: set METAMATE_PROXY_URL', self.output.getvalue())
        self.assertNotIn(invite, self.output.getvalue())

    def test_release_proxy_file_does_not_override_environment(self):
        invite = 'file-only-invite'
        self.write_prod_proxy(json.dumps({'url': 'https://file-proxy.example', 'invite': invite}))
        with patch.dict(mm.os.environ, {'METAMATE_PROXY_URL': 'https://env-proxy.example',
                                        'METAMATE_INVITE': 'environment-invite'}):
            self.assertEqual(mm.cmd_release(self.args), 0)
            self.assertEqual(mm.os.environ['METAMATE_PROXY_URL'], 'https://env-proxy.example')
            self.assertEqual(mm.os.environ['METAMATE_INVITE'], 'environment-invite')
        self.assertNotIn(invite, self.output.getvalue())

    def test_release_refuses_invalid_proxy_file_without_printing_invite(self):
        invite = 'file-only-invite'
        invalid_files = ('{broken json', '[]',
                         json.dumps({'url': 'https://file-proxy.example'}),
                         json.dumps({'invite': invite}))
        for contents in invalid_files:
            with self.subTest(contents=contents), patch.dict(mm.os.environ, {}, clear=True):
                self.write_prod_proxy(contents)
                self.assertEqual(mm.cmd_release(self.args), 1)
                self.assertIn('Release refused: set METAMATE_PROXY_URL', self.output.getvalue())
                self.assertNotIn(invite, self.output.getvalue())
                self.run.assert_not_called()
                self.check.assert_not_called()

    def test_release_build_order_and_installer_report(self):
        self.assertEqual(mm.cmd_release(self.args), 0)
        self.check.assert_called_once()
        commands = [call.args[0] for call in self.run.call_args_list]
        self.assertEqual(commands[0], ['git', 'status', '--porcelain', '--untracked-files=all'])
        self.assertEqual(commands[2][1:], ['run', 'build'])
        self.assertEqual(commands[3][1:7], ['tauri', 'build', '--bundles', 'nsis', '--target',
                                          'x86_64-pc-windows-msvc'])
        self.assertEqual(json.loads(commands[3][-1]), {'build': {'beforeBuildCommand': ''}})
        self.assertIn(str(self.installer), self.output.getvalue())
        self.assertIn('Size: 3 bytes', self.output.getvalue())
        self.assertIn('SHA-256: ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad',
                      self.output.getvalue())

    def test_failed_check_stops_before_commands_even_with_allow_dirty(self):
        self.check.return_value = 1
        self.args.allow_dirty = True
        self.assertEqual(mm.cmd_release(self.args), 1)
        self.run.assert_not_called()

    def test_dirty_tree_refused_and_override_builds(self):
        self.git = (0, '?? untracked.txt')
        self.assertEqual(mm.cmd_release(self.args), 1)
        self.assertEqual(self.run.call_count, 1)
        self.args.allow_dirty = True
        self.assertEqual(mm.cmd_release(self.args), 0)

    def test_failed_git_status_refused_even_with_override(self):
        self.git = (127, '')
        self.args.allow_dirty = True
        self.assertEqual(mm.cmd_release(self.args), 1)
        self.assertEqual(self.run.call_count, 1)

    def test_failed_build_stops_before_tauri(self):
        self.build_code = 1
        self.assertEqual(mm.cmd_release(self.args), 1)
        self.assertEqual(self.run.call_count, 3)

    def test_failed_tauri_does_not_report_existing_installer(self):
        self.run.side_effect = [(0, ''), (0, json.dumps({'target_directory': str(self.target_dir)})),
                                (0, ''), (1, 'bundling failed')]
        self.assertEqual(mm.cmd_release(self.args), 1)
        self.assertNotIn('SHA-256:', self.output.getvalue())

    def test_missing_and_stale_installer_refused(self):
        self.produce = False
        self.assertEqual(mm.cmd_release(self.args), 1)
        self.installer.parent.mkdir(parents=True)
        self.installer.write_bytes(b'old installer')
        self.assertEqual(mm.cmd_release(self.args), 1)
        self.assertNotIn('SHA-256:', self.output.getvalue())

    def test_invalid_metadata_stops_before_build(self):
        self.run.side_effect = [(0, ''), (0, 'invalid json')]
        self.assertEqual(mm.cmd_release(self.args), 1)
        self.assertEqual(self.run.call_count, 2)

    def test_dry_run_prints_steps_without_checks_or_commands(self):
        self.args = mm.build_parser().parse_args(['release', '--dry-run', '--allow-dirty'])
        self.assertEqual(mm.cmd_release(self.args), 0)
        self.run.assert_not_called()
        self.check.assert_not_called()
        for text in ('mm check', 'git status', 'run build', 'tauri build', 'SHA-256'):
            self.assertIn(text, self.output.getvalue())


class HookTests(unittest.TestCase):
    def setUp(self):
        git = shutil.which('git')
        if not git:
            self.skipTest('Git is not installed')
        if os.name == 'nt':
            candidates = [Path(git).parent.parent / 'bin/bash.exe', Path(git).parent / 'bash.exe',
                          Path(git).parent.parent.parent / 'usr/bin/bash.exe']
            self.bash = next((str(p) for p in candidates if p.exists()), None)
        else:
            self.bash = shutil.which('bash')
        if not self.bash:
            self.skipTest('Git Bash is not installed')
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.git = git
        self.hook = Path(__file__).resolve().parents[1] / 'git-hooks/pre-commit'
        self.command('init', '-q')

    def command(self, *args):
        return subprocess.run([self.git, *args], cwd=self.root, capture_output=True, check=True)

    def stage(self, name, content):
        path = self.root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content, encoding='utf-8')
        self.command('add', '-f', '--', name)

    def invoke(self, **extra):
        env = dict(os.environ, MM_FIXTURES_REVIEWED='0', **extra)
        return subprocess.run([self.bash, self.hook.as_posix()], cwd=self.root,
                              env=env, capture_output=True, timeout=30)

    def test_blocks_secret_filename_case_insensitively(self):
        self.stage('.ENV', 'dummy-private-value')
        result = self.invoke()
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertNotIn(b'dummy-private-value', result.stdout + result.stderr)

    def test_blocks_credential_without_printing_any_prefix(self):
        secret = 'sk-' + 'a' * 18
        self.stage('code.txt', secret)
        result = self.invoke()
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertNotIn(secret.encode(), result.stdout + result.stderr)
        self.assertNotIn(b'sk-aaaa', result.stdout + result.stderr)

    def test_blocks_generic_quoted_token(self):
        self.stage('code.txt', "SERVICE_TOKEN = '" + 'a' * 40 + "'\n")
        self.assertEqual(self.invoke().returncode, 1)

    def test_allows_placeholder_example(self):
        self.stage('.env.example', 'SERVICE_TOKEN=replace-me\n')
        self.assertEqual(self.invoke().returncode, 0)

    def test_fixture_requires_review(self):
        self.stage('fixtures/uia/test.json', '{}')
        self.assertEqual(self.invoke().returncode, 1)
        env = dict(os.environ, MM_FIXTURES_REVIEWED='1')
        result = subprocess.run([self.bash, self.hook.as_posix()], cwd=self.root,
                                env=env, capture_output=True, timeout=30)
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_git_bash_wrapper_accepts_absolute_posix_path(self):
        if os.name != 'nt':
            self.skipTest('Windows MSYS path conversion regression')
        project = self.root / 'project with spaces'
        (project / 'tools').mkdir(parents=True)
        wrapper = Path(__file__).resolve().parents[2] / 'mm'
        shutil.copyfile(wrapper, project / 'mm')
        (project / 'tools/mm.py').write_text("print('wrapper-ok')\n", encoding='utf-8')
        launcher = self.root / 'launchers'
        launcher.mkdir()
        python = Path(sys.executable).as_posix().replace("'", "'\"'\"'")
        (launcher / 'py').write_text(f"#!/bin/sh\nshift\nexec '{python}' \"$@\"\n", encoding='utf-8', newline='\n')
        env = dict(os.environ)
        env.pop('MSYS_NO_PATHCONV', None)
        env['PATH'] = str(launcher) + os.pathsep + env.get('PATH', '')
        command = 'exec bash "$(cygpath -u "$1")" demo'
        result = subprocess.run([self.bash, '-c', command, 'wrapper-test', (project / 'mm').as_posix()],
                                cwd=self.root, env=env, capture_output=True, timeout=30)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn(b'wrapper-ok', result.stdout)


class PowerShellTests(unittest.TestCase):
    def test_uia_script_parses_in_windows_powershell(self):
        powershell = shutil.which('powershell.exe')
        if not powershell:
            self.skipTest('Windows PowerShell is unavailable')
        script = Path(__file__).resolve().parents[1] / 'uia_dump.ps1'
        env = dict(os.environ, MM_UIA_SCRIPT=str(script))
        command = "$tokens=$null; $errors=$null; [void][System.Management.Automation.Language.Parser]::ParseFile($env:MM_UIA_SCRIPT,[ref]$tokens,[ref]$errors); if ($errors.Count) { $errors | ForEach-Object { $_.Message }; exit 1 }"
        result = subprocess.run([powershell, '-NoProfile', '-NonInteractive', '-Command', command],
                                env=env, capture_output=True, timeout=30)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_uia_script_is_ascii(self):
        # No BOM: Windows PowerShell 5.1 reads the file as ANSI, so other characters are escapes.
        data = (Path(__file__).resolve().parents[1] / 'uia_dump.ps1').read_bytes()
        self.assertEqual([i for i, b in enumerate(data) if b > 127][:5], [])

    def test_uia_password_words_self_test(self):
        powershell = shutil.which('powershell.exe')
        if not powershell:
            self.skipTest('Windows PowerShell is unavailable')
        script = Path(__file__).resolve().parents[1] / 'uia_dump.ps1'
        result = subprocess.run([powershell, '-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass',
                                 '-File', str(script), '-SelfTest'], capture_output=True, timeout=60)
        output = (result.stdout + result.stderr).decode('utf-8', errors='replace')
        self.assertEqual(result.returncode, 0, output)
        self.assertIn('self-test OK', output)


if __name__ == '__main__':
    unittest.main()
