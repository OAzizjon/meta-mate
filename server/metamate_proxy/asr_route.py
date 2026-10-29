"""`/v1/asr`: a relay of Muse Voice Transcribe's realtime WebSocket, behind the invite and
a daily allowance of seconds reserved before connecting; only checked values reach Meta."""

import asyncio
import json
import re
from typing import TYPE_CHECKING

import websockets
from fastapi import FastAPI, WebSocket, WebSocketDisconnect

from . import policy
from .common import State, invite_of, scrub, today

if TYPE_CHECKING:
    from .app import Settings

ASR_URL = 'wss://api.meta.ai/v1/asr/realtime'
ASR_MODEL = 'muse-voice-transcribe-1.0'
# One push-to-talk question; the ASR price is not known yet, so the length is the cap.
ASR_MAX_SECONDS = 60
ASR_MAX_STREAMS_PER_INVITE = 2
# A session ends at the latest this long after the handshake, or after this much silence
# from the client (the app sends audio every 80 ms while the key is held).
ASR_SESSION_SECONDS = 90
ASR_IDLE_SECONDS = 15
ASR_END_STREAM = '{"type":"endStream"}'
ASR_MODES = {'PUSH_TO_TALK', 'ENDPOINTING'}


def register(app: FastAPI, settings: 'Settings', state: State, connect) -> None:
    @app.websocket('/v1/asr')
    async def asr(ws: WebSocket) -> None:
        await ws.accept()
        try:
            handshake = json.loads(await asyncio.wait_for(ws.receive_text(), 10))
            if not isinstance(handshake, dict):
                raise ValueError('handshake must be an object')
            auth = handshake.get('authorization')
            token = auth.get('accessToken') if isinstance(auth, dict) else None
            encoding = handshake.get('audioEncoding')
            mode = handshake.get('mode', 'PUSH_TO_TALK')
            partial = handshake.get('partialMode', 'CUMULATIVE')
            progress = handshake.get('emitAudioProgress', False)
            if (not isinstance(token, str) or not isinstance(encoding, str) or mode not in ASR_MODES
                    or not isinstance(partial, str) or not re.fullmatch(r'[A-Z_]{1,32}', partial)
                    or not isinstance(progress, bool)):
                raise ValueError('bad handshake')
        except WebSocketDisconnect:
            return
        except (asyncio.TimeoutError, ValueError, TypeError, KeyError, RecursionError):
            # KeyError: a binary first frame has no text.
            await ws.close(code=4400)
            return
        who = invite_of(token, settings)
        if who is None:
            await ws.close(code=4401)
            return
        invite, limits = who
        rate = {'PCM_24KHZ': 24000, 'PCM_16KHZ': 16000}.get(encoding)
        if rate is None:
            await ws.close(code=4400)
            return
        if state.asr_streams.get(invite, 0) >= ASR_MAX_STREAMS_PER_INVITE:
            await ws.close(code=4429)
            return
        day = today()
        # Reserved before connecting: two sessions never share the same remainder.
        reserved = policy.reserve_asr(state.ledger, invite, limits, ASR_MAX_SECONDS, day)
        if reserved < 1:
            policy.settle_asr(state.ledger, invite, reserved, 0, day)
            await ws.close(code=4429)
            return
        # Only checked values go on: nothing the client sends reaches Meta unread.
        upstream_handshake = {'authorization': {'accessToken': 'Bearer ' + settings.meta_key},
                              'audioEncoding': encoding, 'model': ASR_MODEL, 'mode': mode,
                              'partialMode': partial, 'emitAudioProgress': progress}
        limit_bytes = int(reserved * rate * 2)
        sent = 0
        state.asr_streams[invite] = state.asr_streams.get(invite, 0) + 1
        tasks: list[asyncio.Task] = []
        try:
            async with connect(ASR_URL, max_size=2 ** 20) as upstream:
                await upstream.send(json.dumps(upstream_handshake))
                reply = await asyncio.wait_for(upstream.recv(), 10)
                await ws.send_text(scrub(reply if isinstance(reply, str) else '{}', settings.meta_key))

                async def up_from_client() -> bool:
                    """True when the stream was ended (the final transcript is due), False
                    when the client left or broke the protocol."""
                    nonlocal sent
                    while True:
                        message = await asyncio.wait_for(ws.receive(), ASR_IDLE_SECONDS)
                        if message.get('type') == 'websocket.disconnect':
                            return False
                        if message.get('bytes') is not None:
                            if sent + len(message['bytes']) > limit_bytes:
                                await upstream.send(ASR_END_STREAM)
                                return True
                            sent += len(message['bytes'])
                            await upstream.send(message['bytes'])
                        elif message.get('text') is not None:
                            # The only control message is the end of the stream.
                            try:
                                control = json.loads(message['text'])
                            except (ValueError, RecursionError):
                                control = None
                            if control != {'type': 'endStream'}:
                                return False
                            await upstream.send(ASR_END_STREAM)
                            return True

                async def down_to_client() -> None:
                    """Relays events until the final transcript, which ends the session."""
                    async for message in upstream:
                        if not isinstance(message, str):
                            continue
                        await ws.send_text(scrub(message, settings.meta_key))
                        try:
                            event = json.loads(message)
                        except (ValueError, RecursionError):
                            continue
                        if isinstance(event, dict) and event.get('type') == 'transcript' \
                                and event.get('final') is True:
                            return

                tasks = [asyncio.create_task(up_from_client()), asyncio.create_task(down_to_client())]
                done, pending = await asyncio.wait(tasks, timeout=ASR_SESSION_SECONDS,
                                                   return_when=asyncio.FIRST_COMPLETED)
                client = tasks[0]
                ended = client in done and not client.cancelled() and client.exception() is None \
                    and client.result()
                if ended and tasks[1] in pending:
                    # The client finished: let the final transcript come back.
                    await asyncio.wait(pending, timeout=20)
        except (OSError, RuntimeError, websockets.WebSocketException, WebSocketDisconnect,
                asyncio.TimeoutError):
            pass
        finally:
            # Synchronous first: a cancellation at the awaits below cannot skip it.
            state.asr_streams[invite] -= 1
            policy.settle_asr(state.ledger, invite, reserved, min(reserved, sent / (rate * 2)), day)
            for task in tasks:
                task.cancel()
            await asyncio.gather(*tasks, return_exceptions=True)
            try:
                await ws.close()
            except (RuntimeError, OSError, WebSocketDisconnect):
                pass
