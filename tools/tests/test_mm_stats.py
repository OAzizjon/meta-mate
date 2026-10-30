"""Fixed-input parsers and offline dashboard orchestration tests."""

import contextlib
import io
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import mm_stats as stats


class ParserTests(unittest.TestCase):
    def test_scc_uses_code_not_physical_lines_and_ranks_complexity(self):
        data = [{'Name': 'Rust', 'Code': 999, 'Files': [
            {'Name': 'a.rs', 'Location': './crates/mm-core/src/a.rs',
             'Code': 20, 'Lines': 40, 'Complexity': 2},
            {'Name': 'b.rs', 'Location': 'src-tauri/src/b.rs', 'Code': 10, 'Complexity': 8}]},
                {'Name': 'TypeScript', 'Files': [
                    {'Name': 'src/app.ts', 'Code': 7, 'Complexity': 0}]}]
        result = stats.parse_scc(json.dumps(data))
        self.assertEqual(result['loc'], 37)
        self.assertEqual(result['languages'], {'Rust': 30, 'TypeScript': 7})
        self.assertEqual(result['areas']['crates/mm-core'], 20)
        self.assertEqual(result['areas']['tools'], 0)
        self.assertEqual(result['largest'][0]['loc'], 20)
        self.assertEqual(result['complex'][0]['path'], 'src-tauri/src/b.rs')

    def test_scc_absolute_paths_and_top_ten(self):
        root = Path.cwd()
        files = [{'Name': 'x.rs', 'Location': str(root / f'src/{i}.rs'),
                  'Code': i, 'Complexity': 20 - i} for i in range(12)]
        result = stats.parse_scc(json.dumps([{'Name': 'Rust', 'Files': files}]), root)
        self.assertEqual(len(result['largest']), 10)
        self.assertEqual(len(result['complex']), 10)
        self.assertEqual(result['largest'][0]['path'], 'src/11.rs')

    def test_numstat_empty_binary_rename_and_unusual_paths(self):
        text = ('\x1eabc\0\n2\t1\tsrc/a file.ts\0-\t-\timage.png\0'
                '4\t0\t\0old.ts\0src/new\tname\n.ts\0'
                '\x1edef\0\n\x1eghi\0\n0\t3\ttools/x.py\0')
        commits = stats.parse_numstat(text)
        self.assertEqual(len(commits), 3)
        self.assertEqual(commits[1]['changes'], [])
        self.assertTrue(commits[0]['changes'][1]['binary'])
        self.assertEqual(commits[0]['changes'][2]['path'], 'src/new\tname\n.ts')
        result = stats.churn_summary(commits)
        self.assertEqual((result['commits'], result['added'], result['removed']), (3, 6, 4))
        self.assertEqual(stats.parse_numstat(''), [])

    def test_churn_ignores_unrelated_tests_and_detects_hot_spots(self):
        def commit(*paths):
            return {'changes': [{'path': p, 'added': 1, 'removed': 2} for p in paths]}

        commits = [commit('tools/a.py', 'tools/b.py', 'tools/tests/test_b.py') for _ in range(3)]
        result = stats.churn_summary(commits)
        self.assertEqual([r['path'] for r in result['hot_spots']], ['tools/a.py'])
        rows = {r['path']: r for r in result['cochanges']}
        self.assertEqual(rows['tools/b.py']['with_tests'], 3)
        self.assertEqual(rows['tools/a.py']['without_tests'], 3)
        self.assertEqual(rows['tools/b.py']['test_files'], {'tools/tests/test_b.py': 3})
        self.assertEqual(result['test_cochanges'], {'with_tests': 1, 'without_tests': 1})

    def test_cochange_totals_include_files_outside_top_ten(self):
        commits = [{'changes': [{'path': f'tools/{i}.py', 'added': 1, 'removed': 0}
                                for i in range(12)]}]
        result = stats.churn_summary(commits)
        self.assertEqual(len(result['cochanges']), 10)
        self.assertEqual(result['test_cochanges'], {'with_tests': 0, 'without_tests': 12})

    def test_test_associations_and_inline_rust(self):
        self.assertTrue(stats.related_test('src/audio/a.ts', 'src/audio/a.test.ts'))
        self.assertTrue(stats.related_test('crates/mm-core/src/a.rs', 'crates/mm-core/tests/api.rs'))
        self.assertTrue(stats.related_test('src-tauri/src/teacher.rs', 'src-tauri/src/teacher/tests.rs'))
        self.assertFalse(stats.related_test('crates/mm-core/src/a.rs', 'crates/mm-win32/tests/a.rs'))
        commits = [{'changes': [{'path': 'crates/mm-core/src/a.rs', 'added': 1, 'removed': 0}]}]
        self.assertEqual(stats.churn_summary(commits, ['crates/mm-core/src/a.rs'])['cochanges'][0]['with_tests'], 1)

    def test_rust_test_declarations_ignore_comments_and_literals(self):
        text = '''// #[test] fn fake() {}
/* outer /* nested */ #[test] fn fake2() {} */
const S: &str = r##"#[test] fn fake3() {}"##;
#[test]
#[ignore]
fn real() {}
#[test] async fn second() {}
'''
        self.assertEqual(stats.count_tests(text, 'Rust'), 2)

    def test_node_test_calls_and_python_definitions(self):
        self.assertEqual(stats.count_tests('''// test('no', () => {})
const s = "it('no')";
test('yes', () => {}); it ('also', () => {});
''', 'Node'), 2)
        self.assertEqual(stats.count_tests('''# def test_fake():
class Tests:
    def test_one(self): pass
    async def test_two(self): pass
text = "def test_fake2():"
''', 'Python'), 2)

    def test_unwraps_exclude_test_items_but_keep_production_after_them(self):
        text = '''fn prod() { a.unwrap(); let s = "x.unwrap()"; }
#[cfg(test)] mod tests { #[test] fn one() { b.unwrap(); } }
#[test] fn standalone() { c.unwrap(); }
fn more() { d . unwrap (); } // e.unwrap()
'''
        clean = stats.production_rust(text)
        self.assertEqual(len(stats.re.findall(r'\.\s*unwrap\s*\(\s*\)', clean)), 2)
        self.assertEqual(stats.production_rust('#![cfg(test)]\nfn helper() { a.unwrap(); }'), '')

    def test_first_agent_table_is_verbatim(self):
        table = '| Agent | Jobs |\n| --- | --- |\n| local | 2 |\n'
        self.assertEqual(stats.first_table('Report\n\n' + table + '\nOther\n| second |\n'), table)
        self.assertIsNone(stats.first_table('No report available'))
        plain = 'Agent    Jobs\n-------  ----\nlocal    2\n'
        self.assertEqual(stats.first_table('Report\n\n' + plain + '\nOther\n'), plain)

    def test_agents_parse_percentages_missing_values_and_bad_rows(self):
        report = ('Report\nagent    mode      runs ok median useful rated applied patches\n'
                  'gemini   review    20 65% 42s 50% 10 25% 4\n'
                  'local    ask       5 100% - - 0 - 0\n'
                  'broken   row\n'
                  'bad edit nope 0% - - 0 - 0\n\nOther table\n')
        rows = stats.parse_agents(report)
        self.assertEqual(len(rows), 2)
        self.assertEqual(rows[0], {'agent': 'gemini', 'mode': 'review', 'runs': 20,
                                  'ok': 65, 'median': '42s', 'useful': 50, 'rated': 10,
                                  'applied': 25, 'patches': 4})
        self.assertIsNone(rows[1]['median'])
        self.assertIsNone(rows[1]['useful'])
        self.assertIsNone(rows[1]['applied'])
        self.assertIsNone(stats.parse_agents(None))
        self.assertIsNone(stats.parse_agents('no report'))
        self.assertEqual(stats.parse_agents('agent mode runs ok median useful rated applied patches\n'), [])


class DashboardTests(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.root = Path(directory.name)

    def write(self, name, text):
        path = self.root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding='utf-8')

    def snapshot(self):
        with patch.object(stats, 'run', return_value=None), patch.object(stats.shutil, 'which', return_value=None), \
                patch.object(stats, 'agents_report', return_value=None), \
                patch.object(stats, 'contracts_report', return_value=None):
            return stats.collect(self.root, 7)

    def test_missing_tools_leave_scan_sections_working(self):
        self.write('crates/mm-core/src/lib.rs', 'fn x() { a.unwrap(); } // TODO\n#[test] fn check() { b.unwrap(); }')
        self.write('src/a.test.ts', "test('works', () => {});")
        self.write('tools/tests/test_x.py', 'def test_x(): pass\n# FIXME XXX')
        self.write('locales/en.json', '{"used": "Yes", "unused": "No"}')
        self.write('locales/ru.json', '{"used": "Da"}')
        self.write('src/a.ts', 't("used"); const unused_suffix = 1;')
        result = self.snapshot()
        self.assertIsNone(result['code'])
        self.assertIsNone(result['churn'])
        self.assertEqual(result['tests']['total'], 3)
        self.assertEqual(result['hygiene']['unwraps']['crates/mm-core'], 1)
        self.assertEqual(sum(result['hygiene']['todos'].values()), 3)
        self.assertEqual(result['hygiene']['locales'], {'en': 2, 'ru': 1, 'unused': ['unused']})
        screen = stats.render(result)
        for label in ('Code: n/a', 'Churn: n/a', 'Tests: 3', 'Agents: n/a',
                      'Contracts: n/a', 'new since the last snapshot: n/a'):
            self.assertIn(label, screen)
        self.assertEqual(result['hygiene']['unwrap_files'], {'crates/mm-core/src/lib.rs': 1})

    def test_alert_selection_and_compact_layout(self):
        result = self.snapshot()
        result['agent_rows'] = stats.parse_agents(
            'agent mode runs ok median useful rated applied patches\n'
            'gemini review 5 65% 30s 60% 5 - 0\n'
            'local edit 8 80% - 59% 5 - 0\n'
            'good edit 5 80% - 60% 5 - 0\n'
            'few review 4 0% - 0% 4 - 0\n'
            'unrated ask 5 100% - - 0 - 0\n')
        result['hygiene']['locales']['unused'] = [f'key{i}' for i in range(10)]
        result['hygiene']['new_unwrap_files'] = ['src/new.rs']
        result['contracts'] = ['first', 'second', 'third', 'fourth']
        commits = [{'changes': [{'path': 'tools/a.py', 'added': 1, 'removed': 0}]}] * 3
        result['churn'] = stats.churn_summary(commits)
        screen = stats.render(result)
        self.assertTrue(screen.startswith('ALERTS\n'))
        self.assertLess(screen.index('ACTIVITY (last 7 days)'), screen.index('BASELINE'))
        for text in ('tools/a.py (3 changes)', 'gemini review 65% ok', 'local edit 59% useful',
                     '10 — key0, key1', 'new since the last snapshot: src/new.rs',
                     'Contracts: 4 findings — first; second; third', '0/1 files with/without tests'):
            self.assertIn(text, screen)
        for text in ('good edit', 'few review', 'unrated ask', 'key8', 'fourth', 'Languages:'):
            self.assertNotIn(text, screen)
        result['agent_rows'] = []
        result['churn'] = stats.churn_summary([])
        result['contracts'] = []
        self.assertIn('Agents: none', stats.render(result))
        self.assertIn('Hot spots without tests (heuristic): none', stats.render(result))
        self.assertIn('Contracts: 0 findings', stats.render(result))

    def test_snapshot_tracks_per_file_unwrap_increases_even_when_total_is_equal(self):
        self.write('crates/mm-core/src/a.rs', 'fn a() { a.unwrap(); a.unwrap(); }')
        first = self.snapshot()
        path = stats.save_snapshot(first, self.root)
        self.assertIsNone(first['hygiene']['new_unwrap_files'])
        self.write('crates/mm-core/src/a.rs', 'fn a() { a.unwrap(); }')
        self.write('crates/mm-core/src/b.rs', 'fn b() { b.unwrap(); }')
        second = self.snapshot()
        stats.save_snapshot(second, self.root)
        saved = json.loads(path.read_text(encoding='utf-8'))
        self.assertEqual(saved['hygiene']['unwrap_files'],
                         {'crates/mm-core/src/a.rs': 1, 'crates/mm-core/src/b.rs': 1})
        self.assertEqual(saved['hygiene']['new_unwrap_files'], ['crates/mm-core/src/b.rs'])
        self.assertEqual(second['deltas']['unwraps'], 0)
        third = self.snapshot()
        stats.compare_snapshot(third, second)
        self.assertEqual(third['hygiene']['new_unwrap_files'], [])
        del first['hygiene']['unwrap_files']
        stats.compare_snapshot(third, first)
        self.assertIsNone(third['hygiene']['new_unwrap_files'])

    def test_fallback_counts_only_tracked_source_and_handles_missing_file(self):
        self.write('tools/a.py', '# comment\n\nx = 1\n')
        self.write('tools/untracked.py', 'not counted\n')
        self.write('data.json', '{}')
        with patch.object(stats.shutil, 'which', return_value=None), \
                patch.object(stats, 'run', return_value='tools/a.py\0data.json\0missing.py\0'):
            result = stats.collect_code(self.root)
        self.assertEqual(result['loc'], 2)
        self.assertEqual(result['languages'], {'Python': 2})
        self.assertIsNone(result['complex'])

    def test_failed_scc_falls_back_to_git(self):
        self.write('src/x.ts', 'let x = 1;')
        with patch.object(stats.shutil, 'which', return_value='scc'), \
                patch.object(stats, 'run', side_effect=['not json', 'src/x.ts\0']):
            self.assertEqual(stats.collect_code(self.root)['loc'], 1)

    def test_snapshots_delta_corrupt_previous_and_same_minute(self):
        first = self.snapshot()
        first['code'] = stats.code_summary([], 'scc')
        path = stats.save_snapshot(first, self.root)
        self.assertTrue(path.name.endswith('.json'))
        self.assertIsNone(first['deltas']['tests'])
        self.write('.cache/mm-stats/9999-corrupt.json', '{broken')
        second = self.snapshot()
        second['code'] = stats.code_summary([{'path': 'tools/x.py', 'language': 'Python',
                                               'loc': 4, 'complexity': 0}], 'scc')
        second['tests']['total'] = 2
        second['hygiene']['todos']['tools'] = 1
        stats.save_snapshot(second, self.root)
        self.assertEqual(second['deltas'], {'loc': 4, 'tests': 2, 'todos': 1, 'unwraps': 0})
        self.assertEqual(json.loads(path.read_text(encoding='utf-8'))['deltas'], second['deltas'])
        self.assertIn('4 LOC (scc, +4)', stats.render(second))

    def test_counter_method_change_suppresses_loc_delta(self):
        first = self.snapshot()
        first['code'] = stats.code_summary([], 'scc')
        stats.save_snapshot(first, self.root)
        second = self.snapshot()
        second['code'] = stats.code_summary([], 'nonblank lines (git tracked)')
        stats.save_snapshot(second, self.root)
        self.assertIsNone(second['deltas']['loc'])

    def test_agent_mode_never_invokes_team(self):
        with patch('mm.agent_mode_reason', return_value='agent copy'), patch.object(stats, 'run') as run:
            self.assertIsNone(stats.agents_report(self.root))
        run.assert_not_called()

    def test_lead_runs_team_report_and_extracts_first_table(self):
        table = '| Agent |\n| --- |\n| local |\n'
        with patch('mm.agent_mode_reason', return_value=''), \
                patch.object(Path, 'is_file', return_value=True), \
                patch.object(stats, 'run', return_value='Report\n' + table + '\nEnd') as run:
            self.assertEqual(stats.agents_report(self.root), table)
        self.assertEqual(run.call_args.args[0][-3:], ['report', '--days', '7'])

    def test_contracts_missing_module_and_findings(self):
        with patch.dict(sys.modules, {'mm_contracts': None}):
            self.assertIsNone(stats.contracts_report(self.root))
        with patch('mm_contracts.findings', return_value=['one']) as findings:
            self.assertEqual(stats.contracts_report(self.root), ['one'])
        findings.assert_called_once_with(self.root)

    def test_default_reads_previous_snapshot_and_full_keeps_details(self):
        first = self.snapshot()
        stats.save_snapshot(first, self.root)
        current = self.snapshot()
        current['tests']['total'] = 3
        with patch.object(stats, 'ROOT', self.root), patch.object(stats, 'collect', return_value=current), \
                contextlib.redirect_stdout(io.StringIO()) as output:
            self.assertEqual(stats.main([]), 0)
        self.assertIn('Tests: 3 (+3)', output.getvalue())
        with patch.object(stats, 'ROOT', self.root), patch.object(stats, 'collect', return_value=current), \
                contextlib.redirect_stdout(io.StringIO()) as output:
            self.assertEqual(stats.main(['--full']), 0)
        self.assertTrue(output.getvalue().startswith('MetaMate stats | last 7 days'))
        self.assertIn('Hygiene: TODO/FIXME/XXX=', output.getvalue())
        self.assertNotIn('ALERTS', output.getvalue())

    def test_json_save_and_invalid_since(self):
        with patch.object(stats, 'ROOT', self.root), patch.object(stats, 'collect', return_value=self.snapshot()), \
                contextlib.redirect_stdout(io.StringIO()) as output:
            self.assertEqual(stats.main(['--json', '--save', '--since', '2']), 0)
        self.assertIn('deltas', json.loads(output.getvalue()))
        self.assertEqual(len(list((self.root / '.cache/mm-stats').glob('*.json'))), 1)
        with contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit):
            stats.main(['--since', '-1'])


if __name__ == '__main__':
    unittest.main()
