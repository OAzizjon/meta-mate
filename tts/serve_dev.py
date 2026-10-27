"""MMS-TTS on this machine for development: the same `POST /synthesize` contract as the
Modal service, on 127.0.0.1 only.

    MMS_TTS_SECRET=... C:/mmw/py-tts/Scripts/python.exe tts/serve_dev.py [--port 8788]

Request: header `X-MMS-Secret`, JSON `{"lang": "en" | "ru", "text": "...", "speed": 1.0}` (at most
300 characters: one sentence). Response: `audio/wav`, mono 16-bit, 16 kHz. The proxy is
the only client (server/metamate_proxy/app.py); text is never logged.
Speed is optional (default 1.0), a number from 0.8 to 1.3.
"""

import argparse
import hmac
import json
import os
import sys
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import mms  # noqa: E402

MAX_TEXT = 300
MAX_BODY = 4096


class Handler(BaseHTTPRequestHandler):
    secret = ''

    def log_message(self, fmt: str, *args) -> None:  # no request lines: they carry nothing useful
        pass

    def reply(self, status: int, body: bytes, kind: str = 'application/json') -> None:
        self.send_response(status)
        self.send_header('Content-Type', kind)
        self.send_header('Content-Length', str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_POST(self) -> None:
        if self.path != '/synthesize':
            return self.reply(404, b'{}')
        given = self.headers.get('X-MMS-Secret', '').encode('utf-8', errors='replace')
        if not hmac.compare_digest(given, self.secret.encode('utf-8')):
            return self.reply(401, b'{}')
        try:
            length = int(self.headers.get('Content-Length') or 0)
        except ValueError:
            length = -1
        if not 0 < length <= MAX_BODY:
            return self.reply(413, b'{}')
        try:
            request = json.loads(self.rfile.read(length))
            lang, text = request['lang'], request['text']
            if lang not in mms.CHECKPOINTS or not isinstance(text, str) or not 0 < len(text) <= MAX_TEXT:
                raise ValueError
        except (ValueError, KeyError, TypeError, RecursionError):
            return self.reply(400, b'{}')
        try:
            speed = mms.validate_speed(request.get('speed', 1.0))
        except ValueError:
            return self.reply(422, b'{}')
        started = time.perf_counter()
        try:
            samples, rate = mms.synthesize(lang, text, speed)
        except Exception:  # e.g. "...": nothing the VITS tokenizer can speak
            return self.reply(422, b'{}')
        wav = mms.to_wav(samples, rate)
        self.send_response(200)
        self.send_header('Content-Type', 'audio/wav')
        self.send_header('Content-Length', str(len(wav)))
        self.send_header('X-Synth-Ms', str(round(1000 * (time.perf_counter() - started))))
        self.end_headers()
        self.wfile.write(wav)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument('--port', type=int, default=8788)
    args = parser.parse_args()
    secret = os.environ.get('MMS_TTS_SECRET', '')
    if len(secret) < 16:
        print('MMS_TTS_SECRET (16+ characters) is required')
        return 2
    Handler.secret = secret
    for lang in mms.CHECKPOINTS:  # load both voices before the first request
        mms.load(lang)
    print(f'MMS-TTS dev server on http://127.0.0.1:{args.port}', flush=True)
    ThreadingHTTPServer(('127.0.0.1', args.port), Handler).serve_forever()
    return 0


if __name__ == '__main__':
    sys.exit(main())
