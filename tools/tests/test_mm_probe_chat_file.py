"""The pure parts of `mm probe chat --file`: reading QUESTIONS.jsonl, the per model x
language summary table, and the batch argument flags (no network, no cargo, no keys)."""

import argparse
import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

import mm_probe_chat
from mm_probe import ProbeError

SPARK = 'muse-spark-1.3'
WONDER = 'muse-wonder-1.0'


def make_record(model=SPARK, lang='en', first_ms=1000, error=''):
    """An answer row as ask_one writes it; summarize only reads model/lang/first_ms/error."""
    return {'id': 1, 'lang': lang, 'model': model, 'q': 'q',
            'answer': '' if error else 'answer', 'first_ms': first_ms,
            'total_ms': 2000, 'tokens_in': 10, 'tokens_out': 20, 'error': error}


class ReadQuestionsTest(unittest.TestCase):
    def read(self, text: str) -> list[dict]:
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / 'questions.jsonl'
            path.write_text(text, encoding='utf-8')
            return mm_probe_chat.read_questions(str(path))

    def test_reads_lines_in_order(self):
        questions = self.read('{"id": 1, "lang": "en", "q": "one"}\n{"id": 2, "lang": "ru", "q": "two"}\n')
        self.assertEqual(questions, [{'id': 1, 'lang': 'en', 'q': 'one'},
                                     {'id': 2, 'lang': 'ru', 'q': 'two'}])

    def test_skips_blank_lines(self):
        self.assertEqual(len(self.read('\n{"id": 1, "lang": "en", "q": "one"}\n\n  \n')), 1)

    def test_bad_json_names_the_line(self):
        with self.assertRaises(ProbeError) as caught:
            self.read('{"id": 1, "lang": "en", "q": "one"}\nnot json\n')
        self.assertIn('line 2', str(caught.exception))

    def test_missing_key_names_the_line_and_key(self):
        with self.assertRaises(ProbeError) as caught:
            self.read('{"id": 1, "lang": "en"}\n')
        self.assertIn('line 1', str(caught.exception))
        self.assertIn('q', str(caught.exception))

    def test_non_object_line_rejected(self):
        with self.assertRaises(ProbeError) as caught:
            self.read('[1, 2, 3]\n')
        self.assertIn('object', str(caught.exception))

    def test_bad_lang_rejected(self):
        with self.assertRaises(ProbeError) as caught:
            self.read('{"id": 1, "lang": "fr", "q": "one"}\n')
        self.assertIn('en or ru', str(caught.exception))

    def test_missing_file(self):
        with self.assertRaises(ProbeError):
            mm_probe_chat.read_questions('no/such/questions.jsonl')


class ChatSummaryTableTest(unittest.TestCase):
    def test_summary_groups_by_model_then_lang(self):
        records = [make_record(model=SPARK, lang='ru'), make_record(model=WONDER, lang='en'),
                   make_record(model=SPARK, lang='en'), make_record(model=WONDER, lang='ru')]
        rows = mm_probe_chat.summarize(records)
        self.assertEqual([(row['model'], row['lang']) for row in rows],
                         [(SPARK, 'en'), (SPARK, 'ru'), (WONDER, 'en'), (WONDER, 'ru')])
        for row in rows:
            self.assertEqual((row['answered'], row['errors']), (1, 0))

    def test_median_and_p90_of_first_token(self):
        records = [make_record(first_ms=ms) for ms in range(100, 1100, 100)]  # 100..1000
        (row,) = mm_probe_chat.summarize(records)
        self.assertEqual(row['answered'], 10)
        self.assertEqual(row['errors'], 0)
        self.assertEqual(row['median_ms'], 550)
        self.assertEqual(row['p90_ms'], 900)  # nearest rank: ceil(0.9 * 10) = 9th value

    def test_single_answer_median_and_p90(self):
        (row,) = mm_probe_chat.summarize([make_record(first_ms=1234)])
        self.assertEqual(row['median_ms'], 1234)
        self.assertEqual(row['p90_ms'], 1234)

    def test_errors_counted_and_excluded_from_timings(self):
        records = [make_record(first_ms=100), make_record(first_ms=200),
                   make_record(first_ms=None, error='stream not completed')]
        (row,) = mm_probe_chat.summarize(records)
        self.assertEqual((row['answered'], row['errors']), (2, 1))
        self.assertEqual(row['median_ms'], 150)
        self.assertEqual(row['p90_ms'], 200)

    def test_failed_stream_with_first_token_is_not_a_timing(self):
        records = [make_record(first_ms=500, error='connection lost mid-stream (TimeoutError)')]
        (row,) = mm_probe_chat.summarize(records)
        self.assertEqual((row['answered'], row['errors']), (0, 1))
        self.assertIsNone(row['median_ms'])
        self.assertIsNone(row['p90_ms'])

    def test_group_with_only_errors_has_no_timings(self):
        rows = mm_probe_chat.summarize(
            [make_record(model=WONDER, lang='ru', first_ms=None, error='HTTP 500: boom'),
             make_record(model=WONDER, lang='ru', first_ms=None, error='no response.completed event')])
        (row,) = rows
        self.assertEqual((row['answered'], row['errors']), (0, 2))
        line = mm_probe_chat.format_summary(rows).splitlines()[1]
        self.assertIn('-', line)

    def test_empty_summary_is_header_only(self):
        lines = mm_probe_chat.format_summary(mm_probe_chat.summarize([])).splitlines()
        self.assertEqual(len(lines), 1)
        for word in ('model', 'lang', 'answered', 'errors', 'median', 'p90'):
            self.assertIn(word, lines[0])

    def test_summary_table_header_and_one_line_per_group(self):
        records = [make_record(model=SPARK, lang='en'), make_record(model=SPARK, lang='ru'),
                   make_record(model=WONDER, lang='en')]
        rows = mm_probe_chat.summarize(records)
        lines = mm_probe_chat.format_summary(rows).splitlines()
        self.assertEqual(len(lines), 4)
        self.assertIn('answered', lines[0])
        for row, line in zip(rows, lines[1:]):
            self.assertIn(row['model'], line)
            self.assertIn(f"{row['lang']:<4}", line)


class ChatArgumentParsingTest(unittest.TestCase):
    def test_file_batch_flags(self):
        args = mm_probe_chat.parse_chat_args(
            ['--file', 'fixtures/eval/chat/questions.jsonl',
             '--models', 'muse-spark-1.3,muse-wonder-1.0', '--jobs', '8', '--out', 'answers.jsonl'])
        self.assertEqual(args.file, 'fixtures/eval/chat/questions.jsonl')
        self.assertEqual(args.models, ['muse-spark-1.3', 'muse-wonder-1.0'])
        self.assertEqual(args.jobs, 8)
        self.assertEqual(args.out, 'answers.jsonl')
        self.assertIs(args.func, mm_probe_chat.cmd_chat)

    def test_default_models_and_jobs(self):
        args = mm_probe_chat.parse_chat_args(['--file', 'questions.jsonl'])
        self.assertEqual(args.models, [SPARK, WONDER])
        self.assertEqual(args.jobs, 4)
        self.assertIsNone(args.out)

    def test_whitespace_around_models_is_ignored(self):
        args = mm_probe_chat.parse_chat_args(['--file', 'q.jsonl', '--models', ' muse-wonder-1.0 '])
        self.assertEqual(args.models, [WONDER])

    def test_unknown_model_rejected(self):
        with self.assertRaises(SystemExit):
            mm_probe_chat.parse_chat_args(['--file', 'q.jsonl', '--models', 'gpt-5'])

    def test_single_question_mode_defaults(self):
        args = mm_probe_chat.parse_chat_args(['where is the save button?'])
        self.assertEqual(args.question, 'where is the save button?')
        self.assertIsNone(args.file)
        self.assertEqual(args.repeat, 1)

    def test_no_question_and_no_file_rejected(self):
        with self.assertRaises(ProbeError):
            mm_probe_chat.cmd_chat(mm_probe_chat.parse_chat_args([]))


class ChatBatchValidationTest(unittest.TestCase):
    def args_for(self, **overrides):
        values = {'provider': 'meta', 'question': None, 'repeat': 1, 'jobs': 4,
                  'models': [SPARK, WONDER], 'out': None, 'file': 'questions.jsonl',
                  'timeout': 120, 'budget': 10.0, 'max_unpriced': 30, 'price': None,
                  'reasoning': None, 'name': None, 'about': None}
        values.update(overrides)
        return argparse.Namespace(**values)

    def test_question_and_file_are_mutually_exclusive(self):
        with self.assertRaises(ProbeError):
            mm_probe_chat.cmd_chat_file(self.args_for(question='both'))

    def test_repeat_with_file_rejected(self):
        with self.assertRaises(ProbeError):
            mm_probe_chat.cmd_chat_file(self.args_for(repeat=3))

    def test_jobs_below_one_rejected(self):
        with self.assertRaises(ProbeError):
            mm_probe_chat.cmd_chat_file(self.args_for(jobs=0))

    def test_empty_question_file_rejected_before_any_call(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / 'questions.jsonl'
            path.write_text('\n', encoding='utf-8')
            with self.assertRaises(ProbeError) as caught:
                mm_probe_chat.cmd_chat_file(self.args_for(file=str(path)))
        self.assertIn('no questions', str(caught.exception))


if __name__ == '__main__':
    unittest.main()
