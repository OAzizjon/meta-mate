"""Handcrafted diff coverage for added-line hygiene and safe diagnostics."""

import contextlib
import io
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import mm_difflint as difflint


def diff(path, *lines, start=1):
    return (f'--- a/{path}\n+++ b/{path}\n@@ -0,0 +{start},{len(lines)} @@\n' +
            ''.join('+' + line + '\n' for line in lines))


class DiffLintTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)

    def findings(self, text):
        return difflint.lint(difflint.parse_diff(text), self.root)

    def rules(self, text):
        return [f.rule for f in self.findings(text) if not f.rule.startswith('INFO_')]

    def write(self, path, text):
        target = self.root / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(text, encoding='utf-8')

    def run_main(self, args, stdin=''):
        output, errors = io.StringIO(), io.StringIO()
        with patch.object(difflint, 'ROOT', self.root), \
                patch.object(sys, 'stdin', io.StringIO(stdin)), \
                contextlib.redirect_stdout(output), contextlib.redirect_stderr(errors):
            code = difflint.main(args)
        return code, output.getvalue(), errors.getvalue()

    def test_error_rules(self):
        cases = [
            ('src/a.ts', '<<<<<<< HEAD', 'ERROR_CONFLICT'),
            ('src/a.ts', '=======', 'ERROR_CONFLICT'),
            ('src/a.ts', '>>>>>>> incoming', 'ERROR_CONFLICT'),
            ('src/a.ts', '||||||| base', 'ERROR_CONFLICT'),
            ('crates/mm-core/src/a.rs', 'dbg!(value);', 'ERROR_CORE_DEBUG'),
            ('crates/mm-core/src/a.rs', 'println!("hello");', 'ERROR_CORE_DEBUG'),
            ('crates/mm-core/tests/a.rs', 'eprintln!("hello");', 'ERROR_CORE_DEBUG'),
            ('src/a.ts', 'console.log(value);', 'ERROR_CONSOLE_LOG'),
            ('src/a.test.ts', 'describe.only("x", () => {});', 'ERROR_TEST_ONLY'),
            ('tests/a.ts', 'test.only.each(cases);', 'ERROR_TEST_ONLY'),
            ('crates/mm-core/tests/a.rs', '#[ignore]', 'ERROR_IGNORED_TEST'),
            ('crates/mm-core/tests/a.rs', '#[ignore = "slow"]', 'ERROR_IGNORED_TEST'),
        ]
        for path, line, rule in cases:
            with self.subTest(line=line):
                self.assertIn(rule, self.rules(diff(path, line)))

    def test_warning_rules(self):
        cases = [
            ('crates/mm-core/src/a.rs', 'x.unwrap();', 'WARN_RUST_PANIC'),
            ('src-tauri/src/a.rs', 'x.expect("ready");', 'WARN_RUST_PANIC'),
            ('crates/mm-platform/src/a.rs', 'todo!();', 'WARN_UNIMPLEMENTED'),
            ('src-tauri/src/a.rs', 'unimplemented!();', 'WARN_UNIMPLEMENTED'),
            ('tools/a.py', '# TODO remove', 'WARN_TODO'),
            ('src/a.ts', '// FIXME later', 'WARN_TODO'),
            ('src/a.ts', '// XXX later', 'WARN_TODO'),
            ('src/a.ts', 'const x = value as any;', 'WARN_TS_ESCAPE'),
            ('src/a.tsx', '// @ts-ignore', 'WARN_TS_ESCAPE'),
            ('crates/mm-core/src/a.rs', 'unsafe { call(); }', 'WARN_UNSAFE'),
        ]
        for path, line, rule in cases:
            with self.subTest(line=line):
                self.assertEqual(self.rules(diff(path, line)), [rule])

    def test_dom_literals_and_locale_lookups(self):
        for line in ['el.textContent = "Hello";', "el.innerText = 'Hello';",
                     'el.title = `Hello`;', 'el.placeholder = "Name";',
                     'el.setAttribute("aria-label", "Close");',
                     '{ "placeholder": "Name" }']:
            with self.subTest(line=line):
                self.assertIn('WARN_UI_LITERAL', self.rules(diff('src/a.ts', line)))
        for line in ['el.textContent = t("home.title");',
                     'el.title = i18n.t("home.title");',
                     'el.setAttribute("aria-label", t("home.close"));',
                     'el.placeholder = "";', 'el.innerText = value;']:
            with self.subTest(line=line):
                self.assertEqual(self.rules(diff('src/a.ts', line)), [])
        self.assertIn('WARN_UI_LITERAL', self.rules(diff(
            'src/a.ts', 'el.title = "Inline"; other.title = t("home.title");')))

    def test_rule_scope(self):
        for path, line in [('src/a.test.ts', 'console.log(value);'),
                           ('src/__tests__/a.ts', 'console.log(value);'),
                           ('server/a.js', 'console.log(value);'),
                           ('src/a.ts', 'object.only();'),
                           ('crates/mm-win32/src/a.rs', 'println!("x"); unsafe {}'),
                           ('crates/mm-core/tests/a.rs', 'x.unwrap();'),
                           ('src-tauri/src/tests/a.rs', 'x.expect("ready");'),
                           ('other/a.rs', 'x.unwrap();'),
                           ('tools/a.py', 'x as any // @ts-ignore')]:
            with self.subTest(path=path):
                self.assertEqual(self.rules(diff(path, line)), [])

    def test_context_removed_lines_and_crlf_line_numbers(self):
        text = ('--- a/src/a.ts\n+++ b/src/a.ts\n@@ -5,3 +10,4 @@\n'
                ' console.log("old");\n-console.log("removed");\n'
                '+console.log("added");\n+++source;\n unchanged\n'
                '@@ -30 +40 @@\n-old\n+console.log("later");\n'
                '\\ No newline at end of file\n').replace('\n', '\r\n')
        files = difflint.parse_diff(text)
        self.assertEqual([(a.number, a.text) for a in files[0].added],
                         [(11, 'console.log("added");'), (12, '++source;'),
                          (40, 'console.log("later");')])
        found = [f for f in difflint.lint(files, self.root) if f.rule == 'ERROR_CONSOLE_LOG']
        self.assertEqual([f.line for f in found], [11, 40])
        self.assertEqual(str(found[0]), 'src/a.ts:11  ERROR_CONSOLE_LOG  console.log("added");')

    def test_cfg_test_context_persists_across_hunks(self):
        text = ('--- a/crates/mm-core/src/a.rs\n+++ b/crates/mm-core/src/a.rs\n'
                '@@ -1 +1,3 @@\n+x.unwrap();\n #[cfg(test)]\n+test.only();\n'
                '@@ -20 +21 @@\n-old\n+x.expect("test");\n')
        # Both new-side lines of the first hunk plus context make three.
        self.assertEqual(self.rules(text), ['WARN_RUST_PANIC', 'ERROR_TEST_ONLY'])
        self.assertEqual(self.rules(diff('crates/mm-core/src/b.rs',
                                         '#[cfg(test)]', 'x.unwrap();')), [])

    def test_cfg_test_on_disk_and_deleted_marker(self):
        path = 'src-tauri/src/a.rs'
        self.write(path, 'fn production() {}\n#[cfg(test)]\nmod tests {}\n')
        self.assertEqual(self.rules(diff(path, 'x.unwrap();', start=3)), [])
        self.assertEqual(self.rules(diff(path, 'x.unwrap();', start=1)), ['WARN_RUST_PANIC'])
        text = f'--- a/{path}\n+++ b/{path}\n@@ -1 +1 @@\n-#[cfg(test)]\n+x.unwrap();\n'
        self.assertEqual(self.rules(text), ['WARN_RUST_PANIC'])

    def test_secret_values_are_never_printed_even_by_other_rules(self):
        values = ['sk-' + 'A' * 24, 'ghp_' + 'B' * 24,
                  'META_API_KEY="' + 'C' * 40 + '"', 'BEGIN RSA ' + 'PRIVATE KEY']
        for value in values:
            with self.subTest(kind=value[:3]):
                code, output, _ = self.run_main(['-'], diff('src/a.ts', f'console.log("{value}"); // TODO'))
                self.assertEqual(code, 1)
                self.assertIn('ERROR_SECRET', output)
                self.assertIn('ERROR_CONSOLE_LOG', output)
                self.assertIn('WARN_TODO', output)
                self.assertNotIn(value, output)

    def test_secrets_headers_including_deletion_and_rename(self):
        for name in ['.env', '.ENV.local', 'server/.Env.Prod']:
            with self.subTest(name=name):
                self.assertIn('ERROR_SECRETS_FILE', self.rules(diff(name, 'harmless')))
                text = f'diff --git a/{name} b/{name}\n--- a/{name}\n+++ /dev/null\n@@ -1 +0,0 @@\n-old\n'
                self.assertEqual(self.rules(text), ['ERROR_SECRETS_FILE'])
        text = 'diff --git a/.env b/config.txt\nsimilarity index 100%\nrename from .env\nrename to config.txt\n'
        self.assertEqual(self.rules(text), ['ERROR_SECRETS_FILE'])

    def test_rename_and_quoted_paths(self):
        text = ('diff --git "a/src/old name.ts" "b/src/new name.ts"\n'
                'rename from src/old name.ts\nrename to src/new name.ts\n'
                '--- "a/src/old name.ts"\n+++ "b/src/new name.ts"\n'
                '@@ -5 +5 @@\n-old\n+console.log("new");\n')
        found = self.findings(text)
        self.assertEqual((found[0].path, found[0].line), ('src/new name.ts', 5))

    def test_deleted_header_like_source_is_not_a_secrets_header(self):
        text = ('--- a/src/a.ts\n+++ b/src/a.ts\n@@ -1,2 +0,0 @@\n'
                '--- a/.env\n-+++ b/.env\n')
        self.assertEqual(self.rules(text), [])

    def test_binary_header_with_spaces_still_reports_secrets_file(self):
        text = ('diff --git a/server/.ENV.local copy b/server/.ENV.local copy\n'
                'Binary files a/server/.ENV.local copy and b/server/.ENV.local copy differ\n')
        self.assertEqual(self.rules(text), ['ERROR_SECRETS_FILE'])

    def test_missing_file_size_has_lower_bound_from_new_line_numbers(self):
        found = self.findings(diff('src/new.ts', 'one', start=801))
        sizes = [f.text for f in found if f.rule == 'WARN_FILE_SIZE']
        self.assertEqual(sizes, ['at least 801 lines after change'])

    def test_file_size_threshold_and_area_totals(self):
        self.write('src/a.ts', 'line\n' * 801)
        self.write('src/b.ts', 'line\n' * 800)
        found = self.findings(diff('src/a.ts', 'one', 'two') + diff('src/b.ts', 'three') +
                              diff('crates/mm-core/src/a.rs', 'four'))
        self.assertEqual([(f.path, f.line) for f in found if f.rule == 'WARN_FILE_SIZE'], [('src/a.ts', 0)])
        self.assertEqual({f.path: f.text for f in found if f.rule == 'INFO_ADDED'},
                         {'src/a.ts': '2 added lines', 'src/b.ts': '1 added lines',
                          'crates/mm-core/src/a.rs': '1 added lines'})
        self.assertEqual({f.path: f.text for f in found if f.rule == 'INFO_AREA'},
                         {'src': '3 added lines', 'crates/mm-core': '1 added lines'})

    def test_main_patch_stdin_empty_and_exit_levels(self):
        self.write('change.patch', diff('src/a.ts', '// TODO later'))
        self.assertEqual(self.run_main([str(self.root / 'missing.patch')])[0], 2)
        self.assertEqual(self.run_main([str(self.root / 'change.patch')])[0], 0)
        self.assertEqual(self.run_main(['-'], '')[:2], (0, ''))
        self.assertEqual(self.run_main(['-'], diff('src/a.ts', 'console.log(1);'))[0], 1)

    def test_default_includes_untracked_without_opening_secrets(self):
        self.write('src/new.ts', 'console.log(1);\n')
        replies = [diff('src/old.ts', '// TODO later'), 'src/new.ts\0.ENV.local\0']
        with patch.object(difflint, '_git', side_effect=replies) as git, \
                patch.object(difflint.Path, 'read_bytes', autospec=True,
                             side_effect=lambda p: b'console.log(1);\n' if p.name == 'new.ts' else b'') as read:
            code, output, _ = self.run_main([])
        self.assertEqual(code, 1)
        self.assertIn('src/new.ts:1  ERROR_CONSOLE_LOG', output)
        self.assertIn('.ENV.local:0  ERROR_SECRETS_FILE', output)
        self.assertFalse(any(difflint.is_secret(str(c.args[0])) for c in read.call_args_list))
        self.assertIn('HEAD', git.call_args_list[0].args[1])

    def test_untrusted_paths_and_read_errors(self):
        with patch.object(difflint.Path, 'read_bytes') as read:
            self.assertIsNone(difflint._read_file(self.root, '../outside.rs'))
            self.assertIsNone(difflint._read_file(self.root, '.ENV.local'))
            read.assert_not_called()
        with patch.object(difflint, '_git', side_effect=subprocess.TimeoutExpired('git', 60)):
            code, output, errors = self.run_main([])
        self.assertEqual(code, 2)
        self.assertEqual(output, '')
        self.assertIn('cannot read diff', errors)


if __name__ == '__main__':
    unittest.main()
