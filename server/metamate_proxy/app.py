"""The MetaMate proxy: the Meta key stays on the server, the app sends an invite code.

`/v1/responses` applies the pure policy (policy.py: model allowlist, token caps, a
worst-case reservation per invite, the Wonder allowance with a Spark reserve) and
streams Meta's answer through; `/v1/asr` relays Muse Voice Transcribe's realtime
WebSocket; `/v1/tts` speaks one sentence with MMS-TTS (tts/). No request or response
body is stored or logged: only counters.

Every ledger operation is synchronous, so on the one event loop they never interleave
and need no lock, and a settlement in a `finally` cannot be cut by a cancellation.
"""

import asyncio
import json
import os
import re
from dataclasses import dataclass, field
from datetime import datetime, timezone
from urllib.parse import urlsplit

import httpx
import websockets
from fastapi import FastAPI, Request, WebSocket, WebSocketDisconnect
from fastapi.responses import HTMLResponse, JSONResponse, RedirectResponse, Response, StreamingResponse

from . import auth, policy

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
RESPONSES_IN_FLIGHT_PER_INVITE = 4
BODY_READ_SECONDS = 15
STREAM_SECONDS = 180
SSE_EVENT_MAX_BYTES = 1_000_000
# Both carry the whole response with its usage and id (a capped answer ends incomplete).
FINAL_EVENTS = {'response.completed', 'response.incomplete'}
# The usage that settles a call Meta refused with a 4xx: nothing was inferred.
NOTHING_BILLED = {'input_tokens': 0, 'output_tokens': 0}
TTS_MAX_BODY_BYTES = 4096
TTS_IN_FLIGHT_PER_INVITE = 2
TTS_SECONDS = 30
AUTH_SECONDS = 15
CALL_KINDS = {kind.value: kind for kind in policy.CallKind}


@dataclass(frozen=True)
class Settings:
    meta_key: str = field(repr=False)
    meta_base: str
    invites: dict[str, policy.InviteLimits] = field(repr=False)
    budget: policy.Budget
    prices: policy.Prices
    # MMS-TTS (tts/serve_dev.py locally, Modal in production); empty: /v1/tts answers 503.
    tts_url: str = ''
    tts_secret: str = field(default='', repr=False)
    # Sign-in (D54): OAuth clients by provider, the key that signs session tokens, this
    # proxy's public origin (the providers' redirect URIs live under it) and the e-mails
    # that get the judge tier. Without a session secret, sign-in answers 503.
    oauth: dict[str, auth.Client] = field(default_factory=dict, repr=False)
    session_secret: str = field(default='', repr=False)
    public_url: str = 'http://127.0.0.1:8787'
    judge_emails: frozenset[str] = frozenset()

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
        tts_url = (env.get('MMS_TTS_URL') or '').rstrip('/')
        tts_secret = env.get('MMS_TTS_SECRET') or ''
        if tts_url:
            parts = urlsplit(tts_url)
            local = parts.scheme == 'http' and parts.hostname in ('127.0.0.1', 'localhost')
            if (not (parts.scheme == 'https' or local) or not parts.hostname or parts.username is not None
                    or parts.password is not None or parts.query or parts.fragment):
                raise ValueError('MMS_TTS_URL must be https (http only on 127.0.0.1)')
            if len(tts_secret) < 16:
                raise ValueError('MMS_TTS_SECRET of 16+ characters is required with MMS_TTS_URL')
        oauth = {}
        for provider in auth.PROVIDERS:
            client_id = env.get(f'{provider.upper()}_CLIENT_ID') or ''
            client_secret = env.get(f'{provider.upper()}_CLIENT_SECRET') or ''
            if client_id and client_secret:
                oauth[provider] = auth.Client(client_id, client_secret)
        session_secret = env.get('PROXY_SESSION_SECRET') or ''
        if session_secret and len(session_secret) < 32:
            raise ValueError('PROXY_SESSION_SECRET needs 32+ characters')
        public_url = (env.get('PROXY_PUBLIC_URL') or 'http://127.0.0.1:8787').rstrip('/')
        parts = urlsplit(public_url)
        if not (parts.scheme == 'https' or (parts.scheme == 'http' and parts.hostname in ('127.0.0.1', 'localhost'))):
            raise ValueError('PROXY_PUBLIC_URL must be https (http only on 127.0.0.1)')
        judges = frozenset(item.strip().lower() for item in env.get('PROXY_JUDGE_EMAILS', '').split(',')
                           if '@' in item)
        return Settings(key, base, invites, budget, prices, tts_url, tts_secret, oauth, session_secret,
                        public_url, judges)


@dataclass
class State:
    ledger: policy.Ledger = field(default_factory=policy.Ledger)
    owners: policy.ResponseOwners = field(default_factory=policy.ResponseOwners)
    asr_streams: dict[str, int] = field(default_factory=dict)
    in_flight: dict[str, int] = field(default_factory=dict)
    tts_in_flight: dict[str, int] = field(default_factory=dict)
    flows: auth.Flows = field(default_factory=auth.Flows)


def today() -> 'datetime.date':
    return datetime.now(timezone.utc).date()


def invite_of(header: str | None, settings: Settings) -> tuple[str, policy.InviteLimits] | None:
    """An invite code, or a session token from signing in (D54): its account is the ledger
    key, so each person has their own daily limits."""
    if not header or not header.startswith('Bearer '):
        return None
    code = header[len('Bearer '):].strip()
    if code.startswith(auth.TOKEN_PREFIX):
        claims = auth.read_session(code, settings.session_secret, datetime.now(timezone.utc).timestamp())
        if claims is None:
            return None
        return f'user:{claims["p"]}:{claims["s"]}', policy.JUDGE if claims['t'] == 'judge' else policy.TESTER
    limits = settings.invites.get(code)
    return (code, limits) if limits else None


def auth_page(title: str, status: int = 200) -> HTMLResponse:
    """The browser tab's last page; the app shows the real result in its own language."""
    return HTMLResponse(f'<!doctype html><meta charset="utf-8"><title>MetaMate</title>'
                        f'<p style="font:20px system-ui;margin:3em">{title}</p>', status_code=status)


def error(status: int, message: str) -> JSONResponse:
    return JSONResponse({'error': {'message': message}}, status_code=status)


def scrub_key(text: str, key: str) -> str:
    """The key, any 6+ character piece of it (a masked or cut echo), its head and tail."""
    if not key:
        return text
    text = text.replace(key, '[key]')
    text = re.sub(r'[\w.|~+/=-]{6,}',
                  lambda found: '[key]' if found.group(0) in key else found.group(0), text)
    if len(key) >= 12:
        text = text.replace(key[:8], '[key]').replace(key[-6:], '[key]')
    return text


def scrub(text: str, key: str) -> str:
    """Text from Meta goes back to an invite holder: never with the key or a piece of it,
    or a bearer token (as tools/mm_probe.py `scrub`). JSON stays valid."""
    return re.sub(r'(?i)\b(bearer\s+)[\w.~+/=-]+', r'\1[key]', scrub_key(text, key))


def upstream_error(status: int, raw: bytes, key: str) -> JSONResponse:
    """Meta's refusal reduced to its scrubbed message: nothing else of the body is relayed."""
    message = 'upstream error'
    try:
        found = json.loads(raw)['error']['message']
        if isinstance(found, str):
            message = found
    except (ValueError, RecursionError, KeyError, TypeError):
        pass
    return error(status if 400 <= status <= 599 else 502, scrub(message, key)[:500])


def scrub_event(event: bytes, key: str) -> bytes:
    """One SSE event as it goes to the client: no piece of the key passes; an error event
    (Meta may echo the request) also loses bearer tokens. Model text is left alone
    otherwise, so an answer about "Bearer" headers is not mangled."""
    text = event.decode('utf-8', errors='replace')
    if b'error' in event:
        return scrub(text, key).encode('utf-8')
    return scrub_key(text, key).encode('utf-8')


class Relay(StreamingResponse):
    """A streamed answer whose cleanup runs however the response ends, also when the
    client is gone before the first byte (the generator's own `finally` never runs then)."""

    def __init__(self, content, cleanup, **kwargs) -> None:
        super().__init__(content, **kwargs)
        self.cleanup = cleanup

    async def __call__(self, scope, receive, send) -> None:
        try:
            await super().__call__(scope, receive, send)
        finally:
            await self.cleanup()


def create_app(settings: Settings, client: httpx.AsyncClient | None = None,
               asr_connect=None) -> FastAPI:
    """`client` and `asr_connect` are injectable for tests (no network)."""
    app = FastAPI(docs_url=None, redoc_url=None, openapi_url=None)
    state = State()
    # The sign-in endpoints take a `state` query parameter: this name keeps them apart.
    state_flows = state.flows
    http = client or httpx.AsyncClient(timeout=httpx.Timeout(120, connect=10))
    connect = asr_connect or websockets.connect

    @app.get('/healthz')
    async def healthz() -> dict:
        return {'ok': True}

    def redirect_uri(provider: str) -> str:
        return f'{settings.public_url}/auth/{provider}/callback'

    @app.get('/auth/{provider}/start')
    async def auth_start(provider: str, port: int = 0, state: str = '', challenge: str = '') -> Response:
        client = settings.oauth.get(provider)
        if client is None or not settings.session_secret:
            return auth_page('Sign-in is not available.', 503)
        if not (auth.valid_port(port) and auth.valid_app_state(state) and auth.valid_challenge(challenge)):
            return auth_page('This sign-in link is not valid.', 400)
        flow_state = state_flows.start(provider, port, state, challenge, datetime.now(timezone.utc).timestamp())
        details = auth.PROVIDERS[provider]
        query = {'client_id': client.id, 'redirect_uri': redirect_uri(provider), 'response_type': 'code',
                 'scope': details.scope, 'state': flow_state}
        if provider == 'google':
            query.update(access_type='online', prompt='select_account')
        return RedirectResponse(str(httpx.URL(details.authorize, params=query)), status_code=302)

    async def identity_from(provider: str, client: auth.Client, code: str) -> auth.Identity | None:
        details = auth.PROVIDERS[provider]
        form = {'client_id': client.id, 'client_secret': client.secret, 'code': code,
                'redirect_uri': redirect_uri(provider), 'grant_type': 'authorization_code'}
        try:
            token = await http.post(details.token, data=form, headers={'Accept': 'application/json'},
                                    timeout=AUTH_SECONDS)
            access = token.json().get('access_token') if token.status_code == 200 else None
            if not isinstance(access, str) or not access:
                return None
            bearer = {'Authorization': f'Bearer {access}', 'Accept': 'application/json'}
            if provider == 'google':
                info = await http.get(auth.GOOGLE_USERINFO, headers=bearer, timeout=AUTH_SECONDS)
                return auth.google_identity(info.json()) if info.status_code == 200 else None
            user = await http.get(auth.GITHUB_USER, headers=bearer, timeout=AUTH_SECONDS)
            emails = await http.get(auth.GITHUB_EMAILS, headers=bearer, timeout=AUTH_SECONDS)
            if user.status_code != 200:
                return None
            return auth.github_identity(user.json(), emails.json() if emails.status_code == 200 else [])
        except (httpx.HTTPError, ValueError, AttributeError):
            return None

    @app.get('/auth/{provider}/callback')
    async def auth_callback(provider: str, code: str = '', state: str = '') -> Response:
        client = settings.oauth.get(provider)
        flow = state_flows.finish(provider, state, datetime.now(timezone.utc).timestamp())
        if client is None or flow is None or not code or len(code) > 2048:
            return auth_page('This sign-in has expired. Please try again from MetaMate.', 400)
        identity = await identity_from(provider, client, code)
        if identity is None:
            return auth_page('The sign-in did not work. Please try again from MetaMate.', 400)
        login = state_flows.issue(flow, identity, datetime.now(timezone.utc).timestamp())
        back = httpx.URL(f'http://127.0.0.1:{flow["port"]}/callback',
                         params={'code': login, 'state': flow['app_state']})
        return RedirectResponse(str(back), status_code=302)

    @app.post('/auth/token')
    async def auth_token(request: Request) -> Response:
        if not settings.session_secret:
            return error(503, 'sign-in is not available')
        raw = await read_body(request, 4096)
        try:
            body = json.loads(raw) if raw is not None else None
        except (ValueError, RecursionError):
            body = None
        if not isinstance(body, dict):
            return error(400, 'send {"code", "verifier"}')
        now = datetime.now(timezone.utc).timestamp()
        identity = state_flows.redeem(body.get('code'), body.get('verifier'), now)
        if identity is None:
            return error(400, 'the login code is unknown, used or expired')
        tier = 'judge' if identity.email and identity.email in settings.judge_emails else 'user'
        token = auth.sign_session(identity, tier, settings.session_secret, now)
        return JSONResponse({'token': token, 'name': identity.name[:auth.MAX_NAME],
                             'provider': identity.provider, 'tier': tier})

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
        released = False

        def release() -> None:
            nonlocal released
            if not released:
                released = True
                state.in_flight[invite] = max(0, state.in_flight.get(invite, 0) - 1)

        try:
            return await handle(request, invite, limits, kind, release)
        except BaseException:
            release()
            raise

    async def read_body(request: Request, limit: int = policy.MAX_BODY_BYTES) -> bytes | None:
        """The body, or None when it is over the limit or too slow (never fully buffered)."""
        chunks, size = [], 0

        async def collect() -> None:
            nonlocal size
            async for chunk in request.stream():
                size += len(chunk)
                if size > limit:
                    raise ValueError('too large')
                chunks.append(chunk)

        try:
            await asyncio.wait_for(collect(), BODY_READ_SECONDS)
        except (ValueError, asyncio.TimeoutError):
            return None
        return b''.join(chunks)

    async def handle(request: Request, invite: str, limits: policy.InviteLimits,
                     kind: policy.CallKind, release) -> Response:
        """Owns one in-flight slot of `invite` (`release` is idempotent)."""
        raw = await read_body(request)
        if raw is None:
            release()
            return error(413, 'request too large or too slow')
        try:
            body = json.loads(raw)
            if not isinstance(body, dict):
                raise ValueError('body must be an object')
            clamped = policy.clamp_request(body, kind, str(body.get('model', policy.SPARK)))
        except (ValueError, RecursionError) as problem:
            release()
            return error(400, str(problem) if isinstance(problem, ValueError) else 'request nests too deeply')
        depth, history = 0, 0
        previous = clamped.get('previous_response_id')
        if previous is not None:
            chain = state.owners.chain(str(previous), invite)
            if chain is None:
                release()
                return error(403, 'unknown previous response')
            depth, history = chain
            if depth >= policy.MAX_CHAIN_DEPTH:
                release()
                return error(429, 'conversation too long')
        bound = policy.input_token_bound(clamped, depth, history)
        output_cap = clamped.get('max_output_tokens', 0)
        if kind == policy.CallKind.SAM:
            estimate = policy.SAM_CALL_USD
        else:
            # Reserve for the dearer of the requested model and the Spark it may fall back to.
            estimate = max(policy.estimate_cost(model, bound, output_cap, settings.prices)
                           for model in {clamped['model'], policy.SPARK})
        try:
            reservation = policy.reserve(state.ledger, invite, limits, settings.budget,
                                         clamped['model'], estimate, today())
        except ValueError as refused:
            release()
            return error(429, str(refused))
        forward = policy.body_for(reservation, body, kind)
        headers = {'Authorization': 'Bearer ' + settings.meta_key, 'Content-Type': 'application/json'}
        url = settings.meta_base + '/responses'
        answered = {'X-MetaMate-Model': reservation.model}
        if reservation.reason:
            answered['X-MetaMate-Fallback-Reason'] = reservation.reason
        settled = False

        def settle_once(usage: dict | None, model: str | None) -> None:
            """Synchronous: also runs in full under a cancellation."""
            nonlocal settled
            if not settled:
                settled = True
                policy.reconcile(state.ledger, reservation, usage, settings.prices, upstream_model=model)
                release()

        def finished(response: dict) -> None:
            if response.get('id'):
                state.owners.record(str(response['id']), invite, depth + 1, response.get('usage'),
                                    bound + output_cap)

        if not forward.get('stream'):
            usage, model = None, None
            try:
                upstream = await http.post(url, json=forward, headers=headers)
                if upstream.status_code != 200:
                    if upstream.status_code < 500:
                        usage = NOTHING_BILLED  # refused before any inference
                    return upstream_error(upstream.status_code, upstream.content, settings.meta_key)
                data = upstream.json()
                if not isinstance(data, dict):
                    return error(502, 'upstream failed')
                usage, model = data.get('usage'), data.get('model')
                finished(data)
                return JSONResponse(data, headers=answered)
            except (httpx.HTTPError, ValueError, RecursionError):
                return error(502, 'upstream failed')
            finally:
                # Also on cancellation: a cut request keeps its whole reservation.
                settle_once(usage, model)

        try:
            upstream = await http.send(http.build_request('POST', url, json=forward, headers=headers),
                                       stream=True)
        except httpx.HTTPError:
            settle_once(None, None)
            return error(502, 'upstream failed')
        except BaseException:
            settle_once(None, None)
            raise
        if upstream.status_code != 200:
            # Settled before any await, so a cancellation below cannot skip it.
            settle_once(NOTHING_BILLED if upstream.status_code < 500 else None, None)
            try:
                raw = await upstream.aread()
            except httpx.HTTPError:
                raw = b''
            finally:
                await upstream.aclose()
            return upstream_error(upstream.status_code, raw, settings.meta_key)
        seen = {'usage': None, 'model': None, 'final': False}

        async def relay():
            """Whole events only, each scrubbed: a key split across chunks never slips by."""
            deadline = asyncio.get_running_loop().time() + STREAM_SECONDS
            buffer = b''
            async for chunk in upstream.aiter_bytes():
                if asyncio.get_running_loop().time() > deadline:
                    return
                buffer = (buffer + chunk).replace(b'\r\n', b'\n')
                while b'\n\n' in buffer:
                    event, buffer = buffer.split(b'\n\n', 1)
                    # Only the final event is parsed (usage and id); nothing is kept.
                    final = final_response(event)
                    if final is not None:
                        seen.update(usage=final.get('usage'), model=final.get('model'), final=True)
                        finished(final)
                    yield scrub_event(event, settings.meta_key) + b'\n\n'
                if len(buffer) > SSE_EVENT_MAX_BYTES:
                    return  # an event this large is not an answer: the stream ends here
            if buffer.strip():
                yield scrub_event(buffer, settings.meta_key) + b'\n\n'

        async def cleanup() -> None:
            # A stream cut before its final event keeps the whole reservation.
            settle_once(seen['usage'] if seen['final'] else None, seen['model'] if seen['final'] else None)
            await upstream.aclose()

        return Relay(relay(), cleanup, media_type='text/event-stream', headers=answered)

    @app.post('/v1/tts')
    async def tts(request: Request) -> Response:
        """One sentence in Mate's voice: MMS-TTS behind the invite and a daily character
        allowance, charged before the call. The text is neither stored nor logged."""
        who = invite_of(request.headers.get('authorization'), settings)
        if who is None:
            return error(401, 'unknown invite code')
        if not settings.tts_url:
            return error(503, 'speech synthesis is not configured')
        invite, limits = who
        raw = await read_body(request, TTS_MAX_BODY_BYTES)
        if raw is None:
            return error(413, 'request too large or too slow')
        try:
            body = json.loads(raw)
        except (ValueError, RecursionError):
            body = None
        lang = body.get('lang') if isinstance(body, dict) else None
        text = body.get('text') if isinstance(body, dict) else None
        speed = body.get('speed', 1.0) if isinstance(body, dict) else 1.0
        if type(speed) not in (int, float) or not 0.8 <= speed <= 1.3:
            return error(422, 'speed must be a number between 0.8 and 1.3')
        if (not isinstance(lang, str) or lang not in policy.TTS_LANGUAGES or not isinstance(text, str)
                or not text.strip() or len(text) > policy.TTS_MAX_CHARS):
            return error(400, f'lang is en or ru; text is 1 to {policy.TTS_MAX_CHARS} characters')
        if state.tts_in_flight.get(invite, 0) >= TTS_IN_FLIGHT_PER_INVITE:
            return error(429, 'too many requests at once')
        if policy.tts_allowance(state.ledger, invite, limits, today()) < len(text):
            return error(429, 'speech allowance used up for today')
        policy.charge_tts(state.ledger, invite, len(text), today())
        state.tts_in_flight[invite] = state.tts_in_flight.get(invite, 0) + 1
        try:
            upstream = await http.post(settings.tts_url + '/synthesize',
                                       json={'lang': lang, 'text': text, 'speed': speed},
                                       headers={'X-MMS-Secret': settings.tts_secret}, timeout=TTS_SECONDS)
        except httpx.HTTPError:
            return error(502, 'speech synthesis failed')
        finally:
            state.tts_in_flight[invite] -= 1
        if upstream.status_code != 200 or upstream.headers.get('content-type') != 'audio/wav':
            return error(502, 'speech synthesis failed')
        timing = {'X-Synth-Ms': upstream.headers['x-synth-ms']} if 'x-synth-ms' in upstream.headers else {}
        return Response(upstream.content, media_type='audio/wav', headers=timing)

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

    return app


def app_from_env():
    """The production entry point (`uvicorn metamate_proxy.app:app_from_env --factory`,
    server/deploy/): settings from the environment, which systemd fills from a root-owned
    file. A bad or missing setting stops the start, before any request."""
    return create_app(Settings.from_env())


def final_response(event: bytes) -> dict | None:
    """The `response` object of a final SSE event (FINAL_EVENTS), else None."""
    kind, data = None, []
    # split, not splitlines: a JSON line may hold U+2028 or U+0085, which are not line ends.
    for line in event.decode('utf-8', errors='replace').split('\n'):
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
    if (kind or payload.get('type')) not in FINAL_EVENTS:
        return None
    response = payload.get('response')
    return response if isinstance(response, dict) else None
