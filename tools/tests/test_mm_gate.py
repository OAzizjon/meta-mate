"""Tests for tools/mm_gate.py (standard library only)."""

import io
import json
import shutil
import subprocess
import sys
import tempfile
import unittest
from contextlib import redirect_stdout
from pathlib import Path

sys.path.insert(0, str(Path(__file__).absolute().parent.parent))

import mm_gate

NOTE = 'alpha\nbeta\n'
NOTE_PATCH = """\
--- a/note.txt
+++ b/note.txt
@@ -1,2 +1,2 @@
 alpha
-beta
+gamma
"""
DRAFT_NOTE_PATCH = """\
--- a/note.txt\t2026-10-31 09:15:00 +0000
+++ b/note.txt\t2026-10-31 09:15:00 +0000
@@ -1,2 +1,2 @@
 alpha
-beta
+gamma
"""
BAD_NOTE_PATCH = """\
--- a/note.txt
+++ b/note.txt
@@ -1,2 +1,2 @@
 zzz
-beta
+gamma
"""
LOCALES_SERVER_PATCH = """\
--- /dev/null
+++ b/locales/en.json
@@ -0,0 +1 @@
+{"gate.hello": "Hello"}
--- /dev/null
+++ b/server/app.py
@@ -0,0 +1 @@
+ROUTES = []
"""
LEAD_ONLY_PATCH = """\
--- /dev/null
+++ b/server/migrations/001_payment.up.sql
@@ -0,0 +1 @@
+ALTER TABLE t ADD COLUMN c INT;
"""
ENV_PATCH = """\
--- /dev/null
+++ b/.env
@@ -0,0 +1 @@
+METAMATE_TOKEN=abc123secret
"""
BIG_PATCH = """\
--- /dev/null
+++ b/big.rs
@@ -0,0 +801 @@
+fn big() {}
"""
CORE_DEBUG_PATCH = """\
--- a/crates/mm-core/src/lib.rs
+++ b/crates/mm-core/src/lib.rs
@@ -1,1 +1,2 @@
 // init
+pub fn f() { println!("debug"); }
"""


def write(root, name, text):
    path = Path(root, name)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding='utf-8', newline='\n')
    return path


def git(root, *args):
    proc = subprocess.run(['git', *args], cwd=root, capture_output=True, text=True,
                          encoding='utf-8', errors='replace')
    if proc.returncode != 0:
        raise AssertionError(f'git {" ".join(args)} failed:\n{proc.stderr}')
    return proc.stdout


def section(sections, name):
    for candidate in sections:
        if candidate['name'] == name:
            return candidate
    raise AssertionError(f'section {name!r} not found: '
                         + ', '.join(s['name'] for s in sections))


def row_text(sec):
    return '\n'.join(text for _, text in sec['rows'])


class TempDirTests(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory(prefix='mm-gate-')
        self.root = Path(self._tmp.name)

    def tearDown(self):
        self._tmp.cleanup()


@unittest.skipUnless(shutil.which('git'), 'git is not installed')
class ApplyCheckTests(unittest.TestCase):
    """Section 1 against real git repositories created in a temp dir."""

    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory(prefix='mm-gate-')
        self.root = Path(self._tmp.name)
        git(self.root, '-c', 'init.defaultBranch=main', 'init', '-q')
        write(self.root, 'note.txt', NOTE)
        git(self.root, 'add', 'note.txt')
        git(self.root, '-c', 'user.email=t@example.com', '-c', 'user.name=t',
            'commit', '-q', '-m', 'init')

    def tearDown(self):
        self._tmp.cleanup()

    def test_applies_cleanly(self):
        sections = mm_gate.sections_for(NOTE_PATCH, self.root)
        self.assertEqual(mm_gate.section_status(section(sections, 'git apply --check')), 'OK')

    def test_draft_header_is_stripped_before_apply_check(self):
        self.assertNotIn('\t', mm_gate._strip_header_dates(DRAFT_NOTE_PATCH))
        sections = mm_gate.sections_for(DRAFT_NOTE_PATCH, self.root)
        self.assertEqual(mm_gate.section_status(section(sections, 'git apply --check')), 'OK')

    def test_fails_when_the_context_is_gone(self):
        sections = mm_gate.sections_for(BAD_NOTE_PATCH, self.root)
        sec = section(sections, 'git apply --check')
        self.assertEqual(mm_gate.section_status(sec), 'FAIL')
        self.assertTrue(sec['rows'][0][1])


class NoGitTests(TempDirTests):
    def test_no_git_is_a_warn_not_a_fail(self):
        sections = mm_gate.sections_for(LOCALES_SERVER_PATCH, self.root)
        self.assertEqual(mm_gate.section_status(section(sections, 'git apply --check')), 'WARN')
        self.assertFalse(any(mm_gate.section_status(s) == 'FAIL' for s in sections))


class InvariantTests(TempDirTests):
    def test_locales_and_server_rows(self):
        sections = mm_gate.sections_for(LOCALES_SERVER_PATCH, self.root)
        text = row_text(section(sections, 'invariants'))
        self.assertIn('invariant 7', text)
        self.assertIn('invariant 8', text)
        self.assertIn('proxy: owner redeploy needed', text)
        self.assertEqual(mm_gate.section_status(section(sections, 'invariants')), 'WARN')

    def test_auth_payment_migrations_are_lead_only_fails(self):
        sections = mm_gate.sections_for(LEAD_ONLY_PATCH, self.root)
        sec = section(sections, 'invariants')
        self.assertEqual(mm_gate.section_status(sec), 'FAIL')
        self.assertIn('lead only, agents review only', row_text(sec))

    def test_env_is_a_fail_and_flagged_by_difflint(self):
        sections = mm_gate.sections_for(ENV_PATCH, self.root)
        self.assertEqual(mm_gate.section_status(section(sections, 'invariants')), 'FAIL')
        self.assertIn('ERROR_SECRETS_FILE', row_text(section(sections, 'difflint leftovers')))

    def test_static_table_matchers(self):
        self.assertTrue(any('1+2' in row for _, row in
                            mm_gate.invariant_rows('crates/mm-win32/src/focus.rs')))
        self.assertTrue(any('1+2' in row for _, row in
                            mm_gate.invariant_rows('src-tauri/src/highlight.rs')))
        self.assertTrue(any('invariant 3' in row for _, row in
                            mm_gate.invariant_rows('src-tauri/src/capture.rs')))
        self.assertTrue(any('invariant 4' in row for _, row in
                            mm_gate.invariant_rows('src-tauri/src/mate_safety.rs')))
        self.assertTrue(any('invariant 5' in row for _, row in
                            mm_gate.invariant_rows('src-tauri/src/pointing.rs')))
        self.assertEqual(mm_gate.invariant_rows('docs/plan.md'), [])


class DifflintTests(TempDirTests):
    def test_difflint_rules_run_via_import(self):
        write(self.root, 'crates/mm-core/src/lib.rs', '// init\n')
        sections = mm_gate.sections_for(CORE_DEBUG_PATCH, self.root)
        sec = section(sections, 'difflint leftovers')
        self.assertEqual(mm_gate.section_status(sec), 'FAIL')
        self.assertIn('ERROR_CORE_DEBUG', row_text(sec))


class SizeTests(TempDirTests):
    def test_file_pushed_over_800_lines_is_flagged(self):
        sections = mm_gate.sections_for(BIG_PATCH, self.root)
        sec = section(sections, 'file size')
        self.assertEqual(mm_gate.section_status(sec), 'WARN')
        self.assertIn('big.rs', row_text(sec))


class TestsSectionTests(unittest.TestCase):
    def test_tools_patch_names_its_unittest_file(self):
        patch = '--- a/tools/mm_gate.py\n+++ b/tools/mm_gate.py\n@@ -1,1 +1,1 @@\n x\n'
        text = row_text(section(mm_gate.sections_for(patch, mm_gate.ROOT), 'tests to run'))
        self.assertIn('tools:', text)
        self.assertIn('test_mm_gate.py', text)


class MainTests(TempDirTests):
    def test_json_output_and_exit_code(self):
        patch = write(self.root, 'env.patch', ENV_PATCH)
        out = io.StringIO()
        with redirect_stdout(out):
            code = mm_gate.main([str(patch), '--json'])
        self.assertEqual(code, 1)
        payload = json.loads(out.getvalue())
        self.assertFalse(payload['ok'])
        self.assertEqual([s['name'] for s in payload['sections']],
                         ['git apply --check', 'difflint leftovers', 'invariants',
                          'tests to run', 'file size'])
        self.assertTrue(any(s['status'] == 'FAIL' for s in payload['sections']))

    def test_secret_value_is_never_printed(self):
        patch = write(self.root, 'env.patch', ENV_PATCH)
        out = io.StringIO()
        with redirect_stdout(out):
            mm_gate.main([str(patch)])
        self.assertNotIn('abc123secret', out.getvalue())

    def test_stdin_patch(self):
        stdin, sys.stdin = sys.stdin, io.StringIO(ENV_PATCH)
        try:
            out = io.StringIO()
            with redirect_stdout(out):
                code = mm_gate.main(['-'])
        finally:
            sys.stdin = stdin
        self.assertEqual(code, 1)
        self.assertIn('mm gate: (stdin)', out.getvalue())


if __name__ == '__main__':
    unittest.main()
