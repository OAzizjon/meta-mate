"""Sign-in with Google and GitHub (D54), with the providers faked: no network."""

import json
import unittest
from urllib.parse import parse_qs, urlsplit

import httpx
from fastapi.testclient import TestClient

from metamate_proxy import auth
from metamate_proxy.app import Settings, create_app
from tests.test_app import ENV

SECRET = 's' * 40
VERIFIER = 'v' * 50
AUTH_ENV = dict(ENV, GOOGLE_CLIENT_ID='google-client', GOOGLE_CLIENT_SECRET='google-secret',
                GITHUB_CLIENT_ID='github-client', GITHUB_CLIENT_SECRET='github-secret',
                PROXY_SESSION_SECRET=SECRET, PROXY_JUDGE_EMAILS='Judge@Example.com')


class FakeProviders:
    """Google's and GitHub's token and user endpoints, plus Meta for the calls after."""

    def __init__(self, email='person@example.com', verified=True) -> None:
        self.email = email
        self.verified = verified
        self.requests: list[httpx.Request] = []
        # code -> the PKCE challenge of the authorize request it answered
        self.codes: dict[str, str] = {}

    def handler(self, request: httpx.Request) -> httpx.Response:
        self.requests.append(request)
        url = str(request.url)
        if url == auth.PROVIDERS['google'].token or url == auth.PROVIDERS['github'].token:
            form = parse_qs(request.content.decode())
            # The provider checks PKCE: the code is bound to the challenge it was issued for.
            issued_for = self.codes.get(form.get('code', [''])[0])
            verifier = form.get('code_verifier', [''])[0]
            if issued_for is None or issued_for != auth.challenge_of(verifier):
                return httpx.Response(400, json={'error': 'invalid_grant'})
            return httpx.Response(200, json={'access_token': 'provider-access'})
        if url == auth.GOOGLE_USERINFO:
            return httpx.Response(200, json={'sub': '123', 'email': self.email,
                                             'email_verified': self.verified, 'name': 'Ann'})
        if url == auth.GITHUB_USER:
            return httpx.Response(200, json={'id': 77, 'login': 'ann', 'name': None})
        if url == auth.GITHUB_EMAILS:
            return httpx.Response(200, json=[{'email': self.email, 'primary': True, 'verified': self.verified}])
        if request.url.path == '/synthesize':
            return httpx.Response(200, content=b'RIFF....WAVE', headers={'content-type': 'audio/wav'})
        return httpx.Response(404)


def client_for(fake: FakeProviders, env: dict | None = None) -> TestClient:
    upstream = httpx.AsyncClient(transport=httpx.MockTransport(fake.handler))
    client = TestClient(create_app(Settings.from_env(env or AUTH_ENV), client=upstream), follow_redirects=False)
    client.fake = fake
    return client


def sign_in(client: TestClient, provider='google', code='good-code', verifier=VERIFIER):
    start = client.get(f'/auth/{provider}/start', params={
        'port': 51234, 'state': 'app-state-123456789', 'challenge': auth.challenge_of(verifier)})
    assert start.status_code == 302, start.text
    query = parse_qs(urlsplit(start.headers['location']).query)
    proxy_state = query['state'][0]
    # The person signs in: the provider issues `code` for this flow's challenge.
    if code == 'good-code':
        client.fake.codes[code] = query['code_challenge'][0]
    back = client.get(f'/auth/{provider}/callback', params={'code': code, 'state': proxy_state})
    return start, back


class PureTests(unittest.TestCase):
    def test_tokens_verify_and_expire(self):
        person = auth.Identity('google', '123', 'a@b.c', 'Ann')
        token = auth.sign_session(person, 'user', SECRET, now=1000)
        self.assertEqual(auth.read_session(token, SECRET, now=1001)['s'], '123')
        self.assertIsNone(auth.read_session(token, SECRET, now=1000 + auth.SESSION_SECONDS + 1))
        self.assertIsNone(auth.read_session(token, 'x' * 40, now=1001))
        tampered = token[:-2] + ('AA' if not token.endswith('AA') else 'BB')
        self.assertIsNone(auth.read_session(tampered, SECRET, now=1001))
        self.assertIsNone(auth.read_session(token, '', now=1001))
        for junk in ('', 'mms1.', 'mms1.a.b', 'Bearer x', 'mms1.' + 'a' * 2000, 'mms1.a.é', 'mms1.é.a'):
            self.assertIsNone(auth.read_session(junk, SECRET, now=1001))

    def test_identities_need_verified_emails(self):
        self.assertEqual(auth.google_identity({'sub': '1', 'email': 'A@B.C', 'email_verified': True}).email, 'a@b.c')
        self.assertEqual(auth.google_identity({'sub': '1', 'email': 'a@b.c', 'email_verified': 'true'}).email, '')
        self.assertIsNone(auth.google_identity({'email': 'a@b.c'}))
        github = auth.github_identity({'id': 5, 'login': 'x'}, [
            {'email': 'other@b.c', 'primary': False, 'verified': True},
            {'email': 'Main@b.c', 'primary': True, 'verified': True}])
        self.assertEqual((github.subject, github.email, github.name), ('5', 'main@b.c', 'x'))
        self.assertEqual(auth.github_identity({'id': 5}, [{'email': 'a@b', 'primary': True, 'verified': False}]).email, '')
        self.assertIsNone(auth.github_identity({'id': True}, []))

    def test_flows_are_single_use_and_bound_to_the_verifier(self):
        flows = auth.Flows()
        state, _ = flows.start('google', 50000, 'app-state-123456789', auth.challenge_of(VERIFIER), now=0)
        self.assertIsNone(flows.finish('github', state, now=1))
        state, _ = flows.start('google', 50000, 'app-state-123456789', auth.challenge_of(VERIFIER), now=0)
        flow = flows.finish('google', state, now=1)
        self.assertIsNone(flows.finish('google', state, now=1))
        code = flows.issue(flow, auth.Identity('google', '1', '', 'A'), now=1)
        self.assertIsNone(flows.redeem(code, 'w' * 50, now=2))
        self.assertIsNone(flows.redeem(code, VERIFIER, now=2))  # one try only
        code = flows.issue(flow, auth.Identity('google', '1', '', 'A'), now=1)
        self.assertIsNone(flows.redeem(code, VERIFIER, now=1 + auth.CODE_SECONDS + 1))
        code = flows.issue(flow, auth.Identity('google', '1', '', 'A'), now=1)
        self.assertEqual(flows.redeem(code, VERIFIER, now=2).subject, '1')


    def test_a_flood_is_refused_instead_of_evicting_live_sign_ins(self):
        flows = auth.Flows()
        first, _ = flows.start('google', 50000, 'app-state-123456789', 'c' * 43, now=0)
        for _ in range(auth.MAX_PENDING - 1):
            flows.start('google', 50000, 'app-state-123456789', 'c' * 43, now=1)
        self.assertIsNone(flows.start('google', 50000, 'app-state-123456789', 'c' * 43, now=2))
        self.assertIsNotNone(flows.finish('google', first, now=3))
        # Expired entries make room again.
        self.assertIsNotNone(flows.start('google', 50000, 'app-state-123456789', 'c' * 43,
                                         now=2 + auth.PENDING_SECONDS))


class FlowTests(unittest.TestCase):
    def test_a_stolen_provider_code_cannot_be_replayed_through_another_flow(self):
        fake = FakeProviders()
        client = client_for(fake)
        # The victim's flow gets a code the attacker steals before it is used.
        victim = client.get('/auth/google/start', params={
            'port': 51234, 'state': 'victim-state-1234567', 'challenge': auth.challenge_of(VERIFIER)})
        fake.codes['stolen-code'] = parse_qs(urlsplit(victim.headers['location']).query)['code_challenge'][0]
        attacker = client.get('/auth/google/start', params={
            'port': 51235, 'state': 'attacker-state-123456', 'challenge': auth.challenge_of('a' * 50)})
        attacker_state = parse_qs(urlsplit(attacker.headers['location']).query)['state'][0]
        back = client.get('/auth/google/callback', params={'code': 'stolen-code', 'state': attacker_state})
        self.assertEqual(back.status_code, 400)


    def test_google_sign_in_end_to_end_then_the_token_works(self):
        fake = FakeProviders()
        client = client_for(fake)
        start, back = sign_in(client)
        authorize = urlsplit(start.headers['location'])
        query = parse_qs(authorize.query)
        self.assertEqual(f'{authorize.scheme}://{authorize.netloc}{authorize.path}', auth.PROVIDERS['google'].authorize)
        self.assertEqual(query['redirect_uri'], ['http://127.0.0.1:8787/auth/google/callback'])
        self.assertEqual(query['client_id'], ['google-client'])
        self.assertNotIn('google-secret', start.headers['location'])
        self.assertEqual(back.status_code, 302)
        landing = urlsplit(back.headers['location'])
        self.assertEqual((landing.hostname, landing.port, landing.path), ('127.0.0.1', 51234, '/callback'))
        params = parse_qs(landing.query)
        self.assertEqual(params['state'], ['app-state-123456789'])
        wrong = client.post('/auth/token', content=json.dumps({'code': params['code'][0], 'verifier': 'w' * 50}))
        self.assertEqual(wrong.status_code, 400)
        # The code died with the wrong verifier: sign in again.
        _, back = sign_in(client)
        login = parse_qs(urlsplit(back.headers['location']).query)['code'][0]
        issued = client.post('/auth/token', content=json.dumps({'code': login, 'verifier': VERIFIER}))
        self.assertEqual(issued.status_code, 200)
        body = issued.json()
        self.assertEqual((body['name'], body['provider'], body['tier']), ('Ann', 'google', 'user'))
        spoken = client.post('/v1/tts', json={'lang': 'en', 'text': 'Hi'},
                             headers={'Authorization': f'Bearer {body["token"]}'})
        self.assertEqual(spoken.status_code, 200)
        self.assertEqual(client.post('/v1/tts', json={'lang': 'en', 'text': 'Hi'},
                                     headers={'Authorization': 'Bearer mms1.forged.token'}).status_code, 401)

    def test_github_and_the_judge_tier(self):
        fake = FakeProviders(email='judge@example.com')
        client = client_for(fake)
        _, back = sign_in(client, provider='github')
        login = parse_qs(urlsplit(back.headers['location']).query)['code'][0]
        body = client.post('/auth/token', content=json.dumps({'code': login, 'verifier': VERIFIER})).json()
        self.assertEqual((body['provider'], body['tier'], body['name']), ('github', 'judge', 'ann'))
        # An unverified judge e-mail is an ordinary user.
        client = client_for(FakeProviders(email='judge@example.com', verified=False))
        _, back = sign_in(client, provider='github')
        login = parse_qs(urlsplit(back.headers['location']).query)['code'][0]
        self.assertEqual(client.post('/auth/token', content=json.dumps(
            {'code': login, 'verifier': VERIFIER})).json()['tier'], 'user')

    def test_a_start_from_a_link_elsewhere_is_refused(self):
        client = client_for(FakeProviders())
        params = {'port': 51234, 'state': 'app-state-123456789', 'challenge': auth.challenge_of(VERIFIER)}
        for site in ('cross-site', 'same-site', 'same-origin'):
            self.assertEqual(client.get('/auth/google/start', params=params,
                                        headers={'Sec-Fetch-Site': site}).status_code, 403)
        self.assertEqual(client.get('/auth/google/start', params=params,
                                    headers={'Sec-Fetch-Site': 'none'}).status_code, 302)

    def test_bad_starts_callbacks_and_missing_configuration(self):
        client = client_for(FakeProviders())
        for params in ({'port': 80, 'state': 'app-state-123456789', 'challenge': 'c' * 43},
                       {'port': 51234, 'state': 'short', 'challenge': 'c' * 43},
                       {'port': 51234, 'state': 'app-state-123456789', 'challenge': 'c' * 10}):
            self.assertEqual(client.get('/auth/google/start', params=params).status_code, 400)
        self.assertEqual(client.get('/auth/apple/start', params={'port': 51234}).status_code, 503)
        self.assertEqual(client.get('/auth/google/callback', params={'code': 'good-code', 'state': 'nope'}).status_code, 400)
        _, back = sign_in(client, code='bad-code')
        self.assertEqual(back.status_code, 400)
        bare = client_for(FakeProviders(), {k: v for k, v in AUTH_ENV.items() if k != 'PROXY_SESSION_SECRET'})
        self.assertEqual(bare.get('/auth/google/start', params={'port': 51234}).status_code, 503)
        self.assertEqual(bare.post('/auth/token', content='{}').status_code, 503)
        with self.assertRaises(ValueError):
            Settings.from_env(dict(AUTH_ENV, PROXY_SESSION_SECRET='short'))
        with self.assertRaises(ValueError):
            Settings.from_env(dict(AUTH_ENV, PROXY_PUBLIC_URL='http://example.com'))

    def test_guest_trial_counts_answers_per_device_and_hour(self):
        client = client_for(FakeProviders(), dict(AUTH_ENV, PROXY_REQUIRE_SIGN_IN='1'))
        self.assertEqual(client.post('/auth/guest', content='{"device": "short"}').status_code, 400)
        issued = client.post('/auth/guest', content=json.dumps({'device': 'd' * 32}))
        self.assertEqual(issued.status_code, 200)
        body = issued.json()
        self.assertEqual((body['limit'], body['left']), (12, 12))
        bearer = {'Authorization': f'Bearer {body["token"]}'}
        ask = {'model': 'muse-spark-1.3', 'input': 'hi'}
        # A body the proxy refuses is not an answer: nothing is counted.
        broken = client.post('/v1/responses', headers=bearer, content='not json')
        self.assertEqual(broken.status_code, 400)
        self.assertEqual(broken.headers['x-metamate-guest-left'], '12')
        for left in range(11, -1, -1):
            answered = client.post('/v1/responses', headers=bearer, json=ask)
            self.assertEqual(answered.headers['x-metamate-guest-left'], str(left))
        refused = client.post('/v1/responses', headers=bearer, json=ask)
        self.assertEqual(refused.status_code, 429)
        self.assertIn('guest_limit', refused.text)
        self.assertEqual(refused.headers['x-metamate-guest-left'], '0')
        self.assertGreater(int(refused.headers['x-metamate-guest-reset']), 0)
        # Other calls of a question (speech) are not counted; a new token for the same device
        # does not reset the hour.
        again = client.post('/auth/guest', content=json.dumps({'device': 'd' * 32})).json()
        self.assertEqual(again['left'], 0)
        # Ten guest tokens per address and hour.
        for i in range(8):
            client.post('/auth/guest', content=json.dumps({'device': f'{i}' * 32}))
        self.assertEqual(client.post('/auth/guest', content=json.dumps({'device': 'z' * 32})).status_code, 429)

    def test_guest_tokens_verify_and_never_count_as_sign_in(self):
        token = auth.sign_guest('g' * 32, SECRET, now=1000)
        claims = auth.read_session(token, SECRET, now=1001)
        self.assertEqual((claims['p'], claims['t']), ('guest', 'guest'))
        forged = auth.sign_session(auth.Identity('guest', 'g' * 32, '', 'x'), 'judge', SECRET, now=1000)
        self.assertIsNone(auth.read_session(forged, SECRET, now=1001))
        table = {'old': [0.0], 'new': [3000.0], 'empty': []}
        auth.prune_hours(table, 3700.0)
        self.assertEqual(list(table), ['new'])
        hits = [0.0, 10.0, 3700.0]
        self.assertEqual(auth.guest_left(hits, 3700.0), (11, 3601))
        self.assertEqual(hits, [3700.0])

    def test_required_sign_in_refuses_invite_codes(self):
        self.assertTrue(Settings.from_env(AUTH_ENV).invites)
        self.assertEqual(Settings.from_env(dict(AUTH_ENV, PROXY_REQUIRE_SIGN_IN='1')).invites, {})
        with self.assertRaises(ValueError):
            Settings.from_env(dict(ENV, PROXY_REQUIRE_SIGN_IN='1'))
        from tests.test_app import JUDGE_CODE
        strict = client_for(FakeProviders(), dict(AUTH_ENV, PROXY_REQUIRE_SIGN_IN='1'))
        refused = strict.post('/v1/responses', headers={'Authorization': f'Bearer {JUDGE_CODE}'},
                              json={'model': 'muse-spark-1.3', 'input': 'hi'})
        self.assertEqual(refused.status_code, 401)


if __name__ == '__main__':
    unittest.main()
