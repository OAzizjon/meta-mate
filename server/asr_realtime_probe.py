"""Spike S5: Muse Voice Transcribe over the realtime WebSocket, timed like push-to-talk.

Streams a WAV at real-time pace in 80 ms frames (as a microphone would), sends
`endStream` when the "key is released", and measures what the user waits for: the time
from `endStream` to the final transcript. Run with the proxy's venv (websockets):

    C:/mmw/py-proxy/Scripts/python.exe server/asr_realtime_probe.py sample.wav

Protocol: dev.meta.ai/docs/speech-to-text (checked 2026-10-27). The key comes from the
project's secrets file through tools/mm_probe.py and is sent only in the handshake frame.
"""

import asyncio
import json
import sys
import time
import wave
from pathlib import Path

import websockets

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'tools'))
import mm_probe  # noqa: E402

URL = 'wss://api.meta.ai/v1/asr/realtime'
MODEL = 'muse-voice-transcribe-1.0'
FRAME_MS = 80


async def run(path: Path) -> dict:
    with wave.open(str(path)) as w:
        if w.getnchannels() != 1 or w.getsampwidth() != 2 or w.getframerate() not in (16000, 24000):
            raise SystemExit('need mono 16-bit PCM at 16 or 24 kHz')
        rate, pcm = w.getframerate(), w.readframes(w.getnframes())
    _, key, _ = mm_probe.provider_config('meta')
    frame_bytes = rate * 2 * FRAME_MS // 1000
    handshake = {
        'authorization': {'accessToken': 'Bearer ' + key},
        'audioEncoding': f'PCM_{rate // 1000}KHZ',
        'model': MODEL,
        'mode': 'PUSH_TO_TALK',
        'partialMode': 'CUMULATIVE',
        'emitAudioProgress': False,
    }
    events: list[dict] = []
    first_partial_ms = None
    started = time.perf_counter()
    async with websockets.connect(URL, max_size=2 ** 20) as ws:
        await ws.send(json.dumps(handshake))
        ack = json.loads(await asyncio.wait_for(ws.recv(), 10))
        connected_ms = (time.perf_counter() - started) * 1000

        async def receive() -> None:
            nonlocal first_partial_ms
            async for message in ws:
                if isinstance(message, bytes):
                    continue
                event = json.loads(message)
                event['_t_ms'] = round((time.perf_counter() - started) * 1000)
                events.append(event)
                if event.get('type') == 'transcript' and first_partial_ms is None:
                    first_partial_ms = event['_t_ms']
                if event.get('type') == 'transcript' and event.get('final'):
                    return

        reader = asyncio.create_task(receive())
        send_start = time.perf_counter()
        for offset in range(0, len(pcm), frame_bytes):
            await ws.send(pcm[offset:offset + frame_bytes])
            # Real-time pace: the server allows at most ~5 s of audio ahead of processing.
            due = send_start + (offset + frame_bytes) / (rate * 2)
            await asyncio.sleep(max(0.0, due - time.perf_counter()))
        released = time.perf_counter()
        await ws.send(json.dumps({'type': 'endStream'}))
        await asyncio.wait_for(reader, 20)
        final_after_release_ms = (time.perf_counter() - released) * 1000
    final = next((e for e in reversed(events) if e.get('type') == 'transcript' and e.get('final')), {})
    return {
        'session': bool(ack.get('sessionId')),
        'audio_s': round(len(pcm) / (rate * 2), 2),
        'connect_ms': round(connected_ms),
        'first_partial_ms': first_partial_ms,
        'final_after_release_ms': round(final_after_release_ms),
        'transcript': final.get('transcript'),
        'event_types': sorted({e.get('type', '?') for e in events}),
    }


def main() -> int:
    if len(sys.argv) != 2:
        print(__doc__)
        return 2
    report = asyncio.run(run(Path(sys.argv[1])))
    print(json.dumps(report, ensure_ascii=False))
    return 0


if __name__ == '__main__':
    sys.exit(main())
