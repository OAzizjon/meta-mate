# Server: website, proxy, and shutting down JustLog3

Owner's decision (24.10): remove JustLog3 from the VPS and install MetaMate there (or buy a new server). We'll do this on **October 31** (day 6, before the evening tester pilot); until then, MetaMate runs locally using your keys.
I handle deployment and anything involving secrets; agents only review.

## What will run on the server

| Part | What it does |
|---|---|
| Landing page | One page: what it is, video, “Download for Windows” button (file from GitHub Releases), privacy, contact details |
| Proxy `/v1/*` | Meta and Modal keys on the server; invite codes; a daily limit per code and an overall $ limit; no content is stored |
| TLS | Let's Encrypt (certbot), as with JustLog3 now |

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
