"""Tests for tools/mm_locales.py: locale merging and safe key insertion."""

import argparse
import json
import sys
import tempfile
import unittest
from contextlib import redirect_stdout
from io import StringIO
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import mm_locales


EN = '{\n  "chat.hello": "Hello",\n  "chat.bye": "Bye",\n  "settings.title": "Settings"\n}\n'
RU = '{\n  "chat.hello": "Привет",\n  "chat.bye": "Пока",\n  "settings.title": "Настройки"\n}\n'

PATCH = """diff --git a/locales/en.json b/locales/en.json
index 1111111..2222222 100644
--- a/locales/en.json
+++ b/locales/en.json
@@ -1,4 +1,5 @@
 {
+  "chat.retry": "Retry",
   "chat.hello": "Hello",
   "chat.bye": "Bye",
diff --git a/src/main.ts b/src/main.ts
index 3333333..4444444 100644
--- a/src/main.ts
+++ b/src/main.ts
@@ -10,6 +10,7 @@
+export const retry = true;
diff --git a/locales/ru.json b/locales/ru.json
index 5555555..6666666 100644
--- a/locales/ru.json
+++ b/locales/ru.json
@@ -1,4 +1,5 @@
 {
+  "chat.retry": "Повторить",
   "chat.hello": "Привет",
   "chat.bye": "Пока",
"""

CONFLICT_PATCH = """diff --git a/locales/en.json b/locales/en.json
--- a/locales/en.json
+++ b/locales/en.json
@@ -1,4 +1,5 @@
 {
+  "chat.retry": "Retry",
+  "chat.hello": "Hi there",
   "chat.bye": "Bye",
"""


class LocaleToolTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        self.locales = self.root / 'locales'
        self.locales.mkdir()
        (self.locales / 'en.json').write_text(EN, encoding='utf-8')
        (self.locales / 'ru.json').write_text(RU, encoding='utf-8')

    def tearDown(self):
        self.tmp.cleanup()

    def run_cmd(self, func, argv):
        out = StringIO()
        with redirect_stdout(out):
            code = func(mm_locales.build_parser().parse_args(argv), root=self.root)
        return code, out.getvalue()

    def en_lines(self):
        return (self.locales / 'en.json').read_text(encoding='utf-8').splitlines()

    # --- diff parsing -------------------------------------------------------

    def test_parse_patch_collects_only_locale_pairs(self):
        found = mm_locales.parse_patch(PATCH)
        self.assertEqual(found['locales/en.json'], [('chat.retry', 'Retry')])
        self.assertEqual(found['locales/ru.json'], [('chat.retry', 'Повторить')])
        # The added line in src/main.ts and the +++ headers are not pairs.
        self.assertEqual(set(found), {'locales/en.json', 'locales/ru.json'})

    def test_parse_patch_ignores_deleted_file_target(self):
        patch = '--- a/locales/en.json\n+++ /dev/null\n@@ -1 +0,0 @@\n-{\n'
        self.assertEqual(mm_locales.parse_patch(patch)['locales/en.json'], [])

    def test_parse_patch_accepts_tab_timestamped_headers(self):
        patch = ('--- a/locales/en.json\t2026-11-01 02:07:14.000000000 +0500\n'
                 '+++ b/locales/en.json\t2026-11-01 02:07:14.000000000 +0500\n'
                 '@@ -1 +1,2 @@\n'
                 '+  "chat.retry": "Retry",\n')
        self.assertEqual(mm_locales.parse_patch(patch)['locales/en.json'], [('chat.retry', 'Retry')])

    def test_parse_patch_skips_malformed_added_lines(self):
        patch = '--- a/locales/en.json\n+++ b/locales/en.json\n@@ -1 +1,2 @@\n+\n+  not a pair\n'
        self.assertEqual(mm_locales.parse_patch(patch)['locales/en.json'], [])

    # --- merge ---------------------------------------------------------------

    def write_patch(self, text):
        patch = self.root / 'change.patch'
        patch.write_text(text, encoding='utf-8')
        return patch

    def test_merge_adds_missing_keys_after_last_same_prefix(self):
        patch = self.write_patch(PATCH)
        with mock.patch.object(mm_locales, '_git_apply', return_value=0) as git:
            code, out = self.run_cmd(mm_locales.cmd_merge, ['merge', str(patch)])
        self.assertEqual(code, 0, out)
        git.assert_called_once()
        self.assertEqual(json.loads((self.locales / 'en.json').read_text(encoding='utf-8'))['chat.retry'], 'Retry')
        self.assertEqual(json.loads((self.locales / 'ru.json').read_text(encoding='utf-8'))['chat.retry'], 'Повторить')
        lines = self.en_lines()
        self.assertLess(lines.index('  "chat.bye": "Bye",'), lines.index('  "chat.retry": "Retry",'))
        self.assertLess(lines.index('  "chat.retry": "Retry",'), lines.index('  "settings.title": "Settings"'))

    def test_merge_skips_key_with_same_value(self):
        same = '--- a/locales/en.json\n+++ b/locales/en.json\n@@ -1 +1,2 @@\n+  "chat.hello": "Hello",\n'
        patch = self.write_patch(same)
        before = (self.locales / 'en.json').read_bytes()
        with mock.patch.object(mm_locales, '_git_apply', return_value=0):
            code, out = self.run_cmd(mm_locales.cmd_merge, ['merge', str(patch)])
        self.assertEqual(code, 0, out)
        self.assertIn('same value', out)
        self.assertEqual((self.locales / 'en.json').read_bytes(), before)

    def test_merge_conflict_reports_and_changes_only_that_key(self):
        patch = self.write_patch(CONFLICT_PATCH)
        with mock.patch.object(mm_locales, '_git_apply', return_value=0) as git:
            code, out = self.run_cmd(mm_locales.cmd_merge, ['merge', str(patch)])
        self.assertEqual(code, 1)
        self.assertIn('!', out)
        git.assert_called_once()  # the rest of the patch still applies
        en = json.loads((self.locales / 'en.json').read_text(encoding='utf-8'))
        self.assertEqual(en['chat.hello'], 'Hello')  # conflict: unchanged
        self.assertEqual(en['chat.retry'], 'Retry')  # new key: added

    def test_merge_theirs_overwrites_in_place(self):
        patch = self.write_patch(CONFLICT_PATCH)
        with mock.patch.object(mm_locales, '_git_apply', return_value=0):
            code, out = self.run_cmd(mm_locales.cmd_merge, ['merge', str(patch), '--theirs'])
        self.assertEqual(code, 0, out)
        self.assertIn('took theirs', out)
        en_text = (self.locales / 'en.json').read_text(encoding='utf-8')
        self.assertEqual(json.loads(en_text)['chat.hello'], 'Hi there')
        self.assertEqual(en_text.count('"chat.hello"'), 1)  # replaced, not duplicated
        self.assertEqual(len(en_text.splitlines()), len(EN.splitlines()) + 1)

    def test_merge_dry_run_writes_nothing(self):
        patch = self.write_patch(PATCH)
        before_en = (self.locales / 'en.json').read_bytes()
        before_ru = (self.locales / 'ru.json').read_bytes()
        with mock.patch.object(mm_locales, '_git_apply', return_value=0) as git:
            code, out = self.run_cmd(mm_locales.cmd_merge, ['merge', str(patch), '--dry-run'])
        self.assertEqual(code, 0, out)
        self.assertIn('dry-run: would write', out)
        self.assertEqual((self.locales / 'en.json').read_bytes(), before_en)
        self.assertEqual((self.locales / 'ru.json').read_bytes(), before_ru)
        git.assert_called_once_with(patch, self.root, True)

    def test_merge_locales_only_skips_git_apply(self):
        patch = self.write_patch(PATCH)
        with mock.patch.object(mm_locales, '_git_apply', return_value=0) as git:
            code, out = self.run_cmd(mm_locales.cmd_merge, ['merge', str(patch), '--locales-only'])
        self.assertEqual(code, 0, out)
        git.assert_not_called()

    def test_merge_still_applies_rest_without_locale_keys(self):
        patch = self.write_patch('diff --git a/src/a.ts b/src/a.ts\n--- a/src/a.ts\n+++ b/src/a.ts\n@@ -1 +1,2 @@\n+x\n')
        with mock.patch.object(mm_locales, '_git_apply', return_value=0) as git:
            code, out = self.run_cmd(mm_locales.cmd_merge, ['merge', str(patch)])
        self.assertEqual(code, 0, out)
        git.assert_called_once()
        self.assertIn('no locale keys', out)

    def test_merge_git_failure_exits_1(self):
        patch = self.write_patch(PATCH)
        with mock.patch.object(mm_locales, '_git_apply', return_value=1):
            code, out = self.run_cmd(mm_locales.cmd_merge, ['merge', str(patch)])
        self.assertEqual(code, 1)

    def test_merge_missing_patch_exits_2(self):
        code, _ = self.run_cmd(mm_locales.cmd_merge, ['merge', str(self.root / 'nope.patch')])
        self.assertEqual(code, 2)

    def test_merge_refuses_keys_outside_the_charset(self):
        patch = self.write_patch('--- a/locales/en.json\n+++ b/locales/en.json\n@@ -1 +1,2 @@\n+  "Bad Key": "x",\n')
        with mock.patch.object(mm_locales, '_git_apply', return_value=0):
            code, out = self.run_cmd(mm_locales.cmd_merge, ['merge', str(patch)])
        self.assertEqual(code, 0, out)  # reported as skipped, not a conflict
        self.assertIn('not [a-z0-9_.]+', out)
        self.assertNotIn('Bad Key', (self.locales / 'en.json').read_text(encoding='utf-8'))

    # --- git apply -----------------------------------------------------------

    def test_git_apply_uses_list_form_with_excludes(self):
        with mock.patch.object(mm_locales.subprocess, 'run') as run:
            run.return_value = mock.Mock(returncode=0, stdout='', stderr='')
            code = mm_locales._git_apply(Path('fix.patch'), Path('/repo'), dry_run=False)
        self.assertEqual(code, 0)
        (cmd,), kwargs = run.call_args
        self.assertEqual(cmd, ['git', 'apply', '--exclude=locales/en.json',
                               '--exclude=locales/ru.json', 'fix.patch'])
        self.assertEqual(kwargs['cwd'], Path('/repo'))
        self.assertFalse(kwargs.get('shell', False))

    def test_git_apply_failure_prints_output(self):
        with mock.patch.object(mm_locales.subprocess, 'run') as run:
            run.return_value = mock.Mock(returncode=1, stdout='', stderr='error: patch failed')
            out = StringIO()
            with redirect_stdout(out):
                code = mm_locales._git_apply(Path('fix.patch'), Path('/repo'), dry_run=False)
        self.assertEqual(code, 1)
        self.assertIn('error: patch failed', out.getvalue())

    def test_git_apply_dry_run_runs_nothing(self):
        with mock.patch.object(mm_locales.subprocess, 'run') as run:
            code = mm_locales._git_apply(Path('fix.patch'), Path('/repo'), dry_run=True)
        self.assertEqual(code, 0)
        run.assert_not_called()

    # --- add -----------------------------------------------------------------

    def test_add_writes_key_to_both_files(self):
        code, out = self.run_cmd(mm_locales.cmd_add, ['add', 'chat.retry', 'Retry', 'Повторить'])
        self.assertEqual(code, 0, out)
        en_text = (self.locales / 'en.json').read_text(encoding='utf-8')
        ru_text = (self.locales / 'ru.json').read_text(encoding='utf-8')
        self.assertEqual(json.loads(en_text)['chat.retry'], 'Retry')
        self.assertEqual(json.loads(ru_text)['chat.retry'], 'Повторить')
        lines = en_text.splitlines()
        self.assertLess(lines.index('  "chat.bye": "Bye",'), lines.index('  "chat.retry": "Retry",'))
        self.assertLess(lines.index('  "chat.retry": "Retry",'), lines.index('  "settings.title": "Settings"'))

    def test_add_appends_new_prefix_at_end(self):
        code, _ = self.run_cmd(mm_locales.cmd_add, ['add', 'about.title', 'About', 'О программе'])
        self.assertEqual(code, 0)
        lines = self.en_lines()
        self.assertTrue(lines[-3].endswith(','))  # settings.title gained a comma
        self.assertEqual(lines[-2], '  "about.title": "About"')  # last key: no comma
        self.assertEqual(lines[-1], '}')
        json.loads((self.locales / 'en.json').read_text(encoding='utf-8'))

    def test_add_refuses_keys_outside_the_charset(self):
        before = (self.locales / 'en.json').read_bytes()
        for bad in ('Chat.retry', 'chat retry', 'chat..retry!', ''):
            code, out = self.run_cmd(mm_locales.cmd_add, ['add', bad, 'x', 'y'])
            self.assertEqual(code, 2, bad)
        self.assertEqual((self.locales / 'en.json').read_bytes(), before)

    def test_add_accepts_nested_and_underscored_keys(self):
        code, _ = self.run_cmd(mm_locales.cmd_add, ['add', 'a.b_c.d0', 'x', 'y'])
        self.assertEqual(code, 0)

    def test_add_existing_key_same_value_is_idempotent(self):
        code, _ = self.run_cmd(mm_locales.cmd_add, ['add', 'chat.hello', 'Hello', 'Привет'])
        self.assertEqual(code, 0)
        self.assertEqual((self.locales / 'en.json').read_text(encoding='utf-8'), EN)

    def test_add_existing_key_different_value_refused(self):
        before = (self.locales / 'en.json').read_bytes()
        code, _ = self.run_cmd(mm_locales.cmd_add, ['add', 'chat.hello', 'Hi', 'Привет'])
        self.assertEqual(code, 1)
        self.assertEqual((self.locales / 'en.json').read_bytes(), before)

    def test_add_strips_bom_and_crlf(self):
        bom = b'\xef\xbb\xbf' + EN.replace('\n', '\r\n').encode('utf-8')
        (self.locales / 'en.json').write_bytes(bom)
        code, _ = self.run_cmd(mm_locales.cmd_add, ['add', 'chat.retry', 'Retry', 'Повторить'])
        self.assertEqual(code, 0)
        raw = (self.locales / 'en.json').read_bytes()
        self.assertFalse(raw.startswith(b'\xef\xbb\xbf'))
        self.assertNotIn(b'\r', raw)
        json.loads(raw.decode('utf-8'))

    def test_add_escapes_values_and_keeps_cyrillic(self):
        code, _ = self.run_cmd(mm_locales.cmd_add, ['add', 'chat.quote', 'Say "hi"', 'Скажи «привет»'])
        self.assertEqual(code, 0)
        en_text = (self.locales / 'en.json').read_text(encoding='utf-8')
        self.assertEqual(json.loads(en_text)['chat.quote'], 'Say "hi"')
        self.assertIn('"Скажи «привет»"', (self.locales / 'ru.json').read_text(encoding='utf-8'))

    def test_insert_key_into_empty_object(self):
        text = mm_locales.insert_key('{\n}\n', 'a.b', 'x')
        self.assertEqual(json.loads(text), {'a.b': 'x'})

    def test_insert_key_needs_no_comma_fix_when_appending(self):
        text = mm_locales.insert_key('{\n  "a": "x"\n}\n', 'b', 'y')
        self.assertEqual(text.splitlines(), ['{', '  "a": "x",', '  "b": "y"', '}'])

    # --- mm.py wiring ---------------------------------------------------------

    def test_mm_agent_allows_only_locales_add(self):
        try:
            import mm
        except ImportError as error:
            self.skipTest(f'cannot import tools/mm.py here: {error}')
        with redirect_stdout(StringIO()) as out:
            code = mm.cmd_agent(argparse.Namespace(what='locales', rest=['merge', 'x.patch']))
        self.assertEqual(code, 2)
        self.assertIn("only 'add'", out.getvalue())
        with mock.patch.object(mm, '_run_tool', return_value=0) as run_tool:
            with redirect_stdout(StringIO()):
                code = mm.cmd_agent(argparse.Namespace(what='locales', rest=['add', 'chat.x', 'X', 'Икс']))
        self.assertEqual(code, 0)
        run_tool.assert_called_once_with('locales', ['add', 'chat.x', 'X', 'Икс'])


if __name__ == '__main__':
    unittest.main()
