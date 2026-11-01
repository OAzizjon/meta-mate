"""Unit tests for tools/mm_shot.py — no Edge and no npm involved."""

import json
import contextlib
import io
import os
import shutil
import struct
import subprocess
import sys
import tempfile
import unittest
import urllib.error
import urllib.request
from pathlib import Path
from unittest.mock import patch, Mock

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
import mm_shot


class MockJsTests(unittest.TestCase):
    def test_fixture_answers_and_label_are_embedded(self):
        js = mm_shot.render_mock_js({'settings': {'theme': 'dark'}}, [], 'home')
        self.assertIn('var commands = {"settings": {"theme": "dark"}};', js)
        self.assertIn('var events = [];', js)
        self.assertIn('var label = "home";', js)
        self.assertIn('window.__TAURI_INTERNALS__ = internals;', js)

    def test_unknown_command_default_present(self):
        js = mm_shot.render_mock_js({}, [], 'home')
        self.assertIn('Promise.resolve(null)', js)   # the default for unknown commands
        self.assertIn('hasOwnProperty.call(commands, cmd)', js)

    def test_script_tag_breakout_is_escaped(self):
        js = mm_shot.render_mock_js({'echo': '</script><script>alert(1)</script>'}, [], 'home')
        self.assertNotIn('</script><script>', js)
        self.assertIn('<\\/script>', js)

    def test_russian_text_stays_readable(self):
        js = mm_shot.render_mock_js({'greet': 'Привет, Анна'}, [], 'home')
        self.assertIn('Привет, Анна', js)

    def test_fixture_text_is_not_replaced_as_a_template_marker(self):
        value = '/*__MM_EVENTS__*/ /*__MM_LABEL__*/'
        js = mm_shot.render_mock_js({'echo': value}, [{'event': 'ready'}], 'home')
        self.assertIn(json.dumps({'echo': value}), js)
        self.assertIn('var events = [{"event": "ready"}];', js)

    def test_overrides_do_not_mutate_the_fixture(self):
        fixture = {'load_settings': {'theme': 'light', 'language': 'en'}}
        commands, _, _ = mm_shot.mock_context(fixture, 'dark', 'ru')
        self.assertEqual(fixture['load_settings'], {'theme': 'light', 'language': 'en'})
        self.assertEqual(commands['load_settings']['theme'], 'dark')


class InjectMockTests(unittest.TestCase):
    TAG = f'<script src="{mm_shot.MOCK_ROUTE}?page=home&theme=dark&lang=ru"></script>'

    def test_script_tag_first_in_head(self):
        html = ('<!doctype html><html lang="en"><head>'
                '<meta charset="utf-8"><title>t</title></head><body></body></html>')
        out = mm_shot.inject_mock(html, 'home', 'dark', 'ru')
        self.assertEqual(out.count(self.TAG), 1)
        self.assertLess(out.index(self.TAG), out.index('<meta charset'))

    def test_never_injected_twice(self):
        html = '<html><head></head><body></body></html>'
        once = mm_shot.inject_mock(html, 'home', 'light', 'en')
        self.assertEqual(mm_shot.inject_mock(once, 'home', 'light', 'en'), once)
        self.assertEqual(once.count(mm_shot.MOCK_ROUTE), 1)

    def test_page_without_head(self):
        out = mm_shot.inject_mock('<html><body>hi</body></html>', 'mate', 'light', 'en')
        self.assertLess(out.index(mm_shot.MOCK_ROUTE), out.index('<body>'))
        bare = mm_shot.inject_mock('just text', 'mate', 'light', 'en')
        self.assertTrue(bare.startswith('<script src='))


class MateStateTests(unittest.TestCase):
    def test_state_fixtures_load_with_expected_shapes(self):
        consent = mm_shot.load_fixture(mm_shot.FIXTURES, 'mate', state='consent')
        self.assertEqual(consent['stream'], [
            {'at_ms': 50, 'message': {'type': 'line', 'generation': 0,
                                      'key': 'robot.look_consent'}}])
        thinking = mm_shot.load_fixture(mm_shot.FIXTURES, 'mate', state='thinking')
        self.assertEqual(thinking['stream'][0]['message'],
                         {'type': 'state', 'generation': 0, 'state': 'thinking'})
        answer = mm_shot.load_fixture(mm_shot.FIXTURES, 'mate', state='answer')
        item = answer['stream'][0]['message']['item']
        self.assertEqual((item['type'], item['mood']), ('say', 'calm'))
        self.assertTrue(item['text'])
        warning = mm_shot.load_fixture(mm_shot.FIXTURES, 'mate', state='warning')
        self.assertEqual(warning['events'][0]['event'], 'safety-check')
        self.assertEqual(warning['stream'][0]['message']['item']['mood'], 'sad')

    def test_state_mode_presets_the_onboarding_flag(self):
        js = mm_shot.render_mock_js({}, [], 'mate', onboarded=True)
        self.assertIn('var presetOnboarded = true;', js)
        self.assertIn("localStorage.setItem('metamate.onboarded', '1')", js)
        self.assertIn('var presetOnboarded = false;',
                      mm_shot.render_mock_js({}, [], 'mate'))

    def test_state_name_rules(self):
        for good in ('consent', 'thinking', 'answer', 'a-b-1'):
            self.assertIsNotNone(mm_shot.STATE_NAME.fullmatch(good))
        for bad in ('', 'Consent', 'two words', '../home', 'a.b', 'ru_RU', 'x' * 33):
            self.assertIsNone(mm_shot.STATE_NAME.fullmatch(bad))

    def test_missing_state_fixture_is_an_error_but_plain_fixture_defaults(self):
        with tempfile.TemporaryDirectory() as folder:
            fixtures = Path(folder)
            with self.assertRaises(ValueError):
                mm_shot.load_fixture(fixtures, 'mate', state='nope')
            self.assertEqual(mm_shot.load_fixture(fixtures, 'mate'), {})
            (fixtures / 'mate-ok.json').write_text('{"stream": []}', encoding='utf-8')
            self.assertEqual(mm_shot.load_fixture(fixtures, 'mate', state='ok'),
                             {'stream': []})

    def test_bad_state_name_is_rejected_by_loader_and_inject(self):
        with tempfile.TemporaryDirectory() as folder:
            with self.assertRaises(ValueError):
                mm_shot.load_fixture(Path(folder), 'mate', state='../x')
        with self.assertRaises(ValueError):
            mm_shot.inject_mock('<html><head></head></html>', 'mate', 'light', 'en',
                                state='Bad')


class MateStreamMockTests(unittest.TestCase):
    def test_stream_is_embedded_with_replay_wiring(self):
        stream = [{'at_ms': 50, 'message': {'type': 'state', 'generation': 0,
                                            'state': 'thinking'}}]
        js = mm_shot.render_mock_js({}, [], 'mate', stream)
        self.assertIn('var stream = [{"at_ms": 50, "message": {"type": "state", '
                      '"generation": 0, "state": "thinking"}}];', js)
        self.assertIn('replayStream(streamId);', js)
        self.assertIn('runCallback(id, message);', js)

    def test_stream_stays_out_of_commands_and_renders_verbatim(self):
        fixture = {'load_settings': {'theme': 'light'},
                   'events': [{'event': 'safety-check', 'payload': {'x': 1}}],
                   'stream': [{'at_ms': 0, 'message': {'type': 'cancel',
                                                       'generation': 0}}]}
        commands, events, stream = mm_shot.mock_context(fixture, 'dark', 'en')
        self.assertEqual(stream, [{'at_ms': 0, 'message': {'type': 'cancel',
                                                           'generation': 0}}])
        self.assertNotIn('stream', commands)
        self.assertEqual(len(events), 1)
        self.assertEqual(commands['load_settings']['theme'], 'dark')
        js = mm_shot.render_mock_js(commands, events, 'mate', stream)
        self.assertIn('var stream = [{"at_ms": 0, "message": {"type": "cancel", '
                      '"generation": 0}}];', js)

    def test_render_mock_js_accepts_omitted_stream(self):
        self.assertIn('var stream = [];',
                      mm_shot.render_mock_js({}, [], 'mate'))


@unittest.skipUnless(shutil.which('node'), 'Node is needed to execute the mock')
class MateStreamNodeTests(unittest.TestCase):
    def node(self, source):
        proc = subprocess.run([shutil.which('node'), '--input-type=module'],
                              input=source, capture_output=True, text=True, encoding='utf-8',
                              cwd=mm_shot.ROOT, timeout=30)
        self.assertEqual(proc.returncode, 0, proc.stdout + proc.stderr)

    def test_stream_reaches_any_channel_invoke_as_raw_messages(self):
        stream = [
            {'at_ms': 0, 'message': {'type': 'state', 'generation': 0, 'state': 'thinking'}},
            {'at_ms': 10, 'message': {'type': 'line', 'generation': 0,
                                      'key': 'robot.look_consent'}},
            {'at_ms': 20, 'message': {'type': 'item', 'generation': 0,
                                      'item': {'type': 'say', 'text': 'hi', 'mood': 'calm'}}},
        ]
        js = mm_shot.render_mock_js({}, [], 'mate', stream)
        self.node('''
import assert from 'node:assert/strict';
import vm from 'node:vm';
let load;
const timers = [];
const context = { console, setTimeout: (fn, ms) => timers.push(fn),
  document: { documentElement: { setAttribute() {}, style: {} } },
  window: { addEventListener: (name, fn) => { load = fn; } } };
vm.runInNewContext(''' + json.dumps(js) + ''', context);
const ipc = context.window.__TAURI_INTERNALS__;
const received = [];
const callback = ipc.transformCallback(value => received.push(value));
// The command name does not matter: any invoke carrying a Channel opens the
// stream and receives each scripted message as the raw AnswerMsg — the page's
// parser (src/ipc/answer-stream.ts) validates `type`/`generation` directly.
await ipc.invoke('mate_subscribe', {channel: `__CHANNEL__:${callback}`});
// Opened before 'load': at_ms 0 is due at once, 10/20 still wait for their time.
assert.ok(received.length >= 1 && received.length < 3);
load();
while (timers.length) timers.shift()();
assert.deepEqual(received.map(message => message.type), ['state', 'line', 'item']);
assert.equal(received[0].state, 'thinking');
assert.equal(received[1].key, 'robot.look_consent');
assert.equal(received[2].item.mood, 'calm');
assert.equal(await ipc.invoke('mate_subscribe'), null);
''')


class StalenessTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        self.dist = self.root / 'dist'
        (self.root / 'src').mkdir()
        (self.root / 'locales').mkdir()
        self.dist.mkdir()
        (self.dist / 'home.html').write_text('x', encoding='utf-8')
        self.sources = [self.root / 'home.html',
                        self.root / 'src' / 'app.ts',
                        self.root / 'locales' / 'en.json']
        for path in self.sources:
            path.write_text('x', encoding='utf-8')
        self._touch(self.dist / 'home.html', 1000)
        for path in self.sources:
            self._touch(path, 900)

    def tearDown(self):
        self.tmp.cleanup()

    @staticmethod
    def _touch(path, mtime):
        os.utime(path, (mtime, mtime))

    def test_fresh_dist_is_not_stale(self):
        self.assertFalse(mm_shot.dist_stale(self.dist, self.root))

    def test_each_source_family_makes_it_stale(self):
        for path in self.sources:
            self._touch(self.dist / 'home.html', 1000)
            self._touch(path, 2000)
            self.assertTrue(mm_shot.dist_stale(self.dist, self.root), path.name)

    def test_missing_or_empty_dist_is_stale(self):
        self.assertTrue(mm_shot.dist_stale(self.root / 'nope', self.root))
        empty = self.root / 'empty-dist'
        empty.mkdir()
        self.assertTrue(mm_shot.dist_stale(empty, self.root))

    def test_new_asset_cannot_hide_stale_html(self):
        asset = self.dist / 'new.css'
        asset.write_text('body{}', encoding='utf-8')
        self._touch(asset, 3000)
        self._touch(self.sources[0], 2000)
        self.assertTrue(mm_shot.dist_stale(self.dist, self.root))


class ServerTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        base = Path(self.tmp.name)
        self.dist = base / 'dist'
        self.fixtures = base / 'fixtures'
        self.dist.mkdir()
        self.fixtures.mkdir()
        (self.dist / 'home.html').write_text(
            '<!doctype html><html><head><title>home</title></head><body>ok</body></html>',
            encoding='utf-8')
        (self.fixtures / 'home.json').write_text(json.dumps(
            {'load_settings': {'theme': 'light', 'language': 'en'}, 'greeting': 'Привет'},
            ensure_ascii=False), encoding='utf-8')
        self.server = mm_shot.start_server(self.dist, self.fixtures)
        host, port = self.server.server_address[:2]
        self.base = f'http://{host}:{port}'

    def tearDown(self):
        self.server.shutdown()
        self.server.server_close()
        self.tmp.cleanup()

    def get(self, path):
        with urllib.request.urlopen(self.base + path, timeout=10) as response:
            return response.status, response.headers.get('Content-Type'), response.read()

    def test_binds_to_loopback_with_free_port(self):
        host, port = self.server.server_address[:2]
        self.assertEqual(host, '127.0.0.1')
        self.assertGreater(port, 0)

    def test_page_is_injected_once_at_head_start(self):
        status, content_type, body = self.get('/home.html?__mm_theme=dark&__mm_lang=ru')
        text = body.decode('utf-8')
        self.assertEqual(status, 200)
        self.assertIn('text/html', content_type)
        tag = '<script src="/__mm_mock.js?page=home&theme=dark&lang=ru"></script>'
        self.assertEqual(text.count(tag), 1)
        self.assertLess(text.index(tag), text.index('<title>'))

    def test_mock_js_uses_fixture_and_overrides(self):
        status, content_type, body = self.get('/__mm_mock.js?page=home&theme=dark&lang=ru')
        text = body.decode('utf-8')
        self.assertEqual(status, 200)
        self.assertIn('javascript', content_type)
        self.assertIn('"theme": "dark"', text)
        self.assertIn('"language": "ru"', text)
        self.assertIn('Привет', text)
        self.assertIn('"profileAsked": true', text)

    def test_unknown_fixture_gets_null_default(self):
        status, _, body = self.get('/__mm_mock.js?page=ghost&theme=light&lang=en')
        text = body.decode('utf-8')
        self.assertEqual(status, 200)
        self.assertIn('Promise.resolve(null)', text)
        self.assertIn('"theme": "light"', text)

    def test_static_file_and_404(self):
        (self.dist / 'style.css').write_text('body{}', encoding='utf-8')
        status, _, body = self.get('/style.css')
        self.assertEqual((status, body.decode()), (200, 'body{}'))
        with self.assertRaises(urllib.error.HTTPError) as ctx:
            self.get('/missing.html')
        self.assertEqual(ctx.exception.code, 404)
        ctx.exception.close()

    def test_no_traversal_secrets_or_directory_listing_on_get_or_head(self):
        (self.dist / 'assets').mkdir()
        (self.dist.parent / 'outside.css').write_text('outside', encoding='utf-8')
        # Synthetic sentinel files only; no project secrets are opened.
        for name in ('.env', 'private.key', 'private.pem', 'credentials.json'):
            (self.dist / name).write_text('sentinel', encoding='utf-8')
        for path in ('/../outside.css', '/%2e%2e/outside.css', '/assets/../../outside.css',
                     '/..%5coutside.css', '/%252e%252e/outside.css', '/assets/',
                     '/.env', '/private.key', '/private.pem', '/credentials.json',
                     '/home.html::$DATA', '/home.html%00.css'):
            for method in ('GET', 'HEAD'):
                with self.subTest(path=path, method=method):
                    request = urllib.request.Request(self.base + path, method=method)
                    with self.assertRaises(urllib.error.HTTPError) as ctx:
                        urllib.request.urlopen(request, timeout=10)
                    self.assertIn(ctx.exception.code, (403, 404))
                    ctx.exception.close()

    def test_symlink_escape_is_blocked(self):
        outside = self.dist.parent / 'outside.css'
        outside.write_text('outside', encoding='utf-8')
        try:
            (self.dist / 'link.css').symlink_to(outside)
        except OSError:
            self.skipTest('symlinks unavailable on this Windows account')
        with self.assertRaises(urllib.error.HTTPError) as ctx:
            self.get('/link.css')
        self.assertEqual(ctx.exception.code, 403)
        ctx.exception.close()

    def test_injection_does_not_modify_disk_and_head_never_bypasses_it(self):
        page = self.dist / 'home.html'
        before = page.read_bytes()
        self.get('/home.html')
        with urllib.request.urlopen(urllib.request.Request(
                self.base + '/home.html', method='HEAD'), timeout=10) as response:
            self.assertEqual(response.read(), b'')
            self.assertGreater(int(response.headers['Content-Length']), len(before))
        self.assertEqual(page.read_bytes(), before)

    def test_mate_state_reaches_the_mock_script(self):
        (self.dist / 'mate.html').write_text(
            '<!doctype html><html><head><title>mate</title></head><body>ok</body></html>',
            encoding='utf-8')
        (self.fixtures / 'mate-consent.json').write_text(json.dumps(
            {'stream': [{'at_ms': 0, 'message': {'type': 'line', 'generation': 0,
                                                 'key': 'robot.look_consent'}}]}),
            encoding='utf-8')
        status, _, body = self.get(
            '/mate.html?__mm_theme=light&__mm_lang=en&__mm_state=consent')
        self.assertEqual(status, 200)
        self.assertIn(f'{mm_shot.MOCK_ROUTE}?page=mate&theme=light&lang=en&state=consent',
                      body.decode('utf-8'))
        status, _, body = self.get('/__mm_mock.js?page=mate&state=consent&theme=light&lang=en')
        mock = body.decode('utf-8')
        self.assertEqual(status, 200)
        self.assertIn('var stream = [{"at_ms": 0, "message": {"type": "line", '
                      '"generation": 0, "key": "robot.look_consent"}}];', mock)
        self.assertIn('var presetOnboarded = true;', mock)
        self.assertIn('"profileAsked": true', mock)

    def test_bad_state_value_is_rejected_on_page_and_mock(self):
        for path in ('/home.html?__mm_state=Evil',
                     '/__mm_mock.js?page=mate&state=Evil&lang=en'):
            with self.subTest(path=path):
                with self.assertRaises(urllib.error.HTTPError) as ctx:
                    self.get(path)
                self.assertEqual(ctx.exception.code, 400)
                ctx.exception.close()

    def test_query_values_cannot_inject_html_or_script(self):
        for path in ('/home.html?__mm_theme=%22%3E%3Cscript%3E',
                     '/__mm_mock.js?page=../home', '/__mm_mock.js?lang=xx'):
            with self.assertRaises(urllib.error.HTTPError) as ctx:
                self.get(path)
            self.assertEqual(ctx.exception.code, 400)
            ctx.exception.close()


@unittest.skipUnless(shutil.which('node'), 'Node is needed to execute the mock and TS parsers')
class JavaScriptTests(unittest.TestCase):
    def node(self, source):
        proc = subprocess.run([shutil.which('node'), '--input-type=module'],
                              input=source, capture_output=True, text=True, encoding='utf-8',
                              cwd=mm_shot.ROOT, timeout=30)
        self.assertEqual(proc.returncode, 0, proc.stdout + proc.stderr)

    def test_fixtures_survive_the_real_page_parsers(self):
        self.node('''
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { parseLessonCards, CARDS_LIST_COMMAND } from './src/home/lessons.ts';
import { parseLessonCard, LESSON_CARD_EVENT } from './src/card.ts';
import { parseHistory } from './src/chat/model.ts';
import { parseSettings } from './src/settings/model.ts';
const fixture = (page) => JSON.parse(readFileSync(`tools/shot_fixtures/${page}.json`, 'utf8'));
const home = fixture('home');
assert.deepEqual(parseLessonCards(home[CARDS_LIST_COMMAND]), home[CARDS_LIST_COMMAND]);
const card = fixture('card').events.find(e => e.event === LESSON_CARD_EVENT).payload;
assert.ok(card.id);
assert.deepEqual(parseLessonCard(card), card);
assert.deepEqual(parseHistory(fixture('chat').chat_subscribe), fixture('chat').chat_subscribe);
for (const page of ['home', 'card', 'chat']) {
  const settings = fixture(page).load_settings;
  const parsed = parseSettings(settings);
  for (const [key, value] of Object.entries(settings)) assert.deepEqual(parsed[key], value);
}
''')

    def test_v2_callbacks_events_channel_metadata_and_replay(self):
        commands, events, _ = mm_shot.mock_context(
            mm_shot.load_fixture(mm_shot.FIXTURES, 'card'), 'dark', 'ru')
        commands['chat_subscribe'] = [{'role': 'mate', 'text': 'hello', 'timestamp': 0,
                                       'mood': 'happy'}]
        js = mm_shot.render_mock_js(commands, events, 'card')
        self.node('''
import assert from 'node:assert/strict';
import vm from 'node:vm';
let load;
const timers = [];
const context = { console, setTimeout: fn => timers.push(fn),
  document: { documentElement: { setAttribute() {}, style: {} } },
  window: { addEventListener: (name, fn) => { assert.equal(name, 'load'); load = fn; } } };
vm.runInNewContext(''' + json.dumps(js) + ''', context);
const ipc = context.window.__TAURI_INTERNALS__;
assert.equal(ipc.metadata.currentWindow.label, 'card');
assert.equal(ipc.metadata.currentWebview.label, 'card');
assert.equal((await ipc.invoke('load_settings')).language, 'ru');
assert.equal(await ipc.invoke('unknown'), null);
let count = 0;
const callback = ipc.transformCallback(() => count++);
ipc.runCallback(callback, null); ipc.runCallback(callback, null);
assert.equal(count, 2);
ipc.unregisterCallback(callback); ipc.runCallback(callback, null);
assert.equal(count, 2);
const once = ipc.transformCallback(() => count++, true);
ipc.runCallback(once, null); ipc.runCallback(once, null);
assert.equal(count, 3);
let received = [];
const handler = ipc.transformCallback(event => received.push(event));
const id = await ipc.invoke('plugin:event|listen', {event: 'lesson-card', handler});
assert.equal(timers.length, 0);
load(); timers.forEach(fn => fn());
assert.equal(received.length, 1);
assert.equal(received[0].id, id);
assert.equal(received[0].payload.id, 'card-videocall');
await ipc.invoke('plugin:event|emit', {event: 'lesson-card', payload: 5});
assert.equal(received[1].payload, 5);
context.window.__TAURI_EVENT_PLUGIN_INTERNALS__.unregisterListener('lesson-card', id);
await ipc.invoke('plugin:event|unlisten', {event: 'lesson-card', eventId: id});
await ipc.invoke('plugin:event|emit', {event: 'lesson-card', payload: 6});
assert.equal(received.length, 2);
for (const serialized of [false, true]) {
  let response;
  const id = ipc.transformCallback(value => { response = value; });
  await ipc.invoke('chat_subscribe', {channel: serialized ? `__CHANNEL__:${id}` : {id}});
  assert.equal(response.index, 0);
  assert.equal(response.message[0].mood, 'happy');
}
''')


class ProcessTests(unittest.TestCase):
    def test_missing_page_fails_before_edge_or_server(self):
        with tempfile.TemporaryDirectory() as folder, \
                patch.object(mm_shot, 'find_edge', return_value='edge.exe'), \
                patch.object(mm_shot, 'dist_stale', return_value=False), \
                patch.object(mm_shot, 'DIST', Path(folder)), \
                patch.object(mm_shot, 'start_server') as server, \
                contextlib.redirect_stdout(io.StringIO()) as output:
            (Path(folder) / 'home.html').write_text('page', encoding='utf-8')
            self.assertEqual(mm_shot.main(['chat']), 1)
        server.assert_not_called()
        self.assertIn('missing pages: chat', output.getvalue())

    @unittest.skipUnless(os.name == 'nt', 'Windows job lifecycle')
    def test_edge_job_is_assigned_before_resume_and_closed_on_every_exit(self):
        for failure in ('none', 'timeout', 'assign', 'resume', 'launch'):
            with self.subTest(failure=failure):
                order = []
                kernel = Mock()
                native = Mock()
                kernel.CreateJobObjectW.return_value = 42
                kernel.SetInformationJobObject.return_value = 1
                kernel.AssignProcessToJobObject.side_effect = lambda *args: (
                    order.append('assign') or (0 if failure == 'assign' else 1))
                native.NtResumeProcess.side_effect = lambda *args: (
                    order.append('resume') or (-1 if failure == 'resume' else 0))
                proc = Mock(returncode=0, _handle=123)
                proc.poll.return_value = 0 if failure == 'none' else None

                def communicate(**kwargs):
                    order.append('communicate')
                    if failure == 'timeout':
                        raise subprocess.TimeoutExpired('Edge', 120)
                    return '', ''

                proc.communicate.side_effect = communicate
                popen = Mock()
                popen.return_value.__enter__ = Mock(return_value=proc)
                popen.return_value.__exit__ = Mock(return_value=False)
                if failure == 'launch':
                    popen.side_effect = OSError('missing')
                with patch.object(mm_shot.ctypes, 'WinDLL',
                                  side_effect=lambda name, **kw: kernel if name == 'kernel32' else native), \
                        patch.object(mm_shot.subprocess, 'Popen', popen):
                    if failure == 'none':
                        self.assertEqual(mm_shot.run_edge(['edge.exe']).returncode, 0)
                    else:
                        with self.assertRaises((OSError, subprocess.TimeoutExpired)):
                            mm_shot.run_edge(['edge.exe'])
                kernel.CloseHandle.assert_called_once_with(42)
                self.assertEqual(popen.call_args.kwargs['creationflags'], 4)
                self.assertFalse(popen.call_args.kwargs.get('shell', False))
                if failure in ('none', 'timeout'):
                    self.assertEqual(order, ['assign', 'resume', 'communicate'])
                if failure not in ('none', 'launch'):
                    proc.kill.assert_called_once()
                    proc.wait.assert_called_once()

    def test_build_uses_only_fixed_command_and_reports_missing_npm(self):
        with patch.object(mm_shot.shutil, 'which', return_value=None):
            ok, why = mm_shot.build_dist()
        self.assertFalse(ok)
        self.assertIn('npm not found', why)
        with patch.object(mm_shot.shutil, 'which', return_value='npm.cmd'), \
                patch.object(mm_shot.subprocess, 'run', return_value=Mock(returncode=0)) as run:
            self.assertTrue(mm_shot.build_dist()[0])
        self.assertEqual(run.call_args.args[0], ['npm.cmd', 'run', 'build'])
        self.assertFalse(run.call_args.kwargs.get('shell', False))
        for error in (OSError('missing'), subprocess.TimeoutExpired('npm', 900)):
            with patch.object(mm_shot.shutil, 'which', return_value='npm.cmd'), \
                    patch.object(mm_shot.subprocess, 'run', side_effect=error):
                self.assertFalse(mm_shot.build_dist()[0])

    def test_missing_edge_is_a_clear_failure(self):
        with patch.object(mm_shot, 'find_edge', return_value=None), \
                contextlib.redirect_stdout(io.StringIO()) as output:
            self.assertEqual(mm_shot.main([]), 1)
        self.assertIn('Microsoft Edge not found', output.getvalue())

    def test_screenshot_does_not_accept_old_png_or_hide_launch_errors(self):
        with tempfile.TemporaryDirectory() as folder:
            out = Path(folder) / 'home-light-en.png'
            old_png = b'\x89PNG\r\n\x1a\n' + b'\0\0\0\rIHDR' + struct.pack('>II', 1100, 800)
            for failure in (None, OSError('missing'), subprocess.TimeoutExpired('Edge', 120)):
                out.write_bytes(old_png)
                with patch.object(mm_shot, 'run_edge', side_effect=failure,
                                  return_value=Mock(returncode=0)) as run:
                    reason = mm_shot.shoot_page('edge.exe', 'http://127.0.0.1:1234',
                                               'home', 'light', 'en', 1500, Path(folder), folder)
                self.assertIsNotNone(reason)
                self.assertFalse(out.exists())
                self.assertEqual(run.call_args.args[0][0], 'edge.exe')

    def test_state_and_size_reach_the_shot_and_name_the_png(self):
        png = b'\x89PNG\r\n\x1a\n' + b'\0\0\0\rIHDR' + struct.pack('>II', 400, 400)
        with tempfile.TemporaryDirectory() as folder:
            captured = []

            def fake_run(cmd):
                captured.append(cmd)
                shot = next(arg for arg in cmd if arg.startswith('--screenshot='))
                Path(shot.split('=', 1)[1]).write_bytes(png)
                return Mock(returncode=0)

            with patch.object(mm_shot, 'run_edge', side_effect=fake_run):
                reason = mm_shot.shoot_page('edge.exe', 'http://127.0.0.1:9',
                                            'mate', 'light', 'en', 1500,
                                            Path(folder), folder,
                                            size=(400, 400), state='consent')
            self.assertIsNone(reason)
            self.assertIn('--window-size=400,400', captured[0])
            self.assertIn('__mm_state=consent', captured[0][-1])
            self.assertTrue((Path(folder) / 'mate-consent-light-en.png').is_file())
            bad = mm_shot.shoot_page('edge.exe', 'http://127.0.0.1:9', 'mate',
                                     'light', 'en', 1500, Path(folder), folder,
                                     size=(0, 400))
            self.assertIsNotNone(bad)

    def test_missing_state_fixture_fails_before_edge_or_server(self):
        with tempfile.TemporaryDirectory() as folder, \
                patch.object(mm_shot, 'find_edge', return_value='edge.exe'), \
                patch.object(mm_shot, 'dist_stale', return_value=False), \
                patch.object(mm_shot, 'DIST', Path(folder)), \
                patch.object(mm_shot, 'FIXTURES', Path(folder) / 'fixtures'), \
                patch.object(mm_shot, 'start_server') as server, \
                contextlib.redirect_stdout(io.StringIO()) as output:
            (Path(folder) / 'mate.html').write_text('page', encoding='utf-8')
            self.assertEqual(mm_shot.main(['mate', '--state', 'nope']), 1)
        server.assert_not_called()
        self.assertIn('mate-nope.json not found', output.getvalue())

    def test_main_passes_state_and_size_through(self):
        server = Mock(server_address=('127.0.0.1', 1234))
        with tempfile.TemporaryDirectory() as folder, \
                patch.object(mm_shot, 'find_edge', return_value='edge.exe'), \
                patch.object(mm_shot, 'dist_stale', return_value=False), \
                patch.object(mm_shot, 'DIST', Path(folder)), \
                patch.object(mm_shot, 'FIXTURES', Path(folder) / 'fixtures'), \
                patch.object(mm_shot, 'SHOTS', Path(folder) / 'shots'), \
                patch.object(mm_shot, 'start_server', return_value=server), \
                patch.object(mm_shot, 'shoot_page', return_value=None) as shoot, \
                contextlib.redirect_stdout(io.StringIO()):
            (Path(folder) / 'mate.html').write_text('page', encoding='utf-8')
            fixtures = Path(folder) / 'fixtures'
            fixtures.mkdir()
            (fixtures / 'mate-consent.json').write_text('{}', encoding='utf-8')
            self.assertEqual(
                mm_shot.main(['mate', '--state', 'consent', '--size', '400x400']), 0)
        self.assertEqual(shoot.call_args.kwargs['state'], 'consent')
        self.assertEqual(shoot.call_args.kwargs['size'], (400, 400))

    def test_state_rejects_any_page_selection_but_mate(self):
        for argv in (['home', '--state', 'consent'], ['--state', 'consent']):
            with self.subTest(argv=argv):
                with contextlib.redirect_stderr(io.StringIO()):
                    with self.assertRaises(SystemExit) as ctx:
                        mm_shot.main(argv)
                self.assertEqual(ctx.exception.code, 2)

    def test_server_and_profile_cleaned_up_on_screenshot_failure(self):
        server = Mock(server_address=('127.0.0.1', 1234))
        profiles = []

        def fail(*args, **kwargs):
            profiles.append(Path(args[-1]))
            self.assertTrue(profiles[-1].is_dir())
            raise OSError('capture failed')

        with tempfile.TemporaryDirectory() as folder, \
                patch.object(mm_shot, 'find_edge', return_value='edge.exe'), \
                patch.object(mm_shot, 'dist_stale', return_value=False), \
                patch.object(mm_shot, 'DIST', Path(folder)), \
                patch.object(mm_shot, 'SHOTS', Path(folder) / 'shots'), \
                patch.object(mm_shot, 'start_server', return_value=server), \
                patch.object(mm_shot, 'shoot_page', side_effect=fail), \
                contextlib.redirect_stdout(io.StringIO()):
            (Path(folder) / 'home.html').write_text('page', encoding='utf-8')
            self.assertEqual(mm_shot.main(['home']), 1)
        server.shutdown.assert_called_once()
        server.server_close.assert_called_once()
        self.assertFalse(profiles[0].exists())

    def test_server_closed_if_output_directory_cannot_be_created(self):
        server = Mock(server_address=('127.0.0.1', 1234))
        with tempfile.TemporaryDirectory() as folder, \
                patch.object(mm_shot, 'find_edge', return_value='edge.exe'), \
                patch.object(mm_shot, 'dist_stale', return_value=False), \
                patch.object(mm_shot, 'DIST', Path(folder)), \
                patch.object(mm_shot, 'SHOTS', Path(folder) / 'blocked'), \
                patch.object(mm_shot, 'start_server', return_value=server), \
                contextlib.redirect_stdout(io.StringIO()):
            (Path(folder) / 'home.html').write_text('page', encoding='utf-8')
            (Path(folder) / 'blocked').write_text('file', encoding='utf-8')
            self.assertEqual(mm_shot.main(['home']), 1)
        server.shutdown.assert_called_once()
        server.server_close.assert_called_once()


class ParserTests(unittest.TestCase):
    def test_defaults(self):
        args = mm_shot.build_parser().parse_args([])
        self.assertEqual(args.pages, [])
        self.assertEqual((args.theme, args.lang, args.wait_ms, args.build),
                         ('light', 'en', 1500, False))

    def test_full_arguments(self):
        args = mm_shot.build_parser().parse_args(
            ['home', 'chat', '--theme', 'dark', '--lang', 'both', '--wait-ms', '2500', '--build'])
        self.assertEqual(args.pages, ['home', 'chat'])
        self.assertEqual((args.theme, args.lang, args.wait_ms, args.build),
                         ('dark', 'both', 2500, True))

    def test_state_and_size_arguments(self):
        args = mm_shot.build_parser().parse_args(
            ['mate', '--state', 'consent', '--size', '400x400'])
        self.assertEqual((args.state, args.size), ('consent', (400, 400)))
        parser = mm_shot.build_parser()
        for argv in (['mate', '--state', 'Bad'], ['mate', '--state', 'a.b'],
                     ['mate', '--size', '400'], ['mate', '--size', 'big'],
                     ['mate', '--size', '10x10'], ['mate', '--size', '400x99999']):
            with self.assertRaises(SystemExit) as ctx:
                parser.parse_args(argv)
            self.assertEqual(ctx.exception.code, 2)

    def test_bad_page_flag_or_value_is_usage_error(self):
        parser = mm_shot.build_parser()
        for argv in (['nope'], ['--theme', 'blue'], ['--wait-ms', 'soon']):
            with self.assertRaises(SystemExit) as ctx:
                parser.parse_args(argv)
            self.assertEqual(ctx.exception.code, 2)


if __name__ == '__main__':
    unittest.main()
