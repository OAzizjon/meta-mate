"""The static landing page (PROXY_SITE_DIR, from site/), with its own headers."""

from pathlib import Path

from fastapi import FastAPI
from fastapi.responses import Response
from starlette.staticfiles import StaticFiles


class SiteFiles(StaticFiles):
    """Keep the landing page's policy separate from the sign-in pages and API."""

    async def get_response(self, path: str, scope) -> Response:
        response = await super().get_response(path, scope)
        response.headers['X-Content-Type-Options'] = 'nosniff'
        response.headers['Content-Security-Policy'] = (
            "default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self'; "
            "base-uri 'none'; form-action 'none'; frame-ancestors 'none'")
        response.headers['Referrer-Policy'] = 'no-referrer'
        return response


def mount(app: FastAPI, site_dir: str) -> None:
    """Call last: every API route (including WebSockets) takes precedence over static files."""
    if site_dir and Path(site_dir).is_dir():
        app.mount('/', SiteFiles(directory=site_dir, html=True), name='site')
