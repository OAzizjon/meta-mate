"""Pure proxy request and spending policy tests."""

from copy import deepcopy
from datetime import date, timedelta
import unittest

from metamate_proxy.policy import (
    ALLOWED_MODELS, ANSWER_CAP, COMPUTER_CAP, INTENT_CAP, SPEECH_CAP,
    Budget, CallKind, Decision, InviteLimits, JUDGE, Ledger, Prices, SPARK,
    TESTER, WONDER, actual_cost, clamp_request, decide, reconcile, reserve,
)


class RequestTests(unittest.TestCase):
    def test_caps_and_model_forcing_for_every_kind(self):
        caps = (INTENT_CAP, SPEECH_CAP, ANSWER_CAP, COMPUTER_CAP)
        self.assertEqual(caps, (200, 300, 1200, 1200))
        for kind, cap in zip(CallKind, caps):
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
        self.assertEqual(set(result), set(body) - {"unknown"})
        self.assertEqual(result["reasoning"], {"effort": "minimal"})
        self.assertIs(result["stream"], False)
        result["reasoning"]["effort"] = "low"
        self.assertEqual(body, original)

    def test_effort_rules(self):
        for kind in CallKind:
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


class SpendingTests(unittest.TestCase):
    def setUp(self):
        self.today = date(2026, 10, 27)
        self.ledger = Ledger()
        self.limits = InviteLimits(100, 10, 50)
        self.budget = Budget(10)
        # Synthetic conservative placeholders, not Spark's real Billing prices.
        self.prices = Prices(spark_input=5, spark_output=25)

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


if __name__ == "__main__":
    unittest.main()
