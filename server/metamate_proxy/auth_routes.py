"""Sign-in (D54): `/auth/{provider}/start` and `/callback` run the provider's OAuth flow in
the browser, then `/auth/token` trades the app's one-time login code for a session token."""

import json
from datetime import datetime, timezone
from typing import TYPE_CHECKING

import httpx
from fastapi import FastAPI, Request
from fastapi.responses import HTMLResponse, JSONResponse, RedirectResponse, Response

from . import auth
from .common import State, error, read_body

if TYPE_CHECKING:
    from .app import Settings

AUTH_SECONDS = 15
# Guest tokens one client address may take per hour.
GUEST_TOKENS_PER_HOUR = 10


def auth_page(title: str, status: int = 200) -> HTMLResponse:
    """The browser tab's last page; the app shows the real result in its own language."""
    return HTMLResponse(f'<!doctype html><meta charset="utf-8"><title>MetaMate</title>'
                        f'<p style="font:20px system-ui;margin:3em">{title}</p>', status_code=status)


def register(app: FastAPI, settings: 'Settings', state: State, http: httpx.AsyncClient) -> None:
    # The sign-in endpoints take a `state` query parameter: this name keeps them apart.
    state_flows = state.flows

    def redirect_uri(provider: str) -> str:
        return f'{settings.public_url}/auth/{provider}/callback'

    @app.get('/auth/{provider}/start')
    async def auth_start(request: Request, provider: str, port: int = 0, state: str = '',
                         challenge: str = '') -> Response:
        client = settings.oauth.get(provider)
        if client is None or not settings.session_secret:
            return auth_page('Sign-in is not available.', 503)
        # The app opens this page itself (Sec-Fetch-Site: none). A link someone sent, clicked
        # in mail, chat or another site, could carry their loopback port and hand them the
        # session: refused.
        if request.headers.get('sec-fetch-site', 'none') != 'none':
            return auth_page('Please start signing in from MetaMate.', 403)
        if not (auth.valid_port(port) and auth.valid_app_state(state) and auth.valid_challenge(challenge)):
            return auth_page('This sign-in link is not valid.', 400)
        started = state_flows.start(provider, port, state, challenge, datetime.now(timezone.utc).timestamp())
        if started is None:
            return auth_page('Too many sign-ins right now. Please try again in a few minutes.', 503)
        flow_state, provider_verifier = started
        details = auth.PROVIDERS[provider]
        query = {'client_id': client.id, 'redirect_uri': redirect_uri(provider), 'response_type': 'code',
                 'scope': details.scope, 'state': flow_state,
                 'code_challenge': auth.challenge_of(provider_verifier), 'code_challenge_method': 'S256'}
        if provider == 'google':
            query.update(access_type='online', prompt='select_account')
        return RedirectResponse(str(httpx.URL(details.authorize, params=query)), status_code=302)

    async def identity_from(provider: str, client: auth.Client, code: str, verifier: str) -> auth.Identity | None:
        details = auth.PROVIDERS[provider]
        form = {'client_id': client.id, 'client_secret': client.secret, 'code': code,
                'redirect_uri': redirect_uri(provider), 'grant_type': 'authorization_code',
                'code_verifier': verifier}
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
        identity = await identity_from(provider, client, code, flow['provider_verifier'])
        if identity is None:
            return auth_page('The sign-in did not work. Please try again from MetaMate.', 400)
        login = state_flows.issue(flow, identity, datetime.now(timezone.utc).timestamp())
        if login is None:
            return auth_page('Too many sign-ins right now. Please try again in a few minutes.', 503)
        back = httpx.URL(f'http://127.0.0.1:{flow["port"]}/callback',
                         params={'code': login, 'state': flow['app_state']})
        return RedirectResponse(str(back), status_code=302)

    @app.post('/auth/guest')
    async def auth_guest(request: Request) -> Response:
        """The guest trial: a token for this device without an account. Few per address, so a
        new device id per question is not a way around the hourly limit."""
        if not settings.session_secret:
            return error(503, 'the guest trial is not available')
        raw = await read_body(request, 1024)
        try:
            body = json.loads(raw) if raw is not None else None
        except (ValueError, RecursionError):
            body = None
        device = body.get('device') if isinstance(body, dict) else None
        if not isinstance(device, str) or not auth.GUEST_DEVICE.fullmatch(device):
            return error(400, 'send {"device"}: 22-64 URL-safe characters')
        now = datetime.now(timezone.utc).timestamp()
        # Each new guest token is also the moment to forget old devices and addresses.
        for table in (state.guest_issued, state.guest_hits, state.guest_steps):
            auth.prune_hours(table, now)
        address = request.client.host if request.client else 'unknown'
        issued = [at for at in state.guest_issued.get(address, []) if now - at < 3600]
        if len(issued) >= GUEST_TOKENS_PER_HOUR:
            return error(429, 'too many guest sessions from this address')
        issued.append(now)
        state.guest_issued[address] = issued
        left, reset = auth.guest_left(state.guest_hits.setdefault(f'user:guest:{device}', []), now)
        return JSONResponse({'token': auth.sign_guest(device, settings.session_secret, now),
                             'limit': auth.GUEST_ANSWERS_PER_HOUR, 'left': left, 'reset': reset})

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
