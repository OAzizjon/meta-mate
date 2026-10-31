"""Offline pointing fixtures, scoring, streams, reports and shared batch guards."""
from contextlib import redirect_stdout
import base64
import io
import json
from pathlib import Path
import struct
import sys
import tempfile
import threading
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import mm_probe as probe
import mm_probe_point as point
import mm_probe_safety as safety


class PointTests(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory()
        self.addCleanup(temp.cleanup)
        self.root = Path(temp.name)
        self.pages = self.root / 'pages'
        folder = self.pages / 'en'
        folder.mkdir(parents=True)
        self.entries = []
        # Two tiny, self-contained pages, each with a present and absent target.
        for index, name in enumerate(('Send', 'Print')):
            filename = f'{index}.html'
            (folder / filename).write_text(
                '<html><head><style>html,body{margin:0;width:1280px;height:800px;overflow:hidden}'
                '</style></head><body><button id="e1" style="position:absolute;left:100px;'
                f'top:80px;width:100px;height:40px">{name}</button></body></html>', encoding='utf-8')
            self.entries.append(dict(file=filename, lang='en', app='Postbox',
                                     elements=[dict(id='e1', role='button', name=name, rect=[100, 80, 100, 40])],
                                     questions=[dict(q=f'Where is {name}?', target='e1'),
                                                dict(q='Where is fax?', target=None)]))
        self.manifest = folder / 'manifest.json'
        self.save()
        self.enterContext(patch.object(probe, 'LEDGER', self.root / 'ledger.jsonl'))
        self.enterContext(patch.object(probe, '_open', side_effect=AssertionError('network forbidden')))

    def save(self):
        self.manifest.write_text(json.dumps(self.entries), encoding='utf-8')

    def args(self, *extra):
        return probe.build_parser().parse_args(['point', str(self.pages), '--out', str(self.root / 'report'), *extra])

    def body(self):
        png = b'\x89PNG\r\n\x1a\n' + b'\0' * 8 + struct.pack('>II', 640, 400)
        return dict(model=safety.MODELS[0], input=[dict(content=[dict(type='input_image',
                    image_url='data:image/png;base64,' + base64.b64encode(png).decode())])])

    def test_two_pages_and_glob(self):
        self.assertEqual(len(point.load_pages(self.pages)), 2)
        self.assertEqual(point.load_pages(self.pages, 'en/1.*')[0]['page'], 'en/1.html')
        with self.assertRaisesRegex(probe.ProbeError, 'matched'):
            point.load_pages(self.pages, '*.missing')

    def test_bad_rect_target_metadata_and_path(self):
        original = json.loads(json.dumps(self.entries))
        for field, value in [('rect', [1270, 0, 20, 20]), ('rect', [0, 0, 0, 2]),
                             ('rect', [True, 80, 100, 40]), ('id', 'missing'), ('name', None)]:
            with self.subTest(field=field, value=value):
                self.entries = json.loads(json.dumps(original))
                self.entries[0]['elements'][0][field] = value
                self.save()
                with self.assertRaises(probe.ProbeError):
                    point.load_pages(self.pages)
        for change in [dict(file='../0.html'), dict(lang='fr'), dict(elements=[]),
                       dict(questions=[dict(q='Where?', target='e99')]),
                       dict(questions=[dict(q='Where?')])]:
            self.entries = json.loads(json.dumps(original))
            self.entries[0].update(change)
            self.save()
            with self.assertRaises(probe.ProbeError):
                point.load_pages(self.pages)

    def test_inline_rect_and_duplicate_html_id_rejected_before_preparation(self):
        html = self.pages / 'en' / '0.html'
        original = html.read_text(encoding='utf-8')
        for text in [original.replace('left:100px', 'left:101px'),
                     original.replace('position:absolute', 'position:relative'),
                     original.replace('id="e1"', 'id="e9"'), original + '<div id="e1"></div>']:
            html.write_text(text, encoding='utf-8')
            with patch.object(point, 'prepare', side_effect=AssertionError('must validate first')), \
                    patch.object(probe, 'provider_config', side_effect=AssertionError('no keys')):
                self.assertEqual(point.cmd_point(self.args('--dry-run')), 1)

    def test_scoring_ids_pixels_downscale_and_near_boundary(self):
        row = {**self.entries[0], 'target': 'e1', 'elements_mode': 'on', 'sent_size': [640, 400]}
        self.assertEqual(point.score(row, dict(kind='id', value='e1')), 'HIT')
        self.assertEqual(point.score(row, dict(kind='id', value='e9')), 'WRONG')
        row['elements_mode'] = 'off'
        self.assertEqual(point.score(row, dict(kind='id', value='e1')), 'HIT')
        for pixel, expected in [([50, 40], 'HIT'), ([99, 59], 'HIT'), ([100, 40], 'NEAR'),
                                ([112, 40], 'NEAR'), ([113, 40], 'WRONG'),
                                ([0, 0], 'WRONG'), ([640, 40], 'WRONG')]:
            self.assertEqual(point.score(row, dict(kind='pixel', value=pixel)), expected)
        self.assertEqual(point.score(row, None), 'NO-POINT')
        row['target'] = None
        self.assertEqual(point.score(row, None), 'CORRECT')
        self.assertEqual(point.score(row, dict(kind='id', value='e99')), 'FALSE-POINT')

    def test_prepare_cross_product_and_scaled_image_dimensions(self):
        page = point.load_pages(self.pages)[0]
        commands = []
        def rust(example, arguments):
            self.assertEqual(example, 'point_request')
            commands.append(arguments)
            source = arguments[arguments.index('--elements') + 1]
            if source != 'none':
                self.assertEqual(json.loads(Path(source).read_text(encoding='utf-8')), page['elements'])
            return self.body()
        with patch.object(safety, 'render_page', return_value=self.root / 'page.png'), \
                patch.object(point, 'rust', side_effect=rust):
            bodies = point.prepare(page, safety.MODELS[1], self.args('--max-edge', '640', '--reasoning', 'high'))
        self.assertEqual(len(bodies), 4)
        self.assertEqual({(p['q'], p['elements_mode']) for p, _ in bodies},
                         {(q['q'], mode) for q in page['questions'] for mode in ('on', 'off')})
        self.assertTrue(all(p['sent_size'] == (640, 400) for p, _ in bodies))
        self.assertTrue(all(c[-2:] == ['--reasoning', 'high'] for c in commands))

    def test_dry_run_without_keys_or_http(self):
        with patch.object(safety, 'render_page', return_value=self.root / 'page.png'), \
                patch.object(point, 'rust', return_value=self.body()), \
                patch.object(probe, 'provider_config', side_effect=AssertionError('no keys')), \
                redirect_stdout(io.StringIO()) as output:
            self.assertEqual(point.cmd_point(self.args('--dry-run')), 0)
        self.assertIn('[image elided]', output.getvalue())
        self.assertNotIn('base64', output.getvalue())
        data = json.loads((self.root / 'report' / 'report.json').read_text())
        self.assertTrue(data['dry_run'])

    def test_split_tag_time_uses_parser_byte_offset_and_excludes_local_parse(self):
        final = dict(status='completed')
        events = [('response.output_text.delta', dict(delta='Here: <poi')),
                  ('response.output_text.delta', dict(delta='nt e1>')),
                  ('response.output_text.delta', dict(delta='<point e9>')),
                  ('response.completed', dict(response=final))]
        wire = ''.join(f'event: {e}\ndata: {json.dumps(d)}\n\n' for e, d in events).encode()
        parsed = [dict(kind='id', value='e1'), len('Here: <point e1>')]
        with patch.object(probe, '_request', return_value=io.BytesIO(wire)), \
                patch.object(point, 'rust', return_value=parsed), \
                patch.object(point.time, 'perf_counter', side_effect=[0, .1, .2, .3, .4]):
            result, answer = point.exchange(self.body(), 'dummy', 'https://example.invalid', 120)
            row = {**self.entries[0], 'target': 'e1', 'elements_mode': 'on', 'sent_size': [640, 400]}
            self.assertEqual(point.evaluate(row, answer, result)['verdict'], 'HIT')
        self.assertEqual(result['_first_point_ms'], 200)
        self.assertEqual(result['_latency_ms'], 400)
        self.assertEqual(answer, 'Here: <point e1><point e9>')

    def test_batch_every_question_model_mode_repeat_and_ledger_cost(self):
        def prepare(page, model, args):
            return [({**page, **q, 'elements_mode': mode, 'sent_size': [1280, 800]}, self.body())
                    for mode in ('on', 'off') for q in page['questions']]
        final = dict(status='completed', usage=dict(input_tokens=10, output_tokens=2, cost=.01),
                     _arrivals=[], _latency_ms=20)
        with patch.object(point, 'prepare', side_effect=prepare), \
                patch.object(probe, 'provider_config', return_value=({}, 'dummy', 'https://example.invalid')), \
                patch.object(point, 'exchange', return_value=(final, "I can't find it.")), \
                patch.object(point, 'rust', return_value=None):
            self.assertEqual(point.cmd_point(self.args('--repeat', '2')), 0)
        data = json.loads((self.root / 'report' / 'report.json').read_text())
        self.assertEqual(len(data['runs']), 32)
        self.assertEqual(len(data['summary']), 4)
        self.assertTrue(all(g['HIT'] == 0 and g['CORRECT'] == 1 for g in data['summary']))
        ledger = [json.loads(line) for line in probe.LEDGER.read_text().splitlines()]
        self.assertEqual(len(ledger), 32)
        self.assertTrue(all(r['kind'] == 'point' for r in ledger))
        self.assertEqual(sum(r['usd'] for r in ledger), sum(r['usd'] for r in data['runs']))
        markdown = (self.root / 'report' / 'report.md').read_text()
        self.assertIn("I can't find it.", markdown)
        self.assertIn('Most-missed', markdown)

    def test_incomplete_answers_are_preserved_with_usage(self):
        def prepare(page, model, args):
            return [({**page, **page['questions'][0], 'elements_mode': 'off'}, self.body())]
        final = dict(status='incomplete', usage=dict(input_tokens=10, output_tokens=2), _arrivals=[])
        with patch.object(point, 'prepare', side_effect=prepare), \
                patch.object(probe, 'provider_config', return_value=({}, 'dummy', 'https://example.invalid')), \
                patch.object(point, 'exchange', return_value=(final, 'Partial answer')):
            self.assertEqual(point.cmd_point(self.args('--models', safety.MODELS[0])), 1)
        rows = json.loads((self.root / 'report' / 'report.json').read_text())['runs']
        self.assertTrue(all(r['answer'] == 'Partial answer' and r['input_tokens'] == 10 and r['error'] for r in rows))

    def test_unpriced_cap_reserves_inflight_point_calls(self):
        entered = threading.Event()
        real_guard = probe.guard_budget
        calls = []
        def guard(provider, budget, cap):
            if cap == 0:
                entered.set()
            return real_guard(provider, budget, cap)
        def exchange(*args):
            calls.append(1)
            self.assertTrue(entered.wait(5))
            raise probe.ProbeError('failed')
        def prepare(page, model, args):
            return [({**page, **page['questions'][0], 'elements_mode': 'off'}, self.body())]
        with patch.object(point, 'prepare', side_effect=prepare), \
                patch.object(probe, 'provider_config', return_value=({}, 'dummy', 'https://example.invalid')), \
                patch.object(probe, 'guard_budget', side_effect=guard), \
                patch.object(point, 'exchange', side_effect=exchange):
            self.assertEqual(point.cmd_point(self.args('--models', safety.MODELS[0], '--max-unpriced', '1')), 1)
        self.assertEqual(len(calls), 1)
        self.assertEqual(probe.ledger_unpriced('meta'), 1)

    def test_local_parser_failure_still_accounts_for_completed_request(self):
        def prepare(page, model, args):
            return [({**page, **page['questions'][0], 'elements_mode': 'off'}, self.body())]
        final = dict(status='completed', usage=dict(input_tokens=10, output_tokens=2, cost=.01), _arrivals=[])
        with patch.object(point, 'prepare', side_effect=prepare), \
                patch.object(probe, 'provider_config', return_value=({}, 'dummy', 'https://example.invalid')), \
                patch.object(point, 'exchange', return_value=(final, 'Here.')), \
                patch.object(point, 'rust', side_effect=probe.ProbeError('local parse failed')):
            self.assertEqual(point.cmd_point(self.args('--models', safety.MODELS[0])), 1)
        ledger = [json.loads(line) for line in probe.LEDGER.read_text().splitlines()]
        self.assertEqual(len(ledger), 2)
        self.assertTrue(all(r['in'] == 10 and r['out'] == 2 and r['usd'] == .01 for r in ledger))

    def test_aggregate_counts_percentiles_errors_and_unpriced(self):
        rows = [dict(model='m', elements_mode='on', lang='ru', target='e1', verdict=v,
                     first_point_ms=t, latency_ms=t + 10, input_tokens=2, output_tokens=1, usd=None)
                for v, t in [('HIT', 10), ('NEAR', 20), ('WRONG', 30), ('NO-POINT', 40)]]
        rows[3]['first_point_ms'] = None
        rows += [dict(model='m', elements_mode='on', lang='ru', target=None, verdict='CORRECT', latency_ms=100),
                 dict(model='m', elements_mode='on', lang='ru', error='failed')]
        group = point.aggregate(rows)[0]
        self.assertEqual((group['n'], group['absent_n'], group['errors']), (4, 1, 1))
        self.assertEqual((group['HIT'], group['NEAR'], group['WRONG'], group['NO-POINT']), (.25,) * 4)
        self.assertEqual((group['CORRECT'], group['FALSE-POINT']), (1, 0))
        self.assertEqual((group['first_point_ms_median'], group['first_point_ms_p90']), (20, 30))
        self.assertEqual((group['latency_ms_median'], group['latency_ms_p90']), (40, 100))
        self.assertEqual((group['unpriced'], group['input_tokens']), (4, 8))


if __name__ == '__main__':
    unittest.main()
