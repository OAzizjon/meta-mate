#!/usr/bin/env python3
"""Tests for mm_preflight. No network, no git, no gh: all I/O is faked."""

from __future__ import annotations

import hashlib
import io
import json
import subprocess
import sys
import tempfile
import unittest
from contextlib import redirect_stdout
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

import mm_preflight as pf

VERSION = "1.2.3"
PROXY = "https://proxy.example.com"
INSTALLER_REL = (
    Path("target/x86_64-pc-windows-msvc/release/bundle/nsis")
    / f"MetaMate_{VERSION}_x64-setup.exe"
)


class FakeFetch:
    def __init__(self, routes):
        self.routes = dict(routes)
        self.calls = []

    def __call__(self, method, url):
        self.calls.append((method, url))
        result = self.routes[(method, url)]
        if isinstance(result, Exception):
            raise result
        return result


class FakeRunner:
    def __init__(self, routes):
        self.routes = {tuple(k): v for k, v in routes.items()}
        self.calls = []

    def __call__(self, argv):
        self.calls.append(list(argv))
        key = tuple(argv)
        if key not in self.routes:
            raise AssertionError(f"unexpected command: {argv}")
        return self.routes[key]


def resp(status, body=b"{}", headers=None):
    return pf.Response(status, headers or {}, body)


class PreflightBase(unittest.TestCase):
    installer_bytes = b"fake-installer"

    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmp.cleanup)
        self.root = Path(self._tmp.name)
        self._build_good_root()
        sha = hashlib.sha256(self.installer_bytes).hexdigest()
        self.fetch = FakeFetch(
            {
                ("GET", pf.VERSION_URL): resp(
                    200, json.dumps({"version": VERSION}).encode("utf-8")
                ),
                ("HEAD", pf.DOWNLOAD_URL): resp(
                    200, b"", {"content-length": str(len(self.installer_bytes))}
                ),
                ("GET", PROXY + "/healthz"): resp(200, b'{"ok": true}'),
            }
        )
        self.runner = FakeRunner(
            {
                ("gh", "repo", "view", "--json", "visibility"): (
                    0,
                    '{"visibility": "PUBLIC"}',
                    "",
                ),
                ("git", "status", "--porcelain"): (0, "", ""),
                ("git", "rev-parse", "HEAD"): (0, "deadbeef\n", ""),
                ("git", "rev-parse", "origin/main"): (0, "deadbeef\n", ""),
                (
                    "gh",
                    "release", "view", f"v{VERSION}", "--json", "tagName,isDraft",
                ): (0, '{"tagName": "v1.2.3", "isDraft": false}', ""),
            }
        )

    def _build_good_root(self):
        root = self.root
        (root / "src-tauri").mkdir(parents=True)
        (root / "src-tauri" / "tauri.conf.json").write_text(
            json.dumps({"productName": "MetaMate", "version": VERSION}),
            encoding="utf-8",
        )
        (root / "LICENSE").write_text("MIT\n", encoding="utf-8")
        (root / "README.md").write_text(
            "# MetaMate\n\nSee [LICENSE](LICENSE) and [site](https://meta-mate.stream).\n",
            encoding="utf-8",
        )
        (root / "site").mkdir()
        (root / "site" / "version.json").write_text(
            json.dumps(
                {
                    "version": VERSION,
                    "sha256": hashlib.sha256(self.installer_bytes).hexdigest(),
                }
            ),
            encoding="utf-8",
        )
        installer = root / INSTALLER_REL
        installer.parent.mkdir(parents=True)
        installer.write_bytes(self.installer_bytes)
        (root / "docs").mkdir()
        (root / "docs" / "submission.md").write_text(
            "# Submission\n\n- [x] video\n- [x] form\n",
            encoding="utf-8",
        )

    def run_preflight(self, proxy_url=PROXY):
        return pf.run_checks(
            root=self.root,
            fetch=self.fetch,
            runner=self.runner,
            proxy_url=proxy_url,
        )

    @staticmethod
    def by_name(checks, name):
        return next(c for c in checks if c.name == name)


class TestAllPass(PreflightBase):
    def test_all_checks_pass(self):
        checks = self.run_preflight()
        self.assertEqual([c.status for c in checks], [pf.PASS] * 10)

    def test_check_names_in_task_order(self):
        checks = self.run_preflight()
        self.assertEqual(
            [c.name for c in checks],
            [
                "license",
                "repo-visibility",
                "readme",
                "site-version",
                "installer-sha256",
                "download-size",
                "proxy-health",
                "git-state",
                "github-release",
                "submission-doc",
            ],
        )

    def test_overall_and_text_format(self):
        checks = self.run_preflight()
        self.assertEqual(pf._overall(checks), pf.PASS)
        lines = pf.format_text(checks).splitlines()
        self.assertEqual(len(lines), 10)
        self.assertTrue(all(lines[i].startswith("PASS ") for i in range(10)))


class TestFailures(PreflightBase):
    def test_license_missing_fails(self):
        (self.root / "LICENSE").unlink()
        check = self.by_name(self.run_preflight(), "license")
        self.assertEqual(check.status, pf.FAIL)
        self.assertIn("fix:", check.detail)

    def test_repo_private_fails(self):
        self.runner.routes[("gh", "repo", "view", "--json", "visibility")] = (
            0,
            '{"visibility": "PRIVATE"}',
            "",
        )
        check = self.by_name(self.run_preflight(), "repo-visibility")
        self.assertEqual(check.status, pf.FAIL)

    def test_repo_gh_failure_is_warn_not_crash(self):
        self.runner.routes[("gh", "repo", "view", "--json", "visibility")] = (
            1,
            "",
            "network error",
        )
        checks = self.run_preflight()
        self.assertEqual(self.by_name(checks, "repo-visibility").status, pf.WARN)
        self.assertEqual(pf._overall(checks), pf.WARN)

    def test_readme_tbd_fails(self):
        (self.root / "README.md").write_text("# MetaMate\n\nTBD\n", encoding="utf-8")
        self.assertEqual(
            self.by_name(self.run_preflight(), "readme").status, pf.FAIL
        )

    def test_readme_broken_relative_link_fails(self):
        (self.root / "README.md").write_text(
            "# MetaMate\n\n[docs](docs/guide.md) [anchor](#part) "
            "[ext](https://example.com/x) [mail](mailto:a@b.c)\n",
            encoding="utf-8",
        )
        check = self.by_name(self.run_preflight(), "readme")
        self.assertEqual(check.status, pf.FAIL)
        self.assertIn("docs/guide.md", check.detail)

    def test_readme_missing_fails(self):
        (self.root / "README.md").unlink()
        self.assertEqual(
            self.by_name(self.run_preflight(), "readme").status, pf.FAIL
        )

    def test_site_version_mismatch_fails(self):
        self.fetch.routes[("GET", pf.VERSION_URL)] = resp(
            200, json.dumps({"version": "9.9.9"}).encode("utf-8")
        )
        check = self.by_name(self.run_preflight(), "site-version")
        self.assertEqual(check.status, pf.FAIL)

    def test_site_version_http_500_fails(self):
        self.fetch.routes[("GET", pf.VERSION_URL)] = resp(500)
        self.assertEqual(
            self.by_name(self.run_preflight(), "site-version").status, pf.FAIL
        )

    def test_site_version_network_error_is_warn(self):
        self.fetch.routes[("GET", pf.VERSION_URL)] = OSError("timed out")
        self.assertEqual(
            self.by_name(self.run_preflight(), "site-version").status, pf.WARN
        )

    def test_installer_missing_is_warn(self):
        (self.root / INSTALLER_REL).unlink()
        checks = self.run_preflight()
        self.assertEqual(
            self.by_name(checks, "installer-sha256").status, pf.WARN
        )
        self.assertEqual(self.by_name(checks, "download-size").status, pf.WARN)

    def test_installer_hash_mismatch_fails(self):
        (self.root / "site" / "version.json").write_text(
            json.dumps({"version": VERSION, "sha256": "0" * 64}),
            encoding="utf-8",
        )
        self.assertEqual(
            self.by_name(self.run_preflight(), "installer-sha256").status,
            pf.FAIL,
        )

    def test_manifest_missing_fails(self):
        (self.root / "site" / "version.json").unlink()
        self.assertEqual(
            self.by_name(self.run_preflight(), "installer-sha256").status,
            pf.FAIL,
        )

    def test_download_size_mismatch_fails(self):
        self.fetch.routes[("HEAD", pf.DOWNLOAD_URL)] = resp(
            200, b"", {"content-length": "999"}
        )
        self.assertEqual(
            self.by_name(self.run_preflight(), "download-size").status, pf.FAIL
        )

    def test_download_no_content_length_is_warn(self):
        self.fetch.routes[("HEAD", pf.DOWNLOAD_URL)] = resp(200)
        self.assertEqual(
            self.by_name(self.run_preflight(), "download-size").status, pf.WARN
        )

    def test_download_network_error_is_warn(self):
        self.fetch.routes[("HEAD", pf.DOWNLOAD_URL)] = OSError("down")
        self.assertEqual(
            self.by_name(self.run_preflight(), "download-size").status, pf.WARN
        )

    def test_proxy_not_ok_fails(self):
        self.fetch.routes[("GET", PROXY + "/healthz")] = resp(
            200, b'{"ok": false}'
        )
        self.assertEqual(
            self.by_name(self.run_preflight(), "proxy-health").status, pf.FAIL
        )

    def test_proxy_url_missing_is_warn(self):
        checks = pf.run_checks(
            root=self.root, fetch=self.fetch, runner=self.runner
        )
        self.assertEqual(
            self.by_name(checks, "proxy-health").status, pf.WARN
        )

    def test_git_dirty_fails(self):
        self.runner.routes[("git", "status", "--porcelain")] = (
            0,
            " M src-tauri/src/main.rs\n",
            "",
        )
        check = self.by_name(self.run_preflight(), "git-state")
        self.assertEqual(check.status, pf.FAIL)
        self.assertIn("uncommitted", check.detail)

    def test_git_diverged_fails(self):
        self.runner.routes[("git", "rev-parse", "origin/main")] = (
            0,
            "cafe123\n",
            "",
        )
        self.assertEqual(
            self.by_name(self.run_preflight(), "git-state").status, pf.FAIL
        )

    def test_git_error_fails(self):
        self.runner.routes[("git", "status", "--porcelain")] = (
            128,
            "",
            "not a git repository",
        )
        self.assertEqual(
            self.by_name(self.run_preflight(), "git-state").status, pf.FAIL
        )

    def test_release_draft_fails(self):
        self.runner.routes[
            ("gh", "release", "view", f"v{VERSION}", "--json", "tagName,isDraft")
        ] = (0, '{"tagName": "v1.2.3", "isDraft": true}', "")
        check = self.by_name(self.run_preflight(), "github-release")
        self.assertEqual(check.status, pf.FAIL)
        self.assertIn("draft", check.detail)

    def test_release_absent_fails(self):
        self.runner.routes[
            ("gh", "release", "view", f"v{VERSION}", "--json", "tagName,isDraft")
        ] = (1, "", "no releases found")
        self.assertEqual(
            self.by_name(self.run_preflight(), "github-release").status,
            pf.FAIL,
        )

    def test_release_gh_failure_is_warn(self):
        self.runner.routes[
            ("gh", "release", "view", f"v{VERSION}", "--json", "tagName,isDraft")
        ] = (1, "", "authentication failed")
        self.assertEqual(
            self.by_name(self.run_preflight(), "github-release").status,
            pf.WARN,
        )

    def test_submission_unchecked_is_warn(self):
        (self.root / "docs" / "submission.md").write_text(
            "# Submission\n\n- [x] video\n- [ ] form\n",
            encoding="utf-8",
        )
        self.assertEqual(
            self.by_name(self.run_preflight(), "submission-doc").status,
            pf.WARN,
        )

    def test_submission_missing_is_warn(self):
        (self.root / "docs" / "submission.md").unlink()
        checks = self.run_preflight()
        self.assertEqual(
            self.by_name(checks, "submission-doc").status, pf.WARN
        )
        self.assertNotEqual(pf._overall(checks), pf.FAIL)


class TestMain(PreflightBase):
    def test_main_exit_zero_text(self):
        with mock.patch.object(pf, "run_checks", return_value=self.run_preflight()):
            out = io.StringIO()
            with redirect_stdout(out):
                code = pf.main([])
        self.assertEqual(code, 0)
        self.assertIn("PASS license:", out.getvalue())

    def test_main_exit_one_on_fail(self):
        failing = pf.Check("license", pf.FAIL, "no LICENSE; fix: add one")
        with mock.patch.object(
            pf, "run_checks", return_value=[pf.Check("x", pf.PASS, "ok"), failing]
        ):
            out = io.StringIO()
            with redirect_stdout(out):
                code = pf.main([])
        self.assertEqual(code, 1)

    def test_main_json_shape_and_exit(self):
        failing = pf.Check("readme", pf.FAIL, "TBD; fix: fill in")
        warn = pf.Check("submission-doc", pf.WARN, "1 unchecked; fix: do it")
        with mock.patch.object(
            pf, "run_checks", return_value=[failing, warn]
        ):
            out = io.StringIO()
            with redirect_stdout(out):
                code = pf.main(["--json"])
        self.assertEqual(code, 1)
        payload = json.loads(out.getvalue())
        self.assertEqual(payload["overall"], pf.FAIL)
        self.assertEqual(len(payload["checks"]), 2)
        self.assertEqual(
            set(payload["checks"][0]), {"name", "status", "detail"}
        )

    def test_main_json_overall_warn(self):
        warn = pf.Check("proxy-health", pf.WARN, "unreachable; fix: check")
        with mock.patch.object(pf, "run_checks", return_value=[warn]):
            out = io.StringIO()
            with redirect_stdout(out):
                code = pf.main(["--json"])
        self.assertEqual(code, 0)
        self.assertEqual(json.loads(out.getvalue())["overall"], pf.WARN)


class TestDefaultIO(unittest.TestCase):
    def test_fetch_uses_15s_timeout(self):
        class FakeResp:
            status = 200
            headers = {"Content-Length": "2"}

            def read(self):
                return b"{}"

            def __enter__(self):
                return self

            def __exit__(self, *args):
                return False

        with mock.patch.object(pf.urlrequest, "urlopen") as opener:
            opener.return_value = FakeResp()
            response = pf._fetch("GET", "https://example.com/x")
        self.assertEqual(response.status, 200)
        self.assertEqual(
            opener.call_args.kwargs.get("timeout"), pf.TIMEOUT
        )

    def test_fetch_http_error_becomes_response(self):
        error = pf.HTTPError("https://example.com/x", 503, "down", None, None)
        with mock.patch.object(pf.urlrequest, "urlopen", side_effect=error):
            response = pf._fetch("GET", "https://example.com/x")
        self.assertEqual(response.status, 503)

    def test_fetch_url_error_propagates(self):
        with mock.patch.object(
            pf.urlrequest, "urlopen", side_effect=pf.URLError("boom")
        ):
            with self.assertRaises(pf.URLError):
                pf._fetch("GET", "https://example.com/x")

    def test_runner_missing_binary_and_timeout(self):
        with mock.patch.object(
            pf.subprocess, "run", side_effect=FileNotFoundError("gh")
        ):
            rc, _, err = pf._runner(["gh", "--version"])
        self.assertEqual(rc, 127)
        self.assertIn("not found", err)
        with mock.patch.object(
            pf.subprocess,
            "run",
            side_effect=subprocess.TimeoutExpired(cmd=["gh"], timeout=15),
        ):
            rc, _, _ = pf._runner(["gh", "release", "view"])
        self.assertEqual(rc, 124)


if __name__ == "__main__":
    unittest.main()
