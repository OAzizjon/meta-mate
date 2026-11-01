"""Tmp-tree tests for tools/mm_cover.py (`mm cover PATH [--json]`)."""

import contextlib
import io
import json
import sys
import tempfile
import unittest
from pathlib import Path

TOOLS = Path(__file__).absolute().parent.parent
sys.path.insert(0, str(TOOLS))

import mm_cover  # noqa: E402

RUST_SRC = """\
pub struct Point {
    pub x: i32,
    pub y: i32,
}

pub fn distance(a: &Point, b: &Point) -> i32 {
    (b.x - a.x).abs() + (b.y - a.y).abs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_distance() {
        assert_eq!(distance(&Point { x: 1, y: 1 }, &Point { x: 1, y: 1 }), 0);
    }
}
"""


class CoverTest(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory(prefix='mm-cover-')
        self.addCleanup(self._tmp.cleanup)
        self.root = Path(self._tmp.name)

    def write(self, rel, text):
        path = self.root / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding='utf-8')
        return rel

    def run_cover(self, *argv):
        stream = io.StringIO()
        with contextlib.redirect_stdout(stream):
            code = mm_cover.main(list(argv), root=self.root)
        return code, stream.getvalue()

    # --- Rust ----------------------------------------------------------------

    def test_rust_inline_use_mention(self):
        self.write('crates/demo/Cargo.toml', '[package]\nname = "demo"\n')
        self.write('crates/demo/src/lib.rs', 'pub mod coords;\n')
        self.write('crates/demo/src/coords.rs', RUST_SRC)
        self.write('crates/demo/tests/coords.rs',
                   'use demo::coords::{distance, Point};\n\n#[test]\nfn walks() {}\n')
        self.write('crates/demo/tests/legacy.rs',
                   '#[test]\nfn old_way() {\n    let _ = demo::coords::distance;\n}\n')
        self.write('crates/demo/tests/nothing.rs', '#[test]\nfn empty() {}\n')
        code, out = self.run_cover('crates/demo/src/coords.rs')
        self.assertEqual(code, 0)
        self.assertIn('cargo test -p demo --lib coords', out)
        self.assertIn('cargo test -p demo --test coords', out)
        self.assertIn('cargo test -p demo --test legacy', out)
        self.assertNotIn('tests/nothing.rs', out)
        self.assertLess(out.index('tests/coords.rs'), out.index('tests/legacy.rs'))

    def test_rust_src_tauri_package_name(self):
        self.write('src-tauri/Cargo.toml', '[package]\nname = "metamate-app"\nversion = "0.1.0"\n')
        self.write('src-tauri/src/proxy.rs',
                   'pub fn call() {}\n\n#[cfg(test)]\nmod t {\n    #[test]\n    fn it_works() {}\n}\n')
        self.write('src-tauri/tests/proxy.rs', 'use metamate_app::proxy;\n\n#[test]\nfn roundtrip() {}\n')
        code, out = self.run_cover('src-tauri/src/proxy.rs')
        self.assertEqual(code, 0)
        self.assertIn('cargo test -p metamate-app --lib proxy', out)
        self.assertIn('cargo test -p metamate-app --test proxy', out)

    def test_rust_test_file_itself(self):
        self.write('crates/demo/Cargo.toml', '[package]\nname = "demo"\n')
        self.write('crates/demo/tests/coords.rs', '#[test]\nfn walks() {}\n')
        code, out = self.run_cover('crates/demo/tests/coords.rs')
        self.assertEqual(code, 0)
        self.assertIn('cargo test -p demo --test coords', out)
        self.assertIn('itself', out)

    # --- TypeScript -----------------------------------------------------------

    def test_ts_direct_and_hop(self):
        self.write('src/stores/account.ts', 'export function signOut() {}\n')
        self.write('src/stores/index.ts', "export * from './account';\n")
        self.write('src/stores/account.test.ts',
                   "import { signOut } from './account';\n\ntest('signs out', () => {});\n")
        self.write('src/app.test.ts',
                   "import { signOut } from './stores';\n\ntest('app', () => {});\n")
        self.write('src/other.test.ts', "test('other', () => {});\n")
        code, out = self.run_cover('src/stores/account.ts')
        self.assertEqual(code, 0)
        self.assertIn('npm test -- src/stores/account.test.ts', out)
        self.assertIn('npm test -- src/app.test.ts', out)
        self.assertNotIn('other.test.ts', out)
        self.assertLess(out.index('account.test.ts'), out.index('app.test.ts'))

    def test_ts_mention_and_no_tests(self):
        self.write('src/lib/mmBus.ts', 'export const mmBus = {};\n')
        self.write('src/legacy.test.ts', "vi.mock('./lib/mmBus');\ntest('legacy', () => {});\n")
        code, out = self.run_cover('src/lib/mmBus.ts')
        self.assertEqual(code, 0)
        self.assertIn('npm test -- src/legacy.test.ts', out)
        self.assertIn('mentions it', out)
        self.write('src/plain.ts', 'export const plain = 1;\n')
        code, out = self.run_cover('src/plain.ts')
        self.assertEqual(code, 1)
        self.assertIn('NO TESTS', out)

    # --- Python ----------------------------------------------------------------

    def test_python_naming_and_imports(self):
        self.write('tools/mm_thing.py', 'def run():\n    return 1\n')
        self.write('tools/tests/test_mm_thing.py',
                   'import mm_thing\n\nimport unittest\n\n\nclass T(unittest.TestCase):\n'
                   '    def test_run(self):\n        self.assertEqual(mm_thing.run(), 1)\n')
        self.write('tools/tests/test_other.py',
                   'import mm_thing as thing\nimport unittest\n\n\nclass T(unittest.TestCase):\n'
                   '    def test_x(self):\n        self.assertTrue(thing.run())\n')
        self.write('tools/tests/test_from_parent.py', 'from tools import mm_thing\n')
        code, out = self.run_cover('tools/mm_thing.py')
        self.assertEqual(code, 0)
        self.assertIn('python -m unittest tools.tests.test_mm_thing', out)
        self.assertIn('python -m unittest tools.tests.test_other', out)
        self.assertIn('python -m unittest tools.tests.test_from_parent', out)
        self.assertLess(out.index('test_mm_thing.py'), out.index('test_from_parent.py'))

    def test_python_no_tests(self):
        self.write('server/metamate_proxy/policy.py', 'X = 1\n')
        code, out = self.run_cover('server/metamate_proxy/policy.py')
        self.assertEqual(code, 1)
        self.assertIn('NO TESTS', out)

    # --- output shape -----------------------------------------------------------

    def test_json(self):
        self.write('tools/mm_jsonable.py', 'X = 1\n')
        self.write('tools/tests/test_mm_jsonable.py', '')
        code, out = self.run_cover('--json', 'tools/mm_jsonable.py')
        self.assertEqual(code, 0)
        data = json.loads(out)
        self.assertEqual(data['path'], 'tools/mm_jsonable.py')
        self.assertEqual(data['language'], 'python')
        self.assertEqual(len(data['tests']), 1)
        self.assertEqual(data['tests'][0]['command'], 'python -m unittest tools.tests.test_mm_jsonable')

    def test_errors_and_secrets(self):
        code, out = self.run_cover('docs/missing.md')
        self.assertEqual(code, 2)
        self.assertIn('no such file', out)
        self.write('README.md', '# hi\n')
        code, out = self.run_cover('README.md')
        self.assertEqual(code, 2)
        self.assertIn('not a source file', out)
        self.write('.env', 'METAMATE_TOKEN=not-a-real-secret\n')
        code, out = self.run_cover('.env')
        self.assertEqual(code, 2)
        self.assertIn('never opens a secrets file', out)


if __name__ == '__main__':
    unittest.main()
