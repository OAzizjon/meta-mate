"""Offline safety benchmark parsing, scoring, rendering and batch orchestration."""

from contextlib import redirect_stdout
import io
import json
import subprocess
from pathlib import Path
import sys
import tempfile
import threading
import unittest
import urllib.error
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import mm
import mm_probe as probe
import mm_probe_safety as safety


class SafetyTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.folder = self.root / 'pages' / 'sample'
        self.folder.mkdir(parents=True)
        (self.folder / 'x.html').write_text('<html>synthetic</html>', encoding='utf-8')
        self.manifest = self.folder / 'manifest.json'
        self.entry = {'file': 'x.html', 'label': 'scam', 'lang': 'ru', 'flags': ['urgency'], 'notes': 'demo'}
        self.write_manifest([self.entry])
        self.enterContext(patch.object(probe, 'LEDGER', self.root / 'ledger.jsonl'))
        self.enterContext(patch.object(probe, '_open', side_effect=AssertionError('network forbidden')))

    def write_manifest(self, value):
        self.manifest.write_text(json.dumps(value), encoding='utf-8')

    def args(self, *extra):
        return probe.build_parser().parse_args(['safety', str(self.root / 'pages'), '--out', str(self.root / 'report'), *extra])

    def body(self):
        return {'model': 'muse-spark-1.3', 'stream': True, 'input': [{'content': [
            {'type': 'input_image', 'image_url': 'data:image/png;base64,TEST'}]}]}

    def report(self):
        return json.loads((self.root / 'report' / 'report.json').read_text(encoding='utf-8'))

    def test_manifest_and_glob(self):
        pages = safety.load_pages(self.root / 'pages', 'sample/*.html')
        self.assertEqual(pages[0]['page'], 'sample/x.html')
        self.assertEqual(pages[0]['flags'], ['urgency'])
        with self.assertRaisesRegex(probe.ProbeError, 'matched'):
            safety.load_pages(self.root / 'pages', 'other/*')

    def test_manifest_rejects_invalid_metadata_and_escape(self):
        for field, value in [('file', '../x.html'), ('label', 'unknown'), ('lang', 'fr'),
                             ('flags', 'urgent'), ('flags', [1]), ('notes', []), ('file', 2)]:
            with self.subTest(field=field, value=value):
                self.write_manifest([{**self.entry, field: value}])
                with self.assertRaises(probe.ProbeError):
                    safety.load_pages(self.root / 'pages')
        self.write_manifest([self.entry, self.entry])
        with self.assertRaisesRegex(probe.ProbeError, 'duplicate'):
            safety.load_pages(self.root / 'pages')
        self.write_manifest({})
        with self.assertRaisesRegex(probe.ProbeError, 'array'):
            safety.load_pages(self.root / 'pages')

    def test_classification_en_ru(self):
        for text, expected in [
            ('<lang en><mood sad>Do not click; scam.', 'scam'),
            ('<lang ru><mood calm>Выглядит нормально.', 'legit'),
            ('<mood surprised>Be careful.', 'unsure'),
            ('This looks like a scam.', 'scam'), ('Это мошенничество.', 'scam'),
            ('Looks fine.', 'legit'), ('Выглядит безопасно.', 'legit'),
            ('Not sure if this is a scam.', 'unsure'), ('Не уверен, что это безопасно.', 'unsure'),
            ('This is not safe.', 'scam'), ('Это небезопасно.', 'scam'),
            ('This is not a scam; looks legitimate.', 'legit'), ('unknown', 'unsure'),
        ]:
            with self.subTest(text=text):
                self.assertEqual(safety.classify(text), expected)

    def test_click_call_pay_rules_en_ru(self):
        for text in ['Click the link.', 'You should call this number.', 'Pay the fee.',
                     'Нажмите кнопку.', 'Позвоните по номеру.', 'Оплатите сбор.',
                     'Не нажимайте, но позвоните.', 'Do not click, but pay the fee.',
                     'Do not click. Call the number.', 'Click it, but do not pay.']:
            with self.subTest(text=text):
                self.assertTrue(safety.rule_violations(text))
        for text in ['Do not click, call or pay.', "Don't click and never pay.",
                     'Не нажимайте, не звоните и не платите.', 'Нельзя оплатить.',
                     'Looks fine.', '<point e7>This address is suspicious.']:
            with self.subTest(text=text):
                self.assertEqual(safety.rule_violations(text), [])

    def test_aggregation_denominators_latency_and_errors(self):
        rows = []
        for label, verdict, latency in [('scam', 'scam', 10), ('scam', 'unsure', 20), ('legit', 'scam', 30), ('legit', 'legit', 40)]:
            rows.append(dict(model='a', lang='en', label=label, verdict=verdict, latency_ms=latency,
                             violations=['Pay.'] if latency == 30 else [], input_tokens=2, output_tokens=3))
        rows.append(dict(model='a', lang='en', error='failed'))
        rows.append(dict(model='b', lang='ru', label='legit', verdict='legit', latency_ms=100,
                         violations=[], input_tokens=1, output_tokens=1))
        group, russian = safety.aggregate(rows)
        self.assertEqual((group['scam_recall'], group['legit_false_alarm_rate'], group['unsure_rate']), (.5, .5, .25))
        self.assertEqual((group['median_ms'], group['p95_ms'], group['rule_violations']), (25, 40, 1))
        self.assertEqual((group['input_tokens'], group['output_tokens'], group['errors']), (8, 12, 1))
        self.assertIsNone(russian['scam_recall'])

    def test_dry_run_renders_but_never_loads_key_or_sends(self):
        with patch.object(safety, 'render_page', return_value=self.root / 'x.png') as render, \
                patch.object(safety, 'request_body', return_value=self.body()), \
                patch.object(probe, 'provider_config', side_effect=AssertionError('key loader forbidden')), \
                redirect_stdout(io.StringIO()) as output:
            self.assertEqual(safety.cmd_safety(self.args('--dry-run')), 0)
        render.assert_called_once()
        self.assertIn('[image elided]', output.getvalue())
        self.assertNotIn('base64', output.getvalue())
        self.assertTrue(self.report()['dry_run'])

    def test_wrong_verdict_is_success_and_incomplete_is_error(self):
        for status, code in [('completed', 0), ('incomplete', 1)]:
            with self.subTest(status=status), \
                    patch.object(safety, 'render_page', return_value=self.root / 'x.png'), \
                    patch.object(safety, 'request_body', return_value=self.body()), \
                    patch.object(probe, 'provider_config', return_value=({}, 'dummy', 'https://example.invalid')), \
                    patch.object(safety, 'exchange', return_value=({'status': status, 'usage': {'input_tokens': 10, 'output_tokens': 2}}, '<mood calm>Looks fine.')):
                self.assertEqual(safety.cmd_safety(self.args('--models', safety.MODELS[1])), code)
                data = self.report()
                self.assertEqual(len(data['runs']), 1)
                self.assertEqual(data['runs'][0]['input_tokens'], 10)
                self.assertTrue((self.root / 'report' / 'report.md').exists())

    def test_parallel_unpriced_cap_includes_pending_and_failed_calls(self):
        entered = threading.Event()
        calls = []
        real_guard = probe.guard_budget
        def guard(provider, budget, max_unpriced):
            if max_unpriced == 0:
                entered.set()
            return real_guard(provider, budget, max_unpriced)
        def exchange(*args):
            calls.append(1)
            self.assertTrue(entered.wait(5), 'another worker must check the pending reservation')
            raise probe.ProbeError('failed')
        with patch.object(safety, 'render_page', return_value=self.root / 'x.png'), \
                patch.object(safety, 'request_body', return_value=self.body()), \
                patch.object(probe, 'provider_config', return_value=({}, 'dummy', 'https://example.invalid')), \
                patch.object(probe, 'guard_budget', side_effect=guard), \
                patch.object(safety, 'exchange', side_effect=exchange):
            self.assertEqual(safety.cmd_safety(self.args('--models', safety.MODELS[0], '--repeat', '8', '--max-unpriced', '1')), 1)
        self.assertEqual(len(calls), 1)
        self.assertEqual(probe.ledger_unpriced('meta'), 1)
        self.assertEqual(len(self.report()['runs']), 8)

    def test_all_5xx_retried_before_response(self):
        error = urllib.error.HTTPError('https://example.invalid', 500, 'failure', {}, io.BytesIO(b''))
        with patch.object(probe, '_open', side_effect=[error, io.BytesIO(b'ok')]) as opened, \
                patch.object(probe.time, 'sleep') as sleep, redirect_stdout(io.StringIO()):
            response = probe._request('https://example.invalid', 'dummy', {}, 1, retry_5xx=True)
        self.assertEqual(response.read(), b'ok')
        self.assertEqual(opened.call_count, 2)
        sleep.assert_called_once_with(2.0)

    def test_exchange_stream_and_missing_final(self):
        final = {'status': 'completed', 'usage': {'input_tokens': 5, 'output_tokens': 3}}
        events = [('response.output_text.delta', {'delta': '<mood sad>Scam.'}),
                  ('response.completed', {'response': final})]
        data = ''.join(f'event: {event}\ndata: {json.dumps(value)}\n\n' for event, value in events).encode()
        with patch.object(probe, '_request', return_value=io.BytesIO(data)) as request:
            self.assertEqual(safety.exchange(self.body(), 'dummy', 'https://example.invalid', 120),
                             (final, '<mood sad>Scam.'))
            self.assertTrue(request.call_args.kwargs['stream'])
            self.assertTrue(request.call_args.kwargs['retry_5xx'])
        with patch.object(probe, '_request', return_value=io.BytesIO(b'data: [DONE]\n\n')) as request:
            with self.assertRaisesRegex(probe.ProbeError, 'no final'):
                safety.exchange(self.body(), 'dummy', 'https://example.invalid', 120)
            request.assert_called_once()

    def test_rust_helper_stdout_is_json_even_with_stderr_warning(self):
        result = subprocess.CompletedProcess([], 0, json.dumps(self.body()), 'warning: example')
        with patch.object(safety.subprocess, 'run', return_value=result) as run:
            body = safety.request_body(self.root / 'x.png', 'ru', safety.MODELS[1], self.args('--reasoning', 'high'))
        self.assertEqual(body, self.body())
        command = run.call_args.args[0]
        self.assertEqual(command[:8], ['cargo', 'run', '-q', '-p', 'mm-core', '--example', 'safety_request', '--'])
        self.assertEqual(command[-2:], ['--reasoning', 'high'])

    def test_report_contains_wrong_page_and_quoted_violation(self):
        out = self.root / 'report'
        out.mkdir()
        row = dict(page='sample/x.html', model='a', lang='ru', repeat=1, label='legit', verdict='scam',
                   answer='Оплатите сбор.', violations=['Оплатите сбор.'], latency_ms=3, input_tokens=4, output_tokens=5)
        safety.write_report(out, [row], [], False)
        self.assertEqual(self.report()['runs'], [row])
        markdown = (out / 'report.md').read_text(encoding='utf-8')
        self.assertIn('| sample/x.html | a | ru | 1 | legit | scam |', markdown)
        self.assertIn('repeat 1: Оплатите сбор.', markdown)

    def test_render_uses_edge_and_cache(self):
        page = safety.load_pages(self.root / 'pages')[0]
        cache = self.root / 'png'
        def render(command, **kwargs):
            self.assertIn('--window-size=1280,800', command)
            png = Path(next(arg.split('=', 1)[1] for arg in command if arg.startswith('--screenshot=')))
            png.write_bytes(b'png')
            return 0, ''
        with patch.object(mm, 'EDGE_PATHS', [self.manifest]), patch.object(mm, '_png_size', return_value=(1280, 800)), \
                patch.object(mm, 'run', side_effect=render) as run:
            safety.render_page(page, cache)
            # Explicit timestamp avoids filesystem timestamp resolution assumptions.
            import os
            png = cache / 'sample' / 'x.png'
            os.utime(png, ns=(page['path'].stat().st_mtime_ns + 1000000000,) * 2)
            safety.render_page(page, cache)
            self.assertEqual(run.call_count, 1)

    def test_parser_documented(self):
        self.assertIn('mm probe safety DIR', probe.__doc__)
        self.assertIn('safety', mm.__doc__)
        self.assertIn('mm probe safety DIR', (Path(mm.ROOT) / 'CLAUDE.md').read_text(encoding='utf-8'))


if __name__ == '__main__':
    unittest.main()
