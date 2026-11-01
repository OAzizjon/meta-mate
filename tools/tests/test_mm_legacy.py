"""Tests for tools/mm_legacy.py: one tmp-dir fixture tree per section."""

import contextlib
import io
import json
from pathlib import Path
import sys
import tempfile
from unittest import mock
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

import mm_legacy


class LegacyFixture(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.root = Path(self._tmp.name)

    def tearDown(self):
        self._tmp.cleanup()

    def write(self, rel, text):
        path = self.root / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding='utf-8')
        return path

    def names(self, entries):
        return {(e['path'], e['name']) for e in entries}


class RustSection(LegacyFixture):
    def test_allow_attributes(self):
        self.write('crates/demo/src/lib.rs', '\n'.join([
            '#![allow(unused)]',
            '#[allow(dead_code)]',
            'fn hidden() {}',
            '#[allow(unused_variables)]',
            'fn more(x: i32) { let _ = x; }',
            '',
        ]))
        entries = mm_legacy.rust_findings(self.root)
        got = {(e['path'], e['name']) for e in entries if 'attribute' in e['reason']}
        self.assertIn(('crates/demo/src/lib.rs', 'unused'), got)
        self.assertIn(('crates/demo/src/lib.rs', 'dead_code'), got)
        self.assertIn(('crates/demo/src/lib.rs', 'unused_variables'), got)

    def test_unreferenced_pub_items(self):
        self.write('crates/demo/src/lib.rs', 'pub fn dead_helper() {}\npub struct Gone {}\n')
        got = self.names(mm_legacy.rust_findings(self.root))
        self.assertIn(('crates/demo/src/lib.rs', 'dead_helper'), got)
        self.assertIn(('crates/demo/src/lib.rs', 'Gone'), got)

    def test_pub_item_used_later_in_same_file(self):
        self.write('crates/demo/src/lib.rs',
                   'pub fn called_soon() {}\nfn user() { called_soon(); }\n')
        self.assertNotIn(('crates/demo/src/lib.rs', 'called_soon'),
                         self.names(mm_legacy.rust_findings(self.root)))

    def test_pub_struct_with_impl_is_used(self):
        self.write('crates/demo/src/lib.rs', 'pub struct Keeper {}\nimpl Keeper {}\n')
        self.assertNotIn(('crates/demo/src/lib.rs', 'Keeper'),
                         self.names(mm_legacy.rust_findings(self.root)))

    def test_target_directory_is_not_a_reference(self):
        self.write('crates/demo/src/lib.rs', 'pub fn only_in_target() {}\n')
        self.write('target/debug/out.rs', 'fn x() { only_in_target(); }\n')
        got = self.names(mm_legacy.rust_findings(self.root))
        self.assertIn(('crates/demo/src/lib.rs', 'only_in_target'), got)

    def test_src_tauri_src_is_in_scope(self):
        self.write('src-tauri/src/lib.rs', 'pub fn tauri_only() {}\n')
        got = self.names(mm_legacy.rust_findings(self.root))
        self.assertIn(('src-tauri/src/lib.rs', 'tauri_only'), got)

    def test_definitions_outside_scope_are_ignored(self):
        self.write('src/helper.rs', 'pub fn not_scanned() {}\n')
        self.assertNotIn(('src/helper.rs', 'not_scanned'),
                         self.names(mm_legacy.rust_findings(self.root)))


class TsSection(LegacyFixture):
    def test_unreferenced_export(self):
        self.write('src/a.ts', 'export function deadFn() {}\nexport const live = 1;\n')
        self.write('src/b.ts', "import { live } from './a';\nlive;\n")
        got = self.names(mm_legacy.ts_findings(self.root))
        self.assertIn(('src/a.ts', 'deadFn'), got)
        self.assertNotIn(('src/a.ts', 'live'), got)

    def test_export_used_by_an_html_file(self):
        self.write('src/render.ts', 'export function paint() {}\n')
        self.write('src/index.html', '<script>paint();</script>\n')
        self.assertNotIn(('src/render.ts', 'paint'),
                         self.names(mm_legacy.ts_findings(self.root)))

    def test_test_files_are_not_definitions(self):
        self.write('src/thing.test.ts', 'export function testOnly() {}\n')
        self.assertNotIn(('src/thing.test.ts', 'testOnly'),
                         self.names(mm_legacy.ts_findings(self.root)))

    def test_orphan_module_and_root_html(self):
        self.write('src/orphan.ts', 'export const x = 1;\n')
        self.write('src/main.ts', 'export const y = 2;\n')
        self.write('index.html',
                   '<html><script type="module" src="./src/main.ts"></script></html>\n')
        got = self.names(mm_legacy.ts_findings(self.root))
        self.assertIn(('src/orphan.ts', 'orphan.ts'), got)
        self.assertNotIn(('src/main.ts', 'main.ts'), got)

    def test_import_from_test_counts(self):
        self.write('src/helper.ts', 'export const h = 1;\n')
        self.write('src/helper.test.ts', "import { h } from './helper';\nh;\n")
        got = self.names(mm_legacy.ts_findings(self.root))
        self.assertNotIn(('src/helper.ts', 'helper.ts'), got)


class PySection(LegacyFixture):
    def test_unreferenced_top_level(self):
        self.write('tools/alpha.py',
                   'def helper():\n    pass\n\n\ndef used_elsewhere():\n    pass\n')
        self.write('tools/beta.py', 'from alpha import used_elsewhere\nused_elsewhere()\n')
        got = self.names(mm_legacy.py_findings(self.root))
        self.assertIn(('tools/alpha.py', 'helper'), got)
        self.assertNotIn(('tools/alpha.py', 'used_elsewhere'), got)

    def test_cmd_and_test_names_skipped(self):
        self.write('tools/gamma.py',
                   'def cmd_gamma():\n    pass\n\n\ndef test_gamma():\n    pass\n')
        got = self.names(mm_legacy.py_findings(self.root))
        self.assertNotIn(('tools/gamma.py', 'cmd_gamma'), got)
        self.assertNotIn(('tools/gamma.py', 'test_gamma'), got)

    def test_set_defaults_target_skipped(self):
        self.write('tools/delta.py', (
            'import argparse\n\n\n'
            'def run_it():\n    pass\n\n\n'
            'def build_parser():\n'
            '    p = argparse.ArgumentParser()\n'
            '    p.set_defaults(func=run_it)\n'
            '    return p\n'))
        got = self.names(mm_legacy.py_findings(self.root))
        self.assertNotIn(('tools/delta.py', 'run_it'), got)

    def test_test_files_are_skipped(self):
        self.write('tools/tests/test_thing.py', 'def helper():\n    pass\n')
        self.assertEqual(self.names(mm_legacy.py_findings(self.root)), set())

    def test_server_is_in_scope(self):
        self.write('server/app.py', 'def orphan_handler():\n    pass\n')
        got = self.names(mm_legacy.py_findings(self.root))
        self.assertIn(('server/app.py', 'orphan_handler'), got)

    def test_reference_from_a_test_counts(self):
        self.write('tools/util.py', 'def shared():\n    pass\n')
        self.write('tools/tests/test_util.py', 'from util import shared\nshared()\n')
        self.assertNotIn(('tools/util.py', 'shared'),
                         self.names(mm_legacy.py_findings(self.root)))


class LocaleSection(LegacyFixture):
    def test_unused_keys(self):
        self.write('locales/en.json', '{\n  "app.title": "T",\n  "app.dead": "D"\n}\n')
        self.write('locales/ru.json', '{\n  "app.title": "T",\n  "app.dead": "D"\n}\n')
        self.write('src/main.ts', 'const t = "app.title";\n')
        entries = mm_legacy.locale_findings(self.root)
        got = self.names(entries)
        self.assertIn(('locales/en.json', 'app.dead'), got)
        self.assertNotIn(('locales/en.json', 'app.title'), got)
        dead = next(e for e in entries if e['name'] == 'app.dead')
        self.assertEqual(dead['line'], 3)
        self.assertIn('unused locale key', dead['reason'])


class MarkerSection(LegacyFixture):
    def test_comment_markers_only(self):
        self.write('src/old/thing.ts', '// legacy workaround\nconst u = "https://old.example";\n')
        self.write('src-tauri/src/lib.rs', '// TODO remove after refactor\nfn f() {}\n')
        entries = mm_legacy.marker_findings(self.root)
        got = {(e['path'], e['line']) for e in entries}
        self.assertIn(('src/old/thing.ts', 1), got)
        self.assertIn(('src-tauri/src/lib.rs', 1), got)
        self.assertEqual(len(entries), 2)


class DocsSection(LegacyFixture):
    def test_dead_and_live_paths(self):
        self.write('README.md', 'Run `tools/mm.py`, see `docs/gone.md`.\n')
        self.write('tools/mm.py', 'x = 1\n')
        self.write('docs/real.md', 'A real doc.\n')
        self.write('docs/stale.md', 'Uses `src/vanished.ts`.\n')
        got = self.names(mm_legacy.docs_findings(self.root))
        self.assertIn(('README.md', 'docs/gone.md'), got)
        self.assertIn(('docs/stale.md', 'src/vanished.ts'), got)
        self.assertNotIn(('README.md', 'tools/mm.py'), got)


class ExternalSection(LegacyFixture):
    def test_missing_tools_are_skipped(self):
        entries = mm_legacy.external_findings(self.root, which=lambda name: None)
        self.assertEqual(len(entries), 3)
        for entry in entries:
            self.assertEqual(entry['path'], '')
            self.assertIn('not installed', entry['reason'])
        self.assertIn('cargo machete', entries[0]['reason'])

    def test_output_is_parsed(self):
        def which(name):
            return f'/usr/bin/{name}'

        def runner(cmd, root, timeout=120):
            if cmd[0] == 'vulture':
                return 0, "tools/dep.py:7: unused function 'dead' (60% confidence)"
            if cmd[0] == 'ruff':
                return 1, 'tools/dep.py:3:5: F401: `os` imported but unused'
            return 1, 'mm-core -- /opt/crates/demo/Cargo.toml:'

        entries = mm_legacy.external_findings(self.root, which=which, runner=runner)
        got = {(e['path'], e['name'], e['line']) for e in entries}
        self.assertIn(('tools/dep.py', 'dead', 7), got)
        self.assertIn(('tools/dep.py', 'os', 3), got)
        self.assertIn(('/opt/crates/demo/Cargo.toml', 'mm-core', 1), got)
        vulture = next(e for e in entries
                       if e['path'] == 'tools/dep.py' and e['line'] == 7)
        self.assertIn('unused function', vulture['reason'])


class AllowlistSection(LegacyFixture):
    def test_path_name_entry(self):
        self.write('crates/demo/src/lib.rs', 'pub fn dead_helper() {}\n')
        allow = self.write('tools/legacy_allow.txt',
                           '# known false positive\n\ncrates/demo/src/lib.rs:dead_helper\n')
        report = mm_legacy.collect(self.root, only={'rust'}, allowlist_path=allow)
        self.assertEqual(report['rust']['count'], 0)
        self.assertEqual(report['rust']['entries'], [])

    def test_glob_entry(self):
        self.write('crates/demo/src/lib.rs', 'pub fn dead_helper() {}\n')
        allow = self.write('tools/legacy_allow.txt', 'crates/*/src/lib.rs:*\n')
        report = mm_legacy.collect(self.root, only={'rust'}, allowlist_path=allow)
        self.assertEqual(report['rust']['count'], 0)

    def test_default_allowlist_path(self):
        self.write('tools/dead.py', 'def orphan():\n    pass\n')
        self.write('tools/legacy_allow.txt', 'tools/dead.py:orphan\n')
        report = mm_legacy.collect(self.root, only={'py'})
        self.assertEqual(report['py']['count'], 0)

    def test_other_findings_survive(self):
        self.write('crates/demo/src/lib.rs', 'pub fn dead_helper() {}\npub fn other() {}\n')
        allow = self.write('tools/legacy_allow.txt', 'crates/demo/src/lib.rs:dead_helper\n')
        report = mm_legacy.collect(self.root, only={'rust'}, allowlist_path=allow)
        self.assertEqual(report['rust']['count'], 1)
        self.assertEqual(report['rust']['entries'][0]['name'], 'other')


class EnvSection(LegacyFixture):
    def test_env_files_are_never_read(self):
        self.write('.env', 'token_name = "x"\n')
        self.write('tools/.env', 'token_name\n')
        self.write('tools/.env.py', 'def token_name():\n    pass\n')
        self.write('tools/dead.py', 'def token_name():\n    pass\n')
        got = self.names(mm_legacy.py_findings(self.root))
        # A reference that only exists in .env* must not rescue the finding,
        # and .env.py is not scanned for definitions either.
        self.assertIn(('tools/dead.py', 'token_name'), got)
        self.assertNotIn(('tools/.env.py', 'token_name'), got)
        paths = [path for path, _ in mm_legacy._iter_text_files(self.root)]
        self.assertEqual(paths, ['tools/dead.py'])
        for scan in (mm_legacy.rust_findings, mm_legacy.ts_findings,
                     mm_legacy.marker_findings, mm_legacy.docs_findings,
                     mm_legacy.locale_findings):
            scan(self.root)  # a tree with .env files must not crash any section


class MainSection(LegacyFixture):
    def test_json_shape_and_only(self):
        self.write('docs/a.md', '`no/such/file.py`\n')
        self.write('tools/dead.py', 'def orphan():\n    pass\n')
        buffer = io.StringIO()
        with contextlib.redirect_stdout(buffer):
            code = mm_legacy.main(['--json', '--only', 'docs'], root=self.root)
        self.assertEqual(code, 0)
        data = json.loads(buffer.getvalue())
        self.assertEqual(set(data['sections']), {'docs'})
        entry = data['sections']['docs']['entries'][0]
        self.assertEqual(entry['path'], 'docs/a.md')
        self.assertEqual(entry['line'], 1)
        self.assertEqual(entry['name'], 'no/such/file.py')
        self.assertIn('dead docs path', entry['reason'])
        self.assertEqual(data['sections']['docs']['count'], 1)

    def test_exit_zero_with_findings_without_strict(self):
        self.write('docs/a.md', '`no/such/file.py`\n')
        with mock.patch.object(mm_legacy, 'external_findings', lambda root: []):
            with contextlib.redirect_stdout(io.StringIO()):
                code = mm_legacy.main([], root=self.root)
        self.assertEqual(code, 0)

    def test_strict_exit_codes(self):
        self.write('docs/a.md', '`no/such/file.py`\n')
        with mock.patch.object(mm_legacy, 'external_findings', lambda root: []):
            with contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(mm_legacy.main(['--strict'], root=self.root), 1)
            with contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(mm_legacy.main(['--strict', '--only', 'rust'],
                                                root=self.root), 0)

    def test_limit(self):
        self.write('src/m.ts', ''.join(f'// legacy thing {i}\n' for i in range(5)))
        report = mm_legacy.collect(self.root, only={'markers'}, limit=2)
        self.assertEqual(report['markers']['count'], 5)
        self.assertEqual(len(report['markers']['entries']), 2)
        self.assertEqual(report['markers']['truncated'], 3)

    def test_text_render(self):
        self.write('src/m.ts', '// no longer used\n')
        report = mm_legacy.collect(self.root, only={'markers'})
        text = mm_legacy.render(report, self.root, 40)
        self.assertIn('== markers (1)', text)
        self.assertIn('src/m.ts:1  comment marker: no longer', text)


if __name__ == '__main__':
    unittest.main()
