#!/usr/bin/env python3
"""Render a quiet, warm background music track (WAV) for the product video.

Original composition: I-V-vi-IV in C major at ~76 BPM, soft arpeggio over a
root+fifth pad. Deterministic (fixed seed), standard library only.
"""

import argparse
import math
import random
import wave
from array import array
from itertools import repeat
from operator import add, mul

SR = 44100
SECONDS_DEFAULT = 170
SEED = 20261104

BPM = 76.0
BEAT = 60.0 / BPM            # ~0.789 s
CHORD_S = 8 * BEAT           # progression changes every two bars

TARGET_PEAK = 10 ** (-3.0 / 20.0)    # -3 dBFS
ATTACK_S = 0.008
DECAYS = (1.5, 1.8, 2.1, 2.4)        # per-note decay, seconds (1.5-2.5 range)
PARTIALS = (1.0, 0.45, 0.22, 0.10)   # falling amplitudes, partials 1..4
ARPEGGIO_AMP = 0.30
PAD_AMP = 0.016
PAD_EDGE_S = 0.03                    # pad edge softening
PAN_MAX = 0.6                        # short stereo spread
PATTERN = (0, 1, 2, 1)               # arpeggio: root, third, fifth, third

# I, V, vi, IV in C major: (root midi, chord tones midi).
CHORDS = (
    (60, (60, 64, 67)),   # C
    (67, (67, 71, 74)),   # G
    (69, (69, 72, 76)),   # Am
    (65, (65, 69, 72)),   # F
)


def midi_hz(midi):
    return 440.0 * 2.0 ** ((midi - 69) / 12.0)


def sine_buf(freq, n, amp):
    """Sine into a float32 buffer (rotation recurrence, no per-sample sin)."""
    out = array('f', bytes(4 * n))
    step = 2.0 * math.pi * freq / SR
    cw, sw = math.cos(step), math.sin(step)
    re, im = 1.0, 0.0
    for i in range(n):
        out[i] = amp * im
        re, im = re * cw - im * sw, im * cw + re * sw
        if (i & 4095) == 4095:       # renormalize against float drift
            m = math.hypot(re, im) or 1.0
            re, im = re / m, im / m
    return out


def soften_edges(buf, n):
    ramp = [i / n for i in range(n)]
    buf[:n] = array('f', map(mul, buf[:n], ramp))
    tail = buf[len(buf) - n:]
    buf[len(buf) - n:] = array('f', map(mul, tail, ramp[::-1]))


def render_note(midi, dur, rng):
    """One cached arpeggio note: partials 1-4, 8 ms attack, exp decay."""
    n = int(dur * SR)
    out = array('f', bytes(4 * n))
    sin = math.sin
    f0 = midi_hz(midi) * (1.0 + rng.uniform(-0.002, 0.002))
    freqs = (f0, f0 * 2, f0 * 3, f0 * 4)
    amps = PARTIALS
    attack_n = int(ATTACK_S * SR)
    tau = dur / 3.0
    two_pi = 2.0 * math.pi
    scale = ARPEGGIO_AMP * 0.25     # headroom, a few notes overlap
    for i in range(n):
        t = i / SR
        env = (t / ATTACK_S if i < attack_n else 1.0) * math.exp(-t / tau)
        s = (amps[0] * sin(two_pi * freqs[0] * t)
             + amps[1] * sin(two_pi * freqs[1] * t)
             + amps[2] * sin(two_pi * freqs[2] * t)
             + amps[3] * sin(two_pi * freqs[3] * t))
        out[i] = s * env * scale
    return out


def mix_note(left, right, note, start, vel, pan):
    """Mix one cached mono note into the stereo buffers at a sample offset."""
    if start < 0:
        start = 0
    end = min(len(left), start + len(note))
    if end <= start:
        return
    n = end - start
    ang = (pan + 1.0) * (math.pi / 4.0)      # constant-power pan
    gl = math.cos(ang) * vel
    gr = math.sin(ang) * vel
    src = note[:n]
    seg = left[start:end]
    left[start:end] = array('f', map(add, seg, map(mul, src, repeat(gl))))
    seg = right[start:end]
    right[start:end] = array('f', map(add, seg, map(mul, src, repeat(gr))))


def apply_fades(buf):
    """Fade in over 2 s, fade out over the last 4 s."""
    n_in = int(2.0 * SR)
    ramp = [i / n_in for i in range(n_in)]
    buf[:n_in] = array('f', map(mul, buf[:n_in], ramp))
    n_out = int(4.0 * SR)
    start = len(buf) - n_out
    ramp = [(n_out - i) / n_out for i in range(n_out)]
    buf[start:] = array('f', map(mul, buf[start:], ramp))


def comb(buf, delay, g):
    """In-place feedback delay: out[i] = in[i] + g * out[i-delay]. Linear time."""
    for i in range(delay, len(buf)):
        buf[i] = buf[i] + g * buf[i - delay]


def render(path, seconds=SECONDS_DEFAULT, seed=SEED):
    total = int(seconds * SR)
    left = array('f', bytes(4 * total))
    right = array('f', bytes(4 * total))
    rng = random.Random(seed)
    note_cache = {}
    pad_cache = {}
    pad_n = int(CHORD_S * SR)
    edge_n = int(PAD_EDGE_S * SR)
    step = BEAT / 2.0                        # eighth-note arpeggio

    n_chords = int(math.ceil(seconds / CHORD_S))
    for ci in range(n_chords):
        chord_t = ci * CHORD_S
        root, tones = CHORDS[ci % len(CHORDS)]
        for m in (root, root + 7):           # pad: root and fifth
            pad = pad_cache.get(m)
            if pad is None:
                pad = sine_buf(midi_hz(m), pad_n, PAD_AMP)
                soften_edges(pad, edge_n)
                pad_cache[m] = pad
            mix_note(left, right, pad, int(round(chord_t * SR)), 1.0, 0.0)
        k = 0
        while True:
            t = chord_t + k * step
            if t >= seconds - 0.1:
                break
            midi = tones[PATTERN[k % len(PATTERN)]]
            dur = DECAYS[rng.randrange(len(DECAYS))]
            vel = rng.uniform(0.55, 0.95)
            jit = rng.uniform(-0.008, 0.008)     # +/- 8 ms timing jitter
            pan = rng.uniform(-PAN_MAX, PAN_MAX)
            key = (midi, dur)
            note = note_cache.get(key)
            if note is None:
                note = render_note(midi, dur, rng)
                note_cache[key] = note
            mix_note(left, right, note, int(round((t + jit) * SR)), vel, pan)
            k += 1

    apply_fades(left)
    apply_fades(right)
    for buf in (left, right):                # simple feedback-delay reverb
        comb(buf, 1279, 0.32)                # ~29 ms
        comb(buf, 1631, 0.26)                # ~37 ms

    peak = max(max(map(abs, left), default=0.0),
               max(map(abs, right), default=0.0))
    if peak <= 0.0:
        raise ValueError("rendered silence; nothing to normalize")
    qscale = TARGET_PEAK * 32767.0 / peak
    li = array('h', map(int, map(mul, left, repeat(qscale))))
    ri = array('h', map(int, map(mul, right, repeat(qscale))))
    frames = array('h', bytes(4 * total))
    frames[0::2] = li
    frames[1::2] = ri
    with wave.open(path, "wb") as w:
        w.setnchannels(2)
        w.setsampwidth(2)
        w.setframerate(SR)
        w.setnframes(total)
        w.writeframes(frames.tobytes())
    return path


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("path", help="output WAV path")
    parser.add_argument("--seconds", type=float, default=SECONDS_DEFAULT,
                        help="track length in seconds (default: %(default)s)")
    args = parser.parse_args(argv)
    render(args.path, args.seconds)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
