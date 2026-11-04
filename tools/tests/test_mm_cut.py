"""Pure cut planning and CLI tests; ffmpeg is never executed."""

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
import mm_cut as cut


class CutTests(unittest.TestCase):
    def rows(self, **updates):
        return cut.load_list(json.dumps([dict(id='A', src='takes/A.mp4', seconds=5, **updates)]))

    def test_schema(self):
        for field in ({'seconds': 0}, {'in': 4, 'out': 4}, {'in': -1},
                      {'seconds': float('nan')}, {'keep_audio': 'yes'},
                      {'src': '../escape.mp4'}, {'voice': '../escape.wav'},
                      {'captions': [{'text': 'x', 'at': 0, 'for': -1}]}):
            row = dict(id='A', src='takes/A.mp4', seconds=5)
            row.update(field)
            with self.subTest(field=field), self.assertRaises(ValueError):
                cut.load_list(json.dumps([row]))

    def test_all_missing_with_ids(self):
        rows = self.rows(voice='V1.wav') + [dict(id='B', src='takes/B.mp4', seconds=2)]
        with self.assertRaises(ValueError) as caught:
            cut.validate(rows, set())
        self.assertEqual(str(caught.exception).splitlines(),
                         ['A: missing takes/A.mp4', 'A: missing voice/V1.wav', 'B: missing takes/B.mp4'])

    def test_timeline_trim_and_probe_duration(self):
        rows = cut.load_list('[{"id":"A","src":"takes/A.mp4","in":2,"out":5},'
                             '{"id":"B","src":"out/B.mp4","in":1}]')
        timeline = cut.start_times(rows, {'out/B.mp4': {'duration': 5}})
        self.assertEqual([(s, d) for _, s, d in timeline], [(0, 3), (3, 4)])
        with self.assertRaises(ValueError):
            cut.start_times(rows)
        with self.assertRaisesRegex(ValueError, 'past the source'):
            cut.start_times(rows, {'takes/A.mp4': {'duration': 2}})

    def test_probe_uses_ffmpeg_only(self):
        result = type('Result', (), {'stderr': 'Duration: 00:01:02.50\nStream #0:1: Audio: aac'})()
        with patch.object(cut.subprocess, 'run', return_value=result) as run:
            self.assertEqual(cut.probe(Path('take.mp4')), {'duration': 62.5, 'audio': True})
            self.assertEqual(run.call_args.args[0], ['ffmpeg', '-hide_banner', '-i', 'take.mp4'])

    def test_planning_does_not_resolve_filesystem_paths(self):
        with patch.object(Path, 'resolve', side_effect=AssertionError('filesystem access')):
            rows = self.rows()
            cut.validate(rows, {'takes/A.mp4'})
            cut.build_graph(cut.start_times(rows), Path('video'))

    def test_limit_and_longest(self):
        row = dict(id='LONG', src='takes/A.mp4', seconds=180)
        cut.validate([row], {'takes/A.mp4'})
        row['seconds'] = 180.01
        with self.assertRaisesRegex(ValueError, '180.010.*LONG 180.010s'):
            cut.validate([row], {'takes/A.mp4'})

    def test_caption_bounds(self):
        with self.assertRaisesRegex(ValueError, 'caption exceeds'):
            cut.start_times(self.rows(captions=[dict(text='x', at=4, **{'for': 2})]))

    def test_escape_two_parser_layers(self):
        self.assertEqual(cut.drawtext_escape('C:/Windows/Fonts/arialbd.ttf'),
                         'C' + '\\' * 2 + ':/Windows/Fonts/arialbd.ttf')
        self.assertEqual(cut.drawtext_escape("It's: fine"),
                         'It' + '\\' * 3 + "'s" + '\\' * 2 + ': fine')
        self.assertEqual(cut.drawtext_escape('a,b;[c]'), 'a\\,b\\;\\[c\\]')

    def test_graph_audio_voice_music_and_captions(self):
        rows = self.rows(voice='V1.wav', captions=[dict(text="It's: OK", at=1, **{'for': 2})])
        rows += [dict(id='clip', src='out/clip.mp4', seconds=2)]
        inputs, graph = cut.build_graph(cut.start_times(rows), Path('video'), music=Path('music.wav'),
                                       metadata={'voice/V1.wav': {'duration': 3}})
        for text in ('scale=1920:1080', 'setsar=1,fps=30', 'stop_mode=clone',
                     'anullsrc=r=48000:cl=stereo', 'concat=n=2:v=1:a=1',
                     'between(t,0.0,3.0)', '0.316227766', '0.063095734',
                     'afade=t=out:st=4:d=3', 'normalize=0', 'expansion=none',
                     'enable=gte(t\\,1)*lt(t\\,3)'):
            self.assertIn(text, graph)
        self.assertIn('-stream_loop', inputs)
        self.assertIn('[0:a]atrim', graph)
        self.assertIn('[1:a]asetpts', graph)
        self.assertNotIn('[2:a]', graph)

    def test_missing_audio_and_draft_still(self):
        rows = self.rows()
        _, graph = cut.build_graph(cut.start_times(rows), Path('video'), draft=True,
                                   metadata={'takes/A.mp4': {'audio': False}})
        self.assertNotIn('[0:a]', graph)
        self.assertIn('scale=1280:720', graph)
        rows[0]['src'] = 'out/still.png'
        args, _ = cut.build_graph(cut.start_times(rows), Path('video'))
        self.assertEqual(args[:2], ['-loop', '1'])

    def test_srt_absolute_times(self):
        rows = self.rows()
        rows += [dict(id='B', src='out/B.mp4', seconds=3, subtitle='Speech',
                      captions=[dict(text='Caption', at=0.125, **{'for': 1.5})])]
        self.assertEqual(cut.srt(cut.start_times(rows)),
                         '1\n00:00:05,000 --> 00:00:08,000\nSpeech\n\n'
                         '2\n00:00:05,125 --> 00:00:06,625\nCaption\n')

    def test_default_cut_row_boundaries(self):
        rows = cut.load_list((cut.ROOT / 'video/cut.json').read_text(encoding='utf-8'))
        timeline = cut.start_times(rows)
        starts = {r['id']: s for r, s, _ in timeline}
        self.assertEqual([starts[n] for n in ('A', 'TH1', 'B-lesson', 'NOTE', 'D', 'E', 'TH2',
                                              'passwords-black', 'F', 'seven-models', 'scam-stamp', 'scale', 'TH3')],
                         [0, 10, 27.5, 61.5, 74.5, 80.5, 87.5, 101, 104, 120, 133, 150, 160])
        self.assertEqual(sum(d for _, _, d in timeline), 176)
        self.assertEqual({r['voice'] for r in rows if 'voice' in r}, {f'V{i}.wav' for i in range(1, 7)})

    def test_dry_run_no_processes_or_outputs(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'video/takes').mkdir(parents=True)
            (root / 'video/takes/A.mp4').touch()
            listing = root / 'video/cut.json'
            listing.write_text(json.dumps(self.rows()), encoding='utf-8')
            stdout = io.StringIO()
            with patch.object(cut.subprocess, 'run') as run, patch.object(cut.subprocess, 'call') as call:
                with contextlib.redirect_stdout(stdout):
                    self.assertEqual(cut.main(['--dry-run', '--draft'], root=root), 0)
                run.assert_not_called()
                call.assert_not_called()
            self.assertFalse((root / 'video/out').exists())
            self.assertIn('-/filter_complex', stdout.getvalue())
            self.assertIn('ultrafast', stdout.getvalue())

    def test_cli_real_plan_and_missing_validation(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'video/takes').mkdir(parents=True)
            listing = root / 'video/cut.json'
            listing.write_text(json.dumps(self.rows()), encoding='utf-8')
            with patch.object(cut, 'probe', return_value={'duration': 5, 'audio': False}) as probe:
                with patch.object(cut.subprocess, 'call', return_value=0) as call:
                    with contextlib.redirect_stdout(io.StringIO()):
                        self.assertEqual(cut.main([], root=root), 1)
                    probe.assert_not_called()
                    call.assert_not_called()
                    (root / 'video/takes/A.mp4').touch()
                    with contextlib.redirect_stdout(io.StringIO()):
                        self.assertEqual(cut.main([], root=root), 0)
                    command = call.call_args.args[0]
                    self.assertIn('+faststart', command)
                    self.assertIn('192k', command)
                    self.assertIn('anullsrc', (root / 'video/out/final.filter.txt').read_text())
                    self.assertTrue((root / 'video/out/final.srt').exists())

    def test_wiring_and_agent_exclusion(self):
        self.assertNotIn('cut', mm.TOOL_MODULES)
        args = mm.build_parser().parse_args(['cut'])
        with patch.object(cut, 'main', return_value=0) as main:
            self.assertEqual(mm.COMMANDS['cut'](args), 0)
            main.assert_called_once_with([])
        with contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit):
            mm.build_parser().parse_args(['agent', 'cut'])


class ResolveSources(unittest.TestCase):
    def test_other_containers_stand_in_for_the_named_file(self):
        with tempfile.TemporaryDirectory() as folder:
            video = Path(folder)
            (video / 'takes').mkdir()
            (video / 'voice').mkdir()
            (video / 'takes/A.mkv').write_bytes(b'x')
            (video / 'takes/B.mp4').write_bytes(b'x')
            (video / 'voice/V1.m4a').write_bytes(b'x')
            rows = [{'id': 'A', 'src': 'takes/A.mp4', 'voice': 'V1.wav'}, {'id': 'B', 'src': 'takes/B.mp4'},
                    {'id': 'C', 'src': 'takes/C.mp4'}]
            cut.resolve_sources(rows, video)
        self.assertEqual(rows[0]['src'], 'takes/A.mkv')
        self.assertEqual(rows[0]['voice'], 'V1.m4a')
        self.assertEqual(rows[1]['src'], 'takes/B.mp4')
        self.assertEqual(rows[2]['src'], 'takes/C.mp4')


class Warnings(unittest.TestCase):
    def test_out_and_seconds_together_are_refused(self):
        rows = [{'id': 'A', 'src': 'takes/A.mp4', 'out': 5, 'seconds': 4}]
        with self.assertRaisesRegex(ValueError, 'either out or seconds'):
            cut.load_list(json.dumps(rows))

    def test_a_short_take_is_named_but_a_short_clip_is_not(self):
        rows = [{'id': 'T', 'src': 'takes/T.mp4', 'seconds': 10}, {'id': 'C', 'src': 'out/c.mp4', 'seconds': 8}]
        metadata = {'takes/T.mp4': {'duration': 7.0}, 'out/c.mp4': {'duration': 3.0}}
        notes = cut.warnings(cut.start_times(rows, metadata), metadata)
        self.assertEqual(len(notes), 1)
        self.assertIn('takes/T.mp4 ends 3.0 s before', notes[0])

    def test_in_counts_against_the_take(self):
        rows = [{'id': 'T', 'src': 'takes/T.mp4', 'in': 4, 'seconds': 10}]
        metadata = {'takes/T.mp4': {'duration': 12.0}}
        self.assertIn('ends 2.0 s', cut.warnings(cut.start_times(rows, metadata), metadata)[0])

    def test_a_voice_running_into_the_next_one_is_named(self):
        rows = [{'id': 'A', 'src': 'out/a.mp4', 'seconds': 4, 'voice': 'V1.wav'},
                {'id': 'B', 'src': 'out/b.mp4', 'seconds': 4, 'voice': 'V2.wav'}]
        metadata = {'voice/V1.wav': {'duration': 6.0, 'audio': True}, 'voice/V2.wav': {'duration': 3.0, 'audio': True}}
        notes = cut.warnings(cut.start_times(rows, metadata), metadata)
        self.assertEqual(len(notes), 1)
        self.assertIn('V1.wav is 6.0 s and runs 2.0 s', notes[0])

    def test_a_silent_voice_file_is_named(self):
        rows = [{'id': 'A', 'src': 'out/a.mp4', 'seconds': 4, 'voice': 'V1.mp4'}]
        metadata = {'voice/V1.mp4': {'duration': 3.0, 'audio': False}}
        self.assertIn('has no sound', cut.warnings(cut.start_times(rows, metadata), metadata)[0])


class CameraTakes(unittest.TestCase):
    def graph(self, row, burn=False):
        timeline = [(row, 0.0, row.get('seconds', 5))]
        return cut.build_graph(timeline, Path('video'), burn=burn, metadata={row['src']: {'audio': True}})[1]

    def test_a_camera_take_is_cleaned_cropped_and_lit(self):
        graph = self.graph({'id': 'T', 'src': 'takes/T.mp4', 'seconds': 5, 'look': 'camera', 'crop': [1200, 675, 360, 40]})
        self.assertIn('crop=1200:675:360:40,eq=gamma', graph)
        self.assertIn('afftdn', graph)
        self.assertIn('loudnorm=I=-16', graph)

    def test_a_screen_take_is_only_levelled(self):
        graph = self.graph({'id': 'A', 'src': 'takes/A.mp4', 'seconds': 5})
        self.assertIn('loudnorm', graph)
        self.assertNotIn('afftdn', graph)
        self.assertNotIn('crop=', graph)

    def test_burned_subtitles_come_after_the_join(self):
        graph = self.graph({'id': 'A', 'src': 'takes/A.mp4', 'seconds': 5}, burn=True)
        self.assertIn('[vjoined]subtitles=final-burn.srt', graph)
        self.assertIn('[vout]', graph)

    def test_a_bad_crop_is_refused(self):
        with self.assertRaisesRegex(ValueError, 'crop'):
            cut.load_list(json.dumps([{'id': 'A', 'src': 'takes/A.mp4', 'seconds': 5, 'crop': [1, 2]}]))

    def test_sentences_share_the_time_by_their_words(self):
        row = {'id': 'A', 'src': 'takes/A.mp4', 'subtitle': 'One two three. Four.'}
        lines = cut.subtitle_lines([(row, 10.0, 8.0)])
        self.assertEqual([line[2] for line in lines], ['One two three.', 'Four.'])
        self.assertAlmostEqual(lines[0][1], 16.0)
        self.assertAlmostEqual(lines[1][1], 18.0)

    def test_a_closing_quote_ends_a_sentence(self):
        row = {'id': 'A', 'src': 'takes/A.mp4', 'subtitle': 'He asks: "Where do I click?" You are far away.'}
        lines = cut.subtitle_lines([(row, 0.0, 9.0)])
        self.assertEqual([line[2] for line in lines], ['He asks: "Where do I click?"', 'You are far away.'])

    def test_a_short_voice_sets_the_pace(self):
        row = {'id': 'A', 'src': 'out/a.mp4', 'voice': 'V.wav', 'subtitle': 'One two.'}
        lines = cut.subtitle_lines([(row, 0.0, 10.0)], {'voice/V.wav': {'duration': 4.0}})
        self.assertAlmostEqual(lines[0][1], 4.0)


if __name__ == '__main__':
    unittest.main()
