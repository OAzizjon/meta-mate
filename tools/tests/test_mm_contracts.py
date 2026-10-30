"""Cross-language contract tests on small, offline source trees."""

import argparse
import contextlib
import io
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import mm
import mm_checks
import mm_contracts


class ContractTests(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.root = Path(directory.name)
        self.write('locales/en.json', json.dumps({'robot.ready': 'Ready'}))
        self.write('locales/ru.json', json.dumps({'robot.ready': 'Ready'}))

    def write(self, rel, text):
        path = self.root / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding='utf-8')

    def command(self, params='', name='speak', attribute=''):
        self.write('src-tauri/src/voice.rs',
                   f'#[tauri::command{attribute}]\npub async fn {name}({params}) {{}}\n')
        self.write('src-tauri/src/lib.rs', f'tauri::generate_handler![voice::{name}];\n')

    def findings(self):
        return mm_contracts.findings(self.root)

    def test_registration_both_directions_and_unknown_invoke(self):
        self.command()
        self.write('src-tauri/src/lib.rs', 'tauri::generate_handler![voice::typo];')
        self.write('src/main.ts', 'invoke("speak"); invoke("typo");')
        self.assertEqual(self.findings(), [
            "src-tauri/src/lib.rs:1  registered command 'typo' has no #[tauri::command] function",
            "src-tauri/src/voice.rs:2  command 'speak' is not registered in generate_handler!",
            "src/main.ts:1  invoke command 'speak' is not registered",
        ])

    def test_camel_case_shorthand_nested_values_and_injected_types(self):
        self.command("app: tauri::AppHandle, state: State<'_, HashMap<String, Vec<u8>>>, "
                     "window: WebviewWindow, webview: tauri::Webview, request: tauri::ipc::Request<'_>, "
                     "voice_speed: f32, text: String, detail: Option<String>, channel: Channel<Answer>")
        self.write('src/main.ts', '''invoke<void>("speak", {
            voiceSpeed: choose(1, 2), text, channel: { nested: [1, 2] },
        });''')
        self.assertEqual(self.findings(), [])

    def test_missing_unknown_quoted_and_shorthand_keys(self):
        self.command('voice_speed: f32, text: String')
        self.write('src/main.ts', '''invoke("speak", { "voiceSpeed": 1, text });
invoke("speak", { voice_speed: 1 });
invoke("speak");''')
        self.assertEqual(self.findings(), [
            "src/main.ts:2  invoke 'speak': missing argument 'text'",
            "src/main.ts:2  invoke 'speak': missing argument 'voiceSpeed'",
            "src/main.ts:2  invoke 'speak': unknown argument 'voice_speed'",
            "src/main.ts:3  invoke 'speak': missing argument 'text'",
            "src/main.ts:3  invoke 'speak': missing argument 'voiceSpeed'",
        ])

    def test_explicit_snake_case_attribute_and_raw_identifier(self):
        self.command('voice_speed: f32, r#type: String', attribute='(rename_all = "snake_case")')
        self.write('src/main.ts', 'invoke("speak", { voice_speed: 1, type: "a" });')
        self.assertEqual(self.findings(), [])

    def test_dynamic_args_only_verbose_and_optional_args(self):
        self.command('text: String, detail: Option<String>')
        self.write('src/main.ts', '''invoke("speak", args);
invoke("speak", { ...args });
invoke("speak", { [key]: value });
invoke("speak", { text });''')
        for verbose in (False, True):
            output = io.StringIO()
            with contextlib.redirect_stdout(output):
                code = mm_contracts.main(['--verbose'] if verbose else [], root=self.root)
            self.assertEqual(code, 0)
            self.assertEqual(output.getvalue().count('skipped dynamic'), 3 if verbose else 0)

    def test_events_constants_emit_to_once_and_typescript_emit(self):
        self.write('src-tauri/src/lib.rs', '''const READY: &str = "mate-ready";
app.emit_to("mate", READY, ());
app.emit("orphan", ());
app.emit("robot.ready", ());''')
        self.write('src/main.ts', '''const READY = "mate-ready";
once<void>(READY, handler);
listen("missing", handler);
emit("ts-only", {}); listen("ts-only", handler);
listen("robot.ready", handler);''')
        self.assertEqual(self.findings(), [
            "src-tauri/src/lib.rs:3  Rust event 'orphan' has no TypeScript listen/once",
            "src/main.ts:3  listened event 'missing' has no Rust or TypeScript emitter",
        ])
        with patch.object(mm_contracts, 'EVENT_ALLOWLIST', {'orphan', 'missing'}):
            self.assertEqual(self.findings(), [])

    def test_qualified_constants_resolve_across_files(self):
        self.write('src-tauri/src/names.rs', 'pub const READY: &str = "mate-ready";')
        self.write('src-tauri/src/lib.rs', 'app.emit(crate::names::READY, ());')
        self.write('src/main.ts', 'listen("mate-ready", handler);')
        self.assertEqual(self.findings(), [])

    def test_locale_missing_each_language_known_prefix_and_static_template(self):
        self.write('locales/en.json', json.dumps({'robot.ready': 'Ready', 'robot.only_en': 'English'}))
        self.write('src-tauri/src/lib.rs', 'let a = r#"robot.absent"#;\nlet b = "robot.only_en";')
        self.write('src/main.ts', 't(`robot.absent`); t(`robot.${state}`); "unknown.foo";')
        self.assertEqual(self.findings(), [
            "src-tauri/src/lib.rs:1  locale key 'robot.absent' missing from locales/en.json",
            "src-tauri/src/lib.rs:1  locale key 'robot.absent' missing from locales/ru.json",
            "src-tauri/src/lib.rs:2  locale key 'robot.only_en' missing from locales/ru.json",
            "src/main.ts:1  locale key 'robot.absent' missing from locales/en.json",
            "src/main.ts:1  locale key 'robot.absent' missing from locales/ru.json",
        ])

    def test_comments_literals_regex_and_test_code_are_ignored(self):
        self.write('src-tauri/src/lib.rs', '''// app.emit("fake", ());
/* #[tauri::command] fn fake() {} */
let text = r###"app.emit("fake", ()); robot.absent"###;
#[cfg(test)] mod tests {
    #[tauri::command] fn fake() {}
    fn check() { app.emit("fake", ()); let s = "robot.absent"; }
}''')
        self.write('src/main.ts', '''// invoke("fake");
const example = 'invoke("fake")';
const pattern = /listen("fake")/;
const dynamic = `robot.${state}`;
/* listen("fake", handler); */''')
        self.write('src/main.test.ts', 'invoke("fake"); listen("fake", handler); "robot.absent";')
        self.assertEqual(self.findings(), [])

    def test_filenames_are_not_locale_keys_but_other_missing_keys_are(self):
        self.write('card.html', '<html></html>')
        self.write('locales/en.json', json.dumps({'card.title': 'Card', 'chat.title': 'Chat'}))
        self.write('locales/ru.json', json.dumps({'card.title': 'Card', 'chat.title': 'Chat'}))
        self.write('src-tauri/src/lib.rs', '''const PAGE: &str = "card.html";
const FILE_NAME: &str = "chat.json";
const TEMP_NAME: &str = "chat.json.tmp";
path.join("chat.json");
WebviewUrl::App("chat.html".into());
builder(app, "card", "card.html");
let missing = "chat.absent";''')
        self.assertEqual(self.findings(), [
            "src-tauri/src/lib.rs:7  locale key 'chat.absent' missing from locales/en.json",
            "src-tauri/src/lib.rs:7  locale key 'chat.absent' missing from locales/ru.json",
        ])
    def test_missing_locale_and_main_return_failure(self):
        (self.root / 'locales/ru.json').unlink()
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            self.assertEqual(mm_contracts.main([], root=self.root), 1)
        self.assertEqual(output.getvalue(),
                         'locales/ru.json:1  locale file is missing or is not a JSON object\n')

    def test_registered_offline_check_and_agent_default_selection(self):
        self.command('text: String')
        self.write('src/main.ts', 'invoke("speak");')
        result = mm_checks.run_checks(self.root, only={'contracts'})[0]
        self.assertFalse(result.ok)
        self.assertEqual(result.details, self.findings())
        with patch.object(mm, '_extend_path'), patch.object(mm, '_load_checks', return_value=mm_checks), \
                patch.object(mm_checks, 'run_checks', return_value=[]) as checks:
            self.assertEqual(mm.cmd_check(argparse.Namespace(only=None), agent=True), 0)
        self.assertIn('contracts', checks.call_args.kwargs['only'])


if __name__ == '__main__':
    unittest.main()
