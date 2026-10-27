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
- **Accepted request shapes and input pricing:** only the shapes sent by the app are accepted: `instructions` is text; `input` is text or a list of user messages containing `input_text`/`input_image` parts and `computer_call_output` items containing a `computer_screenshot`. Text is bounded at one token per UTF-8 byte. A continuation reserves the previous turn's reported input plus output tokens (with an 8,000-token-per-prior-turn floor).
- **Budget and per-invite daily Wonder allowance:** total daily spend across all codes is capped by `PROXY_DAILY_BUDGET_USD`, with a slice reserved strictly for `muse-spark-1.3` so Wonder ($4 / $20 per 1M input/output tokens) can never starve Spark. Inside the non-reserved budget, each invite has a daily Wonder allowance in **both calls and dollars** alongside its overall daily call/dollar quota (and a call-count cap while Spark prices from the Billing page are still unrecorded, D41).
- **Fallback and model header:** if an invite's daily Wonder allowance or the shared budget outside the Spark reserve is exhausted, the proxy serves the turn with `muse-spark-1.3` instead and sets the `X-MetaMate-Model` response header to the model that answered. Meta errors are returned to the client; they do not trigger a retry with Spark.
- **Meta errors and reservations:** an upstream error is returned as `{"error": {"message": "..."}}` with Meta's HTTP status and message scrubbed of the API key. A Meta 4xx is reconciled to zero dollar spend (the reserved call still counts); a 5xx or a stream cut before the final event keeps the whole reservation.
- **Abuse limits** (Mistral #459): per-code cap on concurrent requests and a per-IP rate limit (a leaked code); images at most 1280 px on the long edge and 2 MB, else 413; the cost is reserved atomically before forwarding and a stream cut before completion still pays its reservation (never a refund: aborting must not buy free tokens); ASR at most 60 s of audio per request.
- **Speech synthesis (`POST /v1/tts`):** send `Authorization: Bearer <invite>` and JSON `{"lang":"en"|"ru","text":"..."}`. Text must be nonblank and 1–300 characters; the body is limited to 4 KB (oversized/slow bodies return 413; invalid JSON or fields return 400). The proxy forwards to `MMS_TTS_URL/synthesize` with `X-MMS-Secret: MMS_TTS_SECRET`; the URL must use HTTPS, except HTTP is allowed for `127.0.0.1` or `localhost`. A successful response is mono 16 kHz PCM16 `audio/wav` with the synthesizer's `X-Synth-Ms` timing header when provided. Statuses: 401 unknown invite, 429 when the invite's daily character allowance is exceeded or more than two synthesis requests are in flight, 502 synthesis failure, and 503 when `MMS_TTS_URL` is unset. Tester invites have 10,000 characters/day; judge invites have 30,000. Characters are charged before synthesis, including failed attempts; this allowance is separate from the model budget.
- **Tester vs. judge invites:** **tester** codes carry a one-session daily cap and a small Wonder allowance for the Day-6 pilot and Day-8 recording; **judge** codes (including the pre-filled judge installer code) carry higher daily call and Wonder allowances and draw from the judge/Spark reserve so judging runs never hit a quota error.

A domain is needed (once the rules about using the word “Meta” are known). Domain options will be available after 26.10.

## Local development

Run `C:/mmw/py-proxy/Scripts/python.exe server/run_dev.py --tts` to start the proxy and the MMS-TTS dev server (`tts/serve_dev.py`) on CPU at `127.0.0.1:8788`. Each run generates a shared secret for the proxy-to-TTS request.

Production uvicorn uses `--ws-max-size 65536 --ws-max-queue 4` (also set by `run_dev.py`): an 80 ms audio frame is under 4 KB, while the default 16 MiB per-message limit lets an unauthenticated socket hold excessive memory.

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
