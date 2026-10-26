"""Synthesize Mate's calm-mode cue sounds (our own work, CC0) into assets/sounds/src/cues/*.wav.

    py tools/synth_cues.py            all cues
    py tools/synth_cues.py ping done  only these

Recipes follow the Gemini sound review (2026-10-24) for older listeners and laptop speakers:
energy kept in 300 Hz - 3.2 kHz (high-pass + low-pass), no attack faster than ~8 ms, peaks at
-3 dBFS, levels set below speech (speech is normalized to about -16 dBFS RMS). `mm sounds normalize`
converts cues to OGG without loudness normalization, so these relative levels survive.
Standard library only.
"""

import math
import random
import struct
import sys
import wave
from pathlib import Path

RATE = 48_000
OUT = Path(__file__).resolve().parent.parent / 'assets' / 'sounds' / 'src' / 'cues'

NOTE = {'D4': 293.66, 'E4': 329.63, 'F4': 349.25, 'G4': 392.00, 'C5': 523.25, 'D5': 587.33,
        'E5': 659.26, 'F#5': 739.99, 'G5': 783.99, 'A5': 880.00}


# --- building blocks ----------------------------------------------------------

def silence(seconds):
    return [0.0] * int(seconds * RATE)


def mix_into(buf, sig, at=0.0, gain=1.0):
    start = int(at * RATE)
    need = start + len(sig)
    if need > len(buf):
        buf.extend([0.0] * (need - len(buf)))
    for i, v in enumerate(sig):
        buf[start + i] += v * gain
    return buf


def envelope(n, attack, decay):
    """Smooth (raised-cosine) attack, then an exponential decay reaching -40 dB at `decay` seconds."""
    a = max(1, int(attack * RATE))
    tau = decay / math.log(100)
    out = []
    for i in range(n):
        t = i / RATE
        rise = 0.5 - 0.5 * math.cos(math.pi * i / a) if i < a else 1.0
        fall = math.exp(-max(0.0, t - attack) / tau)
        out.append(rise * fall)
    return out


def tone(freqs, seconds, attack, decay, harmonics=((1, 1.0),), glide_to=None, glide_time=0.0):
    """Sum of harmonics with an optional exponential pitch glide; `freqs` is the start frequency."""
    n = int(seconds * RATE)
    env = envelope(n, attack, decay)
    phase = 0.0
    out = []
    for i in range(n):
        t = i / RATE
        f = freqs
        if glide_to and glide_time > 0:
            k = min(1.0, t / glide_time)
            f = freqs * (glide_to / freqs) ** k
        phase += 2 * math.pi * f / RATE
        v = sum(amp * math.sin(h * phase) for h, amp in harmonics if h * f < 3400)
        out.append(v * env[i])
    fade = min(len(out), int(0.005 * RATE))
    for i in range(fade):
        out[-1 - i] *= 0.5 - 0.5 * math.cos(math.pi * i / fade)
    return out


def triangle_harmonics(max_freq, f0):
    """Band-limited triangle: odd harmonics with 1/n^2 amplitudes, alternating sign."""
    hs = []
    k = 1
    while k * f0 < max_freq:
        hs.append((k, ((-1) ** ((k - 1) // 2)) / (k * k)))
        k += 2
    return tuple(hs)


def db(x):
    return 10 ** (x / 20)


class Biquad:
    """RBJ cookbook biquad (lowpass, highpass, bandpass with constant 0 dB peak)."""

    def __init__(self, kind, freq, q=0.7071):
        self.kind, self.q = kind, q
        self.z1 = self.z2 = 0.0
        self.set(freq)

    def set(self, freq):
        w = 2 * math.pi * freq / RATE
        c, s = math.cos(w), math.sin(w)
        alpha = s / (2 * self.q)
        if self.kind == 'low':
            b0, b1, b2 = (1 - c) / 2, 1 - c, (1 - c) / 2
        elif self.kind == 'high':
            b0, b1, b2 = (1 + c) / 2, -(1 + c), (1 + c) / 2
        else:
            b0, b1, b2 = alpha, 0.0, -alpha
        a0, a1, a2 = 1 + alpha, -2 * c, 1 - alpha
        self.b0, self.b1, self.b2, self.a1, self.a2 = b0 / a0, b1 / a0, b2 / a0, a1 / a0, a2 / a0

    def __call__(self, x):
        y = self.b0 * x + self.z1
        self.z1 = self.b1 * x - self.a1 * y + self.z2
        self.z2 = self.b2 * x - self.a2 * y
        return y


def pink_noise(n, seed):
    """Paul Kellet's economy pink-noise filter over white noise."""
    rnd = random.Random(seed)
    b0 = b1 = b2 = 0.0
    out = []
    for _ in range(n):
        w = rnd.uniform(-1, 1)
        b0 = 0.99765 * b0 + w * 0.0990460
        b1 = 0.96300 * b1 + w * 0.2965164
        b2 = 0.57000 * b2 + w * 1.0526913
        out.append((b0 + b1 + b2 + w * 0.1848) * 0.2)
    return out


def band(sig, low=300.0, high=3200.0):
    """Speaker- and hearing-aid-friendly band: 2nd-order high-pass and low-pass."""
    hp, lp = Biquad('high', low), Biquad('low', high)
    return [lp(hp(x)) for x in sig]


def finish(sig, rms_db, peak_db=-3.0, tail=0.03, filtered=False):
    """Band-limit (unless already done), set the RMS level, cap the peak, add a short silent tail."""
    sig = (sig if filtered else band(sig)) + silence(tail)
    rms = math.sqrt(sum(x * x for x in sig) / max(1, len(sig))) or 1e-9
    gain = db(rms_db) / rms
    peak = max(abs(x) for x in sig) * gain
    if peak > db(peak_db):
        gain *= db(peak_db) / peak
    return [x * gain for x in sig]


# --- the cues -----------------------------------------------------------------

def cue_ping():
    s = tone(NOTE['A5'], 0.36, 0.012, 0.34, ((1, 1.0), (2, db(-10))))
    glass = tone(NOTE['A5'] * 2.7, 0.08, 0.008, 0.06)
    return finish(mix_into(s, glass, 0.0, db(-18)), -22)


def cue_success():
    warm = lambda f: triangle_harmonics(2800, f) + ((2, db(-12)),)
    buf = tone(NOTE['C5'], 0.30, 0.015, 0.22, warm(NOTE['C5']))
    mix_into(buf, tone(NOTE['G5'], 0.40, 0.015, 0.35, warm(NOTE['G5'])), 0.13)
    return finish(buf, -22)


def cue_oops():
    h = ((1, 1.0), (2, db(-3)))
    buf = tone(NOTE['E4'], 0.18, 0.020, 0.14, h)
    mix_into(buf, tone(NOTE['D4'], 0.22, 0.020, 0.18, h), 0.09)
    return finish(buf, -23)


def cue_think():
    # A seamless 3.75 s loop. Tones and the LFO complete whole cycles in the loop, so only the noise
    # needs the crossfade; everything is band-filtered BEFORE cutting, so the filter has no start-up
    # transient at the seam (Gemini review #262).
    loop = 3.75
    n, xf, pre = int(loop * RATE), int(0.25 * RATE), RATE
    f1 = round(NOTE['F4'] * loop) / loop
    f2 = round(NOTE['C5'] * loop) / loop
    lfo_f = 2 / loop
    total = pre + n + xf
    noise = pink_noise(total, seed=7)
    bp = Biquad('band', 1000.0, q=1.5)
    raw = []
    for i in range(total):
        t = i / RATE
        lfo = db(1.5 * math.sin(2 * math.pi * lfo_f * t))
        v = (math.sin(2 * math.pi * f1 * t) + 0.8 * math.sin(2 * math.pi * f2 * t)) * lfo
        raw.append(v + bp(noise[i]) * db(-18) * 4)
    raw = band(raw)[pre:]                            # drop the filters' warm-up second
    # raw[n + i] continues raw[n - 1 + i]; blending it into the start makes the wrap continuous.
    out = raw[:n]
    for i in range(xf):
        k = i / xf
        out[i] = out[i] * k + raw[n + i] * (1 - k)
    return finish(out, -30, tail=0.0, filtered=True)


def cue_listen():
    s = tone(NOTE['G4'], 0.13, 0.010, 0.11, ((1, 1.0), (2, db(-9))), glide_to=NOTE['D5'], glide_time=0.07)
    return finish(s, -20)                            # short cues sound quieter: +3 dB


def cue_done():
    h = ((1, 1.0), (2, db(-8)), (3, db(-16)))
    buf = tone(NOTE['C5'], 0.30, 0.012, 0.25, h)
    mix_into(buf, tone(NOTE['E5'], 0.30, 0.012, 0.25, h), 0.09)
    mix_into(buf, tone(NOTE['A5'], 0.50, 0.012, 0.45, h), 0.18)
    return finish(buf, -21)


def cue_whoosh():
    n = int(0.28 * RATE)
    noise = pink_noise(n, seed=11)
    bp = Biquad('band', 500.0, q=2.0)
    out = []
    for i, x in enumerate(noise):
        t = i / RATE
        # Parabolic sweep 500 -> 1600 Hz at 130 ms -> 600 Hz.
        if t < 0.13:
            f = 500 + 1100 * (1 - ((0.13 - t) / 0.13) ** 2)
        else:
            f = 1600 - 1000 * ((t - 0.13) / 0.15) ** 2
        if i % 32 == 0:
            bp.set(max(300.0, f))
        out.append(bp(x))
    env = [math.sin(math.pi * min(1.0, i / (0.09 * RATE)) / 2) * (1 if i < 0.09 * RATE else math.exp(-(i / RATE - 0.09) / 0.06))
           for i in range(n)]
    # Noise has more energy up high than tones: a second low-pass (24 dB/oct in total with finish()).
    lp = Biquad('low', 2500.0)
    return finish([lp(x * e) for x, e in zip(out, env)], -24)


def cue_pop():
    s = tone(450.0, 0.075, 0.008, 0.065, ((1, 1.0), (2, db(-14))), glide_to=1150.0, glide_time=0.035)
    return finish(s, -20)                            # short cues sound quieter: +3 dB


def _three(notes, gaps, decays, harmonics_for):
    buf = []
    for f, at, d in zip(notes, gaps, decays):
        mix_into(buf, tone(f, d + 0.05, 0.018, d, harmonics_for(f)), at)
    return buf


def cue_sleep():
    notes = [NOTE['A5'], NOTE['F#5'], NOTE['D5']]
    buf = _three(notes, [0, 0.14, 0.28], [0.22, 0.22, 0.38], lambda f: triangle_harmonics(3000, f))
    return finish(buf, -24)


def cue_wake():
    notes = [NOTE['D5'], NOTE['F#5'], NOTE['A5']]
    buf = _three(notes, [0, 0.12, 0.24], [0.20, 0.20, 0.34], lambda f: ((1, 1.0), (2, db(-10)), (3, db(-18))))
    return finish(buf, -22)


CUES = {
    'ping': cue_ping, 'success': cue_success, 'oops': cue_oops, 'think': cue_think, 'listen': cue_listen,
    'done': cue_done, 'whoosh': cue_whoosh, 'pop': cue_pop, 'sleep': cue_sleep, 'wake': cue_wake,
}


def write_wav(path, samples):
    path.parent.mkdir(parents=True, exist_ok=True)
    frames = b''.join(struct.pack('<h', max(-32767, min(32767, int(round(x * 32767))))) for x in samples)
    with wave.open(str(path), 'wb') as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(RATE)
        w.writeframes(frames)


def main(argv=None):
    names = (argv if argv is not None else sys.argv[1:]) or list(CUES)
    unknown = [n for n in names if n not in CUES]
    if unknown:
        print('Unknown cue(s): ' + ', '.join(unknown) + '. Known: ' + ', '.join(CUES))
        return 1
    for name in names:
        samples = CUES[name]()
        write_wav(OUT / f'{name}.wav', samples)
        peak = max(abs(x) for x in samples)
        rms = math.sqrt(sum(x * x for x in samples) / len(samples))
        print(f'{name:<8} {len(samples) / RATE:5.2f} s  rms {20 * math.log10(rms):6.1f} dBFS  peak {20 * math.log10(peak):5.1f} dBFS')
    return 0


if __name__ == '__main__':
    sys.exit(main())
