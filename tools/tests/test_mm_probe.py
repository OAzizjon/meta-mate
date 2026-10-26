"""Offline tests for provider probes, replay safety and click geometry."""

from contextlib import redirect_stdout
import io
import json
import os
from pathlib import Path
import shutil
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import mm_fixture as fixture
import mm_probe as probe


class ProbeTests(unittest.TestCase):
    def setUp(self) -> None:
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.root = Path(directory.name)
        self.replay = self.root / "fixtures" / "replay"
        self.ledger = self.root / "probe-ledger.jsonl"
        for name, value in (("ROOT", self.root), ("LEDGER", self.ledger),
                            ("REPLAY", self.replay)):
            self.enterContext(patch.object(probe, name, value))
        # Every request goes through probe._open (the no-redirect opener): no test reaches the network.
        self.urlopen = self.enterContext(patch.object(
            probe, "_open", side_effect=AssertionError("Unexpected request")))
        self.key = "fake-probe-key-for-tests"

    def write_secret(self, text: str) -> Path:
        path = self.root / probe.SECRETS_FILE
        path.write_text(text, encoding="utf-8")
        return path

    def copy_fixture(self) -> fixture.Fixture:
        source = Path(__file__).resolve().parents[2] / "fixtures" / "uia"
        folder = self.root / "fixtures" / "uia"
        folder.mkdir(parents=True)
        for suffix in (".json", ".png"):
            shutil.copyfile(source / ("synthetic-text-size" + suffix),
                            folder / ("synthetic-text-size" + suffix))
        return fixture.load_fixture(self.root, "synthetic-text-size")

    def test_iter_sse_multiline_comments_and_final_event(self) -> None:
        body = (": heartbeat\r\n\r\nevent: response.output_text.delta\r\n"
                "data: first\r\n: comment\r\ndata: second\r\n\r\n"
                "data: without event\n\nevent: final\ndata: last")
        expected = [("response.output_text.delta", "first\nsecond"),
                    (None, "without event"), ("final", "last")]
        for lines in (body.splitlines(keepends=True), body.encode().splitlines(keepends=True)):
            with self.subTest(line_type=type(lines[0])):
                self.assertEqual(list(probe.iter_sse(lines)), expected)

    def test_output_text_ignores_junk(self) -> None:
        response = {"output": [None, "junk", 7, {"type": "other"},
                               {"type": "message", "content": [None, "junk", 3,
                                {"type": "output_text", "text": "Hello "},
                                {"type": "text", "text": "world"},
                                {"type": "output_text", "text": 42},
                                {"type": "image", "text": "ignored"}]},
                               {"type": "message", "content": [
                                {"type": "output_text", "text": "!"}]}]}
        self.assertEqual(probe.output_text(response), "Hello world!")
        self.assertEqual(probe.output_text({}), "")

    def test_computer_actions_batch_single_and_safety(self) -> None:
        safety = [{"id": "check-1", "code": "confirm"}]
        click = {"type": "click", "x": 10, "y": 20}
        wait = {"type": "wait"}
        response = {"output": [None, "junk", 7,
                               {"type": "computer_call", "call_id": "meta-1",
                                "actions": [click, None, "junk", 9, wait],
                                "pending_safety_checks": safety},
                               {"type": "computer_call", "call_id": "openai-1",
                                "action": click},
                               {"type": "computer_call", "action": "junk"}]}
        self.assertEqual(probe.computer_actions(response), [
            {**click, "_call_id": "meta-1", "_safety": safety},
            {**wait, "_call_id": "meta-1", "_safety": safety},
            {**click, "_call_id": "openai-1", "_safety": []},
        ])
        self.assertEqual(probe.computer_actions({}), [])

    def test_point_actions_ids_coordinates_and_no_point(self) -> None:
        fx = self.copy_fixture()
        fmap = fixture.FrameMap(fx.shot_frame, (640, 400))
        # The physical centre travels with the action: no rounding through the sent image.
        self.assertEqual(probe.point_actions("<point e11> Click Large.", fx, fmap), [
            {"type": "point", "x": 530, "y": 136, "_phys": (1060.0, 271.0), "_safety": [], "_id": "e11"}])
        self.assertEqual(probe.point_actions("<point 795,301>", fx, fmap), [
            {"type": "point", "x": 795, "y": 301, "_safety": []}])
        for text in ("<point none>", "Cannot find it."):
            with self.subTest(text=text):
                self.assertEqual(probe.point_actions(text, fx, fmap), [])
        self.assertEqual(probe.point_actions("<point e999>", fx, fmap), [
            {"type": "point", "_safety": [], "_unknown_id": "e999"}])
        # A hidden or disabled element counts as unknown: the model was never shown it.
        for flag, value in (("offscreen", True), ("enabled", False)):
            with self.subTest(flag=flag):
                data = json.loads(fx.json_path.read_text(encoding="utf-8"))
                node = next(n for n in data["nodes"] if n["id"] == "e11")
                node[flag] = value
                fx.json_path.write_text(json.dumps(data), encoding="utf-8")
                hidden = fixture.load_fixture(self.root, fx.name)
                self.assertEqual(probe.point_actions("<point e11>", hidden, fmap), [
                    {"type": "point", "_safety": [], "_unknown_id": "e11"}])
                node[flag] = not value
                fx.json_path.write_text(json.dumps(data), encoding="utf-8")

    def test_action_points_click_double_click_and_drag(self) -> None:
        for kind in ("click", "double_click"):
            with self.subTest(kind=kind):
                self.assertEqual(probe.action_points({"type": kind, "x": 12, "y": 34}),
                                 [(12, 34)])
        self.assertEqual(probe.action_points({"type": "drag", "path": [
            {"x": 1, "y": 2}, {"x": 3, "y": 4}, {"x": 5, "y": 6}]}), [(1, 2), (5, 6)])
        for action in ({"type": "wait"}, {"type": "keypress", "keys": ["ENTER"]},
                       {"type": "click", "x": 12}, {"type": "drag", "path": []}):
            with self.subTest(action=action):
                self.assertEqual(probe.action_points(action), [])

    def test_verdict_bands(self) -> None:
        self.assertEqual(probe.verdict(fixture.Hit({"id": "e11"}, True, None)), "HIT")
        for distance, expected in ((8, "HIT"), (30, "NEAR"), (41, "MISS"), (None, "MISS")):
            with self.subTest(distance=distance):
                self.assertEqual(probe.verdict(fixture.Hit(None, False, distance)), expected)

    def test_cost_table_override_and_unknown(self) -> None:
        self.assertEqual(probe.cost_usd("muse-wonder-1.0", 1000000, 500000, None), 14.0)
        args = probe.build_parser().parse_args(["stream", "Hello", "--price", "2", "3"])
        for model in ("muse-wonder-1.0", "unknown-test-model"):
            with self.subTest(model=model):
                self.assertEqual(probe.cost_usd(model, 1000000, 500000, args.price), 3.5)
        self.assertIsNone(probe.cost_usd("unknown-test-model", 100, 200, None))

    def test_ledger_totals_and_budget_cap(self) -> None:
        self.assertEqual(probe.ledger_total("meta"), 0)
        for provider, usd in (("meta", 1.25), ("openai", 7), ("meta", None), ("meta", 0.75)):
            probe.ledger_add({"provider": provider, "usd": usd})
        self.assertEqual(probe.ledger_total("meta"), 2.0)
        self.assertEqual(probe.ledger_total("openai"), 7.0)
        self.assertEqual(probe.ledger_total("groq"), 0)
        probe.guard_budget("meta", 2.01)
        with self.assertRaisesRegex(probe.ProbeError, "budget reached"):
            probe.guard_budget("meta", 2.0)

    def test_scrub_removes_credentials(self) -> None:
        for text, key, expected in (
            ("error " + self.key, self.key, "error [key]"),
            ("Bearer x", "", "Bearer [key]"),
            ("sk-test-secret123", "", "[key]"),
            ("API key provided: private-value", "", "API key provided: [key]"),
        ):
            with self.subTest(text=text):
                self.assertEqual(probe.scrub(text, key), expected)

    def test_read_secret_plain_quotes_export_and_missing(self) -> None:
        self.assertEqual(probe.read_secret("MISSING"), "")
        self.write_secret("PLAIN=plain-value\nDOUBLE=\"double value\" # comment\n"
                          "SINGLE='single value' # comment\nexport EXPORTED=export-value\n")
        for name, expected in (("PLAIN", "plain-value"), ("DOUBLE", "double value"),
                               ("SINGLE", "single value"), ("EXPORTED", "export-value"),
                               ("MISSING", "")):
            with self.subTest(name=name):
                self.assertEqual(probe.read_secret(name), expected)

    def test_read_secret_refuses_hard_link(self) -> None:
        path = self.write_secret("TEST_KEY=private-value\n")
        os.link(path, self.root / "linked-secrets")
        self.assertGreater(path.stat().st_nlink, 1)
        self.assertEqual(probe.read_secret("TEST_KEY"), "")

    def test_read_secret_refuses_symlink(self) -> None:
        target = self.root / "secret-target"
        target.write_text("TEST_KEY=private-value\n", encoding="utf-8")
        path = self.root / probe.SECRETS_FILE
        try:
            path.symlink_to(target)
        except (OSError, NotImplementedError) as error:
            self.skipTest(f"Cannot create symlink: {error}")
        self.assertTrue(path.is_symlink())
        self.assertEqual(probe.read_secret("TEST_KEY"), "")

    def test_cmd_click_point_and_computer_replays_omit_credentials_and_image_data(self) -> None:
        self.copy_fixture()
        self.write_secret("META_API_KEY=" + self.key + "\n")
        for mode, output in (
            ("point", [{"type": "message", "content": [
                {"type": "output_text", "text": "<point e11> Click Large."}]}]),
            ("computer", [{"type": "computer_call", "call_id": "call-1", "actions": [
                {"type": "click", "x": 1060, "y": 271}]}]),
        ):
            with self.subTest(mode=mode):
                response = {"output": output, "usage": {"input_tokens": 10, "output_tokens": 5}}
                self.urlopen.side_effect = None
                self.urlopen.return_value = io.BytesIO(json.dumps(response).encode())
                args = probe.build_parser().parse_args([
                    "click", "synthetic-text-size", "Where is Large?", "--mode", mode,
                    "--max-edge", "0"])
                printed = io.StringIO()
                with redirect_stdout(printed):
                    self.assertEqual(probe.cmd_click(args), 0)
                self.assertRegex(printed.getvalue(), r"(?m)^HIT (point|click) at sent")
                self.assertIn('e11 RadioButton "Large"', printed.getvalue())
                request = self.urlopen.call_args.args[0]
                self.assertEqual(request.get_header("Authorization"), "Bearer " + self.key)
                self.assertIn("base64,", request.data.decode())
                paths = list(self.replay.rglob("*.json"))
                self.assertEqual(len(paths), 1)
                recorded = paths[0].read_text(encoding="utf-8")
                self.assertNotIn(self.key, recorded)
                self.assertNotIn("base64,", recorded)
                replay = json.loads(recorded)
                image = replay["request"]["input"][0]["content"][1]
                self.assertEqual(image["image_path"], "fixtures/uia/synthetic-text-size.png")
                self.assertEqual(image["sent_size"], [1280, 800])
                self.assertEqual(replay["response"], response)
                paths[0].unlink()

    def test_cmd_click_refuses_password_like_before_request(self) -> None:
        fx = self.copy_fixture()
        data = json.loads(fx.json_path.read_text(encoding="utf-8"))
        data["nodes"][0]["password_like"] = True
        fx.json_path.write_text(json.dumps(data), encoding="utf-8")
        args = probe.build_parser().parse_args(["click", fx.name, "Where is Large?"])
        with self.assertRaisesRegex(probe.ProbeError, "refused: Password-like"):
            probe.cmd_click(args)
        self.urlopen.assert_not_called()
        self.assertFalse(self.replay.exists())

    def test_cmd_stream_joins_deltas_and_records_first_token_time(self) -> None:
        self.write_secret("META_API_KEY=" + self.key + "\n")
        for terminal in ("completed", "done"):
            with self.subTest(terminal=terminal):
                body = ('event: response.output_text.delta\ndata: {"delta": "Hello "}\n\n'
                        'data: {"type": "response.output_text.delta", "delta": "world"}\n\n')
                if terminal == "completed":
                    body += ('data: {"type": "response.completed", "response": '
                             '{"usage": {"input_tokens": 10, "output_tokens": 2}}}\n\n')
                else:
                    body += 'data: [DONE]\n\n'
                args = probe.build_parser().parse_args(["stream", "Say hello"])
                printed = io.StringIO()
                with patch.object(probe, "_request", return_value=io.BytesIO(body.encode())) as request:
                    with redirect_stdout(printed):
                        self.assertEqual(probe.cmd_stream(args), 0)
                self.assertTrue(request.call_args.kwargs["stream"])
                self.assertIn("Hello world", printed.getvalue())
                self.assertIn("first token ", printed.getvalue())
                paths = list(self.replay.rglob("*.json"))
                self.assertEqual(len(paths), 1)
                replay = json.loads(paths[0].read_text(encoding="utf-8"))
                self.assertEqual(replay["text"], "Hello world")
                self.assertIsInstance(replay["first_token_ms"], int)
                self.assertGreaterEqual(replay["first_token_ms"], 0)
                paths[0].unlink()

    def test_cmd_stream_incomplete_records_no_replay(self) -> None:
        self.write_secret("META_API_KEY=" + self.key + "\n")
        body = b'data: {"type": "response.output_text.delta", "delta": "Partial"}\n\n'
        args = probe.build_parser().parse_args(["stream", "Say hello"])
        with patch.object(probe, "_request", return_value=io.BytesIO(body)):
            with redirect_stdout(io.StringIO()):
                with self.assertRaisesRegex(probe.ProbeError, "stream not completed"):
                    probe.cmd_stream(args)
        self.assertEqual(list(self.replay.rglob("*.json")), [])

    def test_cmd_stream_failures_record_nothing(self) -> None:
        self.write_secret("META_API_KEY=" + self.key + "\n")
        for name, body in (
            ("done without text", b"data: [DONE]\n\n"),
            ("incomplete status", b'data: {"type": "response.output_text.delta", "delta": "Hi"}\n\n'
                                  b'data: {"type": "response.completed", "response": {"status": "incomplete"}}\n\n'),
            ("error event", b'data: {"type": "response.output_text.delta", "delta": "Hi"}\n\n'
                            b'data: {"type": "error", "error": {"message": "boom"}}\n\ndata: [DONE]\n\n'),
        ):
            with self.subTest(name=name):
                args = probe.build_parser().parse_args(["stream", "Say hello"])
                with patch.object(probe, "_request", return_value=io.BytesIO(body)):
                    with redirect_stdout(io.StringIO()):
                        with self.assertRaisesRegex(probe.ProbeError, "stream not completed"):
                            probe.cmd_stream(args)
                self.assertEqual(list(self.replay.rglob("*.json")), [])

    def test_unpriced_calls_are_capped_and_reported_cost_counts(self) -> None:
        self.assertEqual(probe.cost_usd("unknown-test-model", 10, 20, None, 0.0123), 0.0123)
        self.assertIsNone(probe.cost_usd("unknown-test-model", 10, 20, None, True))
        for _ in range(3):
            probe.ledger_add({"provider": "meta", "usd": None})
        probe.guard_budget("meta", 10, max_unpriced=4)
        with self.assertRaisesRegex(probe.ProbeError, "unknown price"):
            probe.guard_budget("meta", 10, max_unpriced=3)
        # A corrupt line must not hide spending: it counts as a call of unknown price.
        with self.ledger.open("a", encoding="utf-8") as f:
            f.write("{not json\n")
        self.assertEqual(probe.ledger_unpriced("meta"), 4)

    def test_scrub_masked_and_other_prefixes(self) -> None:
        key = "gsk_" + "A1b2C3d4" * 4
        for text in ("invalid token " + key[:8] + "...." + key[-6:], "key " + key, '{"api_key": "abcdefghijklmnop"}'):
            with self.subTest(text=text):
                out = probe.scrub(text, key)
                self.assertNotIn(key[:8], out)
                self.assertNotIn(key[-6:], out)
                self.assertNotIn("abcdefghijklmnop", out)

    def test_record_scrubs_the_key(self) -> None:
        path = probe.record("case", {"provider": "meta", "echo": "header Bearer " + self.key}, self.key)
        self.assertNotIn(self.key, path.read_text(encoding="utf-8"))

    def test_redirects_are_refused(self) -> None:
        handler = probe._NoRedirect()
        with self.assertRaisesRegex(probe.ProbeError, "redirect refused"):
            handler.redirect_request(None, None, 302, "Found", {}, "https://elsewhere.example/")

    def test_key_alias_is_read_and_the_main_name_wins(self) -> None:
        self.write_secret("META_KEY=" + self.key + "\n")
        self.assertEqual(probe.provider_config("meta")[1], self.key)
        self.write_secret("META_KEY=alias-key\nMETA_API_KEY=" + self.key + "\n")
        self.assertEqual(probe.provider_config("meta")[1], self.key)
        self.write_secret("OTHER=1\n")
        with self.assertRaisesRegex(probe.ProbeError, "META_API_KEY or META_KEY"):
            probe.provider_config("meta")

    def test_http_base_url_is_refused(self) -> None:
        self.write_secret("META_API_KEY=" + self.key + "\nMETA_API_BASE=http://api.example\n")
        with self.assertRaisesRegex(probe.ProbeError, "https://"):
            probe.provider_config("meta")

    def test_cmd_click_refuses_a_png_of_the_wrong_size(self) -> None:
        fx = self.copy_fixture()
        data = json.loads(fx.json_path.read_text(encoding="utf-8"))
        data["window"]["shot_frame"] = [0, 0, 1000, 800]
        fx.json_path.write_text(json.dumps(data), encoding="utf-8")
        args = probe.build_parser().parse_args(["click", fx.name, "Where is Large?", "--dry-run"])
        with self.assertRaisesRegex(probe.ProbeError, "is not 1000x800"):
            probe.cmd_click(args)
        self.urlopen.assert_not_called()

    def test_cmd_click_reports_api_errors_and_non_point_actions(self) -> None:
        self.write_secret("META_API_KEY=" + self.key + "\n")
        fx = self.copy_fixture()
        args = probe.build_parser().parse_args(["click", fx.name, "Scroll down", "--mode", "computer", "--price", "1", "1"])
        error = {"error": {"message": "bad key " + self.key}}
        self.urlopen.side_effect = None
        self.urlopen.return_value = io.BytesIO(json.dumps(error).encode())
        with redirect_stdout(io.StringIO()):
            with self.assertRaises(probe.ProbeError) as caught:
                probe.cmd_click(args)
        self.assertNotIn(self.key, str(caught.exception))
        keypress = {"output": [{"type": "computer_call", "call_id": "c1",
                                "actions": [{"type": "keypress", "keys": ["PAGEDOWN"]}]}]}
        self.urlopen.return_value = io.BytesIO(json.dumps(keypress).encode())
        printed = io.StringIO()
        with redirect_stdout(printed):
            self.assertEqual(probe.cmd_click(args), 0)
        self.assertIn("ACTION keypress without coordinates", printed.getvalue())


if __name__ == "__main__":
    unittest.main()
