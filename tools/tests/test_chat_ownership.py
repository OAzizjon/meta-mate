"""Source guards for locale ownership and passive chat updates (not browser tests)."""
from pathlib import Path
import unittest


ROOT = Path(__file__).resolve().parents[2]


class ChatOwnershipTests(unittest.TestCase):
    def test_chat_locale_is_owned_by_settings(self):
        source = (ROOT / 'src/chat.ts').read_text(encoding='utf-8')
        self.assertIn('followSettingsLocale(', source)
        self.assertNotIn('initI18n', source)
        self.assertNotIn('setLocale', source)

    def test_passive_history_commands_do_not_open_or_activate_windows(self):
        source = (ROOT / 'src-tauri/src/chat.rs').read_text(encoding='utf-8')
        commands = source.split('pub fn chat_subscribe(', 1)[1]
        commands = commands.split('#[cfg(test)]', 1)[0]
        for operation in ('activate_on_request', 'set_focus(', '.show(', 'open_chat(', 'presence::wake'):
            self.assertNotIn(operation, commands)
