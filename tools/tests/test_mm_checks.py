"""Tests for the standalone repository checks."""

from pathlib import Path
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import mm_checks


class CheckTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        # resolve(): on Windows TEMP can be an 8.3 short path, run_checks() resolves the root.
        self.root = Path(self.directory.name).resolve()
        # These fixtures are intentionally outside a Git repository.
        self.git = patch.object(mm_checks, "_git_paths",
                                side_effect=lambda ctx: (list(mm_checks._files(ctx)), set()))
        self.git.start()
        self.addCleanup(self.git.stop)

    def write(self, name, content):
        path = self.root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content, encoding="utf-8")
        return path

    def run_check(self, name, **kwargs):
        return mm_checks.run_checks(self.root, only={name}, **kwargs)[0]

    def test_new_file_secret_is_found_and_masked(self):
        key = "sk-" + "a" * 36
        self.write("new.txt", key)
        result = self.run_check("secrets")
        self.assertFalse(result.ok)
        self.assertEqual(result.details, ["new.txt:1 sk-a…"])
        self.assertNotIn(key, repr(result))

    def test_secret_files_are_never_opened(self):
        names = [".env", ".env.local", "settings.env", "private.pem", "private.key"]
        for name in names:
            self.write(name, "must never be read")
        original = Path.open
        attempted = []

        def guarded_open(path, *args, **kwargs):
            if path.name in names:
                attempted.append(path.name)
                raise AssertionError("Secret file was opened")
            return original(path, *args, **kwargs)

        with patch.object(Path, "open", guarded_open):
            result = self.run_check("secrets")
        self.assertTrue(result.ok)
        self.assertEqual(attempted, [])

    def test_tracked_secret_file_is_reported_without_reading(self):
        path = self.root / ".env.production"
        with patch.object(mm_checks, "_git_paths", return_value=([], {path})):
            result = self.run_check("secrets")
        self.assertFalse(result.ok)
        self.assertIn("must not be committed", result.details[0])

    def test_example_assignment_is_allowed_but_real_assignment_is_not(self):
        token = "z" * 40
        self.write("settings.example", 'META_API_KEY="' + token + '"')
        self.write("settings.txt", 'META_API_KEY="' + token + '"')
        result = self.run_check("secrets")
        self.assertEqual(result.details, ["settings.txt:1 zzzz…"])

    def test_binary_large_and_excluded_files_are_ignored(self):
        key = "sk-" + "b" * 36
        self.write("node_modules/leak.txt", key)
        self.write("large.txt", key + " " * mm_checks.MAX_BYTES)
        (self.root / "binary.dat").write_bytes(b"\0" + key.encode())
        self.assertTrue(self.run_check("secrets").ok)

    def test_env_names_reports_missing_python_and_rust_names(self):
        self.write("env_keys_names.txt", "KNOWN # allowed\n")
        self.write("main.py", 'import os\nos.getenv("PY_MISSING")\nos.environ["KNOWN"]\n')
        self.write("main.rs", 'std::env::var("RS_MISSING");\noption_env!("KNOWN");\n')
        result = self.run_check("env-names")
        self.assertFalse(result.ok)
        self.assertEqual(result.details, ["main.py:2 PY_MISSING", "main.rs:1 RS_MISSING"])

    def test_env_names_supports_all_requested_forms(self):
        self.write("env_keys_names.txt", "")
        self.write("main.py", 'os.environ.get("ONE")\nos.environ["TWO"]\nos.getenv("THREE")')
        self.write("main.rs", 'env::var("FOUR"); option_env!("FIVE"); env!("SIX");')
        self.write("main.ts", "process.env.SEVEN; import.meta.env.EIGHT;")
        result = self.run_check("env-names")
        self.assertEqual(len(result.details), 8)

    def test_env_names_reports_rust_var_os(self):
        self.write("env_keys_names.txt", "KNOWN\n")
        self.write("main.rs", 'std::env::var_os("STD_MISSING");\n'
                   'env::var_os("SHORT_MISSING");\nenv::var_os("KNOWN");\n')
        result = self.run_check("env-names")
        self.assertFalse(result.ok)
        self.assertEqual(result.details, ["main.rs:1 STD_MISSING", "main.rs:2 SHORT_MISSING"])

    def test_env_names_accepts_build_variables_where_cargo_and_vite_set_them(self):
        names = ("CARGO_TARGET_DIR", "CARGO_HOME", "RUSTC_WRAPPER", "RUSTFLAGS", "RUST_LOG",
                 "RUST_BACKTRACE", "CARGO_MANIFEST_DIR", "CARGO_PKG_NAME", "CARGO_PKG_VERSION",
                 "CARGO_PKG_VERSION_MAJOR", "CARGO_CRATE_NAME", "CARGO_BIN_NAME",
                 "OUT_DIR", "TARGET", "HOST", "PROFILE", "OPT_LEVEL", "DEBUG", "NUM_JOBS",
                 "CARGO_CFG_TARGET_OS", "CARGO_FEATURE_TRAY", "TAURI_ENV_PLATFORM")
        self.write("main.rs", "\n".join(f'env!("{name}");' for name in names)
                   + '\nenv!("APP_SETTING");\nenv!("TAURI_ENVIRONMENT");\noption_env!("DEP_OPENSSL_ROOT");\n')
        self.write("main.ts", "\n".join(f"import.meta.env.{name};"
                   for name in ("MODE", "DEV", "PROD", "SSR", "BASE_URL", "TAURI_ENV_DEBUG")))
        self.write("vite.config.ts", "const host = process.env.TAURI_DEV_HOST;\n")
        result = self.run_check("env-names")
        self.assertFalse(result.ok)
        self.assertEqual(result.details, [f"main.rs:{len(names) + 1} APP_SETTING",
                                          f"main.rs:{len(names) + 2} TAURI_ENVIRONMENT",
                                          f"main.rs:{len(names) + 3} DEP_OPENSSL_ROOT"])

    def test_env_names_build_variables_are_configuration_elsewhere(self):
        self.write("Cargo.toml", "")
        self.write("build.rs", 'std::env::var("OUT_DIR");\nstd::env::var("PROFILE");\n'
                   'std::env::var("DEP_OPENSSL_ROOT");\nstd::env::var("CARGO_CFG_TARGET_OS");\n')
        self.write("build/main.rs", 'std::env::var("TARGET");\n')
        # Read by the running app, these are real settings and must be listed.
        self.write("app.rs", 'std::env::var("DEBUG");\nenv::var_os("HOST");\nstd::env::var("RUST_LOG");\n'
                   'std::env::var("CARGO_PKG_NAME");\nstd::env::var("TAURI_ENV_PLATFORM");\n')
        # A module named build.rs inside src/ is app code, not Cargo's build script.
        self.write("src/build.rs", 'std::env::var("PROFILE");\n')
        self.write("tool.py", 'import os\nos.getenv("DEBUG")\nos.environ["HOST"]\n'
                   'os.environ.get("CARGO_TARGET_DIR")\n')
        self.write("web.ts", "process.env.HOST;\nimport.meta.env.HOST;\nprocess.env.MODE;\n"
                   "import.meta.env.TAURI_DEV_HOST;\nprocess.env.TAURI_DEV_HOST;\nprocess.env.NODE_ENV;\n")
        result = self.run_check("env-names")
        self.assertFalse(result.ok)
        self.assertCountEqual(result.details, [
            "app.rs:1 DEBUG", "app.rs:2 HOST", "app.rs:4 CARGO_PKG_NAME", "app.rs:5 TAURI_ENV_PLATFORM",
            "src/build.rs:1 PROFILE", "tool.py:2 DEBUG", "tool.py:3 HOST",
            "web.ts:1 HOST", "web.ts:2 HOST", "web.ts:3 MODE", "web.ts:4 TAURI_DEV_HOST",
            "web.ts:5 TAURI_DEV_HOST"])

    def test_env_names_sees_other_read_forms(self):
        self.write("app.rs", 'std::env!("R1");\ncore::option_env!("R2");\nenv!(r"R3");\n'
                   '::std::env::var("R4");\nenv !("R5");\n')
        self.write("web.ts", 'process.env["W1"];\nimport.meta.env[\'W2\'];\nprocess.env?.W3;\n'
                   'process.env?.["W4"];\n')
        self.write("tool.py", 'import os\nfrom os import getenv, environ as env\ngetenv("P1")\n'
                   'env["P2"]\nos.environ.pop("P3")\nos.environ.setdefault("P4", "1")\n'
                   '"P5" in os.environ\nenv.get("P6")\n"marker" == "P7" in os.environ\n')
        result = self.run_check("env-names")
        self.assertCountEqual(result.details, [
            "app.rs:1 R1", "app.rs:2 R2", "app.rs:3 R3", "app.rs:4 R4", "app.rs:5 R5",
            "web.ts:1 W1", "web.ts:2 W2", "web.ts:3 W3", "web.ts:4 W4",
            "tool.py:3 P1", "tool.py:4 P2", "tool.py:5 P3", "tool.py:6 P4", "tool.py:7 P5", "tool.py:8 P6",
            "tool.py:9 P7"])

    def test_env_names_skips_when_there_is_no_code(self):
        result = self.run_check("env-names")
        self.assertTrue(result.ok)
        self.assertTrue(result.skipped)

    def test_python_examples_and_comments_are_not_environment_reads(self):
        self.write("main.py", '# os.getenv("COMMENT")\nexample = \'os.getenv("EXAMPLE")\'\n')
        self.assertTrue(self.run_check("env-names").ok)

    def test_markdown_links_check_files_but_ignore_fragments(self):
        self.write("docs/existing.md", "# Heading\n")
        self.write("README.md", "[ok](docs/existing.md#heading)\n[bad](missing.md)\n"
                   "[anchor](#heading) [remote](https://example.com/missing)\n")
        result = self.run_check("md-links")
        self.assertFalse(result.ok)
        self.assertEqual(result.details, ["README.md:2 missing.md"])

    def test_markdown_reference_links_and_encoded_spaces(self):
        self.write("docs/has space.md", "hello")
        self.write("notes/page.md", "[good](../docs/has%20space.md#part)\n"
                   "[bad][ref]\n[ref]: missing.md\n")
        result = self.run_check("md-links")
        self.assertEqual(result.details, ["notes/page.md:3 missing.md"])

    def test_rust_and_web_skip_without_manifests(self):
        results = mm_checks.run_checks(self.root, full=True, only={"rust", "web"})
        self.assertEqual(len(results), 2)
        self.assertTrue(all(result.ok and result.skipped for result in results))

    def test_missing_tool_is_skipped(self):
        with patch.object(mm_checks.shutil, "which", return_value=None):
            result = self.run_check("typos")
        self.assertTrue(result.ok)
        self.assertTrue(result.skipped)

    def test_web_runs_only_defined_scripts(self):
        self.write("package.json", '{"scripts": {"typecheck": "tsc"}}')
        with patch.object(mm_checks.shutil, "which", return_value="npm"), \
                patch.object(mm_checks._Context, "command") as command:
            command.return_value = subprocess.CompletedProcess([], 0, b"", b"")
            result = self.run_check("web")
        self.assertTrue(result.ok)
        command.assert_called_once_with(["npm", "run", "--silent", "typecheck"])

    def test_web_build_runs_only_in_full_checks_when_defined(self):
        for full in (False, True):
            for build in (False, True):
                scripts = '"typecheck": "tsc", "lint": "eslint"'
                if build:
                    scripts += ', "build": "vite build"'
                self.write("package.json", '{"scripts": {' + scripts + '}}')
                with self.subTest(full=full, build=build), \
                        patch.object(mm_checks.shutil, "which", return_value="npm"), \
                        patch.object(mm_checks._Context, "command") as command:
                    command.return_value = subprocess.CompletedProcess([], 0, b"", b"")
                    result = self.run_check("web", full=full)
                    self.assertTrue(result.ok)
                    expected = ["typecheck", "lint"]
                    if full and build:
                        expected.append("build")
                    self.assertEqual([call.args[0] for call in command.call_args_list],
                                     [["npm", "run", "--silent", name] for name in expected])

    def test_full_rust_falls_back_to_cargo_test(self):
        self.write("Cargo.toml", "")
        with patch.object(mm_checks.shutil, "which",
                          side_effect=lambda name: None if name == "cargo-nextest" else name), \
                patch.object(mm_checks._Context, "command") as command:
            command.return_value = subprocess.CompletedProcess([], 0, b"", b"")
            result = self.run_check("rust", full=True)
        self.assertTrue(result.ok)
        self.assertEqual(command.call_args_list[-1].args[0], ["cargo", "test", "-q"])
        self.assertEqual(command.call_count, 3)

    def test_git_status_handles_rename_and_untracked_paths(self):
        ctx = mm_checks._Context(self.root, False, time.monotonic() + 120)
        replies = [subprocess.CompletedProcess([], 0, b"true\n", b""),
                   subprocess.CompletedProcess([], 0, str(self.root).encode() + b"\n", b""),
                   subprocess.CompletedProcess([], 0, b"R  new.txt\0old.txt\0?? fresh.txt\0", b""),
                   subprocess.CompletedProcess([], 0, b"new.txt\0.env\0", b"")]
        self.git.stop()
        with patch.object(mm_checks._Context, "command", side_effect=replies):
            changed, tracked = mm_checks._git_paths(ctx)
        self.assertEqual(changed, [self.root / "new.txt", self.root / "fresh.txt"])
        self.assertIn(self.root / ".env", tracked)

    def test_git_status_paths_are_relative_to_repository_top(self):
        subdirectory = self.root / "sub"
        subdirectory.mkdir()
        ctx = mm_checks._Context(subdirectory, False, time.monotonic() + 120)
        replies = [subprocess.CompletedProcess([], 0, b"true\n", b""),
                   subprocess.CompletedProcess([], 0, str(self.root).encode() + b"\n", b""),
                   subprocess.CompletedProcess([], 0, b"?? sub/new.txt\0", b""),
                   subprocess.CompletedProcess([], 0, b".env\0", b"")]
        self.git.stop()
        with patch.object(mm_checks._Context, "command", side_effect=replies):
            changed, tracked = mm_checks._git_paths(ctx)
        self.assertEqual(changed, [subdirectory / "new.txt"])
        self.assertEqual(tracked, {subdirectory / ".env"})

    def test_check_timeout_fails_without_diagnostics_leaking(self):
        with patch.dict(mm_checks.CHECKS, {"secrets": lambda ctx: ctx.command(["fake"])}), \
                patch.object(mm_checks._Context, "command",
                             side_effect=subprocess.TimeoutExpired("fake", 120)):
            result = self.run_check("secrets")
        self.assertFalse(result.ok)
        self.assertIn("Timed out", result.summary)

    def test_unknown_check_is_rejected(self):
        with self.assertRaises(ValueError):
            mm_checks.run_checks(self.root, only={"unknown"})


if __name__ == "__main__":
    unittest.main()
