"""What the proxy's routes share: the per-app state, the invite check, error answers that
never carry the key, and a body reader that never buffers past its limit."""

import asyncio
import json
import re
from dataclasses import dataclass, field
from datetime import datetime, timezone
from typing import TYPE_CHECKING

from fastapi import Request
from fastapi.responses import JSONResponse

from . import auth, policy

if TYPE_CHECKING:
    from .app import Settings

BODY_READ_SECONDS = 15


@dataclass
class State:
    ledger: policy.Ledger = field(default_factory=policy.Ledger)
    owners: policy.ResponseOwners = field(default_factory=policy.ResponseOwners)
    asr_streams: dict[str, int] = field(default_factory=dict)
    in_flight: dict[str, int] = field(default_factory=dict)
    tts_in_flight: dict[str, int] = field(default_factory=dict)
    flows: auth.Flows = field(default_factory=auth.Flows)
    # Guest trial: answer times per guest device, and guest tokens issued per client address.
    guest_hits: dict[str, list[float]] = field(default_factory=dict)
    guest_issued: dict[str, list[float]] = field(default_factory=dict)
    guest_steps: dict[str, list[float]] = field(default_factory=dict)
    # /v1/warm: when each GPU service was last woken, and the pings still running.
    warmed: dict[str, float] = field(default_factory=dict)
    warming: set = field(default_factory=set)
    warm_count: dict = field(default_factory=dict)


def today() -> 'datetime.date':
    return datetime.now(timezone.utc).date()


def invite_of(header: str | None, settings: 'Settings') -> tuple[str, policy.InviteLimits] | None:
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
    if status in (401, 403):
        return error(502, 'upstream_unavailable')
    if status == 429:
        return error(503, 'upstream_busy')
    message = 'upstream error'
    try:
        found = json.loads(raw)['error']['message']
        if isinstance(found, str):
            message = found
    except (ValueError, RecursionError, KeyError, TypeError):
        pass
    return error(status if 400 <= status <= 599 else 502, scrub(message, key)[:500])


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


async def json_body(request: Request, limit: int) -> dict | None:
    raw = await read_body(request, limit)
    try:
        body = json.loads(raw) if raw is not None else None
    except (ValueError, RecursionError):
        return None
    return body if isinstance(body, dict) else None
