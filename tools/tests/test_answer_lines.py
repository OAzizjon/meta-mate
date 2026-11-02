"""Every robot.* line Rust can send reaches Mate's bubble (src/ipc/answer-stream.ts).

The WebView drops a `line` message whose key is not in ANSWER_LINE_KEYS, so a key added in
Rust and forgotten there is silent: the look-consent question once never showed, and Mate
seemed to freeze for 30 seconds. Keys that are only ever played as cached audio are listed
in AUDIO_ONLY. Rust names line keys as whole string literals (never built with concat! or
format!), so this scan sees every one.
"""
from pathlib import Path
import re
import unittest


ROOT = Path(__file__).resolve().parents[2]
KEY = re.compile(r'"(robot\.[a-z0-9_]+)"')
# Wait fillers: voice.rs plays their WAV while the model works and never sends them as lines.
AUDIO_ONLY = {'robot.thinking', 'robot.thinking_2', 'robot.thinking_3', 'robot.looking',
              'robot.checking_safety'}


def production(source):
    """The file without its test module."""
    return re.split(r'^#\[cfg\(test\)\]\s*mod ', source, maxsplit=1, flags=re.M)[0]


def rust_keys():
    keys = {}
    for folder in [ROOT / 'src-tauri/src', *sorted((ROOT / 'crates').glob('*/src'))]:
        for path in sorted(folder.rglob('*.rs')):
            if path.name == 'tests.rs' or 'tests' in path.relative_to(ROOT).parts[:-1]:
                continue
            for key in KEY.findall(production(path.read_text(encoding='utf-8'))):
                keys.setdefault(key, path.relative_to(ROOT).as_posix())
    return keys


def line_keys():
    source = (ROOT / 'src/ipc/answer-stream.ts').read_text(encoding='utf-8')
    block = source.split('export const ANSWER_LINE_KEYS = [', 1)[1].split(']', 1)[0]
    return set(KEY.findall(block))


class AnswerLineTests(unittest.TestCase):
    def test_every_rust_line_is_shown(self):
        allowed = line_keys() | AUDIO_ONLY
        missing = {key: path for key, path in rust_keys().items() if key not in allowed}
        self.assertEqual(missing, {}, 'add these to ANSWER_LINE_KEYS (or AUDIO_ONLY if never a line)')

    def test_audio_only_keys_are_still_used(self):
        self.assertLessEqual(AUDIO_ONLY, set(rust_keys()))

    def test_the_consent_bubble_lasts_as_long_as_rust_waits(self):
        rust = (ROOT / 'src-tauri/src/screen.rs').read_text(encoding='utf-8')
        wait = re.search(r'offered\.elapsed\(\) < Duration::from_secs\((\d+)\)', rust)
        page = (ROOT / 'src/mate-logic.ts').read_text(encoding='utf-8')
        shown = re.search(r'LOOK_CONSENT_MS = (\d+)_000;', page)
        self.assertIsNotNone(wait)
        self.assertIsNotNone(shown)
        self.assertEqual(wait.group(1), shown.group(1))

    def test_the_whitelist_has_no_stale_keys(self):
        # Lines Mate's own window shows (src/mate-logic.ts), never sent by Rust.
        window_only = {'robot.consent_expired', 'robot.cant_speak'}
        stale = line_keys() - set(rust_keys()) - window_only
        self.assertEqual(stale, set())


if __name__ == '__main__':
    unittest.main()
