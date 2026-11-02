"""Muse Glimmer as Mate's chat model (D77): a text-only `/v1/responses` answer served by
our own llama.cpp server (inference/glimmer_modal.py) and relayed as Responses events, so
the client parses it like a Spark answer. Anything that is not plain chat, and a Glimmer
that does not start answering in time (a cold GPU), goes to Muse Spark instead."""

import asyncio
import json
import uuid

import httpx

GLIMMER = 'muse-glimmer-30b'
# A cold container takes a minute or more; the person should not wait that long.
FIRST_TEXT_SECONDS = 12.0
STREAM_SECONDS = 120.0
MAX_TOKENS = 1000
MAX_PROMPT_CHARS = 24_000
LINE_MAX_BYTES = 1_000_000


def chat_messages(body: dict) -> list[dict] | None:
    """The chat-completions messages for a plain chat request, or None when it is not
    one (an image, a tool, a chained turn, an unexpected shape)."""
    if body.get('tools') or body.get('previous_response_id'):
        return None
    instructions = body.get('instructions')
    messages = []
    if isinstance(instructions, str) and instructions.strip():
        messages.append({'role': 'system', 'content': instructions})
    items = body.get('input')
    if isinstance(items, str):
        items = [{'role': 'user', 'content': items}]
    if not isinstance(items, list) or not items:
        return None
    for item in items:
        if not isinstance(item, dict) or item.get('role') not in ('user', 'assistant', 'system'):
            return None
        content = item.get('content')
        if isinstance(content, str):
            text = content
        elif isinstance(content, list):
            parts = []
            for part in content:
                if not isinstance(part, dict) or part.get('type') not in ('input_text', 'output_text', 'text'):
                    return None
                if not isinstance(part.get('text'), str):
                    return None
                parts.append(part['text'])
            text = '\n'.join(parts)
        else:
            return None
        messages.append({'role': item['role'], 'content': text})
    if sum(len(m['content']) for m in messages) > MAX_PROMPT_CHARS:
        return None
    return messages


def sse(kind: str, payload: dict) -> bytes:
    payload = {'type': kind, **payload}
    return f'event: {kind}\ndata: {json.dumps(payload, ensure_ascii=False)}\n\n'.encode('utf-8')


def delta_event(text: str) -> bytes:
    return sse('response.output_text.delta', {'delta': text})


def completed_event(response_id: str, usage: dict | None) -> bytes:
    # The client's parser requires `output`; the text already came as deltas.
    response = {'id': response_id, 'object': 'response', 'model': GLIMMER, 'status': 'completed',
                'output': []}
    if usage:
        response['usage'] = usage
    return sse('response.completed', {'response': response})


def content_of(line: bytes) -> tuple[str | None, dict | None, bool]:
    """(text delta, usage, done) of one `data:` line of a chat-completions stream."""
    if not line.startswith(b'data:'):
        return None, None, False
    data = line[5:].strip()
    if data == b'[DONE]':
        return None, None, True
    try:
        chunk = json.loads(data)
    except ValueError:
        return None, None, False
    if not isinstance(chunk, dict):
        return None, None, False
    usage = chunk.get('usage') if isinstance(chunk.get('usage'), dict) else None
    choices = chunk.get('choices')
    if not isinstance(choices, list) or not choices or not isinstance(choices[0], dict):
        return None, usage, False
    delta = choices[0].get('delta')
    text = delta.get('content') if isinstance(delta, dict) else None
    return (text if isinstance(text, str) and text else None), usage, False


def usage_of(usage: dict | None) -> dict | None:
    if not usage:
        return None
    prompt, completion = usage.get('prompt_tokens'), usage.get('completion_tokens')
    if type(prompt) is not int or type(completion) is not int:
        return None
    return {'input_tokens': prompt, 'output_tokens': completion, 'total_tokens': prompt + completion}


async def lines_of(chunks, limit: int = LINE_MAX_BYTES):
    """Lines of a byte stream, each at most `limit` bytes: a longer one ends the stream
    (it is not an answer), so the buffer never grows past the limit."""
    buffer = b''
    async for chunk in chunks:
        buffer += chunk
        while b'\n' in buffer:
            line, buffer = buffer.split(b'\n', 1)
            yield line.rstrip(b'\r')
        if len(buffer) > limit:
            return
    if buffer:
        yield buffer


async def start(http: httpx.AsyncClient, url: str, secret: str, messages: list[dict],
                max_tokens: int):
    """`(events, close)` once Glimmer wrote its first text: an async iterator of SSE bytes
    and the coroutine function that closes the upstream connection (call it however the
    answer ends, also when the client left before the first byte). None when Glimmer
    failed or stayed silent past FIRST_TEXT_SECONDS (the caller asks Spark then)."""
    forward = {'model': GLIMMER, 'messages': messages, 'stream': True, 'temperature': 0.3,
               'max_tokens': max(1, min(max_tokens, MAX_TOKENS)),
               'stream_options': {'include_usage': True},
               # Harmony models think in a separate channel first: keep that short.
               'chat_template_kwargs': {'reasoning_effort': 'low'}}
    loop = asyncio.get_running_loop()
    first_deadline = loop.time() + FIRST_TEXT_SECONDS
    try:
        upstream = await asyncio.wait_for(
            http.send(http.build_request('POST', url + '/v1/chat/completions', json=forward,
                                         headers={'Authorization': 'Bearer ' + secret}), stream=True),
            FIRST_TEXT_SECONDS)
    except (asyncio.TimeoutError, httpx.HTTPError):
        return None
    if upstream.status_code != 200:
        await upstream.aclose()
        return None
    lines = lines_of(upstream.aiter_bytes()).__aiter__()
    early: list[bytes] = []
    usage = None
    done = False
    try:
        while not early and not done:
            remaining = first_deadline - loop.time()
            if remaining <= 0:
                raise asyncio.TimeoutError
            line = await asyncio.wait_for(anext(lines), remaining)
            text, seen_usage, done = content_of(line)
            usage = seen_usage or usage
            if text:
                early.append(delta_event(text))
    except (asyncio.TimeoutError, StopAsyncIteration, httpx.HTTPError):
        await upstream.aclose()
        return None
    except BaseException:
        # The person left (a cancellation) while Glimmer was still silent.
        await upstream.aclose()
        raise
    if not early:
        await upstream.aclose()
        return None

    async def relay():
        nonlocal usage, done
        deadline = loop.time() + STREAM_SECONDS
        for event in early:
            yield event
        while not done:
            remaining = deadline - loop.time()
            if remaining <= 0:
                break
            try:
                line = await asyncio.wait_for(anext(lines), remaining)
            except (asyncio.TimeoutError, StopAsyncIteration, httpx.HTTPError):
                break
            text, seen_usage, done = content_of(line)
            usage = seen_usage or usage
            if text:
                yield delta_event(text)
        # The client ends the answer on this event; a cut stream still ends cleanly.
        yield completed_event('glimmer_' + uuid.uuid4().hex, usage_of(usage))

    return relay(), upstream.aclose
