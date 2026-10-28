# Submission checklist

Submission deadline: **November 4, 2026, 9:00 PM ET**. The project must be public and the submission complete by then. Submit through [Cerebral Valley](https://cerebralvalley.ai/).

**Owners:** Owner means the project owner; Lead means Superclaude, the project lead.

| Done | Requirement | Owner | Lead / check |
|---|---|---|---|
| [ ] | Make the GitHub repository public before submitting; confirm the public link opens without sign-in and contains the final code and README. | Owner | Lead verifies access and final state |
| [ ] | Upload a video of **no more than three minutes** showing working features built during the hackathon. Use genuine footage of the product and make sure the submission video link plays for judges. | Owner records, edits, and uploads; Copilot helps with the edit and copy | Lead verifies feature claims against the build and approves the final cut |
| [ ] | List everyone who contributed to the project in the submission, using their agreed public names or handles; check that no contributor is omitted and spelling is correct. | Owner | Lead cross-checks the contributor list |
| [ ] | Publish the project X post for **Most Viral** by **November 8, 2026**; include the submission/project link and confirm the post is public. | Owner posts; Copilot drafts | Lead approves the final wording and link |
| [ ] | Participate in the **Community Vote / gallery vote**: submit the project to the gallery, verify its entry is visible, and share the voting link so people can find it. | Owner submits and shares | Lead checks the gallery entry and link |
| [ ] | Claim **Best Build on Meta Open Weight Models** only with clear evidence of Muse Glimmer use in the submitted project. | Owner selects the prize category and submits evidence | Lead verifies the Muse Glimmer integration and evidence |
| [ ] | Claim **Best Agentic Use-Case** with a working agentic workflow and evidence in the demo or submission materials. | Owner selects the prize category and submits evidence | Lead verifies the workflow and evidence |
| [ ] | Claim **Best Creative Build** with clear evidence of Muse Image use in the submitted project. | Owner selects the prize category and submits evidence | Lead verifies the Muse Image integration and evidence |

Before final submission, the owner should open every submitted link from a signed-out browser and confirm the repository, video, contributor list, gallery entry, and any prize evidence are accessible.

## Submission text (draft, 2026-10-28)

Drafted by DeepSeek from the lead's list of facts; check it against the build before pasting.

**Tagline**
Mate lights the button. Your parent clicks it.
Alternative: Hold Right Ctrl, ask, and Mate lights the way.

**Elevator pitch**
MetaMate is a Windows desktop helper for older people and anyone who gets lost on a computer: hold Right Ctrl and ask out loud in English or Russian. Mate, a small glowing light-bulb, answers with his voice, flies to the right control in any app and shines a beam on it; for "how do I…" requests he teaches step by step while the user clicks each step themselves, and he never clicks payment, delete, transfer, or password actions. "Is this safe?" checks only the current window, names the red flags it actually sees, and never tells anyone to click, call, or pay; the app captures only that window during a request and paints password fields black before sending.

**Description**

**What it does**
MetaMate is a Windows desktop helper for older people and anyone who gets lost on a computer. Hold Right Ctrl and ask out loud in English or Russian. Mate, a small glowing light-bulb character, answers with his voice, flies to the right button in any app, and shines a beam on it. For "how do I…" questions, he teaches step by step and the person clicks each step themselves; he never clicks payment, delete, transfer, or password actions. For "Is this safe?", Mate looks only at the current window, gives a verdict, names the red flags he actually sees, and points at the worst one. He never tells anyone to click, call, or pay. After a lesson, he leaves a keepsake card: Muse Glimmer rewrites the steps in simple words and Muse Image draws a picture, and it can be printed.

**How we use Meta's models**
Muse Spark 1.3 handles answers, computer-use teaching, and the scam check. Muse Wonder 1.0 provides the "Thorough" setting and escalates lessons after two misses on Quick. Muse Voice Transcribe streams speech recognition from the moment the key goes down. SAM 3.1 finds controls where the accessibility tree is empty, such as canvas pages. Muse Image draws the card picture, while Muse Glimmer 30B open weights runs on our own GPU on Modal and writes the card wording. MMS-TTS open weights runs on a Modal GPU for Mate's voice.

**How it's built**
The app is built in Rust and Tauri 2 with a TypeScript WebView. For evaluation, the judges' path is the release installer with a built-in capped judge invite (no account needed; Google sign-in is optional, and GitHub sign-in is not in the judges' build yet). A small FastAPI proxy holds the keys and per-account allowances. Privacy is narrow: only the target window is captured and only during a request; password fields are painted black before anything is sent, and it fails closed. It was built in 10 days by one indie developer with a team of AI coding agents.

**What's next**
We want to learn from real sessions and reduce the cases where Mate has to escalate to Muse Wonder after two misses on Quick, while improving the safety check for less common window types.

## X post drafts (Most Viral, post by Nov 8 with the video)

Drafted by DeepSeek and oss; never post a made-up incident as if it happened.

1. Your mom kept your report cards. Her laptop now keeps one too.
   MetaMate: hold Right Ctrl, ask out loud. After each lesson, a printable keepsake card: Muse Glimmer writes it, Muse Image draws it. Built on Meta's Muse models. #MetaMate
2. Your mom is one wrong click from a drained bank account.
   MetaMate: hold Right Ctrl, ask out loud. Mate, a glowing bulb, beams the right button. "Is this safe?" and he flags the worst scam signal. He never says click, call or pay. Built on Meta's Muse models. #MetaMate
