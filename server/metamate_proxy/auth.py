"""Sign-in with Google or GitHub for the desktop app (D54). Pure parts: no network here.

The app opens the browser on `/auth/<provider>/start` with a loopback port, its own state
and a PKCE challenge; the proxy runs the provider's code flow with its client secret
(never in the app), then sends the browser back to `http://127.0.0.1:<port>/callback` with
a one-time login code. The app redeems that code with its verifier for a session token:
an HMAC-signed, expiring claim of who signed in and their tier. Another local program that
sees the login code cannot redeem it without the verifier.
"""

import base64
from dataclasses import dataclass, field
import hashlib
import hmac
import json
import re
import secrets

TOKEN_PREFIX = 'mms1.'
SESSION_SECONDS = 30 * 24 * 3600
PENDING_SECONDS = 600
CODE_SECONDS = 120
MAX_PENDING = 10_000
MAX_NAME = 60

_URLSAFE = re.compile(r'[A-Za-z0-9_-]+')
_VERIFIER = re.compile(r'[A-Za-z0-9._~-]{43,128}')


@dataclass(frozen=True)
class Provider:
    authorize: str
    token: str
    scope: str


PROVIDERS = {
    'google': Provider('https://accounts.google.com/o/oauth2/v2/auth', 'https://oauth2.googleapis.com/token',
                       'openid email profile'),
    'github': Provider('https://github.com/login/oauth/authorize', 'https://github.com/login/oauth/access_token',
                       'read:user user:email'),
}
GOOGLE_USERINFO = 'https://openidconnect.googleapis.com/v1/userinfo'
GITHUB_USER = 'https://api.github.com/user'
GITHUB_EMAILS = 'https://api.github.com/user/emails'


@dataclass(frozen=True)
class Client:
    id: str
    secret: str = field(repr=False)


@dataclass(frozen=True)
class Identity:
    provider: str
    subject: str
    email: str
    name: str


def b64url(data: bytes) -> str:
    return base64.urlsafe_b64encode(data).rstrip(b'=').decode('ascii')


def _unb64url(text: str) -> bytes:
    return base64.urlsafe_b64decode(text + '=' * (-len(text) % 4))


def challenge_of(verifier: str) -> str:
    """PKCE S256."""
    return b64url(hashlib.sha256(verifier.encode('ascii')).digest())


def valid_verifier(verifier: object) -> bool:
    return isinstance(verifier, str) and _VERIFIER.fullmatch(verifier) is not None


def valid_app_state(state: object) -> bool:
    return isinstance(state, str) and 16 <= len(state) <= 128 and _URLSAFE.fullmatch(state) is not None


def valid_challenge(challenge: object) -> bool:
    return isinstance(challenge, str) and len(challenge) == 43 and _URLSAFE.fullmatch(challenge) is not None


def valid_port(port: object) -> bool:
    return isinstance(port, int) and not isinstance(port, bool) and 1024 <= port <= 65535


def sign_session(identity: Identity, tier: str, secret: str, now: float) -> str:
    payload = {'p': identity.provider, 's': identity.subject, 'n': identity.name[:MAX_NAME], 't': tier,
               'exp': int(now) + SESSION_SECONDS}
    body = b64url(json.dumps(payload, separators=(',', ':'), ensure_ascii=False).encode('utf-8'))
    mac = b64url(hmac.new(secret.encode('utf-8'), body.encode('ascii'), hashlib.sha256).digest())
    return f'{TOKEN_PREFIX}{body}.{mac}'


def read_session(token: str, secret: str, now: float) -> dict | None:
    """The claims of a valid, unexpired token; None otherwise (never raises)."""
    if not secret or not isinstance(token, str) or not token.startswith(TOKEN_PREFIX) or len(token) > 1024:
        return None
    body, _, mac = token[len(TOKEN_PREFIX):].partition('.')
    # compare_digest raises on non-ASCII text: a forged "mms1.a.é" is just invalid.
    if not body or not mac or not body.isascii() or not mac.isascii():
        return None
    expected = b64url(hmac.new(secret.encode('utf-8'), body.encode('ascii', 'replace'), hashlib.sha256).digest())
    if not hmac.compare_digest(expected, mac):
        return None
    try:
        claims = json.loads(_unb64url(body))
    except (ValueError, UnicodeDecodeError):
        return None
    if (not isinstance(claims, dict) or claims.get('p') not in PROVIDERS or not isinstance(claims.get('s'), str)
            or claims.get('t') not in ('judge', 'user') or not isinstance(claims.get('exp'), int)
            or claims['exp'] < now):
        return None
    return claims


def google_identity(info: object) -> Identity | None:
    """From OpenID userinfo; an unverified e-mail counts as none."""
    if not isinstance(info, dict) or not isinstance(info.get('sub'), str) or not info['sub']:
        return None
    email = info.get('email') if info.get('email_verified') is True and isinstance(info.get('email'), str) else ''
    name = info.get('name') if isinstance(info.get('name'), str) else ''
    return Identity('google', info['sub'], email.lower(), name or email.split('@')[0])


def github_identity(user: object, emails: object) -> Identity | None:
    """From /user and /user/emails: the primary verified e-mail, if any."""
    if not isinstance(user, dict) or not isinstance(user.get('id'), int) or isinstance(user.get('id'), bool):
        return None
    email = ''
    if isinstance(emails, list):
        for item in emails:
            if (isinstance(item, dict) and item.get('primary') is True and item.get('verified') is True
                    and isinstance(item.get('email'), str)):
                email = item['email'].lower()
    login = user.get('login') if isinstance(user.get('login'), str) else ''
    name = user.get('name') if isinstance(user.get('name'), str) and user.get('name') else login
    return Identity('github', str(user['id']), email, name or 'GitHub')


@dataclass
class Flows:
    """Pending sign-ins (proxy state -> where to send the browser back) and login codes
    (code -> who signed in). Both single-use, short-lived and bounded; not thread-safe (the
    app's event loop serializes)."""

    pending: dict[str, dict] = field(default_factory=dict)
    codes: dict[str, dict] = field(default_factory=dict)

    def _prune(self, now: float) -> None:
        """Only expired entries go: a flood never evicts a live sign-in (it is refused)."""
        for store, ttl in ((self.pending, PENDING_SECONDS), (self.codes, CODE_SECONDS)):
            if not store or now - next(iter(store.values()))['at'] <= ttl:
                continue  # insertion order: the oldest is first, so nothing else expired
            for key in [key for key, value in store.items() if now - value['at'] > ttl]:
                del store[key]

    def start(self, provider: str, port: int, app_state: str, challenge: str,
              now: float) -> tuple[str, str] | None:
        """(proxy state, provider PKCE verifier); None when too many sign-ins are open."""
        self._prune(now)
        if len(self.pending) >= MAX_PENDING:
            return None
        state = secrets.token_urlsafe(24)
        # The provider's own PKCE: a provider code stolen from someone else's sign-in cannot
        # be pushed through this flow, because it was issued for that flow's challenge.
        verifier = secrets.token_urlsafe(48)
        self.pending[state] = {'provider': provider, 'port': port, 'app_state': app_state,
                               'challenge': challenge, 'provider_verifier': verifier, 'at': now}
        return state, verifier

    def finish(self, provider: str, state: str, now: float) -> dict | None:
        self._prune(now)
        flow = self.pending.pop(state, None) if isinstance(state, str) else None
        return flow if flow and flow['provider'] == provider else None

    def issue(self, flow: dict, identity: Identity, now: float) -> str | None:
        self._prune(now)
        if len(self.codes) >= MAX_PENDING:
            return None
        code = secrets.token_urlsafe(24)
        self.codes[code] = {'identity': identity, 'challenge': flow['challenge'], 'at': now}
        return code

    def redeem(self, code: object, verifier: object, now: float) -> Identity | None:
        self._prune(now)
        if not isinstance(code, str) or not valid_verifier(verifier):
            return None
        entry = self.codes.pop(code, None)
        if entry is None or not hmac.compare_digest(entry['challenge'], challenge_of(verifier)):
            return None
        return entry['identity']
