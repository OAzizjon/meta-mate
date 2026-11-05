"""Tests for tools/synth_music.py (no audio playback)."""

import math
import os
import sys
import tempfile
import unittest
import wave
from array import array

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
import synth_music  # noqa: E402


def read_wav(path):
    with wave.open(path, "rb") as w:
        params = (w.getframerate(), w.getsampwidth(),
                  w.getnchannels(), w.getnframes())
        samples = array("h", w.readframes(w.getnframes()))
    return params, samples


class SynthMusicTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)

    def render(self, name, seconds=6):
        path = os.path.join(self.tmp.name, name)
        synth_music.render(path, seconds=seconds)
        return path

    def test_rate_width_channels_length(self):
        (rate, width, channels, nframes), _ = read_wav(self.render("a.wav"))
        self.assertEqual(rate, 44100)
        self.assertEqual(width, 2)
        self.assertEqual(channels, 2)
        self.assertEqual(nframes, 6 * 44100)

    def test_peak_near_minus_3_dbfs(self):
        _, samples = read_wav(self.render("b.wav"))
        peak = max(map(abs, samples)) / 32768.0
        db = 20.0 * math.log10(max(peak, 1e-9))
        self.assertGreaterEqual(db, -3.5)
        self.assertLessEqual(db, -2.5)

    def test_deterministic_for_same_seed(self):
        p1 = self.render("c1.wav")
        p2 = self.render("c2.wav")
        with open(p1, "rb") as f1, open(p2, "rb") as f2:
            self.assertEqual(f1.read(), f2.read())

    def test_first_10_ms_near_silence(self):
        _, samples = read_wav(self.render("d.wav"))
        head = samples[:int(0.010 * 44100) * 2]
        self.assertLess(max(map(abs, head)), 500)   # ~-36 dBFS


if __name__ == "__main__":
    unittest.main()
