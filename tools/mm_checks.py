"""Fast repository checks for the mm CLI."""

from __future__ import annotations

import argparse
import ast
from concurrent.futures import ThreadPoolExecutor, TimeoutError as FutureTimeout
from dataclasses import dataclass
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import time
from urllib.parse import unquote, urlsplit


TIMEOUT = 120
MAX_BYTES = 2 * 1024 * 1024
EXCLUDED = {".git", "target", "node_modules", "dist", ".draft-input", "__pycache__"}
CODE_SUFFIXES = {".py", ".rs", ".ts", ".tsx", ".js", ".jsx", ".mjs", ".cjs"}
SECRET_PATTERN = re.compile(
    r"\b(?:sk-(?:or-v1-)?[A-Za-z0-9_-]{16,}|AIza[A-Za-z0-9_-]{20,}"
    r"|gh[pousr]_[A-Za-z0-9_]{16,}|github_pat_[A-Za-z0-9_]{16,}"
    r"|gsk_[A-Za-z0-9_-]{16,}|xox[bap]-[A-Za-z0-9-]{10,}"
    r"|\d{5,}:AA[A-Za-z0-9_-]{20,})\b|(?:-----)?BEGIN [A-Z0-9 ]*PRIVATE KEY"
)
ASSIGNMENT = re.compile(
    r"\b(?:META_API_KEY|[A-Z][A-Z0-9_]*_(?:SECRET|TOKEN))\b"
    r"[\s\"']*[:=]\s*[\"']?([A-Za-z0-9_./+\-=]{32,})"
)
ENV_PATTERNS = {
    ".py": re.compile(
        r"\bos\.(?:environ\s*\[\s*|(?:environ\.get|getenv)\s*\(\s*)"
        r"[\"']([A-Za-z_][A-Za-z0-9_]*)[\"']"
    ),
    ".rs": re.compile(
        r"\b(?P<form>(?:(?:std|core)::)?(?:env::var(?:_os)?\s*\(|(?:option_env|env)\s*!\s*\())\s*"
        r'r?#*"(?P<name>[A-Za-z_][A-Za-z0-9_]*)"'
    ),
}
# process.env.X, process.env?.X, process.env["X"], import.meta.env['X'].
WEB_ENV = re.compile(
    r"\b(?P<form>process\.env|import\.meta\.env)"
    r"(?:\??\.(?P<dot>[A-Za-z_][A-Za-z0-9_]*)|(?:\?\.)?\[\s*(?P<q>[\"'`])(?P<key>[A-Za-z_][A-Za-z0-9_]*)(?P=q)\s*\])"
)
# System and tooling variables are not configuration, so not in env_keys_names.txt.
SYSTEM_ENV = {
    "PATH", "PATHEXT", "HOME", "USERPROFILE", "APPDATA", "LOCALAPPDATA", "TEMP", "TMP", "TMPDIR",
    "ProgramFiles", "ProgramFiles(x86)", "ProgramData", "SystemRoot", "ComSpec", "USERNAME", "CI",
    "MM_AGENT", "CARGO_NET_OFFLINE", "NPM_CONFIG_OFFLINE", "MM_FIXTURES_REVIEWED",
    "CARGO_TARGET_DIR", "CARGO_BUILD_JOBS", "CARGO_HOME", "RUSTC_WRAPPER", "RUSTFLAGS", "RUST_LOG", "RUST_BACKTRACE",
}
# Cargo defines these for build scripts and at compile time (env!, option_env!). Names like
# DEBUG or HOST read by the running app are real configuration, so they count only there.
CARGO_BUILD_ENV = {
    "CARGO", "CARGO_MANIFEST_DIR", "CARGO_MANIFEST_PATH", "CARGO_MANIFEST_LINKS", "CARGO_CRATE_NAME",
    "CARGO_BIN_NAME", "CARGO_PRIMARY_PACKAGE", "CARGO_TARGET_TMPDIR", "CARGO_MAKEFLAGS",
    "CARGO_ENCODED_RUSTFLAGS", "OUT_DIR", "TARGET", "HOST", "PROFILE", "OPT_LEVEL", "DEBUG",
    "NUM_JOBS", "RUSTC", "RUSTDOC", "RUSTC_LINKER",
}
CARGO_BUILD_PREFIXES = ("CARGO_PKG_", "CARGO_CFG_", "CARGO_FEATURE_", "CARGO_BIN_EXE_")
# The Tauri v2 CLI sets these while it runs cargo and the frontend build (beforeDevCommand).
TAURI_BUILD_ENV = {
    "TAURI_ENV_DEBUG", "TAURI_ENV_TARGET_TRIPLE", "TAURI_ENV_ARCH", "TAURI_ENV_PLATFORM",
    "TAURI_ENV_PLATFORM_VERSION", "TAURI_ENV_FAMILY", "TAURI_ENV_PLATFORM_TYPE",
}
# Vite's import.meta.env built-ins. The v2 template reads process.env.TAURI_DEV_HOST in vite.config.ts.
VITE_BUILTINS = {"MODE", "DEV", "PROD", "SSR", "BASE_URL"}
TAURI_CLI_ENV = {"TAURI_DEV_HOST"}


def _is_build_script(path: Path) -> bool:
    """Cargo's build script: build.rs next to Cargo.toml, or a module of a build/ folder there."""
    if path.name == "build.rs":
        return (path.parent / "Cargo.toml").is_file()
    return path.parent.name == "build" and (path.parent.parent / "Cargo.toml").is_file()


def _is_system_env(name: str, path: Path, form: str) -> bool:
    if name in SYSTEM_ENV:
        return True
    if path.suffix == ".rs":
        build_script = _is_build_script(path)
        if not (build_script or "!" in form):
            return False
        return (name in CARGO_BUILD_ENV or name in TAURI_BUILD_ENV or name.startswith(CARGO_BUILD_PREFIXES)
                or (build_script and name.startswith("DEP_")))
    if form.startswith("import.meta"):
        return name in VITE_BUILTINS or name in TAURI_BUILD_ENV
    if form.startswith("process"):
        # Node sets NODE_ENV; the Tauri variables exist only while the Vite config runs.
        return name == "NODE_ENV" or (path.name.startswith("vite.config.")
                                      and (name in TAURI_CLI_ENV or name in TAURI_BUILD_ENV))
    return False
MD_LINK = re.compile(r"!?\[[^\]\n]*\]\(\s*(<[^>]*>|[^\s)]+)(?:\s+[^)]*)?\)")
MD_REFERENCE = re.compile(r"^\s{0,3}\[[^\]]+\]:\s*(<[^>]*>|\S+)", re.MULTILINE)


@dataclass
class CheckResult:
    name: str
    ok: bool
    skipped: bool
    summary: str
    details: list[str]
    seconds: float


@dataclass
class _Context:
    root: Path
    full: bool
    deadline: float

    def remaining(self) -> float:
        remaining = self.deadline - time.monotonic()
        if remaining <= 0:
            raise TimeoutError
        return remaining

    def command(self, args: list[str]) -> subprocess.CompletedProcess:
        # Resolve executables explicitly so npm.cmd works on Windows without a shell.
        executable = shutil.which(args[0])
        if executable is None:
            raise FileNotFoundError(args[0])
        return subprocess.run(
            [executable, *args[1:]], cwd=self.root, capture_output=True,
            timeout=self.remaining(), check=False,
        )


def _result(name: str, summary: str, details: list[str] | None = None,
            *, skipped: bool = False) -> CheckResult:
    return CheckResult(name, not details, skipped, summary, details or [], 0.0)


def _protected(path: Path) -> bool:
    name = path.name.lower()
    if name.endswith(".example"):
        return False
    return (name == ".env" or name.startswith(".env.")
            or name.endswith((".env", ".pem", ".key")))


def _allowed(root: Path, path: Path) -> bool:
    try:
        relative = path.relative_to(root)
    except ValueError:
        return False
    # Only links inside the project matter; a junction above it (a redirected
    # profile folder) must not hide every file from the checks.
    inside = [root.joinpath(*relative.parts[:i]) for i in range(1, len(relative.parts) + 1)]
    return (not relative.is_absolute() and ".." not in relative.parts
            and not any(part.lower() in {p.lower() for p in EXCLUDED} for part in relative.parts)
            and not any(p.is_symlink() or p.is_junction() for p in inside))


def _files(ctx: _Context):
    for folder, directories, names in os.walk(ctx.root, followlinks=False):
        ctx.remaining()
        directories[:] = sorted(d for d in directories if d not in EXCLUDED
                                 and not (Path(folder) / d).is_symlink())
        for name in sorted(names):
            path = Path(folder) / name
            if not path.is_symlink():
                yield path


def _read(ctx: _Context, path: Path) -> str | None:
    ctx.remaining()
    # This guard applies to every in-process reader, not just the secret scan.
    if _protected(path) or not _allowed(ctx.root, path):
        return None
    if not path.is_file() or path.stat().st_size > MAX_BYTES:
        return None
    with path.open("rb") as stream:
        data = stream.read(MAX_BYTES + 1)
    if len(data) > MAX_BYTES or b"\0" in data:
        return None
    try:
        return data.decode("utf-8-sig")
    except UnicodeDecodeError:
        return None


def _git_paths(ctx: _Context) -> tuple[list[Path], set[Path]]:
    try:
        repo = ctx.command(["git", "rev-parse", "--is-inside-work-tree"])
    except FileNotFoundError:
        return list(_files(ctx)), set()
    if repo.returncode or repo.stdout.strip() != b"true":
        return list(_files(ctx)), set()
    top = ctx.command(["git", "rev-parse", "--show-toplevel"])
    if top.returncode:
        raise RuntimeError("Git root discovery failed")
    repo_root = Path(os.fsdecode(top.stdout.rstrip(b"\r\n")))
    status = ctx.command(["git", "status", "--porcelain", "-z", "--untracked-files=all", "--", "."])
    tracked = ctx.command(["git", "ls-files", "-z", "--", "."])
    if status.returncode or tracked.returncode:
        raise RuntimeError("Git file discovery failed")
    paths = []
    records = iter(status.stdout.split(b"\0"))
    for record in records:
        if not record:
            continue
        flags = record[:2]
        path = repo_root / os.fsdecode(record[3:])
        if b"R" in flags or b"C" in flags:
            next(records, None)  # Porcelain -z puts the destination first.
        if flags != b"!!" and _allowed(ctx.root, path):
            paths.append(path)
    committed = {ctx.root / os.fsdecode(p) for p in tracked.stdout.split(b"\0") if p}
    return paths, committed


def check_secrets(ctx: _Context) -> CheckResult:
    paths, tracked = _git_paths(ctx)
    details = [f"{p.relative_to(ctx.root).as_posix()}: must not be committed"
               for p in sorted(tracked) if _allowed(ctx.root, p) and _protected(p)]
    for path in paths:
        content = _read(ctx, path)
        if content is None:
            continue
        example = ".example" in path.name.lower()
        for number, line in enumerate(content.splitlines(), 1):
            values = [m.group() for m in SECRET_PATTERN.finditer(line)]
            if not example:
                values.extend(m.group(1) for m in ASSIGNMENT.finditer(line))
            for value in dict.fromkeys(values):
                details.append(f"{path.relative_to(ctx.root).as_posix()}:{number} {value[:4]}…")
    return _result("secrets", f"{len(details)} potential secrets", details)


def _python_env_names(content: str):
    try:
        tree = ast.parse(content)
    except SyntaxError:
        # Incomplete new files should still be checked before they compile.
        for match in ENV_PATTERNS[".py"].finditer(content):
            yield match.group(1), content.count("\n", 0, match.start()) + 1
        return
    environs, getters = {"os.environ"}, {"os.getenv"}
    # `from os import getenv, environ as env` first: the reads may come before the import in the walk.
    for node in ast.walk(tree):
        if isinstance(node, ast.ImportFrom) and node.module == "os":
            for alias in node.names:
                if alias.name == "environ":
                    environs.add(alias.asname or alias.name)
                elif alias.name == "getenv":
                    getters.add(alias.asname or alias.name)
    getters |= {f"{e}.{method}" for e in environs for method in ("get", "pop", "setdefault")}
    for node in ast.walk(tree):
        keys = []
        if isinstance(node, ast.Subscript) and ast.unparse(node.value) in environs:
            keys = [node.slice]
        elif isinstance(node, ast.Call) and node.args and ast.unparse(node.func) in getters:
            keys = [node.args[0]]
        elif isinstance(node, ast.Compare):
            # Chained: in `a == "X" in os.environ` the key is the operand right before `in`.
            operands = [node.left, *node.comparators]
            keys = [operands[i] for i, op in enumerate(node.ops)
                    if isinstance(op, (ast.In, ast.NotIn)) and ast.unparse(operands[i + 1]) in environs]
        for key in keys:
            if isinstance(key, ast.Constant) and isinstance(key.value, str):
                yield key.value, node.lineno


def check_env_names(ctx: _Context) -> CheckResult:
    paths = [p for p in _files(ctx) if p.suffix in CODE_SUFFIXES]
    if not paths:
        return _result("env-names", "No code files", skipped=True)
    names_file = ctx.root / "env_keys_names.txt"
    content = _read(ctx, names_file) or ""
    allowed = {name for line in content.splitlines() if (name := line.split("#", 1)[0].strip())}
    details = []
    for path in paths:
        content = _read(ctx, path)
        if content is None:
            continue
        if path.suffix == ".py":
            reads = [(name, line, "") for name, line in
                     sorted(_python_env_names(content), key=lambda item: item[1])]
        else:
            pattern = ENV_PATTERNS.get(path.suffix, WEB_ENV)
            reads = []
            for match in pattern.finditer(content):
                found = match.groupdict()
                name = found.get("name") or found.get("dot") or found.get("key")
                reads.append((name, content.count("\n", 0, match.start()) + 1, match.group("form")))
        for name, line, form in reads:
            if name not in allowed and not _is_system_env(name, path, form):
                details.append(f"{path.relative_to(ctx.root).as_posix()}:{line} {name}")
    return _result("env-names", f"{len(details)} unlisted environment names", details)


def check_python(ctx: _Context) -> CheckResult:
    roots = ("server", "tts", "tools", "inference")
    venv_names = {".venv", "venv", "env", "virtualenv"}
    files = []
    for name in roots:
        folder = ctx.root / name
        if not folder.is_dir():
            continue
        for current, directories, names in os.walk(folder, followlinks=False):
            ctx.remaining()
            current_path = Path(current)
            directories[:] = sorted(
                directory for directory in directories
                if directory != "__pycache__"
                and directory.lower() not in venv_names
                and not (current_path / directory).is_symlink()
                and not (current_path / directory / "pyvenv.cfg").is_file()
            )
            files.extend(current_path / filename for filename in sorted(names)
                         if filename.endswith(".py")
                         and not (current_path / filename).is_symlink())
    if not files:
        return _result("python", "No Python files", skipped=True)

    details = []
    for path in files:
        ctx.remaining()
        try:
            compile(path.read_bytes(), str(path), "exec")
        except SyntaxError as error:
            line = error.lineno or 1
            details.append(f"{path.relative_to(ctx.root).as_posix()}:{line} {error.msg}")
    return _result("python", f"{len(details)} Python files failed to compile", details)


def check_md_links(ctx: _Context) -> CheckResult:
    details = []
    for path in _files(ctx):
        relative = path.relative_to(ctx.root)
        if path.suffix != ".md" or not (relative.as_posix() in {"CLAUDE.md", "README.md"}
                                       or relative.parts[0] in {"docs", "notes"}):
            continue
        content = _read(ctx, path)
        if content is None:
            continue
        for pattern in (MD_LINK, MD_REFERENCE):
            for match in pattern.finditer(content):
                target = match.group(1).strip("<>")
                parsed = urlsplit(target)
                if parsed.scheme or parsed.netloc or not parsed.path:
                    continue
                if not (path.parent / unquote(parsed.path)).exists():
                    line = content.count("\n", 0, match.start()) + 1
                    details.append(f"{relative.as_posix()}:{line} {target}")
    return _result("md-links", f"{len(details)} broken relative links", details)


def _commands(ctx: _Context, name: str, commands: list[list[str]]) -> CheckResult:
    if not commands:
        return _result(name, "No scripts defined", skipped=True)
    for command in commands:
        if shutil.which(command[0]) is None:
            return _result(name, f"{command[0]} is not on PATH", skipped=True)
    for command in commands:
        completed = ctx.command(command)
        if completed.returncode:
            output = (completed.stdout + completed.stderr).decode("utf-8", errors="replace")
            # Tool diagnostics can include source lines containing credentials.
            output = SECRET_PATTERN.sub(lambda m: m.group()[:4] + "…", output)
            output = ASSIGNMENT.sub(lambda m: m.group()[:m.start(1) - m.start()]
                                    + m.group(1)[:4] + "…", output)
            return _result(name, f"{' '.join(command)} failed", output.splitlines() or ["No diagnostics"])
    return _result(name, f"{len(commands)} command(s) passed")


def check_typos(ctx: _Context) -> CheckResult:
    return _commands(ctx, "typos", [["typos", "--format", "brief"]])


def check_rust(ctx: _Context) -> CheckResult:
    if not (ctx.root / "Cargo.toml").is_file():
        return _result("rust", "No Cargo.toml", skipped=True)
    commands = [["cargo", "fmt", "--all", "--", "--check"],
                ["cargo", "clippy", "--all-targets", "-q", "--", "-D", "warnings"]]
    if ctx.full:
        commands.append(["cargo", "nextest", "run"] if shutil.which("cargo-nextest")
                        else ["cargo", "test", "-q"])
    return _commands(ctx, "rust", commands)


def check_web(ctx: _Context) -> CheckResult:
    manifest = ctx.root / "package.json"
    if not manifest.is_file():
        return _result("web", "No package.json", skipped=True)
    package = json.loads(_read(ctx, manifest) or "{}")
    scripts = package.get("scripts", {})
    names = ("typecheck", "lint", "build") if ctx.full else ("typecheck", "lint")
    return _commands(ctx, "web", [["npm", "run", "--silent", script]
                                 for script in names if script in scripts])


CHECKS = {"secrets": check_secrets, "env-names": check_env_names,
          "md-links": check_md_links, "typos": check_typos, "python": check_python,
          "rust": check_rust, "web": check_web}


def _run(name: str, root: Path, full: bool) -> CheckResult:
    started = time.monotonic()
    ctx = _Context(root, full, started + TIMEOUT)
    try:
        result = CHECKS[name](ctx)
        ctx.remaining()
    except (TimeoutError, subprocess.TimeoutExpired):
        result = _result(name, f"Timed out after {TIMEOUT} s", ["Check deadline exceeded"])
    except FileNotFoundError:
        result = _result(name, "Required tool is not available", skipped=True)
    except Exception as error:
        # Exception messages may include secret file contents; expose only the type.
        result = _result(name, "Check could not complete", [type(error).__name__])
    result.seconds = time.monotonic() - started
    return result


def run_checks(root: Path, *, full: bool = False,
               only: set[str] | None = None) -> list[CheckResult]:
    if only is not None and only - CHECKS.keys():
        raise ValueError(f"Unknown checks: {', '.join(sorted(only - CHECKS.keys()))}")
    names = [name for name in CHECKS if only is None or name in only]
    if not names:
        return []
    executor = ThreadPoolExecutor(max_workers=len(names))
    started = time.monotonic()
    futures = {name: executor.submit(_run, name, root.resolve(), full) for name in names}
    results = []
    try:
        for name, future in futures.items():
            try:
                results.append(future.result(timeout=max(0, started + TIMEOUT - time.monotonic()) + 0.1))
            except FutureTimeout:
                results.append(CheckResult(name, False, False, f"Timed out after {TIMEOUT} s",
                                           ["Check deadline exceeded"], float(TIMEOUT)))
    finally:
        executor.shutdown(wait=False, cancel_futures=True)
    return results


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--full", action="store_true")
    parser.add_argument("--only", help="Comma-separated check names")
    args = parser.parse_args()
    only = set(args.only.split(",")) if args.only else None
    if only and only - CHECKS.keys():
        parser.error(f"Unknown checks: {', '.join(sorted(only - CHECKS.keys()))}")
    results = run_checks(Path(__file__).resolve().parent.parent, full=args.full, only=only)
    for result in results:
        status = "SKIP" if result.skipped else "OK" if result.ok else "FAIL"
        print(f"{status} {result.name} {result.seconds:.2f}s {result.summary}")
    for result in results:
        if not result.ok:
            for detail in result.details:
                print(f"  {detail}")
    raise SystemExit(1 if any(not result.ok for result in results) else 0)
