# Sound library plan (v0)

Two modes:
- **Calm** (default for new users): gentle cues only, everything capped at `soft`, sounds with `calm: no` are blocked, max 2 sfx per answer.
- **Fun** (by voice: "fun mode" / «весёлый режим»): stories, jokes, drums, thunder, explosions; max 4 sfx per answer.

Format: OGG Vorbis (or Opus), 48 kHz, mono for sfx, stereo for ambience. Loudness: sfx normalized to about −16 LUFS short-term, ambience about −28 LUFS, seamless loops (crossfaded ends). Whole library under ~15 MB.

Sources (licence recorded per file in `assets/sounds/LICENSES.md`):
- Freesound with the **CC0** filter only (no attribution needed; avoid CC-BY-NC).
- Kenney audio packs (CC0): UI clicks, chimes.
- Pixabay sound effects (Pixabay Content License: free, no attribution).
- Our own recordings (e.g. a real door knock) are fine.

Names below are the closed list given to the model (`<sfx NAME>` / `<amb NAME>`).

## Cues (calm mode, the teacher)

**Done (2026-10-24):** all ten cues are synthesized by `tools/synth_cues.py` (our own work, CC0) into `assets/sounds/src/cues/*.wav`; `mm sounds normalize` converts them to `assets/sounds/cues/*.ogg` **without** loudness normalization, because their levels are deliberate: RMS about 5–8 dB below speech (−20 … −24 dBFS; `think` −30 dBFS), peaks under −3 dBFS, energy in 300 Hz – 3.2 kHz (laptop speakers, hearing aids, age-related high-frequency loss). Listen to all of them in `assets/sounds/cues-preview.ogg`. Panning toward the target is capped at ±30% (asymmetric hearing loss is common). Every cue is paired with a visible change on screen.

| name | sounds like | use | dur | level | calm |
|---|---|---|---|---|---|
| ping | soft glass ping, panned to target (clamped to ±30 %) | robot points at something | short | soft | yes |
| success | warm two-note chime | correct click / task done | short | soft | yes |
| oops | soft low "boop" | wrong click (never harsh) | short | soft | yes |
| think | quiet airy hum loop | while the model thinks | loop | soft | yes |
| listen | tiny rising blip | PTT pressed | short | soft | yes |
| done | short bright bell | end of a guide | short | soft | yes |
| whoosh | gentle air swipe | robot flies across the screen | short | soft | yes |
| pop | small bubble pop | bubble/text appears | short | soft | yes |
| sleep | soft descending tones | robot hides to tray | short | soft | yes |
| wake | soft ascending tones | robot comes back | short | soft | yes |

## Ambience (loops)

| name | sounds like | use | calm |
|---|---|---|---|
| forest | leaves, distant birds | fairy tales, relaxing | yes |
| rain | steady soft rain | stories, calm | yes |
| sea | slow waves | relaxing | yes |
| fire | fireplace crackle | evening stories | yes |
| night | crickets, light wind | bedtime stories | yes |
| park | birds, distant children, breeze | everyday scenes | yes |
| cafe | soft chatter, cups | city stories | yes |

## Effects (fun mode; some allowed in calm)

| name | sounds like | use | dur | level | calm |
|---|---|---|---|---|---|
| birds | birdsong burst | morning, nature | medium | soft | yes |
| rooster | rooster crow | morning, wake up | medium | normal | no |
| cat | short meow | animals, jokes | short | normal | yes |
| dog | single friendly bark | animals | short | normal | no |
| owl | owl hoot | night, forest | short | soft | yes |
| horse | neigh | farm stories | medium | normal | no |
| knock | three door knocks | someone arrives | short | normal | yes |
| clock | ticking (2 s) | waiting, time | medium | soft | yes |
| bell | church/school bell, distant | events | medium | normal | yes |
| rimshot | drum ba-dum-tss | after a joke | short | normal | no |
| drumroll | snare roll (2 s) | before a reveal | medium | normal | no |
| applause | warm small applause | celebration | medium | normal | yes |
| cheer | short crowd "yay" | big success | short | normal | no |
| magic | sparkle chime | magic in a story | short | soft | yes |
| fanfare | short brass fanfare | achievement | medium | loud | no |
| thunder | thunder roll | storm in a story | medium | loud | no |
| wind | gust of wind | storm, travel | medium | normal | yes |
| splash | water splash | river, sea | short | normal | yes |
| boom | small cartoon explosion | action, jokes | short | loud | no |
| laser | retro laser zap | robot jokes | short | normal | no |
| boing | spring boing | comedy | short | normal | yes |
| slip | cartoon slide whistle | comedy fall | short | normal | no |
| giggle | small child giggle | jokes | short | soft | yes |
| footsteps | a few steps | someone walks in | medium | soft | yes |
| door | door creak | spooky/arrival | medium | normal | no |
| phone | old phone ring (1 ring) | calls | short | normal | no |
| train | distant train whistle | travel stories | medium | normal | no |
| car | car horn (friendly beep) | city | short | loud | no |
| heart | heartbeat (2 s) | suspense | medium | soft | no |
| tada | "ta-da" stinger | reveal | short | normal | yes |
| sad | sad trombone | gentle humour | medium | normal | no |

## First 12 to prepare (for the video)

ping, success, oops, think, whoosh, done, forest, rain, birds, thunder, rimshot, applause.

## Fun set fetched at the kickoff (D39, 2026-10-26)

Five public-domain files from Wikimedia Commons, picked for jokes, stories and celebrations; listen to all of them in `assets/sounds/fun-preview.ogg` (in this order) and veto any before the Day-3 mixer:

| name | file | why |
|---|---|---|
| rimshot | `File:Sting.ogg`: drum roll + cymbal | the punchline sting; the last of the "first 12" |
| boing | `File:Comic spring up or magic trick.ogg` | jack-in-the-box comedy, allowed in calm mode |
| magic | `File:AsGliss.ogg`: a harp glissando (5.1 s: fade it after ~2.5 s in the mixer) | stories, "a little magic" |
| sad | `File:Wah tah tah taah.ogg`: a voice sings the sad-trombone line | gentle self-deprecating humour |
| cheer | `File:Hurray.ogg`: a few people shout "hurray" | a finished task in fun mode |

Not on Commons as CC0/PD: `tada` (only state fanfares came up) and `owl`. If a sixth is wanted, `tada` can be synthesized like the cues (a C-E then C-E-G-C stinger, recipe in Gemini #420).

Hearing-aid risks among the existing files (Gemini #420), for the Day-3 mixer: `clock` (sharp clicks at −16 LUFS trigger hearing-aid transient suppression), `heart` (40–120 Hz: inaudible on laptops, rattles speakers), `door`, `train`, `rooster` (narrow 1.5–4 kHz whistles can set off feedback cancellers), `thunder` (masks speech). Consider sfx at −20 LUFS with a 300 Hz – 3.2 kHz band-pass instead of −16 LUFS.
