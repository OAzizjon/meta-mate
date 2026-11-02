"""Muse Glimmer as the chat model (D77): plain chat streams from our llama.cpp server as
Responses events; images, tools, other call kinds and a silent Glimmer go to Spark."""

import asyncio
import base64
import json
import unittest
from unittest.mock import patch

try:
    import httpx
    from fastapi.testclient import TestClient
except ImportError:  # the standard-library run of `mm test`
    raise unittest.SkipTest('FastAPI is not installed in this interpreter')

from metamate_proxy import glimmer_chat
from metamate_proxy.app import Settings, create_app
from tests.test_app import ENV, JUDGE_CODE

GLIMMER_ENV = dict(ENV, GLIMMER_URL='https://glimmer.example', MMS_TTS_SECRET='t' * 32)
SPARK_STREAM = (b'event: response.output_text.delta\ndata: {"type":"response.output_text.delta","delta":"Spark"}\n\n'
                b'event: response.completed\ndata: {"type":"response.completed","response":'
                b'{"id":"resp_1","model":"muse-spark-1.3","usage":{"input_tokens":5,"output_tokens":2}}}\n\n')
PNG = b'\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR' + (8).to_bytes(4, 'big') * 2 + b'0' * 16
PNG_URL = 'data:image/png;base64,' + base64.b64encode(PNG).decode()


def chat_chunk(text: str) -> str:
    return 'data: ' + json.dumps({'choices': [{'delta': {'content': text}}]}) + '\n\n'


class FakeUpstreams:
    def __init__(self, glimmer_status=200, glimmer_lines=None) -> None:
        self.glimmer_status = glimmer_status
        self.glimmer_lines = glimmer_lines if glimmer_lines is not None else [
            chat_chunk('<mood happy>'), chat_chunk('Hello!'),
            'data: ' + json.dumps({'choices': [], 'usage': {'prompt_tokens': 9, 'completion_tokens': 3}}) + '\n\n',
            'data: [DONE]\n\n']
        self.requests: list[httpx.Request] = []

    def handler(self, request: httpx.Request) -> httpx.Response:
        self.requests.append(request)
        if request.url.host == 'glimmer.example':
            if self.glimmer_status != 200:
                return httpx.Response(self.glimmer_status, json={'error': 'cold'})
            return httpx.Response(200, content=''.join(self.glimmer_lines).encode(),
                                  headers={'content-type': 'text/event-stream'})
        if request.url.path.endswith('/responses'):
            return httpx.Response(200, content=SPARK_STREAM, headers={'content-type': 'text/event-stream'})
        return httpx.Response(404)


def client_for(fake: FakeUpstreams, env=None) -> TestClient:
    upstream = httpx.AsyncClient(transport=httpx.MockTransport(fake.handler))
    return TestClient(create_app(Settings.from_env(env or GLIMMER_ENV), client=upstream))


def chat_body(**extra) -> dict:
    body = {'model': 'muse-glimmer-30b', 'stream': True, 'instructions': 'You are Mate.',
            'input': [{'type': 'message', 'role': 'user', 'content': [{'type': 'input_text', 'text': 'Hi'}]}],
            'max_output_tokens': 1000, 'reasoning': {'effort': 'minimal'}}
    body.update(extra)
    return body


def image_body() -> dict:
    return chat_body(input=[{'role': 'user', 'content': [
        {'type': 'input_text', 'text': 'Where?'}, {'type': 'input_image', 'image_url': PNG_URL}]}])


def post(client, body, call='answer'):
    return client.post('/v1/responses', json=body,
                       headers={'Authorization': f'Bearer {JUDGE_CODE}', 'X-MetaMate-Call': call})


class MessagesTests(unittest.TestCase):
    def test_plain_chat_becomes_chat_messages(self):
        self.assertEqual(glimmer_chat.chat_messages(chat_body()),
                         [{'role': 'system', 'content': 'You are Mate.'}, {'role': 'user', 'content': 'Hi'}])

    def test_images_tools_chains_and_odd_shapes_are_not_glimmer_chat(self):
        for body in (image_body(), chat_body(tools=[{'type': 'computer'}]), chat_body(previous_response_id='resp_1'),
                     chat_body(input=[]), chat_body(input=[{'role': 'tool', 'content': 'x'}]),
                     chat_body(input=[{'role': 'user', 'content': 'x' * 30_000}])):
            self.assertIsNone(glimmer_chat.chat_messages(body))

    def test_stream_lines(self):
        self.assertEqual(glimmer_chat.content_of(b'data: [DONE]'), (None, None, True))
        self.assertEqual(glimmer_chat.content_of(chat_chunk('A').strip().encode()), ('A', None, False))
        self.assertEqual(glimmer_chat.content_of(b'data: {not json'), (None, None, False))
        self.assertEqual(glimmer_chat.content_of(b': keep-alive'), (None, None, False))
        self.assertEqual(glimmer_chat.usage_of({'prompt_tokens': 2, 'completion_tokens': 3}),
                         {'input_tokens': 2, 'output_tokens': 3, 'total_tokens': 5})
        self.assertIsNone(glimmer_chat.usage_of({'prompt_tokens': '2'}))

    def test_lines_are_split_and_bounded(self):
        async def collect(chunks, limit):
            async def source():
                for chunk in chunks:
                    yield chunk
            return [line async for line in glimmer_chat.lines_of(source(), limit)]
        self.assertEqual(asyncio.run(collect([b'data: a\r\nda', b'ta: b\n', b'tail'], 100)),
                         [b'data: a', b'data: b', b'tail'])
        self.assertEqual(asyncio.run(collect([b'x' * 50, b'y' * 60], 100)), [])

    def test_completed_event_parses_like_a_response(self):
        event = json.loads(glimmer_chat.completed_event('glimmer_1', None).decode().split('data: ', 1)[1])
        self.assertEqual(event['response']['output'], [])
        self.assertEqual(event['response']['status'], 'completed')


class RouteTests(unittest.TestCase):
    def test_chat_streams_from_glimmer_as_responses_events(self):
        fake = FakeUpstreams()
        answer = post(client_for(fake), chat_body())
        self.assertEqual(answer.status_code, 200)
        self.assertEqual(answer.headers['x-metamate-model'], 'muse-glimmer-30b')
        self.assertIn('"delta": "<mood happy>"', answer.text)
        self.assertIn('"delta": "Hello!"', answer.text)
        self.assertIn('event: response.completed', answer.text)
        self.assertIn('"output_tokens": 3', answer.text)
        sent = json.loads(fake.requests[0].content)
        self.assertEqual(fake.requests[0].url.path, '/v1/chat/completions')
        self.assertEqual(fake.requests[0].headers['authorization'], 'Bearer ' + 't' * 32)
        self.assertEqual(sent['messages'][-1], {'role': 'user', 'content': 'Hi'})
        self.assertTrue(sent['stream'])
        self.assertEqual(len(fake.requests), 1)  # Meta was never called

    def test_failing_or_silent_glimmer_falls_back_to_spark(self):
        for fake in (FakeUpstreams(glimmer_status=503), FakeUpstreams(glimmer_lines=['data: [DONE]\n\n'])):
            answer = post(client_for(fake), chat_body())
            self.assertEqual(answer.status_code, 200)
            self.assertEqual(answer.headers['x-metamate-model'], 'muse-spark-1.3')
            self.assertEqual(answer.headers['x-metamate-fallback-reason'], 'glimmer_unavailable')
            self.assertIn('Spark', answer.text)
            self.assertEqual(json.loads(fake.requests[-1].content)['model'], 'muse-spark-1.3')

    def test_a_glimmer_miss_gives_the_daily_count_back(self):
        client = client_for(FakeUpstreams(glimmer_status=503))
        with patch('metamate_proxy.policy.refund_glimmer', wraps=__import__(
                'metamate_proxy.policy', fromlist=['refund_glimmer']).refund_glimmer) as refund:
            post(client, chat_body())
        self.assertEqual(refund.call_count, 1)

    def test_cold_glimmer_times_out_to_spark(self):
        fake = FakeUpstreams(glimmer_lines=[': warming up\n\n'] * 3)
        with patch.object(glimmer_chat, 'FIRST_TEXT_SECONDS', 0.05):
            answer = post(client_for(fake), chat_body())
        self.assertEqual(answer.headers['x-metamate-model'], 'muse-spark-1.3')

    def test_screens_other_calls_and_unconfigured_glimmer_go_to_spark(self):
        cases = [(GLIMMER_ENV, image_body(), 'answer'), (GLIMMER_ENV, chat_body(), 'computer'),
                 (dict(ENV), chat_body(), 'answer')]
        for env, body, call in cases:
            fake = FakeUpstreams()
            answer = post(client_for(fake, env), body, call)
            self.assertEqual(answer.status_code, 200, (body, call))
            self.assertTrue(all(r.url.host != 'glimmer.example' for r in fake.requests))
            self.assertEqual(json.loads(fake.requests[-1].content)['model'], 'muse-spark-1.3')


class WarmTests(unittest.TestCase):
    def test_warm_wakes_tts_and_glimmer_once_a_minute(self):
        fake = FakeUpstreams()
        client = client_for(fake, dict(GLIMMER_ENV, MMS_TTS_URL='https://tts.example'))
        headers = {'Authorization': f'Bearer {JUDGE_CODE}'}
        self.assertEqual(client.post('/v1/warm', json={}).status_code, 401)
        self.assertEqual(client.post('/v1/warm', json={'glimmer': True}, headers=headers).status_code, 204)
        self.assertEqual(client.post('/v1/warm', json={'glimmer': True}, headers=headers).status_code, 204)
        client.get('/healthz')  # lets the background pings run
        paths = sorted((r.url.host, r.url.path) for r in fake.requests)
        self.assertEqual(paths, [('glimmer.example', '/health'), ('tts.example', '/healthz')])


if __name__ == '__main__':
    unittest.main()
