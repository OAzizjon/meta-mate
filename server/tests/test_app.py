"""The proxy's HTTP behaviour against a fake Meta (no network). Needs FastAPI: run with
the proxy's environment (`C:/mmw/py-proxy/Scripts/python.exe -m unittest discover -s
server/tests -t server`); without it these tests are skipped."""

import json
import unittest

try:
    import httpx
    from fastapi.testclient import TestClient
except ImportError:  # the standard-library run of `mm test`
    raise unittest.SkipTest('FastAPI is not installed in this interpreter')

from metamate_proxy.app import Settings, completed_response, create_app

JUDGE_CODE = 'judge-code-0001'
ENV = {
    'META_API_KEY': 'server-side-key',
    'PROXY_INVITE_CODES': f'{JUDGE_CODE}:judge,tester-code-001:tester',
    'PROXY_DAILY_BUDGET_USD': '15',
}


class FakeMeta:
    def __init__(self, stream: bool = False) -> None:
        self.requests: list[httpx.Request] = []
        self.stream = stream

    def handler(self, request: httpx.Request) -> httpx.Response:
        self.requests.append(request)
        body = json.loads(request.content)
        response = {'id': 'resp_1', 'model': body['model'], 'status': 'completed', 'output': [],
                    'usage': {'input_tokens': 100, 'output_tokens': 50}}
        if not self.stream:
            return httpx.Response(200, json=response)
        sse = (b'event: response.output_text.delta\ndata: {"delta":"Hi"}\n\n'
               b'event: response.completed\ndata: ' + json.dumps({'response': response}).encode() + b'\n\n')
        return httpx.Response(200, content=sse, headers={'content-type': 'text/event-stream'})


def client_for(fake: FakeMeta) -> TestClient:
    upstream = httpx.AsyncClient(transport=httpx.MockTransport(fake.handler))
    return TestClient(create_app(Settings.from_env(ENV), client=upstream))


def ask(client: TestClient, body: dict, code: str = JUDGE_CODE, kind: str = 'answer'):
    return client.post('/v1/responses', json=body,
                       headers={'Authorization': f'Bearer {code}', 'X-MetaMate-Call': kind})


class ProxyTests(unittest.TestCase):
    def test_an_unknown_invite_is_refused_before_anything_is_sent(self):
        fake = FakeMeta()
        client = client_for(fake)
        self.assertEqual(ask(client, {'model': 'muse-spark-1.3'}, code='nope').status_code, 401)
        self.assertEqual(client.post('/v1/responses', json={}).status_code, 401)
        self.assertEqual(fake.requests, [])

    def test_the_server_key_replaces_the_invite_and_the_body_is_clamped(self):
        fake = FakeMeta()
        client = client_for(fake)
        answer = ask(client, {'model': 'muse-wonder-1.0', 'input': 'Hi', 'tools': [{'type': 'web_search'}],
                              'max_output_tokens': 99999, 'secret_field': 1}, kind='intent')
        self.assertEqual(answer.status_code, 200)
        self.assertEqual(answer.headers['x-metamate-model'], 'muse-spark-1.3')
        sent = fake.requests[0]
        self.assertEqual(sent.headers['authorization'], 'Bearer server-side-key')
        forwarded = json.loads(sent.content)
        self.assertEqual(forwarded['model'], 'muse-spark-1.3')
        self.assertNotIn('tools', forwarded)
        self.assertNotIn('secret_field', forwarded)
        self.assertEqual(forwarded['reasoning'], {'effort': 'minimal'})
        self.assertLessEqual(forwarded['max_output_tokens'], 200)

    def test_bad_requests_are_refused(self):
        fake = FakeMeta()
        client = client_for(fake)
        self.assertEqual(ask(client, {'model': 'gpt-5'}).status_code, 400)
        self.assertEqual(ask(client, {'model': 'muse-spark-1.3'}, kind='nonsense').status_code, 400)
        self.assertEqual(client.post('/v1/responses', content=b'[1]', headers={
            'Authorization': f'Bearer {JUDGE_CODE}'}).status_code, 400)
        self.assertEqual(fake.requests, [])

    def test_a_previous_response_must_belong_to_the_same_invite(self):
        fake = FakeMeta()
        client = client_for(fake)
        self.assertEqual(ask(client, {'model': 'muse-spark-1.3', 'input': 'a'}).status_code, 200)
        follow = {'model': 'muse-spark-1.3', 'previous_response_id': 'resp_1', 'input': 'b'}
        self.assertEqual(ask(client, follow, code='tester-code-001').status_code, 403)
        self.assertEqual(ask(client, follow).status_code, 200)

    def test_a_stream_passes_through_untouched(self):
        fake = FakeMeta(stream=True)
        client = client_for(fake)
        answer = ask(client, {'model': 'muse-spark-1.3', 'input': 'Hi', 'stream': True})
        self.assertEqual(answer.status_code, 200)
        self.assertIn(b'"delta":"Hi"', answer.content)
        self.assertIn(b'response.completed', answer.content)

    def test_oversized_nested_and_remote_image_bodies_are_refused(self):
        fake = FakeMeta()
        client = client_for(fake)
        huge = client.post('/v1/responses', content=b'{"input":"' + b'a' * 2_600_000 + b'"}',
                           headers={'Authorization': f'Bearer {JUDGE_CODE}'})
        self.assertEqual(huge.status_code, 413)
        nested = '{"input":' * 40 + '"x"' + '}' * 40
        self.assertEqual(client.post('/v1/responses', content=nested.encode(), headers={
            'Authorization': f'Bearer {JUDGE_CODE}'}).status_code, 400)
        remote = {'model': 'muse-spark-1.3', 'input': [{'type': 'message', 'role': 'user', 'content': [
            {'type': 'input_image', 'image_url': 'https://example.com/screen.png'}]}]}
        self.assertEqual(ask(client, remote).status_code, 400)
        self.assertEqual(fake.requests, [])

    def test_a_conversation_chain_is_capped(self):
        from metamate_proxy.policy import MAX_CHAIN_DEPTH
        fake = FakeMeta()
        ids = iter(range(1000))

        def handler(request):
            fake.requests.append(request)
            body = json.loads(request.content)
            return httpx.Response(200, json={'id': f'r{next(ids)}', 'model': body['model'],
                                             'status': 'completed', 'output': [],
                                             'usage': {'input_tokens': 1, 'output_tokens': 1}})

        client = TestClient(create_app(Settings.from_env(ENV), client=httpx.AsyncClient(
            transport=httpx.MockTransport(handler))))
        previous = None
        for turn in range(MAX_CHAIN_DEPTH):
            body = {'model': 'muse-spark-1.3', 'input': 'next'}
            if previous:
                body['previous_response_id'] = previous
            answer = ask(client, body)
            self.assertEqual(answer.status_code, 200, turn)
            previous = answer.json()['id']
        body = {'model': 'muse-spark-1.3', 'input': 'next', 'previous_response_id': previous}
        self.assertEqual(ask(client, body).status_code, 429)

    def test_completed_response_reads_only_the_final_event(self):
        self.assertIsNone(completed_response(b'event: response.output_text.delta\ndata: {"delta":"x"}'))
        self.assertEqual(completed_response(b'data: {"type":"response.completed","response":{"id":"r"}}'),
                         {'id': 'r'})
        self.assertIsNone(completed_response(b'data: not json'))

    def test_settings_refuse_weak_invites_and_plain_http(self):
        with self.assertRaises(ValueError):
            Settings.from_env({**ENV, 'PROXY_INVITE_CODES': 'short:judge'})
        with self.assertRaises(ValueError):
            Settings.from_env({**ENV, 'META_API_BASE': 'http://api.meta.ai/v1'})
        with self.assertRaises(ValueError):
            Settings.from_env({k: v for k, v in ENV.items() if k != 'META_API_KEY'})


if __name__ == '__main__':
    unittest.main()
