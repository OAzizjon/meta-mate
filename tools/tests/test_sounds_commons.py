"""Offline tests for Commons sound selection and provenance."""

import contextlib
import io
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch
from urllib.error import HTTPError
from urllib.parse import parse_qs, urlparse

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import sounds_commons as sounds


def sound(title='File:Thunder.ogg', label='CC0', seconds=4, size=1000,
          mime='audio/ogg'):
    return {'title': title, 'imageinfo': [{
        'url': 'https://upload.wikimedia.org/wikipedia/commons/test.ogg',
        'descriptionurl': 'https://commons.wikimedia.org/wiki/' + title,
        'size': size, 'mime': mime,
        'extmetadata': {'LicenseShortName': {'value': label},
                        'Duration': {'value': str(seconds)},
                        'Artist': {'value': '<a href="/wiki/User:Author">Author</a>'}}
    }]}


def api(*pages):
    return json.dumps({'query': {'pages': dict(enumerate(pages))}}).encode()


class CommonsTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name).resolve()
        self.folder = self.root / 'assets/sounds'
        self.folder.mkdir(parents=True)
        self.entry = {'name': 'thunder', 'kind': 'sfx', 'query': 'thunder',
                      'max_seconds': 8, 'file': None}
        self.sources = self.folder / 'sources.json'
        self.sources.write_text(json.dumps([self.entry]), encoding='utf-8')
        self.licences = self.folder / 'LICENSES.md'
        self.licences.write_text('| File | Source | Licence | Author | Notes |\n'
                                 '|---|---|---|---|---|\n', encoding='utf-8')

    def invoke(self, *args):
        with contextlib.redirect_stdout(io.StringIO()) as output:
            result = sounds.main(list(args), root=self.root)
        return result, output.getvalue()

    def test_licence_allowlist(self):
        for label in ('CC0', 'CC0 1.0', 'Public domain', 'public-domain', 'PD-self'):
            with self.subTest(label=label):
                self.assertEqual(sounds.licence(sound(label=label)['imageinfo'][0]), label)
        for label in ('CC BY-SA', 'CC BY', 'GFDL', '', 'unknown', 'Public domain CC BY'):
            with self.subTest(label=label):
                self.assertIsNone(sounds.licence(sound(label=label)['imageinfo'][0]))
        info = sound()['imageinfo'][0]
        info['extmetadata']['License'] = {'value': 'cc-zero'}
        self.assertEqual(sounds.licence(info), 'CC0')
        info['extmetadata']['License'] = {'value': 'cc-by-sa-4.0'}
        self.assertIsNone(sounds.licence(info))
        self.assertIsNone(sounds.licence({}))

    def test_shortest_acceptable_and_api_parameters(self):
        pages = [sound(seconds=1, label='CC BY-SA'), sound(seconds=7),
                 sound(title='File:Short.wav', seconds=2, mime='audio/wav'),
                 sound(seconds=20, size=10)]
        with patch.object(sounds, 'http', return_value=api(*pages)) as http:
            chosen = sounds.choose(self.entry)
        self.assertEqual(chosen['title'], 'File:Short.wav')
        params = parse_qs(urlparse(http.call_args.args[0]).query)
        self.assertEqual(params['gsrsearch'], ['filetype:audio thunder'])
        self.assertEqual(params['gsrnamespace'], ['6'])
        self.assertEqual(params['gsrlimit'], ['20'])
        self.assertEqual(params['iiprop'], ['url|size|mime|extmetadata'])
        self.assertEqual(params['maxlag'], ['5'])

    def test_pronunciation_and_test_tones_filtered(self):
        titles = ('File:En-us-birdsong.oga', 'File:De-Meer.ogg', 'Ru-more.wav',
                  'File:eng-US-word.mp3', 'File:Bird pronunciation.ogg',
                  'File:Sine tone.wav', 'File:Square-wave-220-hz-5-seconds.wav',
                  'File:Sawtooth.ogg', 'File:White noise.wav',
                  'File:Test tone.wav', 'File:440 Hz.ogg')
        for title in titles:
            with self.subTest(title=title):
                with patch.object(sounds, 'http', return_value=api(sound(title=title), sound())):
                    self.assertEqual([info['title'] for info in sounds.candidates(self.entry)],
                                     ['File:Thunder.ogg'])

    def test_smallest_fallback_and_size_limit(self):
        with patch.object(sounds, 'http', return_value=api(
                sound(seconds=10, size=3000),
                sound(title='File:Small.ogg', seconds=30, size=1000),
                sound(seconds=1, size=sounds.MAX_BYTES + 1))):
            self.assertEqual(sounds.choose(self.entry)['title'], 'File:Small.ogg')

    def test_size_heuristic_and_mime_tie(self):
        wav = sound(title='File:Wave.wav', mime='audio/wav', size=1000)
        ogg = sound(title='File:Ogg.ogg', size=1000)
        with patch.object(sounds, 'http', return_value=api(wav, ogg)):
            self.assertEqual(sounds.choose(self.entry)['title'], 'File:Ogg.ogg')
        info = ogg['imageinfo'][0]
        del info['extmetadata']['Duration']
        self.assertAlmostEqual(sounds.duration(info), 0.0625)

    def test_path_traversal_refused_before_http(self):
        for name in ('../escape', '..\\escape', '/escape', 'C:\\escape', 'NUL'):
            with self.subTest(name=name):
                self.sources.write_text(json.dumps([dict(self.entry, name=name)]), encoding='utf-8')
                with patch.object(sounds, 'http') as http:
                    self.assertEqual(self.invoke()[0], 1)
                    http.assert_not_called()

    def test_licence_row_updated_once_and_ambience_destination(self):
        self.entry.update(kind='amb', file='File:Thunder.ogg')
        self.sources.write_text(json.dumps([self.entry]), encoding='utf-8')
        page = sound()
        with patch.object(sounds, 'http', side_effect=[api(page), b'audio']) as http:
            self.assertEqual(self.invoke()[0], 0)
        self.assertEqual((self.folder / 'src/amb/thunder.ogg').read_bytes(), b'audio')
        params = parse_qs(urlparse(http.call_args_list[0].args[0]).query)
        self.assertEqual(params['titles'], ['File:Thunder.ogg'])
        self.assertNotIn('generator', params)
        page['imageinfo'][0]['extmetadata']['Artist']['value'] = '<b>New Author</b>'
        with patch.object(sounds, 'http', side_effect=[api(page), b'updated']):
            self.assertEqual(self.invoke()[0], 0)
        text = self.licences.read_text(encoding='utf-8')
        self.assertEqual(text.count('`thunder.ogg`'), 1)
        self.assertIn('| New Author |', text)
        self.assertNotIn('<b>', text)
        self.assertIn('fetched ', text)
        self.assertIn('https://commons.wikimedia.org/wiki/File:Thunder.ogg', text)
        self.assertNotIn(b'\r', self.licences.read_bytes())
        self.assertTrue(self.licences.read_bytes().endswith(b'\n'))

    def test_dry_run_writes_nothing(self):
        before = {p.relative_to(self.root): p.read_bytes()
                  for p in self.root.rglob('*') if p.is_file()}
        with patch.object(sounds, 'http', return_value=api(sound())) as http:
            status, output = self.invoke('--dry-run', '--only', 'thunder')
        self.assertEqual(status, 0)
        self.assertEqual(http.call_count, 1)
        self.assertIn('File:Thunder.ogg | CC0 | 4 s | 1000 bytes | audio/ogg | ', output)
        self.assertFalse((self.folder / 'src').exists())
        self.assertEqual(before, {p.relative_to(self.root): p.read_bytes()
                                 for p in self.root.rglob('*') if p.is_file()})

    def test_candidates_count_and_estimated_duration(self):
        estimated = sound(title='File:Sea.ogg', seconds='', size=16000)
        pages = [estimated] + [sound(title=f'File:Thunder {i}.ogg', seconds=i + 2)
                               for i in range(6)]
        with patch.object(sounds, 'http', return_value=api(*pages)) as http:
            status, output = self.invoke('--dry-run', '--candidates', '2')
        self.assertEqual(status, 0)
        self.assertEqual(len(output.splitlines()), 2)
        self.assertIn('File:Sea.ogg | CC0 | ~1 s | 16000 bytes | audio/ogg | '
                      'https://commons.wikimedia.org/wiki/File:Sea.ogg', output)
        self.assertEqual(http.call_count, 1)
        with patch.object(sounds, 'http', return_value=api(*pages)):
            self.assertEqual(len(self.invoke('--dry-run')[1].splitlines()), 5)
        self.assertEqual(self.invoke('--dry-run', '--candidates', '0')[0], 1)

    def test_unpinned_real_run_skips_without_http(self):
        with patch.object(sounds, 'http') as http:
            status, output = self.invoke()
        self.assertEqual(status, 0)
        self.assertIn('thunder: not pinned (run --dry-run and pin a file)', output)
        http.assert_not_called()

    def test_pin_saved_without_http_and_invalid_pins_rejected(self):
        with patch.object(sounds, 'http') as http:
            self.assertEqual(self.invoke('--pin', 'thunder=File:Thunder.ogg')[0], 0)
        http.assert_not_called()
        self.assertEqual(json.loads(self.sources.read_text(encoding='utf-8'))[0]['file'],
                         'File:Thunder.ogg')
        before = self.sources.read_bytes()
        self.assertIn(b'\n  {\n', before)
        for pin in ('thunder=Thunder.ogg', 'thunder=File:', 'unknown=File:Rain.ogg',
                    'thunder'):
            with self.subTest(pin=pin):
                self.assertEqual(self.invoke('--pin', pin)[0], 1)
                self.assertEqual(self.sources.read_bytes(), before)

    def test_failure_reports_entry_and_continues(self):
        self.sources.write_text(json.dumps([self.entry, dict(self.entry, name='rain')]),
                                encoding='utf-8')
        with patch.object(sounds, 'http', side_effect=[api(sound(label='CC BY-SA')), api(sound())]):
            status, output = self.invoke('--dry-run')
        self.assertEqual(status, 1)
        self.assertIn('thunder: no acceptable file', output)
        self.assertIn('rain: File:Thunder.ogg', output)

    def test_http_is_bounded_and_rate_limited(self):
        response = io.BytesIO(b'audio')
        response.headers = {'Content-Length': '5'}
        with patch.object(sounds, '_last_request', 10), \
                patch.object(sounds.time, 'monotonic', side_effect=[10.25, 12]), \
                patch.object(sounds.time, 'sleep') as sleep, \
                patch.object(sounds, 'urlopen', return_value=response) as opener:
            self.assertEqual(sounds.http(sounds.API, 5), b'audio')
        sleep.assert_called_once_with(2.25)
        self.assertEqual(opener.call_args.kwargs['timeout'], 20)
        self.assertEqual(opener.call_args.args[0].get_header('User-agent'), sounds.USER_AGENT)
        for headers in ({}, {'Content-Length': '100'}):
            with self.subTest(headers=headers):
                response = io.BytesIO(b'oversized')
                response.headers = headers
                with patch.object(sounds, '_last_request', None), \
                        patch.object(sounds, 'urlopen', return_value=response):
                    with self.assertRaisesRegex(ValueError, 'size limit'):
                        sounds.http(sounds.API, 5)

    def test_http_retries_retry_after_and_stops_after_four_retries(self):
        for code in (429, 503):
            with self.subTest(code=code):
                error = HTTPError(sounds.API, code, 'busy', {'Retry-After': '7'}, None)
                response = io.BytesIO(b'audio')
                response.headers = {}
                with patch.object(sounds, '_last_request', None), \
                        patch.object(sounds.time, 'sleep') as sleep, \
                        patch.object(sounds, 'urlopen', side_effect=[error, response]) as opener:
                    self.assertEqual(sounds.http(sounds.API, 5), b'audio')
                self.assertEqual(opener.call_count, 2)
                self.assertEqual(sleep.call_args_list[0].args, (7.0,))
                errors = [HTTPError(sounds.API, code, 'busy', {}, None) for _ in range(5)]
                with patch.object(sounds, '_last_request', None), \
                        patch.object(sounds.time, 'sleep') as sleep, \
                        patch.object(sounds, 'urlopen', side_effect=errors) as opener:
                    with self.assertRaises(HTTPError):
                        sounds.http(sounds.API, 5)
                self.assertEqual(opener.call_count, 5)
                self.assertEqual([call.args[0] for call in sleep.call_args_list[::2]],
                                 [2.5, 5, 10, 20])

    def test_maxlag_retries_and_gives_up(self):
        def response(data):
            stream = io.BytesIO(data)
            stream.headers = {}
            return stream

        busy = json.dumps({'error': {'code': 'maxlag'}}).encode()
        for succeeds in (True, False):
            with self.subTest(succeeds=succeeds):
                responses = ([response(busy), response(api(sound()))] if succeeds
                             else [response(busy) for _ in range(5)])
                with patch.object(sounds, '_last_request', None), \
                        patch.object(sounds.time, 'sleep'), \
                        patch.object(sounds, 'urlopen', side_effect=responses) as opener:
                    if succeeds:
                        self.assertEqual(sounds.choose(self.entry)['title'], 'File:Thunder.ogg')
                    else:
                        with self.assertRaisesRegex(ValueError, 'maxlag'):
                            sounds.choose(self.entry)
                self.assertEqual(opener.call_count, 2 if succeeds else 5)

    def test_stdout_configured_for_windows(self):
        with patch.object(sounds.sys, 'stdout') as output:
            self.assertEqual(sounds.main([], root=self.root), 0)
        output.reconfigure.assert_called_once_with(encoding='utf-8', errors='replace')


if __name__ == '__main__':
    unittest.main()
