#!/usr/bin/env python3
"""Preflight: can we submit right now?

One line per check (PASS / FAIL / WARN) with a one-line fix; the process
exits 1 when any check FAILs. Standard library only. Network calls time
out after TIMEOUT seconds; an unreachable host is a WARN, never a crash.
Only known non-secret files are read (LICENSE, README, site/version.json,
src-tauri/tauri.conf.json, docs/submission.md) - never .env, keys or
other secrets files.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import subprocess
import sys
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Callable
from urllib import request as urlrequest
from urllib.error import HTTPError, URLError
from urllib.parse import unquote

TIMEOUT = 15.0
SITE_URL = "https://meta-mate.stream"
VERSION_URL = f"{SITE_URL}/version.json"
DOWNLOAD_URL = f"{SITE_URL}/download/MetaMate-setup.exe"
INSTALLER_DIR = Path("target/x86_64-pc-windows-msvc/release/bundle/nsis")
INSTALLER_NAME = "MetaMate_{version}_x64-setup.exe"

PASS, FAIL, WARN = "PASS", "FAIL", "WARN"

ROOT = Path(__file__).resolve().parent.parent


@dataclass
class Response:
    status: int
    headers: dict[str, str]  # header names are lower-case
    body: bytes


@dataclass
class Check:
    name: str
    status: str
    detail: str


Fetch = Callable[[str, str], Response]
Runner = Callable[[list[str]], tuple[int, str, str]]


def _fetch(method: str, url: str) -> Response:
    # Cloudflare refuses the default Python-urllib agent with 403.
    req = urlrequest.Request(url, method=method, headers={"User-Agent": "MetaMate-preflight"})
    try:
        with urlrequest.urlopen(req, timeout=TIMEOUT) as resp:
            return Response(
                resp.status,
                {k.lower(): v for k, v in resp.headers.items()},
                resp.read(),
            )
    except HTTPError as exc:
        return Response(exc.code, {}, b"")


def _runner(argv: list[str]) -> tuple[int, str, str]:
    try:
        proc = subprocess.run(
            argv,
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=TIMEOUT,
        )
    except FileNotFoundError:
        return 127, "", f"{argv[0]} not found"
    except subprocess.TimeoutExpired:
        return 124, "", "command timed out"
    return proc.returncode, proc.stdout, proc.stderr


def _tauri_version(root: Path) -> str | None:
    path = root / "src-tauri" / "tauri.conf.json"
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return None
    version = data.get("version")
    return version if isinstance(version, str) and version else None


def _installer_path(root: Path, version: str) -> Path:
    return root / INSTALLER_DIR / INSTALLER_NAME.format(version=version)


def check_license(root: Path) -> Check:
    for name in ("LICENSE", "LICENSE.md"):
        if (root / name).is_file():
            return Check("license", PASS, f"{name} present")
    return Check(
        "license",
        FAIL,
        "no LICENSE file at the repo root; fix: add one (e.g. the MIT license text)",
    )


def check_repo_visibility(runner: Runner) -> Check:
    rc, out, _ = runner(["gh", "repo", "view", "--json", "visibility"])
    if rc != 0:
        return Check(
            "repo-visibility",
            WARN,
            "`gh repo view` failed; fix: check `gh auth login` and the network, then re-run",
        )
    try:
        visibility = json.loads(out).get("visibility")
    except json.JSONDecodeError:
        return Check(
            "repo-visibility",
            WARN,
            "`gh repo view` returned invalid JSON; fix: update the GitHub CLI",
        )
    if visibility == "PUBLIC":
        return Check("repo-visibility", PASS, "repository is PUBLIC")
    return Check(
        "repo-visibility",
        FAIL,
        f"repository visibility is {visibility!r}; fix: set the repo to public on GitHub",
    )


_LINK_RE = re.compile(r"!?\[[^\]]*\]\(([^)]+)\)")


def check_readme(root: Path) -> Check:
    path = None
    for name in ("README.md", "README"):
        candidate = root / name
        if candidate.is_file():
            path = candidate
            break
    if path is None:
        return Check(
            "readme",
            FAIL,
            "no README.md at the repo root; fix: add one",
        )
    text = path.read_text(encoding="utf-8", errors="replace")
    if "TBD" in text:
        return Check(
            "readme",
            FAIL,
            'README still contains "TBD"; fix: fill in every placeholder before submitting',
        )
    broken = []
    for match in _LINK_RE.finditer(text):
        target = match.group(1).split()[0].strip("<>")
        if (
            target.startswith(("#", "//"))
            or "://" in target
            or target.startswith(("mailto:", "tel:", "data:"))
        ):
            continue
        local = unquote(re.split(r"[#?]", target, maxsplit=1)[0])
        if not local:
            continue
        if not (path.parent / local).exists():
            broken.append(target)
    if broken:
        shown = ", ".join(broken[:3])
        extra = f" (+{len(broken) - 3} more)" if len(broken) > 3 else ""
        return Check(
            "readme",
            FAIL,
            f"broken relative link(s): {shown}{extra}; fix: repair or remove them",
        )
    return Check("readme", PASS, "no TBD, all relative links resolve")


def check_site_version(root: Path, fetch: Fetch) -> Check:
    version = _tauri_version(root)
    if not version:
        return Check(
            "site-version",
            FAIL,
            'no top-level "version" in src-tauri/tauri.conf.json; fix: set the app version',
        )
    try:
        response = fetch("GET", VERSION_URL)
    except OSError as exc:
        return Check(
            "site-version",
            WARN,
            f"{SITE_URL} unreachable ({exc}); fix: check the network, then re-run",
        )
    if response.status != 200:
        return Check(
            "site-version",
            FAIL,
            f"{VERSION_URL} returned HTTP {response.status}; fix: deploy the site",
        )
    try:
        remote = json.loads(response.body).get("version")
    except (json.JSONDecodeError, UnicodeDecodeError):
        return Check(
            "site-version",
            FAIL,
            "site version.json is not valid JSON; fix: deploy a valid version.json",
        )
    if remote == version:
        return Check("site-version", PASS, f"site and tauri.conf.json both have {version}")
    return Check(
        "site-version",
        FAIL,
        f"site serves version {remote!r} but tauri.conf.json has {version!r}; "
        "fix: redeploy the site or bump the app version",
    )


def check_installer_sha256(root: Path) -> Check:
    version = _tauri_version(root)
    if not version:
        return Check(
            "installer-sha256",
            FAIL,
            'no "version" in src-tauri/tauri.conf.json; fix: set the app version',
        )
    installer = _installer_path(root, version)
    if not installer.is_file():
        return Check(
            "installer-sha256",
            WARN,
            f"no local installer ({installer.name}); fix: run `mm release`, then re-check",
        )
    manifest = root / "site" / "version.json"
    if not manifest.is_file():
        return Check(
            "installer-sha256",
            FAIL,
            "site/version.json is missing; fix: generate it during the release",
        )
    try:
        expected = json.loads(manifest.read_text(encoding="utf-8")).get("sha256")
    except json.JSONDecodeError:
        return Check(
            "installer-sha256",
            FAIL,
            "site/version.json is not valid JSON; fix: regenerate it from the installer",
        )
    if not isinstance(expected, str) or not expected:
        return Check(
            "installer-sha256",
            FAIL,
            "site/version.json has no sha256 field; fix: regenerate it from the installer",
        )
    digest = hashlib.sha256()
    with installer.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1 << 20), b""):
            digest.update(chunk)
    if digest.hexdigest() == expected.strip().lower():
        return Check("installer-sha256", PASS, "site/version.json sha256 matches the installer")
    return Check(
        "installer-sha256",
        FAIL,
        "site/version.json sha256 does not match the installer; "
        "fix: regenerate site/version.json from the built setup exe",
    )


def check_download_size(root: Path, fetch: Fetch) -> Check:
    version = _tauri_version(root)
    if not version:
        return Check(
            "download-size",
            FAIL,
            'no "version" in src-tauri/tauri.conf.json; fix: set the app version',
        )
    installer = _installer_path(root, version)
    if not installer.is_file():
        return Check(
            "download-size",
            WARN,
            f"no local installer ({installer.name}); fix: run `mm release`, then re-check",
        )
    try:
        response = fetch("HEAD", DOWNLOAD_URL)
    except OSError as exc:
        return Check(
            "download-size",
            WARN,
            f"{SITE_URL} unreachable ({exc}); fix: check the network, then re-run",
        )
    if response.status != 200:
        return Check(
            "download-size",
            FAIL,
            f"HEAD {DOWNLOAD_URL} returned HTTP {response.status}; fix: deploy the download",
        )
    raw = response.headers.get("content-length")
    try:
        length = int(raw) if raw is not None else None
    except ValueError:
        length = None
    if length is None:
        return Check(
            "download-size",
            WARN,
            "download endpoint sent no usable Content-Length; "
            "fix: serve the file with a known size",
        )
    size = installer.stat().st_size
    if length == size:
        return Check("download-size", PASS, f"Content-Length matches the installer ({size} bytes)")
    return Check(
        "download-size",
        FAIL,
        f"Content-Length is {length} but the installer is {size} bytes; "
        "fix: re-upload the current installer",
    )


def check_proxy_health(fetch: Fetch, proxy_url: str | None) -> Check:
    if not proxy_url:
        return Check(
            "proxy-health",
            WARN,
            "no proxy URL supplied; fix: run via `mm preflight` so the release proxy URL is passed",
        )
    url = proxy_url.rstrip("/") + "/healthz"
    try:
        response = fetch("GET", url)
    except OSError as exc:
        return Check(
            "proxy-health",
            WARN,
            f"proxy unreachable ({exc}); fix: check the proxy deployment and the URL",
        )
    if response.status != 200:
        return Check(
            "proxy-health",
            FAIL,
            f"proxy /healthz returned HTTP {response.status}; fix: check the proxy deployment",
        )
    try:
        ok = json.loads(response.body).get("ok")
    except (json.JSONDecodeError, UnicodeDecodeError):
        return Check(
            "proxy-health",
            FAIL,
            "proxy /healthz did not return JSON; fix: check the proxy deployment",
        )
    if ok is True:
        return Check("proxy-health", PASS, "proxy /healthz reports ok")
    return Check(
        "proxy-health",
        FAIL,
        f"proxy /healthz ok is {ok!r}; fix: check the proxy logs and deployment",
    )


def check_git_state(runner: Runner) -> Check:
    results = {}
    for key, argv in (
        ("status", ["git", "status", "--porcelain"]),
        ("head", ["git", "rev-parse", "HEAD"]),
        ("origin", ["git", "rev-parse", "origin/main"]),
    ):
        rc, out, err = runner(argv)
        if rc != 0:
            reason = err.strip() or "unknown error"
            return Check(
                "git-state",
                FAIL,
                f"{' '.join(argv)} failed: {reason}; "
                "fix: run preflight inside the clone with origin configured",
            )
        results[key] = out
    problems = []
    dirty = [line for line in results["status"].splitlines() if line.strip()]
    if dirty:
        problems.append(f"{len(dirty)} uncommitted change(s)")
    head = results["head"].strip()
    origin = results["origin"].strip()
    if head != origin:
        problems.append("HEAD != origin/main")
    if problems:
        return Check(
            "git-state",
            FAIL,
            "; ".join(problems)
            + "; fix: commit or stash changes and push to origin/main",
        )
    return Check("git-state", PASS, "working tree clean, HEAD == origin/main")


def check_github_release(runner: Runner, version: str | None = None) -> Check:
    # This version's release, not whatever is latest: an older release must not pass.
    tag_args = [f"v{version}"] if version else []
    rc, out, err = runner(["gh", "release", "view", *tag_args, "--json", "tagName,isDraft"])
    if rc != 0:
        if "no release" in err.lower() or "not found" in err.lower():
            return Check(
                "github-release",
                FAIL,
                f"no GitHub release {tag_args[0] if tag_args else ''} yet; fix: mm ship release",
            )
        return Check(
            "github-release",
            WARN,
            "`gh release view` failed; fix: check `gh auth login` and the network",
        )
    try:
        data = json.loads(out)
        tag, draft = data.get("tagName"), data.get("isDraft")
    except json.JSONDecodeError:
        return Check(
            "github-release",
            WARN,
            "`gh release view` returned invalid JSON; fix: update the GitHub CLI",
        )
    if draft is True:
        return Check(
            "github-release",
            FAIL,
            f"release {tag} is still a draft; fix: publish it on GitHub",
        )
    if draft is False:
        return Check("github-release", PASS, f"release {tag} is published")
    return Check(
        "github-release",
        WARN,
        "could not read isDraft from `gh release view`; fix: update the GitHub CLI",
    )


_UNCHECKED_RE = re.compile(r"\[ \]")


def check_submission_doc(root: Path) -> Check:
    path = root / "docs" / "submission.md"
    if not path.is_file():
        return Check(
            "submission-doc",
            WARN,
            "docs/submission.md is missing; fix: add the submission checklist",
        )
    text = path.read_text(encoding="utf-8", errors="replace")
    open_items = len(_UNCHECKED_RE.findall(text))
    if open_items:
        return Check(
            "submission-doc",
            WARN,
            f"{open_items} unchecked item(s) in docs/submission.md; "
            "fix: complete the checklist before submitting",
        )
    return Check("submission-doc", PASS, "submission checklist fully checked")


def run_checks(
    root: Path | str | None = None,
    fetch: Fetch | None = None,
    runner: Runner | None = None,
    proxy_url: str | None = None,
) -> list[Check]:
    base = Path(root) if root is not None else ROOT
    do_fetch = fetch or _fetch
    do_runner = runner or _runner
    return [
        check_license(base),
        check_repo_visibility(do_runner),
        check_readme(base),
        check_site_version(base, do_fetch),
        check_installer_sha256(base),
        check_download_size(base, do_fetch),
        check_proxy_health(do_fetch, proxy_url),
        check_git_state(do_runner),
        check_github_release(do_runner, _tauri_version(base)),
        check_submission_doc(base),
    ]


def _overall(checks: list[Check]) -> str:
    if any(c.status == FAIL for c in checks):
        return FAIL
    if any(c.status == WARN for c in checks):
        return WARN
    return PASS


def format_text(checks: list[Check]) -> str:
    return "\n".join(f"{c.status} {c.name}: {c.detail}" for c in checks)


def main(argv: list[str] | None = None, *, proxy_url: str | None = None) -> int:
    parser = argparse.ArgumentParser(
        prog="mm preflight",
        description="Check whether MetaMate is ready to submit.",
    )
    parser.add_argument(
        "--json",
        action="store_true",
        help="print machine-readable JSON instead of text lines",
    )
    args = parser.parse_args(argv)
    checks = run_checks(proxy_url=proxy_url)
    if args.json:
        payload = {"overall": _overall(checks), "checks": [asdict(c) for c in checks]}
        print(json.dumps(payload, indent=2))
    else:
        print(format_text(checks))
    return 1 if any(c.status == FAIL for c in checks) else 0


if __name__ == "__main__":
    sys.exit(main())
