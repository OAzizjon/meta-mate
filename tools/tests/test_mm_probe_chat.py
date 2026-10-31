"""Unit tests for `mm probe chat`: argument parsing and the --repeat table. No network
and no cargo: the parser object is built, but no probe function talks to the API."""
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).absolute().parent.parent))

import mm_probe
import mm_probe_chat


class ChatArgumentParsingTest(unittest.TestCase):
    def parse(self, argv):
        return mm_probe.build_parser().parse_args(argv)

    def test_question_only_keeps_the_app_defaults(self):
        args = self.parse(['chat', 'Where is the save button?'])
        self.assertEqual(args.cmd, 'chat')
        self.assertEqual(args.question, 'Where is the save button?')
        self.assertIs(args.func, mm_probe_chat.cmd_chat)
        self.assertIsNone(args.lang)
        self.assertIsNone(args.name)
        self.assertIsNone(args.about)
        self.assertIsNone(args.model)
        self.assertEqual(args.repeat, 1)
        self.assertEqual(args.provider, 'meta')
        self.assertEqual(args.budget, mm_probe.DEFAULT_BUDGET_USD)
        self.assertEqual(args.max_unpriced, 30)

    def test_every_flag(self):
        args = self.parse(['chat', 'Say hello', '--lang', 'ru', '--name', 'Anya',
                           '--about', 'a retired teacher', '--model', 'muse-wonder-1.0',
                           '--reasoning', 'low', '--repeat', '3', '--budget', '2.5',
                           '--max-unpriced', '5', '--price', '4', '20'])
        self.assertEqual((args.lang, args.name, args.about), ('ru', 'Anya', 'a retired teacher'))
        self.assertEqual(args.model, 'muse-wonder-1.0')
        self.assertEqual(args.reasoning, 'low')
        self.assertEqual(args.repeat, 3)
        self.assertEqual(args.budget, 2.5)
        self.assertEqual(args.max_unpriced, 5)
        self.assertEqual(args.price, [4.0, 20.0])

    def test_model_and_lang_come_from_fixed_lists(self):
        for argv in (['chat', 'Hi', '--model', 'meta-llama/llama-4-scout-17b-16e-instruct'],
                     ['chat', 'Hi', '--lang', 'de']):
            with self.assertRaises(SystemExit, msg=argv):
                self.parse(argv)

    def test_bad_repeat_value_is_rejected(self):
        for bad in ('often', '2.5'):
            with self.assertRaises(SystemExit, msg=bad):
                self.parse(['chat', 'Hi', '--repeat', bad])

    def test_non_meta_provider_is_refused_before_any_network(self):
        args = self.parse(['chat', 'Hi', '--provider', 'groq'])
        with self.assertRaises(mm_probe.ProbeError):
            mm_probe_chat.cmd_chat(args)

    def test_repeat_must_be_positive_at_runtime(self):
        args = self.parse(['chat', 'Hi', '--repeat', '-1'])
        with self.assertRaises(mm_probe.ProbeError):
            mm_probe_chat.cmd_chat(args)


class ChatTableTest(unittest.TestCase):
    def rows(self):
        return [{'sentences': 2, 'show': True, 'name': 'Anya', 'chars': 84},
                {'sentences': 1, 'show': False, 'name': '-', 'chars': 41}]

    def test_header_and_one_line_per_run(self):
        lines = mm_probe_chat.format_table(self.rows()).splitlines()
        self.assertEqual(len(lines), 3)
        for word in ('run', 'sentences', '<show>', 'name', 'chars'):
            self.assertIn(word, lines[0])
        self.assertTrue(lines[1].startswith('  1'))
        self.assertIn('yes', lines[1])
        self.assertIn('Anya', lines[1])
        self.assertIn('84', lines[1])
        self.assertTrue(lines[2].startswith('  2'))
        self.assertNotIn('yes', lines[2])
        self.assertIn('-', lines[2])
        self.assertIn('41', lines[2])

    def test_empty_table_is_header_only(self):
        self.assertEqual(len(mm_probe_chat.format_table([]).splitlines()), 1)


if __name__ == '__main__':
    unittest.main()
