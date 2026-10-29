"""`/v1/responses`: the pure policy (policy.py) before the call, Meta's answer passed through
(a stream as whole events, each scrubbed) and the reservation settled however the call ends."""

import asyncio
import json
from datetime import datetime, timezone
from typing import TYPE_CHECKING

import httpx
from fastapi import FastAPI, Request
from fastapi.responses import JSONResponse, Response, StreamingResponse

from . import auth, policy
from .common import State, error, invite_of, read_body, scrub, scrub_key, today, upstream_error

if TYPE_CHECKING:
    from .app import Settings

RESPONSES_IN_FLIGHT_PER_INVITE = 4
STREAM_SECONDS = 180
SSE_EVENT_MAX_BYTES = 1_000_000
# Both carry the whole response with its usage and id (a capped answer ends incomplete).
FINAL_EVENTS = {'response.completed', 'response.incomplete'}
# The usage that settles a call Meta refused with a 4xx: nothing was inferred.
NOTHING_BILLED = {'input_tokens': 0, 'output_tokens': 0}
CALL_KINDS = {kind.value: kind for kind in policy.CallKind}


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


def guest_headers(left: int, reset: int) -> dict[str, str]:
    return {'X-MetaMate-Guest-Left': str(left), 'X-MetaMate-Guest-Limit': str(auth.GUEST_ANSWERS_PER_HOUR),
            'X-MetaMate-Guest-Reset': str(reset)}


def register(app: FastAPI, settings: 'Settings', state: State, http: httpx.AsyncClient) -> None:
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
        # The guest trial counts answers (one per question) per device and hour.
        guest = None
        if invite.startswith('user:guest:'):
            hits = state.guest_hits.setdefault(invite, [])
            now = datetime.now(timezone.utc).timestamp()
            left, reset = auth.guest_left(hits, now)
            if kind is policy.CallKind.ANSWER:
                if left == 0:
                    refused = error(429, 'guest_limit')
                    refused.headers.update(guest_headers(0, reset))
                    return refused
                hits.append(now)
                left, reset = auth.guest_left(hits, now)
            else:
                # Lesson steps, intents, speech: not messages, but capped for guests too.
                steps = state.guest_steps.setdefault(invite, [])
                steps_left, _ = auth.guest_left(steps, now, auth.GUEST_STEPS_PER_HOUR)
                if steps_left == 0:
                    refused = error(429, 'guest_limit')
                    refused.headers.update(guest_headers(left, reset))
                    return refused
                steps.append(now)
            guest = guest_headers(left, reset)
        state.in_flight[invite] = state.in_flight.get(invite, 0) + 1
        released = False

        def release() -> None:
            nonlocal released
            if not released:
                released = True
                state.in_flight[invite] = max(0, state.in_flight.get(invite, 0) - 1)

        try:
            response = await handle(request, invite, limits, kind, release)
            if guest is not None:
                response.headers.update(guest)
            return response
        except BaseException:
            release()
            raise

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
