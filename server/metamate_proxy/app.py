"""The MetaMate proxy: the Meta key stays on the server, the app sends an invite code.

`/v1/responses` applies the pure policy (policy.py: model allowlist, token caps, a
worst-case reservation per invite, the Wonder allowance with a Spark reserve) and
streams Meta's answer through; `/v1/asr` relays Muse Voice Transcribe's realtime
WebSocket; `/v1/tts` speaks one sentence with MMS-TTS (tts/). No request or response
body is stored or logged: only counters.

Every ledger operation is synchronous, so on the one event loop they never interleave
and need no lock, and a settlement in a `finally` cannot be cut by a cancellation.

The routes live in their own modules (auth_routes, responses_route, media_routes,
asr_route, site; shared helpers in common); `create_app` registers them in that order.
"""

import os
from dataclasses import dataclass, field
from urllib.parse import urlsplit

import httpx
import websockets
from fastapi import FastAPI

from . import asr_route, auth, auth_routes, media_routes, policy, responses_route, site
from .common import State, today
# The names this module held before the routes moved out stay importable from here.
from .asr_route import (  # noqa: F401
    ASR_END_STREAM, ASR_IDLE_SECONDS, ASR_MAX_SECONDS, ASR_MAX_STREAMS_PER_INVITE, ASR_MODEL, ASR_MODES,
    ASR_SESSION_SECONDS, ASR_URL)
from .auth_routes import AUTH_SECONDS, auth_page  # noqa: F401
from .common import BODY_READ_SECONDS, error, invite_of, scrub, scrub_key, upstream_error  # noqa: F401
from .media_routes import TTS_IN_FLIGHT_PER_INVITE, TTS_MAX_BODY_BYTES, TTS_SECONDS  # noqa: F401
from .responses_route import (  # noqa: F401
    CALL_KINDS, FINAL_EVENTS, NOTHING_BILLED, RESPONSES_IN_FLIGHT_PER_INVITE, SSE_EVENT_MAX_BYTES, STREAM_SECONDS,
    Relay, final_response, scrub_event)
from .site import SiteFiles  # noqa: F401


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
    # Muse Glimmer on Modal (D53), behind the same shared secret as the TTS; empty: 503.
    glimmer_url: str = ''
    # Sign-in (D54): OAuth clients by provider, the key that signs session tokens, this
    # proxy's public origin (the providers' redirect URIs live under it) and the e-mails
    # that get the judge tier. Without a session secret, sign-in answers 503.
    oauth: dict[str, auth.Client] = field(default_factory=dict, repr=False)
    session_secret: str = field(default='', repr=False)
    public_url: str = 'http://127.0.0.1:8787'
    judge_emails: frozenset[str] = frozenset()
    ledger_path: str = ''
    site_dir: str = ''

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
        glimmer_url = (env.get('GLIMMER_URL') or '').rstrip('/')
        if glimmer_url and (not glimmer_url.startswith('https://') or len(tts_secret) < 16):
            raise ValueError('GLIMMER_URL must be https and needs MMS_TTS_SECRET')
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
        return Settings(key, base, invites, budget, prices, tts_url, tts_secret, glimmer_url, oauth,
                        session_secret, public_url, judges, env.get('PROXY_LEDGER_PATH') or '',
                        env.get('PROXY_SITE_DIR') or '')


def create_app(settings: Settings, client: httpx.AsyncClient | None = None,
               asr_connect=None, ledger_commit=None) -> FastAPI:
    """`client` and `asr_connect` are injectable for tests (no network)."""
    app = FastAPI(docs_url=None, redoc_url=None, openapi_url=None)
    state = State(ledger=policy.Ledger(path=settings.ledger_path, commit=ledger_commit))
    state.ledger.load(today())
    http = client or httpx.AsyncClient(timeout=httpx.Timeout(120, connect=10))
    connect = asr_connect or websockets.connect

    @app.get('/healthz')
    async def healthz() -> dict:
        return {'ok': True}

    auth_routes.register(app, settings, state, http)
    responses_route.register(app, settings, state, http)
    media_routes.register(app, settings, state, http)
    asr_route.register(app, settings, state, connect)
    # Last: every API route (including WebSockets) takes precedence over static files.
    site.mount(app, settings.site_dir)

    return app


def app_from_env(ledger_commit=None):
    """The production entry point (`uvicorn metamate_proxy.app:app_from_env --factory`,
    server/deploy/): settings from the environment, which systemd fills from a root-owned
    file. A bad or missing setting stops the start, before any request."""
    return create_app(Settings.from_env(), ledger_commit=ledger_commit)
