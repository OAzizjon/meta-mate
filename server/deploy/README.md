# Deploying the proxy and the landing page (owner's steps)

The lead prepared everything; the steps that touch the server and its secrets are the
owner's (docs/server.md: agents never deploy). Run them after JustLog3 is off (that day's
explicit "yes", docs/server.md checklist) or on a new Ubuntu server.

## 1. Code and Python

```bash
sudo useradd --system --home /opt/metamate --shell /usr/sbin/nologin metamate
sudo mkdir -p /opt/metamate /etc/metamate
sudo git clone https://github.com/OAzizjon/meta-mate.git /opt/metamate/src   # private: use a deploy key
sudo ln -s /opt/metamate/src/server /opt/metamate/server
sudo ln -s /opt/metamate/src/site /opt/metamate/site
sudo python3 -m venv /opt/metamate/venv
sudo /opt/metamate/venv/bin/pip install -r /opt/metamate/server/requirements.txt
```

## 2. Secrets: `/etc/metamate/proxy.env`

Only names are listed here (values never go into git, prompts or chat; env_keys_names.txt):

```
META_API_KEY=...
PROXY_INVITE_CODES=<judge-code>:judge,<tester-code>:tester
PROXY_DAILY_BUDGET_USD=...
PROXY_PRICES=spark_in,spark_out,wonder_in,wonder_out
MMS_TTS_URL=...        # printed by `python tts/deploy_modal.py` (also in server/.dev-tts.json)
MMS_TTS_SECRET=...
```

```bash
sudo chown root:metamate /etc/metamate/proxy.env && sudo chmod 0640 /etc/metamate/proxy.env
```

The judge code in `PROXY_INVITE_CODES` is the `METAMATE_INVITE` the release is built with
(D51): give it a judge tier and keep `PROXY_DAILY_BUDGET_USD` low enough for the credits.

## 3. Service and site

```bash
sudo cp /opt/metamate/server/deploy/metamate-proxy.service /etc/systemd/system/
sudo systemctl daemon-reload && sudo systemctl enable --now metamate-proxy
sudo cp /opt/metamate/server/deploy/nginx-metamate.conf /etc/nginx/sites-available/metamate
sudo sed -i 's/DOMAIN/your.domain/' /etc/nginx/sites-available/metamate
sudo ln -s /etc/nginx/sites-available/metamate /etc/nginx/sites-enabled/
sudo nginx -t && sudo systemctl reload nginx
sudo certbot --nginx -d your.domain
curl https://your.domain/healthz            # {"ok":true}
```

## 4. The release that talks to it (on the Windows laptop)

```powershell
$env:METAMATE_PROXY_URL = "https://your.domain"
$env:METAMATE_INVITE = "<judge-code>"
.\mm release
```

`mm release` refuses without both variables and prints only the installer's path, size and
SHA-256.
