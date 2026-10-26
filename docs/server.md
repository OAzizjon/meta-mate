# Server: website, proxy, and shutting down JustLog3

Owner's decision (24.10): remove JustLog3 from the VPS and install MetaMate there (or buy a new server). We'll do this on **October 31** (day 6, before the evening tester pilot); until then, MetaMate runs locally using your keys.
I handle deployment and anything involving secrets; agents only review.

## What will run on the server

| Part | What it does |
|---|---|
| Landing page | One page: what it is, video, “Download for Windows” button (file from GitHub Releases), privacy, contact details |
| Proxy `/v1/*` | Meta (`https://api.meta.ai/v1`, verified in S4, D41) and Modal keys on the server; model allowlist and token caps; invite codes with separate tester vs. judge limits and a daily Wonder allowance inside `PROXY_DAILY_BUDGET_USD`; no content is stored |
| TLS | Let's Encrypt (certbot), as with JustLog3 now |

Proxy rules (D40, D41):

- **Model allowlist:** `/v1/responses` accepts only `muse-spark-1.3` (default, *Quick*) and `muse-wonder-1.0` (*Thorough*, user-selected in Settings; never switched by voice or MateScript tag); `/v1/asr*` uses `muse-voice-transcribe-1.0`; segmentation uses `sam-3.1`. Intent routing and spoken-line calls are pinned to `muse-spark-1.3`. Every Spark model reasons by default (9.9 s, 1,703 output tokens on S4), so spoken answers use `reasoning.effort: "minimal"` (1.9 s, 131 tokens, D41).
- **Server-side `max_output_tokens` caps:** the proxy clamps `max_output_tokens` on every `/v1/responses` call (intent, spoken MateScript, and computer-use turns) so a client request or default reasoning cannot burn unbounded output tokens.
- **Budget and per-invite daily Wonder allowance:** total daily spend across all codes is capped by `PROXY_DAILY_BUDGET_USD`, with a slice reserved strictly for `muse-spark-1.3` so Wonder ($4 / $20 per 1M input/output tokens) can never starve Spark. Inside the non-reserved budget, each invite has a daily Wonder allowance in **both calls and dollars** alongside its overall daily call/dollar quota (and a call-count cap while Spark prices from the Billing page are still unrecorded, D41).
- **Quiet fallback to Spark + model header:** if an invite's daily Wonder allowance (or the shared budget outside the Spark reserve) is exhausted, or if `muse-wonder-1.0` returns an error or times out, the proxy serves the turn with `muse-spark-1.3` instead and sets the `X-MetaMate-Model` response header to the model that actually answered (`muse-spark-1.3` or `muse-wonder-1.0`) so the debug view shows it.
- **Abuse limits** (Mistral #459): per-code cap on concurrent requests and a per-IP rate limit (a leaked code); images at most 1280 px on the long edge and 2 MB, else 413; the cost is reserved atomically before forwarding and a stream cut before completion still pays its reservation (never a refund: aborting must not buy free tokens); ASR at most 60 s of audio per request; TTS a daily character cap per code, separate from the model budget.
- **Tester vs. judge invites:** **tester** codes carry a one-session daily cap and a small Wonder allowance for the Day-6 pilot and Day-8 recording; **judge** codes (including the pre-filled judge installer code) carry higher daily call and Wonder allowances and draw from the judge/Spark reserve so judging runs never hit a quota error.

A domain is needed (once the rules about using the word “Meta” are known). Domain options will be available after 26.10.

## Shutting down JustLog3: checklist (only after your explicit “yes” that day)

JustLog3 is a live service (NOWPayments payments, Telegram bot, users, SEO, GitHub App). Steps:

1. **Decide what to do with the jl3-cloud.site domain:** keep a “Service paused” page (better for SEO and users), or point it to MetaMate.
2. **Provider snapshot of the whole VPS** before anything is stopped: the rollback path if the MetaMate deploy fails.
3. **Backups** (to your computer, not git):
   - databases (SQLite and PostgreSQL for logs), uploaded files;
   - server settings file with secrets — **you** copy it (I won't open it);
   - nginx configs, systemd units, crontab, TLS certificates and keys.
   Then **test-restore** the database backup into a temporary instance and spot-check a few records.
4. Users, if any: export their data and notify them (email/bot); disable the NOWPayments webhook (IPN), stop the bot, pause the GitHub App.
5. Stop and disable everything of JustLog3: the `justlog3-*` services (`systemctl list-units --type=service`, then `systemctl disable --now`), its cron jobs (`crontab -l`) and containers (`docker ps -a`), if any. **Do not delete** the directories for a week; keep its secrets at `chmod 600` meanwhile.
6. Audit open ports (`ss -tlnp`, firewall): only 22, 80 and 443 remain.
7. Check available resources for the proxy: `free -h`, `df -h`.
8. Install MetaMate (proxy + landing page), nginx vhost, certbot, health check.
9. Test externally: landing page, `/healthz`, proxy with a test invite code.
10. A week later: remove JustLog3's directories.

Rejected (D39): running JustLog3 and MetaMate side by side; JustLog3 is backed up and shut down on Day 6 (D10).

## Modal (TTS)

- MMS-TTS service: one CPU container; keep one container warm on recording and judging days (31.10–08.11: pilot, backup video, recording, judging), otherwise it goes to sleep.
- Secret between the proxy and Modal (`MMS_TTS_SECRET`) to prevent others from using up credits.
- Keep an eye on Modal credits ($100 for selected participants).
