"""Muse Image (/v1/image) and Muse Glimmer (/v1/glimmer) through the proxy, faked upstream."""

import base64
import json
import unittest

try:
    import httpx
    from fastapi.testclient import TestClient
except ImportError:  # the standard-library run of `mm test`
    raise unittest.SkipTest('FastAPI is not installed in this interpreter')

from metamate_proxy.app import Settings, create_app
from tests.test_app import ENV, JUDGE_CODE, KEY

PNG = b'\x89PNG\r\n\x1a\n' + b'0' * 64
GLIMMER_ENV = dict(ENV, GLIMMER_URL='https://glimmer.example', MMS_TTS_URL='https://tts.example',
                   MMS_TTS_SECRET='t' * 32)
TESTER = 'tester-code-001'


class FakeUpstream:
    def __init__(self, image_status=200, glimmer_text='1. Open Settings.') -> None:
        self.image_status = image_status
        self.glimmer_text = glimmer_text
        self.requests: list[httpx.Request] = []

    def handler(self, request: httpx.Request) -> httpx.Response:
        self.requests.append(request)
        if request.url.path.endswith('/images/generations'):
            if self.image_status != 200:
                return httpx.Response(self.image_status, json={'error': {'message': f'blocked {KEY}'}})
            return httpx.Response(200, json={'data': [{'b64_json': base64.b64encode(PNG).decode()}]})
        if request.url.path == '/v1/chat/completions':
            return httpx.Response(200, json={'choices': [{'message': {'content': self.glimmer_text}}]})
        return httpx.Response(404)


def client_for(fake: FakeUpstream, env=None) -> TestClient:
    upstream = httpx.AsyncClient(transport=httpx.MockTransport(fake.handler))
    return TestClient(create_app(Settings.from_env(env or GLIMMER_ENV), client=upstream))


def auth(code=JUDGE_CODE):
    return {'Authorization': f'Bearer {code}'}


class ImageTests(unittest.TestCase):
    def test_one_png_with_a_fixed_request_shape(self):
        fake = FakeUpstream()
        client = client_for(fake)
        answer = client.post('/v1/image', json={'prompt': 'A card', 'model': 'other', 'n': 9}, headers=auth())
        self.assertEqual(answer.status_code, 200)
        self.assertEqual(base64.b64decode(answer.json()['png_b64']), PNG)
        sent = json.loads(fake.requests[-1].content)
        self.assertEqual(sent, {'model': 'muse-image-1.0', 'prompt': 'A card', 'n': 1, 'size': '1024x1024',
                                'response_format': 'b64_json', 'output_format': 'png'})

    def test_bad_prompts_the_daily_cap_and_refunds(self):
        fake = FakeUpstream()
        client = client_for(fake)
        for body in ({}, {'prompt': ''}, {'prompt': 'x' * 1501}, {'prompt': 3}):
            self.assertEqual(client.post('/v1/image', json=body, headers=auth()).status_code, 400)
        self.assertEqual(client.post('/v1/image', json={'prompt': 'x'}).status_code, 401)
        for _ in range(5):  # the tester's five images a day
            self.assertEqual(client.post('/v1/image', json={'prompt': 'x'}, headers=auth(TESTER)).status_code, 200)
        self.assertEqual(client.post('/v1/image', json={'prompt': 'x'}, headers=auth(TESTER)).status_code, 429)
        refused = client_for(FakeUpstream(image_status=400))
        blocked = refused.post('/v1/image', json={'prompt': 'x'}, headers=auth())
        self.assertEqual(blocked.status_code, 400)
        self.assertNotIn(KEY, blocked.text)

    def test_refusals_refund_dollars_but_keep_the_hard_attempt_allowance(self):
        fake = FakeUpstream(image_status=400)
        client = client_for(fake, {**GLIMMER_ENV, 'PROXY_DAILY_BUDGET_USD': '0.01'})
        for _ in range(5):
            self.assertEqual(client.post('/v1/image', json={'prompt': 'x'},
                                         headers=auth(TESTER)).status_code, 400)
        self.assertEqual(client.post('/v1/image', json={'prompt': 'x'},
                                     headers=auth(TESTER)).status_code, 429)
        self.assertEqual(len(fake.requests), 5)


class GlimmerTests(unittest.TestCase):
    def test_text_in_text_out_behind_the_secret(self):
        fake = FakeUpstream()
        client = client_for(fake)
        answer = client.post('/v1/glimmer', json={'messages': [{'role': 'user', 'content': 'Steps?'}],
                                                  'max_tokens': 200}, headers=auth())
        self.assertEqual(answer.json(), {'text': '1. Open Settings.'})
        sent = fake.requests[-1]
        self.assertEqual(sent.headers['authorization'], 'Bearer ' + 't' * 32)
        body = json.loads(sent.content)
        self.assertEqual(body['model'], 'muse-glimmer-30b')
        self.assertEqual(body['chat_template_kwargs'], {'reasoning_effort': 'low'})

    def test_only_short_system_and_user_text(self):
        client = client_for(FakeUpstream())
        for body in ({'messages': []}, {'messages': [{'role': 'assistant', 'content': 'x'}]},
                     {'messages': [{'role': 'user', 'content': 'x', 'name': 'n'}]},
                     {'messages': [{'role': 'user', 'content': 'x' * 4001}]},
                     {'messages': [{'role': 'user', 'content': 'x'}], 'max_tokens': 601},
                     {'messages': [{'role': 'user', 'content': [{'type': 'image_url'}]}]}):
            self.assertEqual(client.post('/v1/glimmer', json=body, headers=auth()).status_code, 400, body)
        empty = client_for(FakeUpstream(glimmer_text='  '))
        self.assertEqual(empty.post('/v1/glimmer', json={'messages': [{'role': 'user', 'content': 'x'}]},
                                    headers=auth()).status_code, 502)
        off = client_for(FakeUpstream(), ENV)
        self.assertEqual(off.post('/v1/glimmer', json={'messages': [{'role': 'user', 'content': 'x'}]},
                                  headers=auth()).status_code, 503)


if __name__ == '__main__':
    unittest.main()
