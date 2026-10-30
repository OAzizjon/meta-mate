"""The code tools never read a secrets-named file, whatever its extension or case (review #1170)."""

import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import mm_map
import mm_stats


class SecretFilterTests(unittest.TestCase):
    def test_map_and_stats_skip_secrets_named_sources(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            (root / 'tools').mkdir()
            for name in ('.env.py', '.ENV.local.py'):
                (root / 'tools' / name).write_text('SECRET_VALUE = 1\n', encoding='utf-8')
            (root / 'tools' / 'ok.py').write_text('"""Ok."""\ndef visible():\n    pass\n', encoding='utf-8')
            text = mm_map.render(root)
            self.assertNotIn('SECRET_VALUE', text)
            self.assertNotIn('.env', text.lower())
            self.assertIn('visible', text)
            self.assertEqual([p.name for p in mm_stats.source_files(root)], ['ok.py'])


if __name__ == '__main__':
    unittest.main()
