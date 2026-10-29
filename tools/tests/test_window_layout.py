"""Small static guard against oversized fixed widths in window styles.

This is a source check, not a browser layout test. Percentage/function widths and
max-width caps can shrink; media-scoped rules may intentionally target wide screens.
"""

from pathlib import Path
import re
import unittest


ROOT = Path(__file__).resolve().parents[2]
PAGES = ("home.html", "settings.html", "signin.html", "card.html")


def oversized_widths(css):
    # Preserve offsets so failures still point at the original source line.
    css = re.sub(r"/\*.*?\*/", lambda m: re.sub(r"[^\n]", " ", m[0]),
                 css, flags=re.S)
    tokens = re.finditer(
        r"@media\b[^{}]*|[{}]|(?<![\w-])(?:min-)?(?:width|inline-size)"
        r"\s*:\s*(\d+(?:\.\d+)?)px\s*(?:!important\s*)?(?=;|})", css)
    scopes = []
    media = False
    failures = []
    for token in tokens:
        value = token[0]
        if value.startswith("@media"):
            media = True
        elif value == "{":
            scopes.append(media)
            media = False
        elif value == "}":
            scopes.pop()
        elif not any(scopes) and float(token[1]) > 360:
            failures.append((css.count("\n", 0, token.start()) + 1, value))
    return failures


class WindowLayoutTests(unittest.TestCase):
    def test_window_styles_have_no_oversized_fixed_widths(self):
        for page in PAGES:
            source = (ROOT / page).read_text(encoding="utf-8")
            styles = list(re.finditer(r"<style\b[^>]*>(.*?)</style>", source, re.S))
            self.assertTrue(styles, page)
            for style in styles:
                offset = source.count("\n", 0, style.start(1))
                for line, declaration in oversized_widths(style[1]):
                    self.fail(f"{page}:{offset + line}: {declaration}")

    def test_guard_distinguishes_caps_functions_and_media_scopes(self):
        css = """
        /* width: 999px; */
        .a { width: 360px; max-width: 820px; width: min(100%, 820px); }
        @media (min-width: 800px) {
          .b { width: 700px; }
          @supports (display: grid) { .c { min-width: 600px; } }
        }
        .d { width: 361px; min-width: 500px; inline-size: 900px; }
        """
        self.assertEqual([value for _, value in oversized_widths(css)],
                         ["width: 361px", "min-width: 500px", "inline-size: 900px"])


if __name__ == "__main__":
    unittest.main()
