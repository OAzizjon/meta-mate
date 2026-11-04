"""mm ship: what it builds and what it refuses; nothing here deploys."""

import contextlib
import hashlib
import io
import json
import os
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import mm_ship  # noqa: E402


class BuildSite(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        (self.root / 'site/images').mkdir(parents=True)
        (self.root / 'site/index.html').write_text('<p>hi</p>', encoding='utf-8')
        (self.root / 'site/README.md').write_text('notes', encoding='utf-8')
        (self.root / 'site/_headers').write_text('/*', encoding='utf-8')
        self.installer = self.root / 'setup.exe'
        self.installer.write_bytes(b'installer')
        self.digest = hashlib.sha256(b'installer').hexdigest()

    def manifest(self, digest):
        (self.root / 'site/version.json').write_text(json.dumps({'version': '1.0.0', 'sha256': digest}), encoding='utf-8')

    def test_copies_the_site_and_the_installer(self):
        self.manifest(self.digest)
        out = self.root / 'dist'
        self.assertEqual(mm_ship.build_site(self.root, out, self.installer), self.digest)
        self.assertTrue((out / 'index.html').exists())
        self.assertTrue((out / '_headers').exists())
        self.assertTrue((out / 'version.json').exists())
        self.assertFalse((out / 'README.md').exists())
        self.assertEqual((out / 'download/MetaMate-setup.exe').read_bytes(), b'installer')
        self.assertEqual((out / 'download/SHA256.txt').read_text(encoding='utf-8'), f'{self.digest}  MetaMate-setup.exe\n')

    def test_replaces_an_older_build(self):
        self.manifest(self.digest)
        out = self.root / 'dist'
        out.mkdir()
        (out / 'stale.txt').write_text('old', encoding='utf-8')
        mm_ship.build_site(self.root, out, self.installer)
        self.assertFalse((out / 'stale.txt').exists())

    def test_refuses_an_installer_the_updater_would_reject(self):
        self.manifest('0' * 64)
        out = self.root / 'dist'
        with self.assertRaises(SystemExit):
            mm_ship.build_site(self.root, out, self.installer)
        self.assertFalse(out.exists())


class Notes(unittest.TestCase):
    def test_takes_only_this_version_from_the_changelog(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            (root / 'CHANGELOG.md').write_text('# Changelog\n\n## 0.2.1\n\nNew.\n\n## 0.2.0\n\nOld.\n', encoding='utf-8')
            notes = mm_ship.release_notes(root, '0.2.1')
        self.assertIn('New.', notes)
        self.assertNotIn('Old.', notes)
        self.assertIn('SHA256.txt', notes)


class Main(unittest.TestCase):
    def quiet(self, argv):
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            code = mm_ship.main(argv)
        return code, out.getvalue()

    def test_dry_run_changes_nothing_and_lists_every_step(self):
        with mock.patch.dict(os.environ, {'MM_AGENT': ''}), mock.patch.object(mm_ship.subprocess, 'run') as called:
            code, text = self.quiet(['--dry-run'])
        self.assertEqual(code, 0)
        called.assert_not_called()
        for step in mm_ship.STEPS:
            self.assertIn(f'== {step}', text)

    def test_agents_are_refused(self):
        with mock.patch.dict(os.environ, {'MM_AGENT': '1'}), mock.patch.object(mm_ship.subprocess, 'run') as called:
            code, text = self.quiet(['--dry-run'])
        self.assertEqual(code, 2)
        called.assert_not_called()
        self.assertIn('agents never deploy', text)

    def test_repo_step_needs_a_license(self):
        with tempfile.TemporaryDirectory() as folder, mock.patch.object(mm_ship, 'ROOT', Path(folder)):
            with mock.patch.dict(os.environ, {'MM_AGENT': ''}), self.assertRaises(SystemExit) as raised:
                self.quiet(['repo', '--yes'])
        self.assertIn('LICENSE', str(raised.exception))

    def test_a_no_stops_the_publish(self):
        run = mm_ship.Runner(False, False)
        with mock.patch('builtins.input', return_value='n'), mock.patch.object(mm_ship.subprocess, 'run') as called:
            with contextlib.redirect_stdout(io.StringIO()), tempfile.TemporaryDirectory() as folder:
                root = Path(folder)
                (root / 'LICENSE').write_text('x', encoding='utf-8')
                with mock.patch.object(mm_ship, 'ROOT', root), mock.patch.object(mm_ship.Runner, 'run'):
                    with self.assertRaises(SystemExit):
                        mm_ship.step_repo(run)
        called.assert_not_called()

    def test_yes_never_answers_for_going_public(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            (root / 'LICENSE').write_text('x', encoding='utf-8')
            with mock.patch.object(mm_ship, 'ROOT', root), mock.patch.object(mm_ship.Runner, 'run') as ran:
                with mock.patch('builtins.input', return_value='n') as asked:
                    with contextlib.redirect_stdout(io.StringIO()), self.assertRaises(SystemExit):
                        mm_ship.step_repo(mm_ship.Runner(False, True))
        asked.assert_called_once()
        self.assertFalse([call for call in ran.call_args_list if 'edit' in call.args[0]])

    def test_a_no_on_an_early_step_never_reaches_the_public_one(self):
        with mock.patch.dict(os.environ, {'MM_AGENT': ''}), mock.patch('builtins.input', return_value='n'):
            with mock.patch.object(mm_ship, 'tool', side_effect=lambda name: name):
                with mock.patch.object(mm_ship.subprocess, 'run') as called, self.assertRaises(SystemExit):
                    self.quiet(['glimmer', 'repo'])
        called.assert_not_called()

    def test_a_failed_preflight_fails_the_run(self):
        result = mock.Mock(returncode=1)
        with mock.patch.dict(os.environ, {'MM_AGENT': ''}), mock.patch.object(mm_ship.subprocess, 'run', return_value=result):
            with self.assertRaises(SystemExit):
                self.quiet(['check'])

    def test_a_changed_proxy_link_stops_before_the_site(self):
        links = iter([{'url': 'a', 'invite': 'b'}, {'url': 'c', 'invite': 'b'}])
        run = mm_ship.Runner(False, True)
        with mock.patch.object(mm_ship, 'proxy_link', side_effect=lambda: next(links)), mock.patch.object(mm_ship.Runner, 'run'):
            with contextlib.redirect_stdout(io.StringIO()), self.assertRaises(SystemExit) as raised:
                mm_ship.step_proxy(run)
        self.assertIn('mm release', str(raised.exception))

    def test_a_missing_tool_is_named(self):
        with mock.patch.object(mm_ship.shutil, 'which', return_value=None), self.assertRaises(SystemExit) as raised:
            mm_ship.tool('gh')
        self.assertIn('gh', str(raised.exception))


if __name__ == '__main__':
    unittest.main()
