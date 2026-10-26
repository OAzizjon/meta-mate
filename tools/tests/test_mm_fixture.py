"""Tests for recorded UIA fixture safety and physical pixel geometry."""

import json
from pathlib import Path
import struct
import sys
import tempfile
import unittest
import zlib

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import mm_fixture as fixture


def node(identifier: str, kind: str = "Button", rect: list | None = None, **flags) -> dict:
    return {"id": identifier, "type": kind, "name": "Text size", "rect": rect,
            "enabled": True, "offscreen": False, "password": False,
            "password_like": False, **flags}


class FixtureTests(unittest.TestCase):
    def setUp(self) -> None:
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.root = Path(directory.name)
        self.folder = self.root / "fixtures" / "uia"
        self.folder.mkdir(parents=True)
        self.data = {"window": {"frame": [-100, 20, 80, 60],
                                "shot_frame": [-100, 20, 100, 80], "truncated": False,
                                "dpi_awareness": "per-monitor"},
                     "nodes": [node("e17", rect=[-90, 30, 20, 10])]}

    def write(self, png: bool = True) -> fixture.Fixture:
        (self.folder / "demo.json").write_text(json.dumps(self.data), encoding="utf-8")
        if png:
            def chunk(kind: bytes, payload: bytes) -> bytes:
                return (struct.pack(">I", len(payload)) + kind + payload
                        + struct.pack(">I", zlib.crc32(kind + payload)))

            # A real synthetic RGB PNG matching shot_frame, with no image dependency.
            _, _, w, h = self.data["window"].get("shot_frame", self.data["window"]["frame"])
            image = (b"\x89PNG\r\n\x1a\n"
                     + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0))
                     + chunk(b"IDAT", zlib.compress((b"\0" * (1 + 3 * w)) * h))
                     + chunk(b"IEND", b""))
            (self.folder / "demo.png").write_bytes(image)
        return fixture.load_fixture(self.root, "demo")

    def test_unsafe_without_per_monitor_dpi(self) -> None:
        for awareness in ("system", "unaware", None):
            with self.subTest(awareness=awareness):
                self.data["window"]["dpi_awareness"] = awareness
                self.assertIn("DPI", fixture.unsafe_reason(self.write()))

    def test_load_and_legacy_frame(self) -> None:
        fx = self.write()
        self.assertEqual(fx.name, "demo")
        self.assertEqual(fx.shot_frame, (-100, 20, 100, 80))
        self.assertEqual(fx.nodes, self.data["nodes"])
        self.assertEqual(fx.window, self.data["window"])
        self.assertEqual(fx.json_path, self.folder / "demo.json")
        self.assertEqual(fx.png_path, self.folder / "demo.png")
        self.assertIsNone(fixture.unsafe_reason(fx))
        del self.data["window"]["shot_frame"]
        self.assertEqual(self.write().shot_frame, (-100, 20, 80, 60))

    def test_missing_invalid_file_and_name(self) -> None:
        with self.assertRaisesRegex(ValueError, "Cannot load fixture"):
            fixture.load_fixture(self.root, "missing")
        for content in ("{", "[]", '{"window": {}}',
                        '{"window": {"frame": [0, 0, 0, 5]}, "nodes": []}'):
            with self.subTest(content=content):
                (self.folder / "demo.json").write_text(content, encoding="utf-8")
                with self.assertRaises(ValueError):
                    fixture.load_fixture(self.root, "demo")
        for name in ("../demo", "demo.json", "", "x" * 61):
            with self.subTest(name=name), self.assertRaises(ValueError):
                fixture.load_fixture(self.root, name)

    def test_invalid_nodes_and_geometry(self) -> None:
        for nodes in ({}, [None], [{}], [node("e0", rect=[0, 0, -1, 4])],
                      [node("e0", rect=[0, 0, float("nan"), 4])]):
            self.data["nodes"] = nodes
            with self.subTest(nodes=nodes), self.assertRaisesRegex(ValueError, "Invalid fixture"):
                self.write()

    def test_unsafe_password_flags_even_offscreen_or_disabled(self) -> None:
        for flag, expected in (("password", "Password"), ("password_like", "Password-like")):
            for value in (True, None, "false"):
                with self.subTest(flag=flag, value=value):
                    self.data["nodes"] = [node("e0", **{flag: value, "offscreen": True,
                                                       "enabled": False})]
                    self.assertIn(expected, fixture.unsafe_reason(self.write()))
            del self.data["nodes"][0][flag]
            self.assertIsNotNone(fixture.unsafe_reason(self.write()))

    def test_missing_png_and_truncated(self) -> None:
        fx = self.write(png=False)
        self.assertIsNone(fx.png_path)
        self.assertIn("Missing PNG", fixture.unsafe_reason(fx))
        for value in (True, None, "false"):
            self.data["window"]["truncated"] = value
            self.assertIn("truncated", fixture.unsafe_reason(self.write()))
        del self.data["window"]["truncated"]
        self.assertIsNotNone(fixture.unsafe_reason(self.write()))
        fx = self.write()
        fx.window["truncated"] = False
        fx.png_path.unlink()
        self.assertIn("Missing PNG", fixture.unsafe_reason(fx))

    def test_element_coordinates_filters_and_stable_order(self) -> None:
        self.data["nodes"] = [node("e0", "Pane", [-100, 20, 100, 80]),
                              node("e1", rect=[-90, 30, 10, 10]),
                              node("e2", rect=[-100, 20, 80, 60]),
                              node("e3", rect=[-110, 10, 20, 20]),
                              node("e4", rect=[0, 20, 10, 10]),
                              node("e5", rect=None),
                              node("e6", rect=[-90, 30, 5, 5], enabled=False),
                              node("e7", rect=[-90, 30, 5, 5], offscreen=True),
                              node("e8", rect=[-90, 30, 0, 5])]
        fx = self.write()
        fmap = fixture.FrameMap(fx.shot_frame, (50, 40))
        self.assertEqual(fixture.element_list(fx, fmap), [
            'e1 Button "Text size" [5,5,5,5]',
            'e2 Button "Text size" [0,0,40,30]',
            'e3 Button "Text size" [0,0,5,5]',
            'e0 Pane "Text size" [0,0,50,40]',
        ])
        self.assertEqual(len(fixture.element_list(fx, fmap, limit=2)), 2)
        self.assertEqual(fixture.element_list(fx, fmap, limit=0), [])
        self.assertEqual(fixture.element_list(fx, fmap, limit=-1), [])

    def test_hit_prefers_smallest_interactive_and_reports_distance(self) -> None:
        self.data["nodes"] = [node("e0", "Pane", [-100, 20, 100, 80]),
                              node("e1", rect=[-90, 30, 40, 40]),
                              node("e2", "Text", [-78, 42, 2, 2]),
                              node("e3", rect=[-80, 40, 10, 10]),
                              node("e4", rect=[-78, 42, 1, 1], enabled=False)]
        fx = self.write()
        hit = fixture.hit_test(fx, -77, 43)
        self.assertEqual(hit.node["id"], "e3")
        self.assertTrue(hit.interactive)
        self.assertIsNone(hit.distance_px)
        self.assertIn('e3 Button "Text size" (interactive)', fixture.describe(hit))
        hit = fixture.hit_test(fx, -47, 74)
        self.assertEqual(hit.node["id"], "e0")
        self.assertFalse(hit.interactive)
        self.assertEqual(hit.distance_px, 5.0)
        hit = fixture.hit_test(fx, 3, 104)
        self.assertIsNone(hit.node)
        self.assertFalse(hit.interactive)
        self.assertAlmostEqual(hit.distance_px, (53 ** 2 + 34 ** 2) ** 0.5)
        self.assertIn("nearest interactive edge", fixture.describe(hit))

    def test_no_interactive_distance_and_single_line_description(self) -> None:
        self.data["nodes"] = [node("e0", "Text", [-90, 30, 10, 10], name='a\n"b')]
        fx = self.write()
        hit = fixture.hit_test(fx, -85, 35)
        self.assertIsNone(hit.distance_px)
        self.assertNotIn("\n", fixture.describe(hit))
        self.assertEqual(fixture.describe(fixture.hit_test(fx, 0, 0)), "miss")


class FrameMapTests(unittest.TestCase):
    def test_round_trips_with_negative_origin(self) -> None:
        for size in ((1919, 1080), (960, 540), (1280, 720)):
            frame = (-1919, -100, 1919, 1080)
            # Exact half scale uses even physical dimensions.
            if size == (960, 540):
                frame = (-1919, -100, 1920, 1080)
            fmap = fixture.FrameMap(frame, size)
            for sent in ((0, 0), (11, 23), (size[0] - 1, size[1] - 1)):
                with self.subTest(size=size, sent=sent):
                    self.assertEqual(fmap.to_sent(*fmap.to_phys(*sent)), sent)
            for phys in ((frame[0], -100), (-1234, 411), (-1, 979)):
                restored = fmap.to_phys(*fmap.to_sent(*phys))
                self.assertLessEqual(abs(restored[0] - phys[0]), 1)
                self.assertLessEqual(abs(restored[1] - phys[1]), 1)

    def test_round_half_up(self) -> None:
        fmap = fixture.FrameMap((-100, -100, 100, 100), (100, 100))
        self.assertEqual(fmap.to_sent(-97.5, -96.5), (3, 4))
        # Half up everywhere: the same point maps the same way at negative X (-99.5 -> -99).
        self.assertEqual(fmap.to_phys(0.5, 1.5), (-99, -98))

    def test_fit_and_invalid_dimensions(self) -> None:
        fmap = fixture.FrameMap((-1919, 0, 1919, 1080), (1280, 720))
        self.assertEqual(fmap.fit(1280), (1280, 720))
        self.assertEqual(fmap.fit(4000), (1919, 1080))
        self.assertEqual(fixture.FrameMap((0, 0, 10, 20), (10, 20)).fit(10), (5, 10))
        self.assertEqual(fixture.FrameMap((0, 0, 1, 100), (1, 100)).fit(1), (1, 1))
        for limit in (0, -1):
            with self.assertRaises(ValueError):
                fmap.fit(limit)
        for frame, size in (((0, 0, 0, 10), (1, 1)), ((0, 0, 10, 10), (0, 1))):
            with self.assertRaises(ValueError):
                fixture.FrameMap(frame, size)


if __name__ == "__main__":
    unittest.main()
