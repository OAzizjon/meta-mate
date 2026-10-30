"""Tests for the code map generator on a small fixed tree."""

import contextlib
import io
import json
from pathlib import Path
import sys
import tempfile
import textwrap
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import mm_map


FILES = {
    'crates/demo/src/lib.rs': """
        //! Demo crate: shapes and their
        //! names. Second sentence here.
        //!
        //! Details paragraph.

        pub mod shapes;

        /// Not a module doc.
        pub fn area(width: u32, height: u32) -> u32 {
            width * height
        }

        pub(crate) const LIMIT: u32 = 3;

        const RAW: &str = r#"pub fn fake() {} // "quoted" { "#;
        const QUOTE: char = '"';

        /* outer /* nested pub fn hidden() {} */ still a comment { */
        pub struct Square<'a> {
            pub name: &'a str,
        }

        impl Square<'_> {
            pub fn side(&self) -> u32 { 1 }
        }

        pub async fn later() {}

        #[cfg(test)]
        pub fn only_for_tests() {}

        #[cfg(test)]
        mod tests {
            #[test]
            fn area_works() {
                assert_eq!(super::area(2, 3), 6);
            }

            #[test]
            fn literals_in_tests_are_ignored() {
                let _ = "robot.unused";
            }
        }
    """,
    'crates/demo/src/shapes.rs': """
        //! Shapes.

        pub trait Shape {}
    """,
    'crates/demo/tests/shapes.rs': """
        #[test]
        fn one() {}

        #[tokio::test]
        async fn two() {}

        // #[test] in a comment
        const S: &str = "#[test]";
    """,
    'src-tauri/src/lib.rs': """
        //! The shell.

        const EVENT: &str = "settings-changed";
        const HEADER: &str = "X-MetaMate-Call";

        #[tauri::command]
        pub async fn load_settings(app: AppHandle) -> Result<(), String> {
            app.emit(EVENT, ()).ok();
            app.emit_to("mate", "mate-woke", ()).ok();
            let _ = crate::locale_text("en", "robot.hello");
            Ok(())
        }

        #[tauri::command]
        fn unused_command() {}

        pub enum CallKind {
            Answer,
            Sam,
        }

        impl CallKind {
            fn header(self) -> &'static str {
                match self {
                    Self::Answer => "answer",
                    Self::Sam => "sam",
                }
            }
        }

        pub fn respond(base: &str, provider: &str) -> String {
            let _ = CallKind::Answer;
            let url = format!("{}/v1/responses", base);
            let _start = format!("{base}/auth/{provider}/start?port=1");
            // format!("{}/v1/tts") in a comment is not a caller
            url
        }

        #[cfg(test)]
        mod tests {
            #[test]
            fn emits_in_tests_are_ignored() {
                super::APP.emit("test-only", ());
            }
        }
    """,
    'src/settings.ts': """
        // Settings window: loads and saves.
        // Second line of the header.

        import { invoke } from "@tauri-apps/api/core";
        import { listen } from "@tauri-apps/api/event";
        import { SETTINGS_CHANGED_EVENT } from "./model.ts";

        const QUOTES = /"/g; export const AFTER_REGEX = 1;
        const URL = "https://example.com/a//b"; export const AFTER_URL = 1;
        const HALF = 4 / 2; export const AFTER_DIVISION = "x"; // 2 / 1

        export async function load(): Promise<unknown> {
          // invoke("unused_command") in a comment does not count
          await listen<{ minutes: number }>(SETTINGS_CHANGED_EVENT, () => undefined);
          await listen("mate-woke", () => undefined);
          const label = t("chat.send");
          const dynamic = t(`settings.key_${"f9"}`);
          return invoke("load_settings");
        }

        export type Mode = "calm" | "fun";
        export interface Shape { readonly name: string }
        export class Store {}
        const local = 1;
    """,
    'src/model.ts': """
        /**
         * Settings model: pure.
         */
        export const SETTINGS_CHANGED_EVENT = "settings-changed" as const;
    """,
    'src/after-imports.ts': """
        import { load } from "./settings.ts";

        // Comes after the imports.
        export function again() { return load(); }
    """,
    'src/env.d.ts': """
        /// <reference types="vite/client" />
    """,
    'src/settings.test.ts': """
        import { test } from "node:test";
        test("one", () => {});
        test("two", () => { invoke("load_settings"); });
        it.skip("three", () => {});
    """,
    'server/metamate_proxy/app.py': '''
        """The proxy app: routes.

        More text.
        """
        from enum import Enum


        class CallKind(Enum):
            ANSWER = "answer"
            SAM = "sam"


        def register(app):
            @app.post('/v1/responses')
            async def responses(request):
                return request.headers.get('x-metamate-call', 'answer')

            @app.get('/auth/{provider}/start')
            async def start(provider):
                return provider

            @app.post('/v1/tts')
            async def tts(request):
                return request

            @app.websocket('/v1/asr')
            async def asr(socket):
                return socket


        def _private():
            pass
    ''',
    'tools/demo.py': '''
        """Demo tool. It does nothing."""


        def main(argv=None):
            return 0
    ''',
    'tools/tests/test_demo.py': '''
        import unittest


        class DemoTests(unittest.TestCase):
            def test_one(self):
                pass

            def test_two(self):
                pass

            def helper(self):
                pass
    ''',
}
LOCALE = {'robot.hello': 'Hello', 'robot.unused': 'Unused', 'chat.send': 'Send', 'settings.key_f9': 'F9'}


class CodeMapTests(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.root = Path(directory.name)
        for rel, text in FILES.items():
            path = self.root / rel
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(textwrap.dedent(text).lstrip('\n'), encoding='utf-8')
        (self.root / 'locales').mkdir()
        (self.root / 'locales' / 'en.json').write_text(json.dumps(LOCALE), encoding='utf-8')
        self.text = mm_map.render(self.root)
        self.lines = self.text.splitlines()

    def line(self, rel, needle):
        text = (self.root / rel).read_text(encoding='utf-8')
        return text[:text.index(needle)].count('\n') + 1

    def items_after(self, entry):
        return self.lines[self.lines.index(entry) + 1]

    def test_header_and_determinism(self):
        self.assertEqual(self.lines[0], mm_map.HEADER)
        self.assertIn('do not edit', self.lines[0])
        self.assertEqual(mm_map.render(self.root), self.text)
        self.assertNotIn('\n\n\n', self.text)

    def test_rust_purpose_and_items_skip_comments_strings_impls_and_tests(self):
        rel = 'crates/demo/src/lib.rs'
        self.assertIn(f'- {rel} — Demo crate: shapes and their names.', self.lines)
        expected = ' '.join(f'{name}:{self.line(rel, needle)}' for name, needle in (
            ('shapes', 'pub mod shapes'), ('area', 'pub fn area'), ('LIMIT', 'pub(crate) const'),
            ('Square', 'pub struct'), ('later', 'pub async fn later')))
        self.assertEqual(self.items_after(f'- {rel} — Demo crate: shapes and their names.'), '  ' + expected)
        for hidden in ('fake', 'hidden', 'side:', 'only_for_tests', 'RAW', 'QUOTE'):
            self.assertNotIn(hidden, self.text)
        self.assertIn('- crates/demo/src/shapes.rs — Shapes.', self.lines)

    def test_typescript_purpose_and_exports_survive_regex_urls_and_division(self):
        rel = 'src/settings.ts'
        entry = f'- {rel} — Settings window: loads and saves.'
        self.assertIn(entry, self.lines)
        names = [token.split(':')[0] for token in self.items_after(entry).split()]
        self.assertEqual(names, ['AFTER_REGEX', 'AFTER_URL', 'AFTER_DIVISION', 'load', 'Mode', 'Shape', 'Store'])
        self.assertIn('- src/model.ts — Settings model: pure.', self.lines)
        self.assertIn('- src/after-imports.ts — Comes after the imports.', self.lines)
        self.assertIn('- src/env.d.ts', self.lines)
        self.assertNotIn('settings.test.ts —', self.text)

    def test_python_purpose_and_public_items(self):
        rel = 'server/metamate_proxy/app.py'
        entry = f'- {rel} — The proxy app: routes.'
        self.assertEqual(self.items_after(entry), f'  CallKind:{self.line(rel, "class CallKind")} '
                                                  f'register:{self.line(rel, "def register")}')
        self.assertIn('- tools/demo.py — Demo tool.', self.lines)
        sections = [line for line in self.lines if line.startswith('### ')]
        self.assertEqual(sections[:5], ['### crates/demo', '### src-tauri', '### src', '### tools',
                                        '### server/metamate_proxy'])

    def test_tauri_commands_and_their_typescript_callers(self):
        rust, ts = 'src-tauri/src/lib.rs', 'src/settings.ts'
        self.assertIn(f'- `load_settings` {rust}:{self.line(rust, "load_settings")} <- '
                      f'{ts}:{self.line(ts, "invoke(\"load_settings")}', self.lines)
        self.assertIn(f'- `unused_command` {rust}:{self.line(rust, "unused_command")} <- no TypeScript caller',
                      self.lines)

    def test_events_resolve_constants_on_both_sides(self):
        rust, ts = 'src-tauri/src/lib.rs', 'src/settings.ts'
        self.assertIn(f'- `settings-changed`: rs emit {rust}:{self.line(rust, "emit(EVENT")}; '
                      f'ts listen {ts}:{self.line(ts, "listen<")}; '
                      f'const {rust}:{self.line(rust, "const EVENT")}, src/model.ts:4', self.lines)
        self.assertIn(f'- `mate-woke`: rs emit {rust}:{self.line(rust, "emit_to")}; '
                      f'ts listen {ts}:{self.line(ts, "listen(\"mate-woke")}', self.lines)
        self.assertNotIn('test-only', self.text)

    def test_routes_match_rust_urls_but_not_comments(self):
        app, rust = 'server/metamate_proxy/app.py', 'src-tauri/src/lib.rs'
        self.assertIn(f'- GET /auth/{{provider}}/start {app}:{self.line(app, "@app.get")} <- '
                      f'{rust}:{self.line(rust, "/auth/")}', self.lines)
        self.assertIn(f'- POST /v1/responses {app}:{self.line(app, "/v1/responses")} <- '
                      f'{rust}:{self.line(rust, "/v1/responses")}', self.lines)
        self.assertIn(f'- POST /v1/tts {app}:{self.line(app, "/v1/tts")} <- no Rust caller', self.lines)
        self.assertIn(f'- WS /v1/asr {app}:{self.line(app, "/v1/asr")} <- no Rust caller', self.lines)

    def test_call_kinds_pair_server_values_with_rust_variants(self):
        app, rust = 'server/metamate_proxy/app.py', 'src-tauri/src/lib.rs'
        self.assertIn(f'- header: rs {rust}:{self.line(rust, "X-MetaMate-Call")}; '
                      f'py {app}:{self.line(app, "x-metamate-call")}', self.lines)
        self.assertIn(f'- `answer`: py {app}:{self.line(app, "ANSWER =")}; '
                      f'rs {rust}:{self.line(rust, "Self::Answer")}; '
                      f'used {rust}:{self.line(rust, "CallKind::Answer")}', self.lines)
        self.assertIn(f'- `sam`: py {app}:{self.line(app, "SAM =")}; rs {rust}:{self.line(rust, "Self::Sam")}',
                      self.lines)

    def test_locale_keys_from_rust_and_typescript(self):
        rust, ts = 'src-tauri/src/lib.rs', 'src/settings.ts'
        self.assertIn(f'- `robot.hello`: rs {rust}:{self.line(rust, "robot.hello")}', self.lines)
        self.assertIn(f'- `chat.send`: ts {ts}:{self.line(ts, "chat.send")}', self.lines)
        unused = self.lines.index('- no literal use (built at run time, or unused):')
        self.assertEqual(self.lines[unused + 1], '  `robot.unused` `settings.key_f9`')

    def test_tests_index_counts_real_tests_only(self):
        tests = self.lines[self.lines.index('## Tests'):]
        for expected in ('### crates/demo: 4 tests in 2 files', '- crates/demo/src/lib.rs: 2',
                         '- crates/demo/tests/shapes.rs: 2', '### src-tauri: 1 tests in 1 files',
                         '- src-tauri/src/lib.rs: 1', '### src: 3 tests in 1 files', '- src/settings.test.ts: 3',
                         '### tools: 2 tests in 1 files', '- tools/tests/test_demo.py: 2'):
            self.assertIn(expected, tests)
        self.assertNotIn('- crates/demo/src/shapes.rs: 0', tests)

    def test_main_writes_then_check_detects_stale_file(self):
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            self.assertEqual(mm_map.main(['--check'], root=self.root), 1)
            self.assertFalse((self.root / mm_map.OUTPUT).exists())
            self.assertEqual(mm_map.main([], root=self.root), 0)
            self.assertEqual((self.root / mm_map.OUTPUT).read_bytes(), self.text.encode('utf-8'))
            self.assertEqual(mm_map.main(['--check'], root=self.root), 0)
            with (self.root / 'tools' / 'demo.py').open('a', encoding='utf-8') as stream:
                stream.write('\n\nclass Added:\n    pass\n')
            self.assertEqual(mm_map.main(['--check'], root=self.root), 1)
        self.assertIn('stale', out.getvalue())


class ScanTests(unittest.TestCase):
    def test_rust_raw_strings_chars_and_nested_comments(self):
        text = 'let s = r##"a "# b"##; let c = \'"\'; /* /* x */ y */ let d = b"e"; let l: &\'a str;'
        code, bare, strings, comments = mm_map.mask(text, 'rust')
        self.assertEqual(sorted(strings.values()), ['a "# b', 'e'])
        self.assertEqual([body for _, body in comments], ['/* /* x */ y */'])
        self.assertEqual(len(code), len(text))
        self.assertEqual(len(bare), len(text))
        self.assertNotIn('x', bare.replace('let', ''))
        self.assertIn("&'a str", bare)

    def test_typescript_templates_with_holes_are_not_values(self):
        text = 'const a = `x${"b" + `c${d}`}e`; const z = "z"; const t = `plain`;'
        _, bare, strings, _ = mm_map.mask(text, 'ts')
        self.assertEqual(sorted(strings.values()), ['plain', 'z'])
        self.assertTrue(bare.endswith('const t = "     ";'))

    def test_summary_takes_the_first_sentence_and_caps_its_length(self):
        self.assertEqual(mm_map.summary('One line, e.g. this one. Two.\nmore\n\nOther.'), 'One line, e.g. this one.')
        self.assertEqual(mm_map.summary('No full stop\nhere'), 'No full stop here')
        long = mm_map.summary('word ' * 100)
        self.assertTrue(long.endswith(' …'))
        self.assertLessEqual(len(long), mm_map.SUMMARY_CHARS + 2)


if __name__ == '__main__':
    unittest.main()
