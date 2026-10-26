# MateScript parser: test cases (draft for day 3)

Drafted by oss (job #242), checked against [matescript.md](matescript.md) v0.2; cases 8, 9, 14 and 25 were corrected. Each case becomes a golden test; every case is also run through the fuzzer that splits the input into random chunks. Sound mode is **fun** unless a case says **calm**. Known sounds: see [sounds.md](sounds.md).

| # | Case | Input | Spoken transcript | Timeline |
|---|---|---|---|---|
| 1 | `<` in math is text | `<lang en>3 < 5 is true.` | 3 < 5 is true. | Lang(en), Say#1 |
| 2 | Tag split across chunks | chunks `<lang en><mo` + `od happy>Hello.` | Hello. | Lang(en), Say#1{mood: happy} |
| 3 | Unclosed tag at end of stream | `<lang en>Hello <sfx ping` | Hello | Lang(en), Say#1 |
| 4 | Argument longer than 32 chars | `<lang en><point aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa>Hi.` | Hi. | Lang(en), Say#1 (BadTag) |
| 5 | Newline inside a tag | `<lang en><sfx birds`⏎`soft>Morning.` | Morning. | Lang(en), Say#1 (tag discarded up to the newline) |
| 6 | Unknown tag | `<lang en><foo bar>Welcome.` | Welcome. | Lang(en), Say#1 (BadTag) |
| 7 | Sound name fuzzy-matched | `<lang en><sfx thundr>Storm coming.` | Storm coming. | Lang(en), Say#1{sfx: [thunder]} |
| 8 | **calm**: blocked and downgraded sounds | `<lang en><sfx boom><sfx knock loud>Bang!` | Bang! | Lang(en), Say#1{sfx: [knock soft]} (boom is `calm: no`) |
| 9 | More than 4 sfx (fun) | `<lang en><sfx ping><sfx pop><sfx boing><sfx tada><sfx cat>Done.` | Done. | Lang(en), Say#1{sfx: [ping, pop, boing, tada]} |
| 10 | Decimal | `<lang en>Version 1.5 is out.` | Version 1.5 is out. | Lang(en), Say#1 |
| 11 | Abbreviation | `<lang en>Dr. Smith arrived.` | Dr. Smith arrived. | Lang(en), Say#1 |
| 12 | URL | `<lang en>Visit example.com for help.` | Visit example.com for help. | Lang(en), Say#1 |
| 13 | Ellipsis | `<lang en>Wait… What?` | Wait… What? | Lang(en), Say#1 "Wait…", Say#2 "What?" |
| 14 | Quote then lowercase | `<lang en>"Done!" she said.` | "Done!" she said. | Lang(en), Say#1 (lowercase continuation rule) |
| 15 | Russian abbreviation | `<lang ru>Это, т.е., пример.` | Это, т.е., пример. | Lang(ru), Say#1 |
| 16 | Full-width punctuation | `<lang ja>こんにちは。元気ですか？` | こんにちは。元気ですか？ | Lang(ja), Say#1, Say#2 |
| 17 | Point id not in the list | `<lang en><point e99>Look here.` | Look here. | Lang(en), Say#1 (point dropped) |
| 18 | Point outside the screenshot | `<lang en><point 99999,5>See it.` | See it. | Lang(en), Say#1 (point dropped) |
| 19 | Two ambience tags | `<lang en><amb rain soft><amb forest soft>Birds sing.` | Birds sing. | Lang(en), Amb(rain), Amb(forest), Say#1 |
| 20 | Pause clamped | `<lang en><pause 99999>Ready.` | Ready. | Lang(en), Pause(3000), Say#1 |
| 21 | Tags only, with end | `<lang en><mood happy><end>` | (empty) | Lang(en), Mood(happy) after the last sentence, End |
| 22 | Tags only, no end | `<lang en><sfx tada><sfx pop>` | (empty) | Lang(en), Sfx(tada), Sfx(pop) |
| 23 | No final punctuation | `<lang en>Hello world` | Hello world | Lang(en), Say#1 (flushed at end of stream) |
| 24 | Wait between sentences | `<lang en>Hello.<wait click>How are you?` | Hello. How are you? | Lang(en), Say#1, Wait(click), Say#2 |
| 25 | Solo sound is a barrier | `<lang en><sfx rimshot solo>That's funny.<sfx success>Nice.` | That's funny. Nice. | Lang(en), Sfx(rimshot, solo), Say#1, Say#2{sfx: [success]} |
