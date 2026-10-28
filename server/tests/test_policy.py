"""Pure proxy request and spending policy tests."""

from copy import deepcopy
from datetime import date, timedelta
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import Mock, patch

# A real 1x1 PNG built here: every image must be a valid embedded PNG.
def _png_1x1() -> str:
    import base64
    import struct
    import zlib

    def chunk(kind: bytes, data: bytes) -> bytes:
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))

    png = (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", 1, 1, 8, 2, 0, 0, 0))
           + chunk(b"IDAT", zlib.compress(b"\x00\xff\xff\xff")) + chunk(b"IEND", b""))
    return "data:image/png;base64," + base64.b64encode(png).decode()


TINY_PNG = _png_1x1()

from metamate_proxy.policy import (
    ALLOWED_MODELS, ANSWER_CAP, COMPUTER_CAP, FRAMING_TOKENS, IMAGE_TOKENS, INTENT_CAP,
    MAX_BODY_BYTES, MAX_IMAGES, PREVIOUS_TURN_TOKENS, SPEECH_CAP,
    Budget, CallKind, Decision, InviteLimits, JUDGE, Ledger, Prices, ResponseOwners,
    SAM, SAM_CALL_USD, SPARK, TESTER, WONDER, actual_cost, body_for, clamp_request, decide,
    input_token_bound, reconcile, reserve,
)

# The model-answer kinds; SAM has its own fixed shape (SamTests).
TEXT_KINDS = [kind for kind in CallKind if kind != CallKind.SAM]


class LedgerFileTests(unittest.TestCase):
    def test_round_trip_all_daily_counters_and_recent_dates(self):
        from metamate_proxy.policy import DayTotals, Totals

        today = date(2026, 10, 29)
        totals = DayTotals(overall=Totals(3, 0.3), invites={'inv': Totals(3, 0.3)},
                           wonder={'inv': Totals(1, 0.1)}, asr_seconds={'inv': 1.5},
                           tts_chars={'inv': 10}, sam={'inv': Totals(1, 0.02)},
                           images={'inv': Totals(1, 0.01)}, glimmer={'inv': 2})
        with tempfile.TemporaryDirectory() as folder:
            path = str(Path(folder) / 'ledger.json')
            original = Ledger({today - timedelta(days=n): deepcopy(totals) for n in (-1, 0, 1, 2)}, path)
            original.save()
            loaded = Ledger(path=path)
            loaded.load(today)
            self.assertEqual(loaded.days, {today: totals, today - timedelta(days=1): totals})
            self.assertEqual(loaded._pending, {})

    def test_corrupt_files_start_empty_and_log(self):
        today = date(2026, 10, 29)
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / 'ledger.json'
            for content in ('{', '[]', '{"2026-10-29": {"overall": {"usd": -1}}}',
                            '{"2026-10-29": null}'):
                path.write_text(content, encoding='utf-8')
                loaded = Ledger(path=str(path))
                with self.assertLogs('metamate_proxy.policy', level='WARNING'):
                    loaded.load(today)
                self.assertEqual(loaded.days, {})

    def test_charges_settlements_refunds_and_pending_reservations(self):
        from metamate_proxy.policy import (IMAGE, charge_glimmer, charge_tts, refund_image,
                                           reserve_asr, settle_asr)

        today = date(2026, 10, 29)
        prices = Prices(1, 1, 1, 1)
        with tempfile.TemporaryDirectory() as folder:
            path = str(Path(folder) / 'ledger.json')
            commit = Mock()
            ledger = Ledger(path=path, commit=commit)

            def reload():
                result = Ledger(path=path)
                result.load(today)
                return result.days[today]

            settled = reserve(ledger, 'inv', JUDGE, Budget(10), WONDER, 0.1, today)
            pending = reserve(ledger, 'inv', JUDGE, Budget(10), SPARK, 0.2, today)
            self.assertEqual(reload().overall.calls, 0)
            reconcile(ledger, settled, {'input_tokens': 100, 'output_tokens': 50}, prices)
            self.assertEqual(reload().overall.calls, 1)
            self.assertAlmostEqual(reload().overall.usd, 0.00015)
            reconcile(ledger, pending, None, prices)
            self.assertAlmostEqual(reload().overall.usd, 0.20015)
            image = reserve(ledger, 'inv', JUDGE, Budget(10), IMAGE, 0.01, today)
            reconcile(ledger, image, None, prices)
            refund_image(ledger, image)
            self.assertEqual(reload().images['inv'].usd, 0)
            charge_tts(ledger, 'inv', 25, today)
            self.assertEqual(reload().tts_chars['inv'], 25)
            charge_glimmer(ledger, 'inv', today)
            self.assertEqual(reload().glimmer['inv'], 1)
            seconds = reserve_asr(ledger, 'inv', JUDGE, 60, today)
            self.assertEqual(reload().asr_seconds['inv'], 0)
            settle_asr(ledger, 'inv', seconds, 12, today)
            self.assertEqual(reload().asr_seconds['inv'], 12)
            self.assertEqual(commit.call_count, 11)
            self.assertEqual(list(Path(folder).iterdir()), [Path(path)])

    def test_failed_replace_preserves_previous_snapshot(self):
        from metamate_proxy.policy import charge_tts

        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / 'ledger.json'
            ledger = Ledger(path=str(path))
            charge_tts(ledger, 'inv', 1, date(2026, 10, 29))
            before = path.read_bytes()
            with patch('metamate_proxy.policy.os.replace', side_effect=OSError('disk error')):
                with self.assertRaises(OSError):
                    ledger.save()
            self.assertEqual(path.read_bytes(), before)
            self.assertEqual(list(Path(folder).iterdir()), [path])


class RequestTests(unittest.TestCase):
    def test_caps_and_model_forcing_for_every_kind(self):
        caps = (INTENT_CAP, SPEECH_CAP, ANSWER_CAP, COMPUTER_CAP)
        self.assertEqual(caps, (200, 300, 1200, 1200))
        for kind, cap in zip(TEXT_KINDS, caps):
            with self.subTest(kind=kind):
                for requested in (None, 10, 10000):
                    body = {"model": "untrusted", "stream": True}
                    if requested is not None:
                        body["max_output_tokens"] = requested
                    result = clamp_request(body, kind, WONDER)
                    self.assertEqual(result["max_output_tokens"],
                                     cap if requested is None else min(requested, cap))
                    self.assertEqual(result["model"], SPARK if kind in (
                        CallKind.INTENT, CallKind.SPEECH) else WONDER)
                    self.assertIs(result["stream"], True)
                    self.assertEqual(clamp_request(body, kind, SPARK)["model"], SPARK)

    def test_allowlist_and_original_body(self):
        body = {
            "model": WONDER, "input": [{"role": "user", "content": "Hello"}],
            "instructions": "Answer briefly", "tools": [], "stream": False,
            "previous_response_id": "previous", "truncation": "auto",
            "reasoning": {"effort": "high", "unknown": True},
            "unknown": True, "max_output_tokens": 999,
        }
        original = deepcopy(body)
        result = clamp_request(body, CallKind.ANSWER, WONDER)
        self.assertIsNot(result, body)
        self.assertEqual(body, original)
        self.assertEqual(set(result), set(body) - {"unknown", "tools"})
        self.assertEqual(result["reasoning"], {"effort": "minimal"})
        self.assertIs(result["stream"], False)
        result["reasoning"]["effort"] = "low"
        self.assertEqual(body, original)

    def test_effort_rules(self):
        for kind in TEXT_KINDS:
            for effort in ("minimal", "low", "medium", "high", "none", None, []):
                with self.subTest(kind=kind, effort=effort):
                    result = clamp_request({"reasoning": {"effort": effort}}, kind, SPARK)
                    expected = "low" if effort == "low" and kind in (
                        CallKind.ANSWER, CallKind.COMPUTER) else "minimal"
                    self.assertEqual(result["reasoning"], {"effort": expected})
            for reasoning in (None, {}, "invalid"):
                self.assertEqual(clamp_request({"reasoning": reasoning}, kind, SPARK)
                                 ["reasoning"], {"effort": "minimal"})

    def test_invalid_models_and_token_limits(self):
        self.assertEqual(ALLOWED_MODELS, {SPARK, WONDER})
        for kind in (CallKind.ANSWER, CallKind.COMPUTER):
            with self.assertRaises(ValueError):
                clamp_request({}, kind, "other")
        for value in (-1, True, 1.5, "600", None):
            with self.subTest(value=value), self.assertRaises(ValueError):
                clamp_request({"max_output_tokens": value}, CallKind.ANSWER, SPARK)


    def test_tools_are_only_the_computer_tool(self):
        for kind in TEXT_KINDS:
            for tools in (None, [], [{"type": "web_search"}, {"type": "computer",
                                                                  "extra": True}]):
                body = {} if tools is None else {"tools": tools}
                original = deepcopy(body)
                result = clamp_request(body, kind, SPARK)
                if kind == CallKind.COMPUTER:
                    self.assertEqual(result["tools"], [{"type": "computer"}])
                else:
                    self.assertNotIn("tools", result)
                self.assertEqual(body, original)

    def test_nested_image_limit(self):
        self.assertEqual(MAX_IMAGES, 2)
        images = [{"type": "input_image", "image_url": TINY_PNG}]
        body = {"input": [{"role": "user", "content": images},
                          {"role": "user", "content": deepcopy(images)}]}
        clamp_request(body, CallKind.COMPUTER, SPARK)
        body["input"].append({"role": "user", "content": deepcopy(images)})
        with self.assertRaisesRegex(ValueError, "images"):
            clamp_request(body, CallKind.COMPUTER, SPARK)

    def test_json_body_byte_limit(self):
        self.assertEqual(MAX_BODY_BYTES, 2_500_000)
        body = {"input": ""}
        overhead = len(json.dumps(body).encode("utf-8"))
        body["input"] = "a" * (MAX_BODY_BYTES - overhead)
        clamp_request(body, CallKind.ANSWER, SPARK)
        body["input"] += "a"
        with self.assertRaisesRegex(ValueError, "too large"):
            clamp_request(body, CallKind.ANSWER, SPARK)
        for body in ({"input": "a" * 3_000_000}, {"unknown": "a" * 3_000_000},
                     {"input": "\u0430" * 500_000}):
            with self.assertRaisesRegex(ValueError, "too large"):
                clamp_request(body, CallKind.ANSWER, SPARK)

    def test_input_bound_includes_images_and_previous_context(self):
        body = {"input": "abc", "instructions": "de"}
        base = input_token_bound(clamp_request(body, CallKind.ANSWER, SPARK))
        self.assertEqual(base, FRAMING_TOKENS + 5)
        # One token per UTF-8 byte: Cyrillic counts twice.
        self.assertEqual(input_token_bound({"input": "\u0430" * 10}), FRAMING_TOKENS + 20)
        body["input"] = [{"role": "user", "content": "abc"},
                         {"role": "user", "content": [{"type": "input_image", "image_url": TINY_PNG}]}]
        with_image = input_token_bound(clamp_request(body, CallKind.ANSWER, SPARK))
        self.assertEqual(with_image, base + 2 * len("user") + IMAGE_TOKENS)
        body["previous_response_id"] = "previous"
        self.assertEqual(input_token_bound(clamp_request(body, CallKind.ANSWER, SPARK)),
                         with_image + PREVIOUS_TURN_TOKENS)


class ResponseOwnerTests(unittest.TestCase):
    def test_foreign_and_unknown_ids_refused(self):
        owners = ResponseOwners()
        owners.record("response", "alice")
        self.assertTrue(owners.check("response", "alice"))
        self.assertFalse(owners.check("response", "bob"))
        self.assertFalse(owners.check("unknown", "alice"))

    def test_oldest_ids_evicted_at_cap(self):
        owners = ResponseOwners()
        for index in range(10_000):
            owners.record(str(index), "alice")
        self.assertTrue(owners.check("0", "alice"))
        owners.record("9999", "alice")
        owners.record("10000", "bob")
        self.assertFalse(owners.check("0", "alice"))
        self.assertTrue(owners.check("1", "alice"))
        self.assertTrue(owners.check("10000", "bob"))
        owners.record("10001", "bob")
        self.assertFalse(owners.check("1", "alice"))


class SpendingTests(unittest.TestCase):
    def setUp(self):
        self.today = date(2026, 10, 27)
        self.ledger = Ledger()
        self.limits = InviteLimits(100, 10, 50)
        self.budget = Budget(10)
        # Synthetic prices for tests, not real Billing prices.
        self.prices = Prices(spark_input=5, spark_output=25,
                             wonder_input=4, wonder_output=20)

    def reserve(self, model=WONDER, cost=1, invite="tester", today=None):
        return reserve(self.ledger, invite, self.limits, self.budget,
                       model, cost, today or self.today)

    def decide(self, model=WONDER, cost=1, limits=None, today=None):
        return decide(self.ledger, "tester", limits or self.limits, self.budget,
                      model, cost, today or self.today)

    def test_prices_and_usage(self):
        self.assertAlmostEqual(actual_cost(WONDER, {
            "input_tokens": 1000, "output_tokens": 2000,
            "output_tokens_details": {"reasoning_tokens": 1000},
        }, self.prices), 0.044)
        self.assertAlmostEqual(actual_cost(SPARK, {
            "input_tokens": 1000, "output_tokens": 2000,
        }, self.prices), 0.055)
        from metamate_proxy.policy import estimate_cost
        self.assertEqual(estimate_cost(WONDER, 1000, 2000, self.prices), 0.044)
        with self.assertRaises(TypeError):
            Prices()
        with self.assertRaises(TypeError):
            Prices(spark_input=5, spark_output=25)
        with self.assertRaises(TypeError):
            Prices(spark_input=5, spark_output=25, wonder_input=4)
        with self.assertRaises(ValueError):
            estimate_cost("other", 1, 1, self.prices)

    def test_wonder_downgrade_by_calls_and_usd(self):
        self.reserve(cost=1)
        self.assertEqual(self.decide(limits=InviteLimits(100, 1, 50)),
                         Decision(SPARK, "wonder_allowance"))
        self.assertEqual(self.decide(limits=InviteLimits(100, 10, 1)),
                         Decision(SPARK, "wonder_allowance"))
        self.assertEqual(self.decide(cost=1, limits=InviteLimits(100, 10, 1.5)),
                         Decision(SPARK, "wonder_allowance"))
        self.assertEqual(self.decide(cost=1, limits=InviteLimits(100, 10, 2)),
                         Decision(WONDER))

    def test_spark_reserve_keeps_spark_alive(self):
        self.reserve(cost=7)
        self.assertEqual(self.decide(), Decision(SPARK, "wonder_budget"))
        fallback = self.reserve(cost=1, invite="another")
        self.assertEqual(fallback.model, SPARK)
        self.assertEqual(fallback.reason, "wonder_budget")
        self.assertNotIn("another", self.ledger.days[self.today].wonder)
        self.assertEqual(self.decide(SPARK, 2), Decision(SPARK))
        self.reserve(SPARK, 2)
        self.assertEqual(self.decide(SPARK, 0), Decision(None, "daily_budget"))

    def test_body_for_forwards_downgraded_model(self):
        self.reserve(cost=7)
        pending = self.reserve(cost=1)
        self.assertEqual(pending.model, SPARK)
        body = {"model": WONDER, "tools": [{"type": "web_search"}]}
        forwarded = body_for(pending, body, CallKind.COMPUTER)
        self.assertEqual(forwarded, clamp_request(body, CallKind.COMPUTER, SPARK))
        self.assertEqual(forwarded["model"], SPARK)
        self.assertEqual(body["model"], WONDER)

    def test_upstream_model_mismatch_keeps_full_reservation(self):
        for model, upstream in ((WONDER, SPARK), (SPARK, WONDER), (SPARK, "other")):
            with self.subTest(model=model, upstream=upstream):
                pending = self.reserve(model=model)
                before = deepcopy(self.ledger.days[self.today])
                self.assertEqual(reconcile(self.ledger, pending,
                                           {"input_tokens": 0, "output_tokens": 0},
                                           self.prices, upstream), "model_mismatch")
                self.assertEqual(self.ledger.days[self.today], before)
                with self.assertRaisesRegex(ValueError, "already reconciled"):
                    reconcile(self.ledger, pending, None, self.prices)

    def test_matching_upstream_model_reconciles(self):
        pending = self.reserve()
        self.assertIsNone(reconcile(self.ledger, pending,
                                    {"input_tokens": 0, "output_tokens": 0},
                                    self.prices, WONDER))
        self.assertEqual(self.ledger.days[self.today].overall.usd, 0)

    def test_overall_spend_also_limits_wonder(self):
        self.reserve(SPARK, 6)
        self.assertEqual(self.decide(cost=2), Decision(SPARK, "wonder_budget"))

    def test_invite_and_daily_budget_refusals(self):
        self.reserve(SPARK, 1)
        self.assertEqual(self.decide(SPARK, 0, InviteLimits(1, 10, 1)),
                         Decision(None, "invite_budget"))
        self.assertEqual(self.decide(SPARK, 1, InviteLimits(1.5, 10, 1)),
                         Decision(None, "invite_budget"))
        self.assertEqual(self.decide(SPARK, 10), Decision(None, "daily_budget"))
        with self.assertRaisesRegex(ValueError, "daily_budget"):
            self.reserve(SPARK, 10)
        self.assertEqual(self.ledger.days[self.today].overall.calls, 1)

    def test_day_rollover_and_late_reconciliation(self):
        previous = self.reserve(cost=7)
        tomorrow = self.today + timedelta(days=1)
        self.assertEqual(self.decide(today=tomorrow), Decision(WONDER))
        self.assertNotIn(tomorrow, self.ledger.days)
        self.reserve(cost=2, today=tomorrow)
        reconcile(self.ledger, previous, {"input_tokens": 0, "output_tokens": 1000},
                  self.prices)
        self.assertAlmostEqual(self.ledger.days[self.today].overall.usd, 0.02)
        self.assertEqual(self.ledger.days[tomorrow].overall.usd, 2)

    def test_judge_has_higher_limits(self):
        self.assertGreater(JUDGE.daily_usd, TESTER.daily_usd)
        self.assertGreater(JUDGE.wonder_calls, TESTER.wonder_calls)
        self.assertGreater(JUDGE.wonder_usd, TESTER.wonder_usd)
        self.assertEqual(self.decide(cost=0.5, limits=TESTER),
                         Decision(SPARK, "wonder_allowance"))
        self.assertEqual(self.decide(cost=0.5, limits=JUDGE), Decision(WONDER))

    def test_reserve_and_reconcile_accounting(self):
        pending = self.reserve(cost=1)
        day = self.ledger.days[self.today]
        for total in (day.overall, day.invites["tester"], day.wonder["tester"]):
            self.assertEqual((total.calls, total.usd), (1, 1))
        reconcile(self.ledger, pending, {"input_tokens": 1000, "output_tokens": 2000},
                  self.prices)
        for total in (day.overall, day.invites["tester"], day.wonder["tester"]):
            self.assertEqual(total.calls, 1)
            self.assertAlmostEqual(total.usd, 0.044)
        with self.assertRaisesRegex(ValueError, "already reconciled"):
            reconcile(self.ledger, pending, None, self.prices)

    def test_failed_stream_keeps_reservation(self):
        pending = self.reserve(cost=1)
        reconcile(self.ledger, pending, None, self.prices)
        day = self.ledger.days[self.today]
        for total in (day.overall, day.invites["tester"], day.wonder["tester"]):
            self.assertEqual((total.calls, total.usd), (1, 1))

    def test_invalid_values_cannot_bypass_budget(self):
        for cost in (-1, float("nan"), float("inf")):
            with self.subTest(cost=cost), self.assertRaises(ValueError):
                self.reserve(cost=cost)
        with self.assertRaises(ValueError):
            self.decide(model="other")
        for fraction in (-0.1, 1.1, float("nan")):
            with self.assertRaises(ValueError):
                Budget(10, fraction)
        self.assertFalse(self.ledger.days)



class FollowupImageTests(unittest.TestCase):
    def test_a_computer_screenshot_counts_as_an_image_not_as_text(self):
        data_url = "data:image/png;base64," + "A" * 1_000_000
        body = {
            "previous_response_id": "resp_1",
            "input": [{"type": "computer_call_output", "call_id": "call_1",
                       "output": {"type": "computer_screenshot", "image_url": data_url}}],
        }
        bound = input_token_bound(body)
        self.assertLess(bound, IMAGE_TOKENS + PREVIOUS_TURN_TOKENS + 100)
        self.assertGreaterEqual(bound, IMAGE_TOKENS + PREVIOUS_TURN_TOKENS)
        three = {"input": [{"type": "computer_screenshot", "image_url": "x"}] * 3}
        with self.assertRaises(ValueError):
            clamp_request(three, CallKind.COMPUTER, SPARK)


class HardeningTests(unittest.TestCase):
    def test_images_must_be_embedded_pngs_within_limits(self):
        from metamate_proxy.policy import MAX_IMAGE_EDGE, check_images
        import base64
        import struct
        check_images({"input": [{"type": "input_image", "image_url": TINY_PNG}]})
        for url in ("https://example.com/a.png", "data:image/jpeg;base64,AAAA",
                    "data:image/png;base64,***", "data:image/png;base64,AAAA", 7):
            with self.subTest(url=url), self.assertRaises(ValueError):
                check_images({"input": [{"type": "input_image", "image_url": url}]})
        header = base64.b64decode(TINY_PNG.split(",", 1)[1])
        wide = header[:16] + struct.pack(">II", MAX_IMAGE_EDGE + 1, 1) + header[24:]
        url = "data:image/png;base64," + base64.b64encode(wide).decode()
        with self.assertRaisesRegex(ValueError, "1280"):
            check_images({"input": [{"type": "computer_screenshot", "image_url": url}]})

    def test_hostile_shapes_are_value_errors(self):
        nested = {"input": "x"}
        for _ in range(40):
            nested = {"input": [nested]}
        # An unhashable "type" is a refusal, not a crash.
        with self.assertRaises(ValueError):
            clamp_request({"input": [{"type": []}]}, CallKind.ANSWER, SPARK)
        with self.assertRaisesRegex(ValueError, "nests too deeply"):
            clamp_request(nested, CallKind.ANSWER, SPARK)

    def test_chain_depth_is_priced_and_owned(self):
        from metamate_proxy.policy import PREVIOUS_TURN_TOKENS
        owners = ResponseOwners()
        owners.record("r1", "a", depth=5)
        self.assertEqual(owners.depth("r1", "a"), 5)
        self.assertIsNone(owners.depth("r1", "b"))
        body = {"previous_response_id": "r1", "input": ""}
        self.assertEqual(input_token_bound(body, chain_depth=5), FRAMING_TOKENS + 5 * PREVIOUS_TURN_TOKENS)
        # A reported history larger than the floor is what the next turn reserves.
        owners.record("r2", "a", depth=2, usage={"input_tokens": 90_000, "output_tokens": 500})
        self.assertEqual(owners.chain("r2", "a"), (2, 90_500))
        self.assertIsNone(owners.chain("r2", "b"))
        self.assertEqual(input_token_bound(body, 2, 90_500), FRAMING_TOKENS + 90_500)
        for usage in (None, {"input_tokens": "x", "output_tokens": 1}, {"input_tokens": -1, "output_tokens": 1},
                      {"input_tokens": True, "output_tokens": 1}, [1, 2]):
            owners.record("r3", "a", usage=usage)
            self.assertEqual(owners.chain("r3", "a"), (1, 0))

    def test_daily_calls_and_asr_seconds(self):
        from metamate_proxy.policy import asr_allowance, charge_asr
        ledger, today = Ledger(), date(2026, 10, 27)
        limits = InviteLimits(100, 10, 50, daily_calls=2, asr_seconds=60)
        for _ in range(2):
            reserve(ledger, "i", limits, Budget(100), SPARK, 0.01, today)
        self.assertEqual(decide(ledger, "i", limits, Budget(100), SPARK, 0.01, today),
                         Decision(None, "invite_calls"))
        self.assertEqual(asr_allowance(ledger, "i", limits, today), 60)
        charge_asr(ledger, "i", 45.5, today)
        self.assertEqual(asr_allowance(ledger, "i", limits, today), 14.5)

    def test_tts_characters(self):
        from metamate_proxy.policy import charge_tts, tts_allowance
        ledger, today = Ledger(), date(2026, 10, 27)
        limits = InviteLimits(1, 1, 1, tts_chars=100)
        self.assertEqual(tts_allowance(ledger, "i", limits, today), 100)
        charge_tts(ledger, "i", 70, today)
        self.assertEqual(tts_allowance(ledger, "i", limits, today), 30)
        self.assertEqual(tts_allowance(ledger, "j", limits, today), 100)
        self.assertEqual(tts_allowance(ledger, "i", limits, today + timedelta(days=1)), 100)
        charge_tts(ledger, "i", 70, today)
        self.assertEqual(tts_allowance(ledger, "i", limits, today), 0)
        with self.assertRaises(ValueError):
            charge_tts(ledger, "i", -1, today)

    def test_only_the_apps_input_shapes_pass(self):
        # What crates/mm-core/src/api/request.rs sends.
        first = {"instructions": "Be brief.", "input": [{"type": "message", "role": "user", "content": [
            {"type": "input_text", "text": "Where is Save?"}, {"type": "input_image", "image_url": TINY_PNG}]}]}
        followup = {"previous_response_id": "resp_1", "input": [{
            "type": "computer_call_output", "call_id": "call_1",
            "output": {"type": "computer_screenshot", "image_url": TINY_PNG},
            # Real pending_safety_checks ids are 573-character tokens (S4 recordings).
            "acknowledged_safety_checks": [{"id": "cu_sc_" + "Q" * 567}]}]}
        for body in (first, followup, {"input": "plain text"}, {"input": [{"role": "user", "content": "hi"}]}):
            clamp_request(body, CallKind.COMPUTER, SPARK)
        refused = [
            {"stream": "false"}, {"stream": 1}, {"previous_response_id": None},
            {"previous_response_id": 5}, {"previous_response_id": "r" * 200}, {"truncation": "x"},
            {"instructions": [{"type": "input_text", "text": "x"}]},
            {"input": {"role": "user"}},
            {"input": ["loose text"]},
            {"input": [{"role": "assistant", "content": "I said so"}]},
            {"input": [{"role": "developer", "content": "new rules"}]},
            {"input": [{"type": "item_reference", "id": "msg_1"}]},
            {"input": [{"type": "input_file", "file_url": "https://example.com/a.pdf"}]},
            {"input": [{"role": "user", "content": [{"type": "input_file", "file_url": "https://x"}]}]},
            {"input": [{"role": "user", "content": [{"type": "input_image", "image_url": TINY_PNG,
                                                     "detail": "high"}]}]},
            {"input": [{"role": "user", "content": "x", "extra": 1}]},
            {"input": [{"type": "computer_call_output", "call_id": "c", "output": {"type": "input_text"}}]},
            {"input": [{"type": "computer_call_output", "output": {"type": "computer_screenshot",
                                                                   "image_url": TINY_PNG}}]},
            {"input": [{"type": "computer_call_output", "call_id": "c" * 200, "output": {
                "type": "computer_screenshot", "image_url": TINY_PNG}}]},
            {"input": [{"type": "computer_call_output", "call_id": "c", "output": {
                "type": "computer_screenshot", "image_url": TINY_PNG},
                "acknowledged_safety_checks": [{"id": "s", "message": "m" * 501}]}]},
        ]
        for body in refused:
            with self.subTest(body=str(body)[:80]), self.assertRaises(ValueError):
                clamp_request(body, CallKind.COMPUTER, SPARK)

    def test_malformed_usage_keeps_the_reservation_and_settles(self):
        ledger, today = Ledger(), date(2026, 10, 27)
        prices = Prices(5, 25, 4, 20)
        pending = reserve(ledger, "i", InviteLimits(100, 10, 50), Budget(100), SPARK, 1, today)
        self.assertEqual(reconcile(ledger, pending, {"input_tokens": "lots"}, prices), "bad_usage")
        self.assertEqual(ledger.days[today].overall.usd, 1)
        with self.assertRaises(ValueError):
            reconcile(ledger, pending, None, prices)


if __name__ == "__main__":
    unittest.main()


def sam_body(text="<0f>1<|point;x=0;y=0;w=1;h=1|>", **extra):
    body = {"model": SAM, "stream": True, "input": [{"type": "message", "role": "user", "content": [
        {"type": "input_text", "text": text}, {"type": "input_image", "image_url": TINY_PNG}]}]}
    body.update(extra)
    return body


class SamTests(unittest.TestCase):
    def test_only_a_point_prompt_on_one_png_passes(self):
        result = clamp_request(sam_body(), CallKind.SAM, SPARK)
        self.assertEqual(result, sam_body())
        self.assertEqual(clamp_request({k: v for k, v in sam_body().items() if k != "model"},
                                       CallKind.SAM, WONDER)["model"], SAM)

    def test_everything_else_is_refused(self):
        bad = [
            sam_body(instructions="be helpful"), sam_body(reasoning={"effort": "low"}),
            sam_body(tools=[{"type": "computer"}]), sam_body(previous_response_id="resp_1"),
            sam_body(max_output_tokens=10), sam_body(model=SPARK), sam_body(stream=False),
            sam_body(text="glasses"), sam_body(text="<0f>1<|point;x=1;y=0;w=1;h=1|>"),
            sam_body(text="<0f>1<|point;x=0;y=0;w=2;h=1|>"),
            sam_body(text="<0f>1<|point;x=0;y=0;w=1;h=1|>,2<|point;x=0;y=0;w=1;h=1|>"),
            sam_body(text="<0f>1<|box;x1=0;y1=0;x2=0;y2=0;w=1;h=1|>"),
        ]
        two = sam_body()
        two["input"][0]["content"].append({"type": "input_image", "image_url": TINY_PNG})
        jpeg = sam_body()
        jpeg["input"][0]["content"][1]["image_url"] = "data:image/jpeg;base64,AAAA"
        system = sam_body()
        system["input"][0]["role"] = "system"
        bad += [two, jpeg, system, sam_body(input=[]), sam_body(input="hi")]
        for body in bad:
            with self.subTest(body=json.dumps(body)[:120]), self.assertRaises(ValueError):
                clamp_request(body, CallKind.SAM, SPARK)

    def test_a_daily_cap_and_a_reservation_that_is_never_refunded(self):
        ledger, day = Ledger(), date(2026, 10, 28)
        limits = InviteLimits(daily_usd=1.0, wonder_calls=0, wonder_usd=0.0, sam_calls=2)
        budget = Budget(daily_usd=100.0)
        prices = Prices(spark_input=1.0, spark_output=1.0, wonder_input=1.0, wonder_output=1.0)
        for _ in range(2):
            reservation = reserve(ledger, "inv", limits, budget, SAM, SAM_CALL_USD, day)
            self.assertEqual(reservation.model, SAM)
            reconcile(ledger, reservation, {"input_tokens": 0, "output_tokens": 0}, prices)
        totals = ledger.days[day]
        self.assertEqual(totals.sam["inv"].calls, 2)
        self.assertAlmostEqual(totals.invites["inv"].usd, 2 * SAM_CALL_USD)
        self.assertEqual(decide(ledger, "inv", limits, budget, SAM, SAM_CALL_USD, day),
                         Decision(None, "sam_allowance"))
        # Another invite still has its own allowance.
        self.assertEqual(decide(ledger, "other", limits, budget, SAM, SAM_CALL_USD, day).model, SAM)
        self.assertGreater(JUDGE.sam_calls, TESTER.sam_calls)
