"""mm where and mm cite on small temporary trees (no git: they walk the folder)."""

import contextlib
import io
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import mm_cite
import mm_where


def tree(files):
    directory = tempfile.TemporaryDirectory()
    root = Path(directory.name)
    for path, text in files.items():
        Path(root, path).parent.mkdir(parents=True, exist_ok=True)
        Path(root, path).write_text(text, encoding='utf-8')
    return directory, root


NO_GIT = patch('subprocess.run', side_effect=OSError('no git here'))


class WhereTests(unittest.TestCase):
    def setUp(self):
        self.directory, self.root = tree({
            'src-tauri/src/auth.rs': ('#[tauri::command]\npub fn sign_out(app: AppHandle) {\n'
                                     '    forget(&app);\n}\n#[cfg(test)]\nmod tests {\n'
                                     '    fn t() { sign_out(x); }\n}\n'),
            'src/account.ts': ('export async function signOut() {\n  await invoke("sign_out");\n}\n'
                               'store.signOut();\n'),
            'src/account.test.ts': 'test("sign_out", () => {});\n',
            'locales/en.json': '{\n  "robot.sign_in_again": "Sign in again."\n}\n',
            'locales/ru.json': '{\n  "robot.sign_in_again": "Войдите снова."\n}\n',
            'src-tauri/src/voice.rs': 'let key = "robot.sign_in_again";\n',
            'docs/notes.md': 'The sign-out button.\n',
            '.env': 'sign_out=secret\n',
        })
        self.addCleanup(self.directory.cleanup)

    def test_spellings(self):
        self.assertEqual(mm_where.variants('sign_out')[:4], ['sign_out', 'signOut', 'SignOut', 'sign-out'])
        self.assertIn('sign_out', mm_where.variants('signOut'))
        self.assertEqual(mm_where.variants('robot.sign_in_again'), ['robot.sign_in_again'])
        self.assertEqual(mm_where.variants('x'), ['x'])

    def test_definitions_commands_tests_and_areas(self):
        with NO_GIT:
            hits = mm_where.search(self.root, 'sign_out')
        by_place = {(h['path'], h['line']): h for h in hits}
        self.assertEqual(by_place[('src-tauri/src/auth.rs', 2)]['kind'], 'command')
        self.assertEqual(by_place[('src-tauri/src/auth.rs', 7)]['area'], 'tests')
        self.assertEqual(by_place[('src/account.ts', 1)]['kind'], 'def')
        self.assertEqual(by_place[('src/account.ts', 2)]['area'], 'ui')
        self.assertIn(('src/account.ts', 4), by_place, 'a member call is a use')
        self.assertEqual(by_place[('src/account.test.ts', 1)]['area'], 'tests')
        self.assertEqual(by_place[('docs/notes.md', 1)]['area'], 'docs')
        self.assertFalse(any(h['path'] == '.env' for h in hits), 'a secrets file is never read')
        text = mm_where.report('sign_out', hits)
        self.assertIn('[tauri command]', text)
        self.assertIn('[invoke]', text)
        self.assertLess(text.index('defined'), text.index('ui ('))

    def test_locale_key(self):
        with NO_GIT:
            hits = mm_where.search(self.root, 'robot.sign_in_again')
        self.assertEqual(sorted(h['kind'] for h in hits), ['key', 'key', 'use'])
        self.assertIn('ru: Войдите снова.', mm_where.locale_text(self.root, 'robot.sign_in_again'))
        self.assertIsNone(mm_where.locale_text(self.root, 'robot.nothing'))

    def test_event_names_are_followed_through_their_constants(self):
        directory, root = tree({
            'src-tauri/src/card.rs': 'const EVENT: &str = "lesson-card";\nfn send() { app.emit_to(W, EVENT, c); }\n',
            'src-tauri/src/other.rs': 'const EVENT: &str = "other";\nfn x() { emit(EVENT); }\n',
            'src/card.ts': 'export const LESSON_CARD_EVENT = "lesson-card";\n',
            'src/card-page.ts': 'await listen(LESSON_CARD_EVENT, f);\n',
        })
        self.addCleanup(directory.cleanup)
        with NO_GIT:
            hits = mm_where.search(root, 'lesson-card')
            hits += mm_where.via_constants(root, 'lesson-card', hits)
        via = {(h['path'], h['via']) for h in hits if h.get('via')}
        # A Rust constant counts in its own file only; an exported TS one anywhere.
        self.assertEqual(via, {('src-tauri/src/card.rs', 'EVENT'), ('src/card-page.ts', 'LESSON_CARD_EVENT')})
        text = mm_where.report('lesson-card', hits)
        self.assertIn('[constant]', text)
        self.assertIn('via LESSON_CARD_EVENT (1)', text)

    def test_words_from_a_window_lead_to_their_key(self):
        self.assertEqual(mm_where.keys_with_text(self.root, 'войдите СНОВА'), ['robot.sign_in_again'])
        self.assertEqual(mm_where.keys_with_text(self.root, '«Sign in again»'), ['robot.sign_in_again'])
        self.assertEqual(mm_where.keys_with_text(self.root, 'zz'), [])

    def test_caps_each_area(self):
        hits = [{'path': f'src/a{i}.ts', 'line': 1, 'text': 'x', 'kind': 'use', 'area': 'ui'} for i in range(9)]
        text = mm_where.report('x', hits, limit=3)
        self.assertIn('+6 more', text)
        self.assertNotIn('+6 more', mm_where.report('x', hits, limit=3, show_all=True))


class CiteParseTests(unittest.TestCase):
    def parsed(self, text):
        return [(p.replace('\\', '/'), a, b) for p, a, b, _ in mm_cite.references(text)]

    def test_spellings_agents_write(self):
        for text, expected in [
            ('src-tauri/src/auth.rs:272', ('src-tauri/src/auth.rs', 272, 272)),
            ('auth.rs:272', ('auth.rs', 272, 272)),
            ('crates/mm-core/src/api/intent.rs:449-516', ('crates/mm-core/src/api/intent.rs', 449, 516)),
            ('see `src/voice.rs:10-25` here', ('src/voice.rs', 10, 25)),
            (r'crates\mm-win32\src\overlay.rs:15-30', ('crates/mm-win32/src/overlay.rs', 15, 30)),
            ('file.rs#L12-L20', ('file.rs', 12, 20)),
            ('file.rs:12:5', ('file.rs', 12, 12)),
            ('[auth](src-tauri/src/auth.rs#L50)', ('src-tauri/src/auth.rs', 50, 50)),
            ('src-tauri/src/flight.rs (line 12)', ('src-tauri/src/flight.rs', 12, 12)),
            ('tools/mm.py (lines 88-100)', ('tools/mm.py', 88, 100)),
            ('lines 12-14 of crates/mm-core/src/lib.rs', ('crates/mm-core/src/lib.rs', 12, 14)),
            ('line 63 of CLAUDE.md', ('CLAUDE.md', 63, 63)),
            ('src-tauri/src/ptt.rs:45 — handles release', ('src-tauri/src/ptt.rs', 45, 45)),
            ('Defined in crates/mm-win32/src/keyboard.rs:104.', ('crates/mm-win32/src/keyboard.rs', 104, 104)),
            ('(implemented in src/audio/mixer.ts:80)', ('src/audio/mixer.ts', 80, 80)),
            ('src/home.ts:12–14', ('src/home.ts', 12, 14)),
        ]:
            with self.subTest(text=text):
                self.assertEqual(self.parsed(text), [expected])

    def test_line_only_continuations_take_the_previous_path(self):
        self.assertEqual(self.parsed('`crates/mm-core/src/api/intent.rs:449`, `:516`'),
                         [('crates/mm-core/src/api/intent.rs', 449, 449),
                          ('crates/mm-core/src/api/intent.rs', 516, 516)])

    def test_not_references(self):
        for text in ('https://example.com:8080/index.html', 'version 1.3:2', 'at 10:30', 'e17:3',
                     'Muse Spark 1.3', 'ratio 4.5:1'):
            with self.subTest(text=text):
                self.assertEqual(self.parsed(text), [])

    def test_repeats_are_dropped(self):
        self.assertEqual(len(self.parsed('a.rs:1 and again a.rs:1')), 1)


class CiteShowTests(unittest.TestCase):
    def setUp(self):
        self.directory, self.root = tree({
            'src-tauri/src/voice.rs': ''.join(f'line {n}\n' for n in range(1, 21)),
            'src/a/util.ts': 'x\n', 'src/b/util.ts': 'y\n',
            '.env': 'KEY=secret\n',
        })
        self.addCleanup(self.directory.cleanup)

    def show(self, text):
        with NO_GIT:
            return mm_cite.show(self.root, text, context=1)

    def test_lines_with_the_claim(self):
        report, problems = self.show('voice.rs:10 uses an undefined `error`')
        self.assertEqual(problems, 0)
        self.assertIn('» voice.rs:10 uses an undefined `error`', report)
        self.assertIn('src-tauri/src/voice.rs:10 (as voice.rs)', report)
        self.assertIn('>   10 line 10', report)
        self.assertIn('     9 line 9', report)

    def test_problems_are_flagged(self):
        report, problems = self.show('nope.rs:3 and src-tauri/src/voice.rs:99 and util.ts:1')
        self.assertEqual(problems, 3)
        self.assertIn('MISSING', report)
        self.assertIn('not in the file (20 lines)', report)
        self.assertIn('AMBIGUOUS', report)

    def test_secrets_are_never_opened(self):
        for text in ('.env:1', '.ENV:1', 'sub/.env.local:2', 'see `.Env`:1'):
            with self.subTest(text=text):
                report, _ = self.show(text)
                self.assertNotIn('KEY=', report)
        self.assertTrue(mm_cite.references('.env:1'), 'a secrets file is recognised, then refused')
        for name in ('.env', '.ENV', '.env.local.json', '.Env.'):
            with contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(mm_cite.main([name], root=self.root), 2)
            self.assertTrue(mm_where.is_secret(name))

    def test_review_cases(self):
        report, problems = self.show('voice.rs:10-999 and voice.rs:0 and ../outside.rs:1')
        self.assertEqual(problems, 3)
        self.assertIn('outside the project', report)
        report, problems = self.show(r'C:\Users\Acer Swift\proj\src-tauri\src\voice.rs:3')
        self.assertEqual(problems, 0, report)
        self.assertIn('>    3 line 3', report)
        report, problems = self.show('`src-tauri/src/voice.rs`:4 and /src-tauri/src/voice.rs:5')
        self.assertEqual(problems, 0, report)
        self.assertIn('>    4 line 4', report)
        self.assertIn('>    5 line 5', report)
        self.assertEqual(mm_cite.references('https://github.com/o/r/src/voice.rs:10'), [])


if __name__ == '__main__':
    unittest.main()
