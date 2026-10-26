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

    def test_russian_values_use_cyrillic_and_english_values_do_not(self):
        english = self.load_locale("en")
        russian = self.load_locale("ru")

        for key in english:
            with self.subTest(key=key):
                self.assertIsNone(CYRILLIC.search(english[key]))
                self.assertIsNotNone(CYRILLIC.search(russian[key]))


if __name__ == "__main__":
    unittest.main()
