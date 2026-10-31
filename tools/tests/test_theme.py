"""Dark palette contrast and token coverage, without a browser dependency."""

from pathlib import Path
import re
import unittest


ROOT = Path(__file__).resolve().parents[2]


def tokens(block):
    return dict(re.findall(r"(--[\w-]+)\s*:\s*([^;]+);", block))


def contrast(first, second):
    def luminance(color):
        channels = [int(color[index:index + 2], 16) / 255 for index in (1, 3, 5)]
        linear = [value / 12.92 if value <= 0.04045
                  else ((value + 0.055) / 1.055) ** 2.4 for value in channels]
        return sum(value * weight for value, weight in zip(linear, (0.2126, 0.7152, 0.0722)))
    high, low = sorted((luminance(first), luminance(second)), reverse=True)
    return (high + 0.05) / (low + 0.05)


class ThemeTests(unittest.TestCase):
    def setUp(self):
        source = (ROOT / "src/theme.css").read_text(encoding="utf-8")
        self.light = tokens(re.search(r":root\s*\{([^}]+)\}", source)[1])
        self.dark = tokens(re.search(r':root\[data-theme="dark"\]\s*\{([^}]+)\}', source)[1])

    def test_every_light_token_has_a_dark_value(self):
        self.assertEqual(self.light.keys(), self.dark.keys())

    def test_chat_text_and_timestamps_have_normal_contrast_in_both_themes(self):
        for palette in (self.light, self.dark):
            for foreground in ("--text", "--text-muted"):
                for background in ("--surface", "--accent-soft", "--surface-soft",
                                   "--chat-happy-bg", "--chat-surprised-bg", "--chat-sad-bg"):
                    with self.subTest(foreground=foreground, background=background,
                                      surface=palette["--surface"]):
                        self.assertGreaterEqual(contrast(palette[foreground], palette[background]), 4.5)

    def test_dark_text_and_muted_text_have_normal_text_contrast(self):
        surfaces = ["--bg", "--surface", "--surface-soft", "--surface-hover",
                    "--accent-soft", "--accent-soft-hover", "--success-bg", "--error-soft",
                    "--selection"]
        surfaces += re.findall(r"#[0-9a-fA-F]{6}", self.dark["--hero-bg"])
        for foreground in ("--text", "--text-muted"):
            for background in surfaces:
                color = self.dark.get(background, background)
                with self.subTest(foreground=foreground, background=background):
                    self.assertGreaterEqual(contrast(self.dark[foreground], color), 4.5)

    def test_dark_control_edges_have_non_text_contrast(self):
        for edge in ("--border-soft", "--border-strong", "--accent-line"):
            for surface in ("--bg", "--surface", "--surface-soft", "--surface-hover",
                            "--accent-soft", "--accent-soft-hover"):
                with self.subTest(edge=edge, surface=surface):
                    self.assertGreaterEqual(contrast(self.dark[edge], self.dark[surface]), 3)
        for surface in ("--accent-soft", "--accent-soft-hover", "--surface"):
            self.assertGreaterEqual(
                contrast(self.dark["--accent-soft-border"], self.dark[surface]), 3)

    def test_yellow_buttons_keep_navy_text(self):
        self.assertEqual(self.dark["--on-accent"], self.light["--on-accent"])
        self.assertEqual(self.dark["--on-accent"], "#1f2a44")
        for background in ("--accent", "--accent-hover"):
            self.assertGreaterEqual(
                contrast(self.dark["--on-accent"], self.dark[background]), 4.5)

    def test_card_print_palette_overrides_the_dark_selector(self):
        source = (ROOT / "card.html").read_text(encoding="utf-8")
        print_css = source.split("@media print {", 1)[1]
        match = re.search(r':root,\s*:root\[data-theme="dark"\]\s*\{([^}]+)\}', print_css)
        self.assertIsNotNone(match)
        printed = tokens(match[1])
        self.assertEqual(printed["--bg"], "#ffffff")
        self.assertEqual(printed["--panel-bg"], "#ffffff")
        self.assertEqual(printed["--text"], "#000000")
        self.assertEqual(printed["--text-muted"], "#000000")


if __name__ == "__main__":
    unittest.main()
