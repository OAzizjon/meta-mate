# Code signing: why Windows warns, and how to stop it

Checked 2026-11-02 on the 0.1.0 installer: Microsoft Defender finds no threat
(`MpCmdRun -Scan -ScanType 3`), the site serves it as `application/octet-stream` with
`Content-Disposition: attachment` over HTTPS. The warnings people see are about **reputation
and an unsigned file**, not a detection:

- **Edge/Chrome**: "MetaMate-setup.exe isn't commonly downloaded" (a new file with few downloads).
- **SmartScreen** at first run: "Windows protected your PC" (unsigned, unknown publisher).

Only a code-signing certificate plus downloads over time removes them. Nothing in the code can.

## What is done in the repository

- The installer and the app carry publisher, description, copyright and homepage
  (`src-tauri/tauri.conf.json` `bundle`), so Windows shows a real product, not a blank one.
- `mm release` signs the installer with `signtool` when `MM_SIGN_PFX` (a `.pfx` path) and
  optionally `MM_SIGN_PASSWORD` are set in the environment; the password is never printed.
- The landing page tells people what to click (More info, then Run anyway) and shows the SHA-256.

## Options for the owner (fastest first)

| Option | Cost | Time | Notes |
|---|---|---|---|
| Submit the installer to Microsoft as a false positive / for reputation: <https://www.microsoft.com/en-us/wdsi/filesubmission> ("Software developer", file + SHA-256) | free | hours to days | Helps SmartScreen and Defender learn the file; repeat for each release |
| [SignPath Foundation](https://signpath.org/) | free for OSI-licensed open source | about a week after applying | Needs the public repository, an OSI license (D59 is still open), MFA on GitHub; the signer shown is "SignPath Foundation" |
| [Certum](https://shop.certum.eu/code-signing.html) open-source or standard code signing | from about EUR 69 a year | 1-3 days identity check | Tied to the person; check that the product gives SmartScreen trust on Windows before buying |
| Azure Artifact Signing | about USD 10 a month | - | Individual developers only in the USA and Canada (Microsoft quickstart), so not available to us now |

Even a valid certificate builds SmartScreen reputation over the first downloads; it does not
remove the dialog on day one. For judges: README and the site keep the two-click instruction.
