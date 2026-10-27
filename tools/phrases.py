"""Build and check the English and Russian cached MMS-TTS phrases."""

import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import wave

ROOT = Path(__file__).resolve().parent.parent
TTS_PYTHON = 'C:/mmw/py-tts/Scripts/python.exe'
PLACEHOLDER = re.compile(r'\{[^{}]+\}')


def _texts(root):
    keys = (root / 'assets/phrases/keys.txt').read_text(encoding='utf-8').splitlines()
    keys = sorted(set(key.strip() for key in keys if key.strip()))
    if any(not re.fullmatch(r'[a-zA-Z0-9_.]+', key) for key in keys):
        raise ValueError('invalid phrase key')
    texts = {}
    for lang in ('en', 'ru'):
        locale = json.loads((root / 'locales' / f'{lang}.json').read_text(encoding='utf-8'))
        texts[lang] = {}
        for key in keys:
            text = locale.get(key)
            if not isinstance(text, str) or not text.strip():
                raise ValueError(f'{lang}/{key}: missing locale text')
            if not PLACEHOLDER.search(text):
                texts[lang][key] = text
    return texts


def _hash(text):
    return hashlib.sha256(text.encode('utf-8')).hexdigest()


def _manifest(base):
    path = base / 'manifest.json'
    if not path.exists():
        return {}
    result = json.loads(path.read_text(encoding='utf-8'))
    if not isinstance(result, dict) or any(not isinstance(entries, dict) for entries in result.values()):
        raise ValueError('invalid phrase manifest')
    return result


def synthesize(root, tts_python, lang, text, destination):
    """Keep heavy model dependencies in the dedicated TTS interpreter."""
    code = (
        'import sys; from pathlib import Path; '
        'sys.path.insert(0, sys.argv[1]); import mms; '
        'samples, rate = mms.synthesize(sys.argv[2], sys.argv[3]); '
        'Path(sys.argv[4]).write_bytes(mms.to_wav(samples, rate))'
    )
    subprocess.run([str(tts_python), '-c', code, str(root / 'tts'), lang, text, str(destination)],
                   cwd=root, check=True, stdin=subprocess.DEVNULL)


def _duration(path):
    with wave.open(str(path), 'rb') as audio:
        if (audio.getframerate(), audio.getnchannels(), audio.getsampwidth(), audio.getcomptype()) != (16000, 1, 2, 'NONE'):
            raise ValueError('phrase audio must be 16 kHz mono PCM16')
        return round(audio.getnframes() * 1000 / audio.getframerate())


def build(root=ROOT, *, tts_python=TTS_PYTHON, force=False, synthesizer=None):
    """Return the manifest; the injectable synthesizer writes a WAV for offline tests."""
    root = Path(root)
    base = root / 'assets/phrases'
    texts = _texts(root)
    previous = _manifest(base)
    result = {}
    for lang, entries in texts.items():
        result[lang] = {}
        for key, text in entries.items():
            filename = f'{lang}/{key}.wav'
            destination = base / filename
            digest = _hash(text)
            old = previous.get(lang, {}).get(key)
            if (not force and isinstance(old, dict) and old.get('text_sha256') == digest
                    and old.get('file') == filename and destination.is_file()):
                result[lang][key] = old
                continue
            destination.parent.mkdir(parents=True, exist_ok=True)
            with tempfile.TemporaryDirectory(prefix='.phrase-', dir=destination.parent) as folder:
                temporary = Path(folder) / 'audio.wav'
                if synthesizer is None:
                    synthesize(root, tts_python, lang, text, temporary)
                else:
                    synthesizer(lang, text, temporary)
                ms = _duration(temporary)
                temporary.replace(destination)
            result[lang][key] = {'file': filename, 'text_sha256': digest, 'ms': ms}
    with tempfile.TemporaryDirectory(prefix='.manifest-', dir=base) as folder:
        temporary = Path(folder) / 'manifest.json'
        temporary.write_text(json.dumps(result, ensure_ascii=False, sort_keys=True, indent=2) + '\n',
                             encoding='utf-8', newline='\n')
        temporary.replace(base / 'manifest.json')
    return result


def check(root=ROOT):
    """Return stale/missing entry errors without importing or running MMS."""
    root = Path(root)
    base = root / 'assets/phrases'
    texts = _texts(root)
    if not (base / 'manifest.json').is_file():
        return ['missing phrase manifest']
    manifest = _manifest(base)
    errors = []
    for lang in sorted(set(texts) | set(manifest)):
        for key in sorted(set(texts.get(lang, {})) | set(manifest.get(lang, {}))):
            text = texts.get(lang, {}).get(key)
            entry = manifest.get(lang, {}).get(key)
            label = f'{lang}/{key}'
            if not isinstance(entry, dict):
                errors.append(f'{label}: missing or invalid entry')
                continue
            if text is None or entry.get('text_sha256') != _hash(text):
                errors.append(f'{label}: stale text')
            filename = f'{lang}/{key}.wav'
            if entry.get('file') != filename:
                errors.append(f'{label}: invalid file path')
            elif not (base / filename).is_file():
                errors.append(f'{label}: missing file')
    return errors


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest='action', required=True)
    sp = sub.add_parser('build', help='synthesize changed phrases')
    sp.add_argument('--tts-python', default=TTS_PYTHON)
    sp.add_argument('--force', action='store_true')
    sub.add_parser('check', help='verify cached phrases without TTS')
    args = parser.parse_args(argv)
    try:
        if args.action == 'build':
            build(tts_python=args.tts_python, force=args.force)
            print('phrases: OK')
            return 0
        errors = check()
        for error in errors:
            print(error)
        print('phrases: ' + ('FAILED' if errors else 'OK'))
        return 1 if errors else 0
    except (OSError, ValueError, wave.Error, subprocess.SubprocessError) as error:
        print(f'phrases: FAILED ({error})')
        return 1


if __name__ == '__main__':
    sys.exit(main())
