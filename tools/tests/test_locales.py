"""Tests for the English and Russian locale files."""

import json
from pathlib import Path
import re
import unittest


ROOT = Path(__file__).resolve().parents[2]
PLACEHOLDER = re.compile(r"\{[^{}]+\}")
CYRILLIC = re.compile(r"[\u0400-\u04ff]")


class LocaleTests(unittest.TestCase):
    def load_locale(self, language):
        path = ROOT / "locales" / f"{language}.json"
        with path.open(encoding="utf-8") as locale_file:
            return json.load(locale_file)

    def test_locales_have_matching_nonempty_values_and_placeholders(self):
        english = self.load_locale("en")
        russian = self.load_locale("ru")

        self.assertEqual(set(english), set(russian))
        for key in english:
            with self.subTest(key=key):
                self.assertIsInstance(english[key], str)
                self.assertTrue(english[key].strip())
                self.assertIsInstance(russian[key], str)
                self.assertTrue(russian[key].strip())
                self.assertEqual(set(PLACEHOLDER.findall(english[key])),
                                 set(PLACEHOLDER.findall(russian[key])))

    def test_language_choices_have_native_names_and_localized_headings(self):
        for language in ('en', 'ru'):
            strings = self.load_locale(language)
            self.assertEqual(strings['settings.language_en'], 'English')
            self.assertEqual(strings['settings.language_ru'], '\u0420\u0443\u0441\u0441\u043a\u0438\u0439')
            self.assertIn('Windows', strings['settings.language_auto'])
            self.assertTrue(strings['settings.language_title'].strip())
    def test_russian_values_use_cyrillic_and_english_values_do_not(self):
        english = self.load_locale("en")
        russian = self.load_locale("ru")

        for key in english:
            with self.subTest(key=key):
                if key != 'settings.language_ru':
                    self.assertIsNone(CYRILLIC.search(english[key]))
                # Hardware keys, language names and Mate's name retain their native spelling.
                if key not in {'settings.language_en', 'chat.from_mate', 'settings.key_capsLock', 'settings.key_insert',
                               'settings.key_f9', 'settings.key_f10',
                               'settings.keycap_ctrl', 'settings.keycap_rightAlt',
                               'settings.keycap_capsLock', 'settings.keycap_insert',
                               'settings.keycap_f9', 'settings.keycap_f10',
                               'hints.key_fn', 'hints.key_f8',
                               # Meta's model names.
                               'chat.model_spark', 'chat.model_wonder', 'home.model_quick',
                               'home.model_thorough', 'home.model_open'}:
                    self.assertIsNotNone(CYRILLIC.search(russian[key]))


if __name__ == "__main__":
    unittest.main()
