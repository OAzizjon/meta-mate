"""Cached phrases use fake WAV synthesis; no model dependencies or network."""

import contextlib
import io
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch
import wave

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import mm
import phrases


class PhraseTests(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.root = Path(directory.name)
        self.base = self.root / 'assets/phrases'
        self.base.mkdir(parents=True)
        (self.root / 'locales').mkdir()
        (self.base / 'keys.txt').write_text('robot.z\nrobot.a\nrobot.step\n', encoding='utf-8')
        self.locales = {'en': {'robot.z': 'Think.', 'robot.a': 'Hello.', 'robot.step': 'Step {n}.'},
                        'ru': {'robot.z': '\u0414\u0443\u043c\u0430\u044e.', 'robot.a': '\u041f\u0440\u0438\u0432\u0435\u0442.', 'robot.step': '{n}'}}
        self.save_locales()
        self.calls = []

    def save_locales(self):
        for lang, locale in self.locales.items():
            (self.root / 'locales' / f'{lang}.json').write_text(json.dumps(locale), encoding='utf-8')

    def synthesize(self, lang, text, destination):
        self.calls.append((lang, text))
        with wave.open(str(destination), 'wb') as audio:
            audio.setnchannels(1)
            audio.setsampwidth(2)
            audio.setframerate(16000)
            audio.writeframes(b'\0\0' * 1600)

    def build(self, **kwargs):
        return phrases.build(self.root, synthesizer=self.synthesize, **kwargs)

    def test_placeholders_skipped_and_duration_recorded(self):
        manifest = self.build()
        self.assertEqual(len(self.calls), 4)
        for lang in ('en', 'ru'):
            self.assertEqual(set(manifest[lang]), {'robot.a', 'robot.z'})
            self.assertEqual(manifest[lang]['robot.a']['ms'], 100)
            self.assertFalse((self.base / lang / 'robot.step.wav').exists())
        self.assertEqual(phrases.check(self.root), [])

    def test_bundle_resource_map_preserves_phrase_layout(self):
        root = Path(__file__).resolve().parents[2]
        config_dir = root / 'src-tauri'
        config = json.loads((config_dir / 'tauri.conf.json').read_text(encoding='utf-8'))
        with tempfile.TemporaryDirectory() as directory:
            resource_dir = Path(directory)
            for source, destination in config['bundle']['resources'].items():
                source = config_dir / source
                destination = resource_dir / destination
                destination.parent.mkdir(parents=True, exist_ok=True)
                if source.is_dir():
                    shutil.copytree(source, destination)
                else:
                    shutil.copyfile(source, destination)
            base = resource_dir / 'phrases'
            self.assertTrue((base / 'NOTICE.md').is_file())
            manifest = json.loads((base / 'manifest.json').read_text(encoding='utf-8'))
            for lang in ('en', 'ru'):
                self.assertTrue(manifest[lang])
                for entry in manifest[lang].values():
                    self.assertTrue((base / entry['file']).is_file(), entry['file'])

    def test_unchanged_skipped_changed_and_missing_rebuilt_force_rebuilds(self):
        self.build()
        self.calls.clear()
        self.build()
        self.assertEqual(self.calls, [])
        self.locales['en']['robot.a'] = 'Changed.'
        self.save_locales()
        self.build()
        self.assertEqual(self.calls, [('en', 'Changed.')])
        self.calls.clear()
        (self.base / 'ru/robot.z.wav').unlink()
        self.build()
        self.assertEqual(self.calls, [('ru', self.locales['ru']['robot.z'])])
        self.calls.clear()
        self.build(force=True)
        self.assertEqual(len(self.calls), 4)

    def test_check_reports_stale_missing_and_removed_entries_without_tts(self):
        self.build()
        self.locales['en']['robot.a'] = 'Changed.'
        self.locales['ru']['robot.z'] = 'Now {placeholder}'
        self.save_locales()
        (self.base / 'en/robot.z.wav').unlink()
        with patch.object(phrases, 'synthesize', side_effect=AssertionError('TTS called')):
            errors = phrases.check(self.root)
        self.assertIn('en/robot.a: stale text', errors)
        self.assertIn('en/robot.z: missing file', errors)
        self.assertIn('ru/robot.z: stale text', errors)

    def test_check_reports_missing_manifest_and_entry(self):
        self.assertEqual(phrases.check(self.root), ['missing phrase manifest'])
        manifest = self.build()
        del manifest['en']['robot.a']
        (self.base / 'manifest.json').write_text(json.dumps(manifest), encoding='utf-8')
        self.assertIn('en/robot.a: missing or invalid entry', phrases.check(self.root))

    def test_manifest_sorted_lf_and_repeatable(self):
        manifest = self.build()
        first = (self.base / 'manifest.json').read_bytes()
        expected = (json.dumps(manifest, sort_keys=True, ensure_ascii=False, indent=2) + '\n').encode('utf-8')
        self.assertEqual(first, expected)
        self.assertNotIn(b'\r', first)
        self.build(force=True)
        self.assertEqual((self.base / 'manifest.json').read_bytes(), first)

    def test_synthesis_failure_preserves_manifest_and_audio(self):
        self.build()
        original = (self.base / 'manifest.json').read_bytes()
        audio = (self.base / 'en/robot.a.wav').read_bytes()
        with patch.object(phrases, '_duration', side_effect=ValueError('bad WAV')):
            with self.assertRaises(ValueError):
                self.build(force=True)
        self.assertEqual((self.base / 'manifest.json').read_bytes(), original)
        self.assertEqual((self.base / 'en/robot.a.wav').read_bytes(), audio)

    def test_subprocess_uses_selected_interpreter_and_mms(self):
        destination = self.base / 'sample.wav'
        with patch.object(phrases.subprocess, 'run') as run:
            phrases.synthesize(self.root, 'custom python.exe', 'en', 'Hello.', destination)
        argv = run.call_args.args[0]
        self.assertEqual(argv[:2], ['custom python.exe', '-c'])
        self.assertIn('import mms', argv[2])
        self.assertIn('mms.to_wav', argv[2])
        self.assertEqual(argv[3:], [str(self.root / 'tts'), 'en', 'Hello.', str(destination)])
        self.assertTrue(run.call_args.kwargs['check'])
        self.assertEqual(run.call_args.kwargs['stdin'], subprocess.DEVNULL)

    def test_mm_refuses_phrases_in_agent_mode(self):
        for action in ('build', 'check'):
            with patch.object(mm, 'agent_mode_reason', return_value='test'), \
                    patch.object(mm, 'cmd_phrases') as command, contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(mm.main(['phrases', action]), 2)
                command.assert_not_called()

    def test_mm_forwards_build_options_and_check(self):
        for argv, expected in ((['phrases', 'build', '--tts-python', 'custom', '--force'],
                                ['build', '--tts-python', 'custom', '--force']),
                               (['phrases', 'check'], ['check'])):
            args = mm.build_parser().parse_args(argv)
            with patch.object(phrases, 'main', return_value=0) as main:
                self.assertEqual(mm.cmd_phrases(args), 0)
                main.assert_called_once_with(expected)


if __name__ == '__main__':
    unittest.main()
