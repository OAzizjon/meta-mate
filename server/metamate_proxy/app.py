"""The MetaMate proxy: the Meta key stays on the server, the app sends an invite code.

`/v1/responses` applies the pure policy (policy.py: model allowlist, token caps, a
worst-case reservation per invite, the Wonder allowance with a Spark reserve) and
streams Meta's answer through; `/v1/asr` relays Muse Voice Transcribe's realtime
WebSocket. No request or response body is stored or logged: only counters.
"""

import asyncio
import json
import os
from dataclasses import dataclass, field
from datetime import datetime, timezone

import httpx
import websockets
from fastapi import FastAPI, Request, WebSocket
from fastapi.responses import JSONResponse, Response, StreamingResponse

from . import policy

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
RESPONSES_IN_FLIGHT_PER_INVITE = 4
BODY_READ_SECONDS = 15
STREAM_SECONDS = 180
SSE_EVENT_MAX_BYTES = 1_000_000
CALL_KINDS = {kind.value: kind for kind in policy.CallKind}


@dataclass(frozen=True)
class Settings:
    meta_key: str
    meta_base: str
    invites: dict[str, policy.InviteLimits]
    budget: policy.Budget
    prices: policy.Prices

    @staticmethod
    def from_env(env: dict[str, str] | None = None) -> 'Settings':
        """PROXY_INVITE_CODES is `code:judge,code:tester`; PROXY_PRICES is
        `spark_in,spark_out,wonder_in,wonder_out` in USD per 1M tokens (from the Billing
        page). Until Spark's price is known, Spark is priced like Wonder: it over-reserves,
        never under-reserves."""
        env = dict(os.environ if env is None else env)
        key = env.get('META_API_KEY') or env.get('META_KEY') or ''
        if not key:
            raise ValueError('META_API_KEY is not set')
        base = (env.get('META_API_BASE') or 'https://api.meta.ai/v1').rstrip('/')
        if not base.startswith('https://'):
            raise ValueError('META_API_BASE must be https')
        presets = {'judge': policy.JUDGE, 'tester': policy.TESTER}
        invites = {}
        for item in filter(None, (part.strip() for part in env.get('PROXY_INVITE_CODES', '').split(','))):
            code, _, kind = item.partition(':')
            if len(code) < 12 or kind not in presets:
                raise ValueError('PROXY_INVITE_CODES entries are code:judge or code:tester, codes of 12+ characters')
            invites[code] = presets[kind]
        raw_prices = env.get('PROXY_PRICES', '4,20,4,20').split(',')
        if len(raw_prices) != 4:
            raise ValueError('PROXY_PRICES needs four numbers')
        prices = policy.Prices(*(float(value) for value in raw_prices))
        budget = policy.Budget(float(env.get('PROXY_DAILY_BUDGET_USD', '15')))
        return Settings(key, base, invites, budget, prices)


@dataclass
class State:
    ledger: policy.Ledger = field(default_factory=policy.Ledger)
    owners: policy.ResponseOwners = field(default_factory=policy.ResponseOwners)
    lock: asyncio.Lock = field(default_factory=asyncio.Lock)
    asr_streams: dict[str, int] = field(default_factory=dict)
    in_flight: dict[str, int] = field(default_factory=dict)


def today() -> 'datetime.date':
    return datetime.now(timezone.utc).date()


def invite_of(header: str | None, settings: Settings) -> tuple[str, policy.InviteLimits] | None:
    if not header or not header.startswith('Bearer '):
        return None
    code = header[len('Bearer '):].strip()
    limits = settings.invites.get(code)
    return (code, limits) if limits else None


def error(status: int, message: str) -> JSONResponse:
    return JSONResponse({'error': {'message': message}}, status_code=status)


def create_app(settings: Settings, client: httpx.AsyncClient | None = None,
               asr_connect=None) -> FastAPI:
    """`client` and `asr_connect` are injectable for tests (no network)."""
    app = FastAPI(docs_url=None, redoc_url=None, openapi_url=None)
    state = State()
    http = client or httpx.AsyncClient(timeout=httpx.Timeout(120, connect=10))
    connect = asr_connect or websockets.connect

    async def settle(reservation: policy.Reservation, usage: dict | None, model: str | None) -> None:
        async with state.lock:
            policy.reconcile(state.ledger, reservation, usage, settings.prices, upstream_model=model)

    @app.get('/healthz')
    async def healthz() -> dict:
        return {'ok': True}

    @app.post('/v1/responses')
    async def responses(request: Request) -> Response:
        who = invite_of(request.headers.get('authorization'), settings)
        if who is None:
            return error(401, 'unknown invite code')
        invite, limits = who
        kind = CALL_KINDS.get(request.headers.get('x-metamate-call', 'answer'))
        if kind is None:
            return error(400, 'unknown call kind')
        if state.in_flight.get(invite, 0) >= RESPONSES_IN_FLIGHT_PER_INVITE:
            return error(429, 'too many requests at once')
        state.in_flight[invite] = state.in_flight.get(invite, 0) + 1
        try:
            return await handle(request, invite, limits, kind)
        except BaseException:
            release(invite)
            raise

    def release(invite: str) -> None:
        state.in_flight[invite] = max(0, state.in_flight.get(invite, 0) - 1)

    async def read_body(request: Request) -> bytes | None:
        """The body, or None when it is over the limit or too slow (never fully buffered)."""
        chunks, size = [], 0

        async def collect() -> None:
            nonlocal size
            async for chunk in request.stream():
                size += len(chunk)
                if size > policy.MAX_BODY_BYTES:
                    raise ValueError('too large')
                chunks.append(chunk)

        try:
            await asyncio.wait_for(collect(), BODY_READ_SECONDS)
        except (ValueError, asyncio.TimeoutError):
            return None
        return b''.join(chunks)

    async def handle(request: Request, invite: str, limits: policy.InviteLimits,
                     kind: policy.CallKind) -> Response:
        """Owns one in-flight slot of `invite`: every path releases it exactly once."""
        raw = await read_body(request)
        if raw is None:
            release(invite)
            return error(413, 'request too large or too slow')
        try:
            body = json.loads(raw)
            if not isinstance(body, dict):
                raise ValueError('body must be an object')
            clamped = policy.clamp_request(body, kind, str(body.get('model', policy.SPARK)))
        except (ValueError, RecursionError) as problem:
            release(invite)
            return error(400, str(problem) if isinstance(problem, ValueError) else 'request nests too deeply')
        depth = 0
        previous = clamped.get('previous_response_id')
        if previous is not None:
            depth = state.owners.depth(str(previous), invite) or 0
            if depth == 0:
                release(invite)
                return error(403, 'unknown previous response')
            if depth >= policy.MAX_CHAIN_DEPTH:
                release(invite)
                return error(429, 'conversation too long')
        bound = policy.input_token_bound(clamped, depth)
        # Reserve for the dearer of the requested model and the Spark it may fall back to.
        estimate = max(policy.estimate_cost(model, bound, clamped['max_output_tokens'], settings.prices)
                       for model in {clamped['model'], policy.SPARK})
        async with state.lock:
            try:
                reservation = policy.reserve(state.ledger, invite, limits, settings.budget,
                                             clamped['model'], estimate, today())
            except ValueError as refused:
                release(invite)
                return error(429, str(refused))
        forward = policy.body_for(reservation, body, kind)
        headers = {'Authorization': 'Bearer ' + settings.meta_key, 'Content-Type': 'application/json'}
        url = settings.meta_base + '/responses'
        answered = {'X-MetaMate-Model': reservation.model}
        if reservation.reason:
            answered['X-MetaMate-Fallback-Reason'] = reservation.reason
        settled = False

        async def settle_once(usage: dict | None, model: str | None) -> None:
            nonlocal settled
            if not settled:
                settled = True
                await settle(reservation, usage, model)
                release(invite)

        if not forward.get('stream'):
            usage, model = None, None
            try:
                upstream = await http.post(url, json=forward, headers=headers)
                data = upstream.json()
                ok = upstream.status_code == 200 and isinstance(data, dict)
                if ok:
                    usage, model = data.get('usage'), data.get('model')
                    if data.get('id'):
                        state.owners.record(str(data['id']), invite, depth + 1)
                return JSONResponse(data, status_code=upstream.status_code, headers=answered)
            except (httpx.HTTPError, ValueError):
                return error(502, 'upstream failed')
            finally:
                # Also on cancellation: a cut request keeps its whole reservation.
                await settle_once(usage, model)

        async def relay():
            usage, model, completed = None, None, False
            deadline = asyncio.get_running_loop().time() + STREAM_SECONDS
            try:
                async with http.stream('POST', url, json=forward, headers=headers) as upstream:
                    if upstream.status_code != 200:
                        yield await upstream.aread()
                        return
                    buffer = b''
                    async for chunk in upstream.aiter_bytes():
                        if asyncio.get_running_loop().time() > deadline:
                            return
                        yield chunk
                        buffer = (buffer + chunk).replace(b'\r\n', b'\n')
                        # Only the final event is parsed (usage and id); nothing is kept.
                        while b'\n\n' in buffer:
                            event, buffer = buffer.split(b'\n\n', 1)
                            final = completed_response(event)
                            if final is not None:
                                usage, model, completed = final.get('usage'), final.get('model'), True
                                if final.get('id'):
                                    state.owners.record(str(final['id']), invite, depth + 1)
                        if len(buffer) > SSE_EVENT_MAX_BYTES:
                            buffer = b''  # an event this large is not one we read
            finally:
                await settle_once(usage if completed else None, model if completed else None)

        return StreamingResponse(relay(), media_type='text/event-stream', headers=answered)

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
            if not isinstance(token, str) or not isinstance(encoding, str):
                raise ValueError('bad handshake')
        except (asyncio.TimeoutError, ValueError, RecursionError):
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
        async with state.lock:
            allowance = policy.asr_allowance(state.ledger, invite, limits, today())
        if allowance < 1 or state.asr_streams.get(invite, 0) >= ASR_MAX_STREAMS_PER_INVITE:
            await ws.close(code=4429)
            return
        upstream_handshake = {key: value for key, value in handshake.items()
                              if key in ('audioEncoding', 'mode', 'partialMode', 'emitAudioProgress',
                                         'languageBias', 'keywords')}
        upstream_handshake.update({'authorization': {'accessToken': 'Bearer ' + settings.meta_key},
                                   'model': ASR_MODEL})
        limit_bytes = int(min(ASR_MAX_SECONDS, allowance) * rate * 2)
        sent = 0
        state.asr_streams[invite] = state.asr_streams.get(invite, 0) + 1
        tasks: list[asyncio.Task] = []
        try:
            async with connect(ASR_URL, max_size=2 ** 20) as upstream:
                await upstream.send(json.dumps(upstream_handshake))
                await ws.send_text(await asyncio.wait_for(upstream.recv(), 10))

                async def up_from_client() -> None:
                    nonlocal sent
                    while True:
                        message = await asyncio.wait_for(ws.receive(), ASR_IDLE_SECONDS)
                        if message.get('type') == 'websocket.disconnect':
                            return
                        if message.get('bytes') is not None:
                            sent += len(message['bytes'])
                            if sent > limit_bytes:
                                await upstream.send(ASR_END_STREAM)
                                return
                            await upstream.send(message['bytes'])
                        elif message.get('text') is not None:
                            # The only control message is the end of the stream.
                            try:
                                control = json.loads(message['text'])
                            except ValueError:
                                control = None
                            if control == {'type': 'endStream'}:
                                await upstream.send(ASR_END_STREAM)
                            return

                async def down_to_client() -> None:
                    async for message in upstream:
                        if isinstance(message, bytes):
                            await ws.send_bytes(message)
                        else:
                            await ws.send_text(message)

                tasks = [asyncio.create_task(up_from_client()), asyncio.create_task(down_to_client())]
                done, pending = await asyncio.wait(tasks, timeout=ASR_SESSION_SECONDS,
                                                   return_when=asyncio.FIRST_COMPLETED)
                if tasks[0] in done and tasks[1] in pending:
                    # The client finished: let the final transcript come back.
                    await asyncio.wait(pending, timeout=20)
        except (OSError, websockets.WebSocketException, asyncio.TimeoutError):
            pass
        finally:
            for task in tasks:
                task.cancel()
            await asyncio.gather(*tasks, return_exceptions=True)
            state.asr_streams[invite] -= 1
            async with state.lock:
                policy.charge_asr(state.ledger, invite, sent / (rate * 2), today())
            try:
                await ws.close()
            except RuntimeError:
                pass

    return app


def completed_response(event: bytes) -> dict | None:
    """The `response` object of a `response.completed` SSE event, else None."""
    kind, data = None, []
    for line in event.decode('utf-8', errors='replace').splitlines():
        if line.startswith('event:'):
            kind = line[6:].strip()
        elif line.startswith('data:'):
            data.append(line[5:].strip())
    if not data:
        return None
    try:
        payload = json.loads('\n'.join(data))
    except ValueError:
        return None
    if not isinstance(payload, dict):
        return None
    if (kind or payload.get('type')) != 'response.completed':
        return None
    response = payload.get('response')
    return response if isinstance(response, dict) else None
