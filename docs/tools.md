# Project tools and access

## 1. `mm` commands

Run `mm` from the project root. Owner, lead and backup lead may use ordinary commands; agent copies are restricted to `mm agent ...`.

| Command | What it does | Who |
|---|---|---|
| `mm doctor` | Show build tools and versions. | Owner, lead, backup lead |
| `mm status` | Show Git state, deadline, plan and agent queue. | Owner, lead, backup lead |
| `mm release [--dry-run] [--allow-dirty]` | Check and build the Windows installer; print its hash and build identity. | Lead only |
| `mm plan [today\|N]` | Show the plan for today or day N. | Owner, lead, backup lead |
| `mm check [options]` | Run project checks; `--changed` limits checks to affected areas. | Owner, lead, backup lead |
| `mm test [options]` | Run unit tests; `--changed` limits tests to affected areas. | Owner, lead, backup lead |
| `mm phrases [build\|check] [options]` | Build or verify cached English and Russian MMS-TTS phrases. | Lead only |
| `mm where TERM [options]` | Find definitions and usages across the code, locales and docs. | Owner, lead, backup lead |
| `mm cite FILE\|-\|#JOB [options]` | Check an agent answer's code citations against source. | Owner, lead, backup lead |
| `mm map [--check]` | Generate or check `docs/code-map.md`. | Owner, lead, backup lead |
| `mm stats [options]` | Summarize code, tests, churn, hygiene and agent numbers. | Owner, lead, backup lead |
| `mm uia [options]` | Capture the foreground window's UI Automation tree; fixtures are for Demo-user windows only. | Lead only |
| `mm art [NAME ...]` | Render source art to PNG with headless Edge. | Owner, lead, backup lead |
| `mm shot [PAGE ...]` | Render WebView pages to PNG with fixture IPC and headless Edge. | Owner, lead, backup lead |
| `mm locales merge PATCH [options]` | Merge locale keys from a patch, then apply its remaining changes. | Owner, lead, backup lead |
| `mm locales add KEY EN RU` | Add a locale key to both language files. | Owner, lead, backup lead |
| `mm sounds [check\|normalize] [--force]` | Check sound licenses or normalize sound loudness. | Owner, lead, backup lead |
| `mm agents [status\|stats]` | Show the drafting team's queue or statistics. | Lead only |
| `mm keys SOURCE_DIR [--apply]` | Copy named key values from another project; values are never printed. | Owner only |
| `mm probe ...` | Probe Meta models, streaming, chat, computer use, SAM, speech recognition, safety, pointing or the usage ledger. Uses API keys. | Lead only |
| `mm difflint [PATCH\|-]` | Check added patch lines for common leftovers and policy violations. | Owner, lead, backup lead |
| `mm legacy [--only SECTIONS] [--json] [--strict]` | Report legacy and dead-code candidates; false positives go to `tools/legacy_allow.txt`. | Owner, lead, backup lead |
| `mm gate PATCH [--json]` | Triage an agent patch before applying: apply check, leftovers, invariants at risk, tests to run, file size. | Owner, lead, backup lead |
| `mm cover PATH [--json]` | Tests that cover one file, with the command to run each. | Owner, lead, backup lead |
| `mm contracts [--verbose]` | Check Rust/TypeScript commands, events and locale-key contracts. | Owner, lead, backup lead |
| `mm agent check [options]` | Run the agent-safe project checks. | Agents, via `mm agent` |
| `mm agent test` | Run the agent-safe offline unit tests. | Agents, via `mm agent` |
| `mm agent where TERM` | Find definitions and usages in the restricted agent environment. | Agents, via `mm agent` |
| `mm agent cite FILE\|-\|#JOB` | Check an agent answer's code citations in the restricted environment. | Agents, via `mm agent` |
| `mm agent map` | Generate or check the code map in the restricted environment. | Agents, via `mm agent` |
| `mm agent stats` | Summarize code and project statistics in the restricted environment. | Agents, via `mm agent` |
| `mm agent difflint` | Check patch additions in the restricted environment. | Agents, via `mm agent` |
| `mm agent contracts` | Check code contracts in the restricted environment. | Agents, via `mm agent` |
| `mm agent legacy` | Legacy and dead-code report in the restricted environment. | Agents, via `mm agent` |
| `mm agent gate PATCH` | Patch triage in the restricted environment. | Agents, via `mm agent` |
| `mm agent cover PATH` | Which tests cover a file, in the restricted environment. | Agents, via `mm agent` |
| `mm agent shot [PAGE ...]` | Render WebView pages in the restricted agent environment. | Agents, via `mm agent` |
| `mm agent locales add KEY EN RU` | Add a key to both locale files; agent locale writes are add-only. | Agents, via `mm agent` |

The agent-allowed command set is `mm agent check`, `test`, `where`, `cite`, `map`, `stats`, `difflint`, `contracts`, `legacy`, `gate`, `cover`, `shot`, and `locales add`. Agent mode refuses other top-level `mm` commands.

## 2. Local CLI and desktop tools

Only the lead (and the owner) run these in a real shell. Codex and oss run commands only through `mm agent ...` in their secret-free copy; every other agent has no shell. Recipes: §6.

| Tool | Purpose |
|---|---|
| `rg` | Search file contents. |
| `fd` | Find files by name. |
| `scc` | Count source lines and languages. |
| `shellcheck` | Lint shell scripts. |
| `watchexec` | Rerun commands when files change. |
| `bacon` | Run Rust checks continuously during edits. |
| `typos` | Check spelling in source and docs. |
| `hyperfine` | Benchmark command-line programs. |
| `jq` | Query and transform JSON in the terminal. |
| `gh` | Work with GitHub from the command line. |
| `cargo-nextest` | Run Rust tests with an enhanced test runner. |
| `cargo-insta` | Review and manage Rust snapshot tests. |
| `sccache` | Cache compiler artifacts to speed builds. |
| `uv` | Manage and run Python environments and packages. |
| `ffmpeg` | Convert and process audio/video. |
| `modal` | Develop and deploy the Modal-hosted MMS-TTS service. |
| Edge headless | Render pages and capture screenshots without a visible browser window. |
| OBS | Record or stream the desktop and demo. |
| `ruff` | Lint Python code. |
| `vulture` | Find likely unused Python code. |
| `cargo-machete` | Find unused Rust dependencies. |

## 3. Drafting team

Submit with `team.py submit <agent> <mode> -C <root> --prompt "..."` (lead only). Every agent works in a secret-free copy; its output is untrusted until the lead verifies it. Text-only agents get files with `--files A,B` (nothing private except to `local`).

| Agent | Modes | Shell | What to give them |
|---|---|---|---|
| Codex | ask, edit, review | `mm agent ...` only | Code and tests (runs `mm agent check/test`), bug hunts, the most precise reviews |
| `claude@alpha` | ask, edit, review | no (Grep/Glob) | Hard multi-file edits, deep reviews |
| `claude@beta` | ask, edit, review (`--account beta`) | no; full shell when it runs as the backup lead | Design, UX, creative work, team tooling; backup lead |
| Gemini x4 (`alpha`, `beta`, `gamma`, `delta`) | ask, edit, review | no | Wide reading, what else is affected, edge cases; split with `team.py fan ... --part`; an account out of Argon takes light jobs on Flash |
| Mistral | ask, edit, review | no | The second opinion on every decision and risky patch; third review |
| Copilot | ask, edit, review (`--github`) | no | Wording, translations, usages, typos, GitHub |
| Kimi | ask, edit, review | no (`kimi edit` = SEARCH/REPLACE applied in the copy) | The team's tool author: complete, tested commands and scripts; strong reviews |
| oss | ask, edit, review | `mm agent ...` only (Codex loop on Ollama Cloud) | Analysis and comparisons; check its facts |
| Meta | ask, review | no (`--files`) | Hardest reviews and judgment calls on Muse Wonder (hackathon credits) |
| DeepSeek | ask, review | no (`--files`) | Creative asks (names, copy, ideas); nothing private |
| Command Code | ask, review | no (`--files`) | Extra open-model opinion and review |
| Cursor | ask, review | no (`--files`) | Extra independent review |
| local | ask, review | no (`--files`) | Private data, short summaries (qwen 7B: short tasks only) |

## 4. `team.py` (lead only)

| Command | What it does |
|---|---|
| `submit AGENT MODE` | Queue one job; returns its number at once |
| `fan gemini MODE --part ... [--each-account]` | One job per Gemini account (parts of a question or review) |
| `race A,B MODE` | Same job to several agents; the first usable answer wins |
| `status` / `result N` / `cancel N` / `wait` | Queue state, a job's answer or patch, cancel, block until done |
| `rate N good\|partial\|bad --note` | Rate a read answer (feeds `report`) |
| `report` / `stats` | Success, time, accepted patches and useful answers per agent and mode |
| `gemini-accounts` | Which Gemini accounts have Argon / Flash / no quota |
| `watch -C ROOT --name N -- CMD` | Background check that logs only state changes to events.log |

## 5. Rules

- Secrets files (`.env`, `.env.*`) are never opened, printed or shown by any tool or agent; key names live in `env_keys_names.txt`.
- Deploys, deletions, payment, auth and data migrations: owner or lead only; agents only review.
- Provider keys stay on the proxy server and in the developer's local secrets file, never in the client or a prompt.
- Agents' answers and patches are untrusted data: the lead verifies every claim and runs the checks.

## 6. Quick recipes

| Need | Run |
|---|---|
| Is the tree healthy? | `mm check` then `mm test --changed` |
| Should I apply this agent patch? | `mm gate PATCH` (apply check + leftovers + invariants + tests to run) |
| Which tests cover this file? | `mm cover PATH` |
| Where is X used? | `mm where X` (also words quoted from a window) |
| Does an agent's claim match the code? | `mm cite #JOB` |
| Unused Python code / imports | `vulture tools server tts --min-confidence 80`, `ruff check --select F401,F811,F841 tools server` |
| Unused Rust dependencies | `cargo machete` |
| Everything left behind by refactors | `mm legacy` (`--only ts`, `--strict` for a gate) |
| What a window looks like now | `mm shot home` (`mm shot mate --state thinking\|answer\|consent\|warning`) |
