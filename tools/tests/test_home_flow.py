"""Run Home's dependency-free DOM tests in the offline agent suite."""

from pathlib import Path
import shutil
import subprocess
import unittest


ROOT = Path(__file__).resolve().parents[2]


@unittest.skipUnless(shutil.which('node'), 'Node is needed to execute Home tests')
class HomeFlowTests(unittest.TestCase):
    def test_home_controller(self):
        result = subprocess.run(
            [shutil.which('node'), '--test', 'src/home.test.ts'],
            cwd=ROOT, capture_output=True, text=True, encoding='utf-8', timeout=60)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
