# MateScript v0.2 (draft, 2026-10-24; v0.2 adds segmentation, the ordered timeline and recovery rules after the Codex review #233)

MateScript is the tiny streaming language the model writes its answers in. Plain text is speech; short tags in angle brackets make the robot play sounds, change mood, point at things and run step-by-step guides. The app parses it token by token while the answer is still streaming.

Design goals:
1. **Easy for a model to write**: one tag shape, about ten tag names, all self-closing, no nesting, no quoting.
2. **Safe to stream**: a tag split across chunks is fine; a broken tag degrades to nothing, never to garbage speech.
3. **Readable without tags**: strip the tags and you get the transcript.
4. **Closed vocabulary**: sounds, moods and element ids come from lists given in the prompt; anything else is dropped.

## 1. Syntax

```
answer   := item*
item     := text | tag
tag      := "<" name ( " " arg )* ">"        (all tags except show)
          | "<show " payload ">"
name     := [a-z]{2,6}
arg      := [A-Za-z0-9_.,/:+-]{1,32}
payload  := any Unicode except "<", ">" and newline, 1..64 characters
text     := any characters except a "<" that starts a tag
```

- A `<` followed by a lowercase ASCII letter starts a tag. Any other `<` (e.g. `3 < 5`) is text. A `<` at the very end of a chunk is held back until the next chunk shows what follows it.
- **Recovery:** if a started tag sees a newline or 64 characters without `>`, everything from `<` is discarded, never spoken, up to the next `>` (consumed) or the next newline, at most 256 characters; parsing resumes after it. A newline inside the tag ends the sentence before it and starts the discard, so in golden case 5 (`<sfx birds⏎soft>Morning.`) the tail `soft>` is dropped too. The 64 count the whole tag body (name included; the parser discards on the 64th character), so a `<show>` payload fits at most 58 characters in practice.
- Tags are never placed inside a word.
- Each sentence is one TTS chunk (see §4 for the exact segmentation rules).

## 2. Tags

| Tag | Meaning | Example |
|---|---|---|
| `<lang CODE>` | Language of the answer (ISO 639-1 or 639-3). First tag of every answer. Selects the TTS voice. | `<lang ru>` |
| `<mood NAME>` | Robot mood: `calm`, `happy`, `thinking`, `surprised`, `sad`, `proud`, `sleepy` | `<mood happy>` |
| `<sfx NAME [soft\|normal\|loud] [solo]>` | One-shot sound from the list. Default: plays with the start of the next sentence (speech waits 150 ms). With `solo`: plays alone, speech continues after it ends. | `<sfx rimshot solo>` |
| `<amb NAME [soft\|normal]>` / `<amb off>` | Start or stop a looping ambience bed (ducked under speech). One bed at a time; a new one crossfades. | `<amb forest soft>` |
| `<pause MS>` | Silence, 100–3000 ms. | `<pause 600>` |
| `<point ID>` / `<point X,Y>` | Point at an element from the candidate list, or at a pixel of the last screenshot. | `<point e17>` |
| `<step N/M ID>` | (App-generated in teacher mode; the model does not write it.) Step N of M of a guide, pointing at ID (or `X,Y`). The sentence after it is the instruction. | `<step 1/3 e4>` |
| `<wait click>` | (App-generated in teacher mode.) Pause the answer until the user clicks the current target (or says "next"). | `<wait click>` |
| `<show TEXT>` | Show a short text bubble the user must type or read (URL, number). Free Unicode text up to `>`, spaces allowed. | `<show 125%>` |
| `<end>` | The model is done (optional; end of stream also ends). | `<end>` |

Timeline: the parser turns the answer into **one ordered timeline**; the player runs it in order.
- `sfx` (without `solo`), `mood`, `point`, `step`, `show` **bind to the next sentence**: they start together with it. So write them right **before** the sentence they go with, never after it (a trailing `<sfx>` plays with the following sentence).
- `sfx … solo`, `pause`, `wait`, `amb`, `lang`, `end` are **timeline items** at their position: e.g. a `<wait click>` after sentence 2 is reached only when sentence 2 has finished playing; a `solo` sound is a barrier (the next sentence starts when it ends).
- Bound tags with no following sentence (end of answer) become timeline items after the last sentence.
- Two bound tags of the same kind before one sentence: the last one wins (one mood, one point per sentence); two sfx are both kept.
- In **teacher mode** (computer use) the `point`/`step`/`wait` tags are produced by the app from `computer_call` actions, not by the model; the model then writes only `lang`, `mood`, `sfx` and speech.

## 3. Examples

**Fairy tale (fun mode)**
```
<lang en><mood calm><amb forest soft>Once upon a time, a little fox lived at the edge of a quiet forest.
<sfx birds soft>Every morning the birds woke him up.
One day the sky turned dark. <sfx thunder solo><mood surprised>"Oh no," said the fox, "a storm!"
He ran home just in time, <amb rain soft>and listened to the rain from his warm bed. <mood sleepy>
```

**"Where is the Save button?" (Word, element list given)**
```
<lang en><mood happy><point e12><sfx ping soft>It's right here, the little disk at the top left.
You can also press Control and S together.
```

**3-step guide: bigger text in Windows Settings**
```
<lang ru><mood calm>Сейчас покажу, это три шага.
<step 1/3 e3>Нажмите «Специальные возможности» слева. <wait click>
<step 2/3 e8>Теперь нажмите «Размер текста». <wait click>
<step 3/3 e21>Передвиньте ползунок вправо и нажмите «Применить». <wait click>
<sfx success soft><mood proud>Готово! Текст стал крупнее.
```

In Teacher mode the app generates the `<step>`/`<wait>` timeline from `computer_call` turns and the model never emits MateScript inside `computer_call` (the golden test parses the app-generated timeline).

**Joke with a rimshot (fun mode)**
```
<lang en><mood happy>Why did the computer go to the doctor?
Because it had a virus! <sfx rimshot solo><mood proud>
```

Stripped transcript of the joke: "Why did the computer go to the doctor? Because it had a virus!"

## 4. Streaming parser

### Sentence segmentation

A sentence ends at a terminator followed by whitespace, a closing quote/bracket and whitespace, a tag, or the end of stream:
- Terminators: `.` `!` `?` `…` `...` and full-width `。` `！` `？` (these need no following space).
- Closing characters after a terminator stay with the sentence: `"` `”` `»` `’` `)` `]`.
- **Not** an end: a `.` between digits (`1.5`, `3.14`), inside a URL or e-mail (`example.com`, no whitespace after), after a known abbreviation (`Dr.` `Mr.` `Mrs.` `e.g.` `i.e.` `etc.` `т.е.` `т.д.` `г.` `ул.` — list per language), or after a single capital letter (initials: `J. Smith`).
- **Not** an end either: a terminator (with its closing quote) followed by whitespace and a **lowercase** letter (`"Done!" she said.`, `approx. five`).
- A newline always ends a sentence.
- Lookahead: a terminator at the end of a chunk waits for the next character before deciding (up to 300 ms; at end of stream it ends the sentence).
- Max length flush: a sentence longer than 220 characters is cut at the last comma/semicolon/space so TTS can start.

### State machine

States: `TEXT`, `LT` (saw `<`, waiting for the next character), `TAG`, `DISCARD`.

- `TEXT`: append to the sentence buffer. `<` → `LT`. On a sentence end → push `Say{id, text, bound}` to the timeline and clear `bound`. End of stream → flush the buffer as a sentence.
- `LT`: next char is a lowercase ASCII letter → `TAG`; otherwise the `<` was text → back to `TEXT`. End of stream → `<` dropped.
- `TAG`: collect until `>`. On `>` → validate (known name, valid args/payload) → bound kinds go to `bound`, the others become timeline items in order. Invalid → `BadTag` (debug), dropped. Newline or 64 chars without `>` → `DISCARD`.
- `DISCARD`: drop characters until `>` (consumed) or a newline (kept as a sentence end), at most 256 characters; then `TEXT`.
- Chunk boundaries never matter: all state is kept between chunks.
- Whitespace around tags is collapsed so the transcript has no double spaces.

Timeline items: `Lang`, `Say{id, text, sfx[], mood?, point?, step?, show?}`, `Sfx{solo}`, `Amb`, `Pause`, `Wait`, `End`; plus `BadTag` for the debug view only.

## 5. Failure rules

| Problem | Rule |
|---|---|
| Unknown tag name | Drop, log `BadTag`. |
| Unknown sound / mood | Fuzzy match (edit distance ≤ 2) within the list, else drop. |
| `loud` in calm mode | Downgrade to `soft`; sounds marked `calm: no` are dropped. |
| More than 4 sfx per answer (2 in calm mode) | Drop the extras. |
| `point` id not in the list, or point outside the screenshot | Drop the point; Mate says he can't find it if nothing else was pointed. |
| Missing `<lang>` | Use the language of the user's transcript. |
| Tag inside a word, or markup the model invented | Removed from speech; never spoken. |
| Stream stops mid-tag | Drop the partial tag. |
| `<pause>` outside 100–3000 ms | Clamp into the range. |

The sound mode (calm / fun) is a **parser setting** from the user's settings, not a tag; `<mood calm>` only changes Mate's face.

## 6. Why not JSON tool calls or a compiled language

- JSON must be complete before it parses; speech and sounds would wait for the whole answer. MateScript fires the first sentence as soon as it is written.
- A missing brace breaks a whole JSON object; a broken MateScript tag loses one sound.
- Stripping tags gives the transcript for free (subtitles, accessibility, chat history).
- A compiled language would make no audible difference: latency comes from the network, the model and TTS, not the CPU.

Weak points: no structure checking at write time (only at runtime), a fixed set of tags (adding one needs a parser and prompt update), and the model must be reminded of the rules with examples in the system prompt.

## 7. Tests to write (day 3)

- Golden files: the four examples above → expected event lists.
- Fuzz: every example split into random chunks (1..20 chars) → same events.
- Broken input: unclosed tags, `<` in math, unknown names, 10 kB without a sentence end, tags glued to words.
- Segmentation: `1.5`, `example.com`, `Dr. Smith`, `т.е.`, `"Done!" she said.`, `…`, `。`, a terminator split across two chunks.
- Timeline order: `<wait click>` and `<amb off>` placed between sentences fire after the previous sentence, not at parse time.
