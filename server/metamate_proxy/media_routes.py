"""One call each behind the invite and a daily allowance, taken before the call: `/v1/tts`
(MMS-TTS, tts/), `/v1/image` (Muse Image) and `/v1/glimmer` (Muse Glimmer on Modal)."""

import base64
import binascii
import json
from typing import TYPE_CHECKING

import httpx
from fastapi import FastAPI, Request
from fastapi.responses import JSONResponse, Response

from . import policy
from .common import State, error, invite_of, json_body, read_body, today, upstream_error

if TYPE_CHECKING:
    from .app import Settings

TTS_MAX_BODY_BYTES = 4096
TTS_IN_FLIGHT_PER_INVITE = 2
TTS_SECONDS = 30


def register(app: FastAPI, settings: 'Settings', state: State, http: httpx.AsyncClient) -> None:
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

    @app.post('/v1/image')
    async def image(request: Request) -> Response:
        """One picture from Muse Image (the lesson card, D55): a fixed request shape, one
        PNG, $0.01 reserved under a daily image allowance; a refused image is refunded."""
        who = invite_of(request.headers.get('authorization'), settings)
        if who is None:
            return error(401, 'unknown invite code')
        if who[0].startswith('user:guest:'):
            return error(403, 'sign in for the lesson card')
        invite, limits = who
        body = await json_body(request, 8192)
        prompt = body.get('prompt') if body else None
        if not isinstance(prompt, str) or not prompt.strip() or len(prompt) > policy.IMAGE_PROMPT_CHARS:
            return error(400, f'send {{"prompt"}} of 1 to {policy.IMAGE_PROMPT_CHARS} characters')
        try:
            reservation = policy.reserve(state.ledger, invite, limits, settings.budget, policy.IMAGE,
                                         policy.IMAGE_USD, today())
        except ValueError as refused:
            return error(429, str(refused))
        forward = {'model': policy.IMAGE, 'prompt': prompt, 'n': 1, 'size': '1024x1024',
                   'response_format': 'b64_json', 'output_format': 'png'}
        headers = {'Authorization': 'Bearer ' + settings.meta_key, 'Content-Type': 'application/json'}
        try:
            upstream = await http.post(settings.meta_base + '/images/generations', json=forward,
                                       headers=headers, timeout=90)
        except httpx.HTTPError:
            policy.reconcile(state.ledger, reservation, None, settings.prices)
            return error(502, 'image generation failed')
        policy.reconcile(state.ledger, reservation, None, settings.prices)
        if upstream.status_code != 200:
            if upstream.status_code < 500:
                policy.refund_image(state.ledger, reservation)
            return upstream_error(upstream.status_code, upstream.content, settings.meta_key)
        try:
            encoded = upstream.json()['data'][0]['b64_json']
            png = base64.b64decode(encoded, validate=True)
        except (ValueError, KeyError, IndexError, TypeError, binascii.Error):
            return error(502, 'image generation failed')
        if png[:8] != b'\x89PNG\r\n\x1a\n' or len(png) > 12_000_000:
            return error(502, 'image generation failed')
        return JSONResponse({'png_b64': base64.b64encode(png).decode('ascii')})

    @app.post('/v1/glimmer')
    async def glimmer(request: Request) -> Response:
        """Muse Glimmer (Meta's open weights) on our own GPU (D53): a short chat for work
        that can wait a few seconds, such as the lesson card's wording. Only system and user
        text messages pass; the reply is plain text."""
        who = invite_of(request.headers.get('authorization'), settings)
        if who is None:
            return error(401, 'unknown invite code')
        if who[0].startswith('user:guest:'):
            return error(403, 'sign in for the lesson card')
        if not settings.glimmer_url:
            return error(503, 'Muse Glimmer is not configured')
        invite, limits = who
        body = await json_body(request, 16384)
        messages = body.get('messages') if body else None
        tokens = body.get('max_tokens', 300) if body else 300
        if (not isinstance(messages, list) or not 1 <= len(messages) <= 4
                or any(not isinstance(m, dict) or set(m) != {'role', 'content'} or m['role'] not in ('system', 'user')
                       or not isinstance(m['content'], str) or not m['content'] or len(m['content']) > 4000
                       for m in messages)
                or type(tokens) is not int or not 1 <= tokens <= policy.GLIMMER_MAX_TOKENS):
            return error(400, 'send {"messages": [system/user text], "max_tokens": 1..600}')
        if policy.glimmer_allowance(state.ledger, invite, limits, today()) < 1:
            return error(429, 'glimmer allowance used up for today')
        policy.charge_glimmer(state.ledger, invite, today())
        forward = {'model': 'muse-glimmer-30b', 'messages': messages, 'max_tokens': tokens, 'temperature': 0.3,
                   # Harmony models think in a separate channel first: keep that short.
                   'chat_template_kwargs': {'reasoning_effort': 'low'}}
        try:
            upstream = await http.post(settings.glimmer_url + '/v1/chat/completions', json=forward,
                                       headers={'Authorization': 'Bearer ' + settings.tts_secret}, timeout=120)
            text = upstream.json()['choices'][0]['message']['content'] if upstream.status_code == 200 else None
        except (httpx.HTTPError, ValueError, KeyError, IndexError, TypeError):
            text = None
        if not isinstance(text, str) or not text.strip():
            return error(502, 'Muse Glimmer did not answer')
        return JSONResponse({'text': text.strip()})
