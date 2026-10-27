"""Pure request and spending policy; callers must serialize ledger operations."""

from dataclasses import dataclass, field
from datetime import date
from enum import Enum
import json
from math import isfinite
from typing import Mapping


SPARK = "muse-spark-1.3"
WONDER = "muse-wonder-1.0"
ALLOWED_MODELS = {SPARK, WONDER}
# Reasoning counts as output (D41: ~100 tokens even at minimal, 356-570 for a computer
# turn), so a cap below that ends the turn before any text: keep these generous.
INTENT_CAP = 200
SPEECH_CAP = 300
ANSWER_CAP = 1200
COMPUTER_CAP = 1200
MAX_BODY_BYTES = 2_500_000
MAX_IMAGES = 2
# S4 measured about 3.7k tokens for a 1280x800 screenshot with 15 elements.
IMAGE_TOKENS = 4000
PREVIOUS_TURN_TOKENS = 8000
# A first turn sends input_image; a computer_call_output sends computer_screenshot.
IMAGE_TYPES = {"input_image", "computer_screenshot"}


class CallKind(Enum):
    INTENT = "intent"
    SPEECH = "speech"
    ANSWER = "answer"
    COMPUTER = "computer"


OUTPUT_CAPS = {
    CallKind.INTENT: INTENT_CAP,
    CallKind.SPEECH: SPEECH_CAP,
    CallKind.ANSWER: ANSWER_CAP,
    CallKind.COMPUTER: COMPUTER_CAP,
}
REQUEST_KEYS = {
    "model", "input", "instructions", "tools", "stream", "max_output_tokens",
    "reasoning", "previous_response_id", "truncation",
}


def _nonnegative(value: float, name: str) -> None:
    if not isfinite(value) or value < 0:
        raise ValueError(f"{name} must be finite and nonnegative")


def _model(model: str) -> None:
    if model not in ALLOWED_MODELS:
        raise ValueError("model outside allowlist")


def _tokens(value: int) -> None:
    if type(value) is not int or value < 0:
        raise ValueError("token counts must be nonnegative integers")


@dataclass(frozen=True)
class Prices:
    """USD per million tokens; caller supplies all prices from the Billing page."""

    spark_input: float
    spark_output: float
    wonder_input: float
    wonder_output: float

    def __post_init__(self) -> None:
        for name, value in vars(self).items():
            _nonnegative(value, name)


def _input_parts(value):
    pending = [value]
    while pending:
        part = pending.pop()
        yield part
        if isinstance(part, dict):
            pending.extend(part.values())
        elif isinstance(part, list):
            pending.extend(part)


def _image_count(body: dict) -> int:
    return sum(isinstance(part, dict) and part.get("type") in IMAGE_TYPES
               for part in _input_parts(body.get("input")))


def input_token_bound(body: dict) -> int:
    """Bound clamped input/instructions text, images and billed previous context."""
    characters = 0
    pending = [body.get("input"), body.get("instructions")]
    while pending:
        part = pending.pop()
        if isinstance(part, str):
            characters += len(part)
        elif isinstance(part, dict):
            if part.get("type") not in IMAGE_TYPES:
                pending.extend(part.values())
        elif isinstance(part, list):
            pending.extend(part)
    return ((characters + 1) // 2 + _image_count(body) * IMAGE_TOKENS
            + (PREVIOUS_TURN_TOKENS if "previous_response_id" in body else 0))


def clamp_request(body: dict, kind: CallKind, model_choice: str) -> dict:
    """Return an allowlisted request without modifying the client's body."""
    if len(json.dumps(body).encode("utf-8")) > MAX_BODY_BYTES:
        raise ValueError("request body too large")
    if _image_count(body) > MAX_IMAGES:
        raise ValueError("too many input images")
    pinned = kind in {CallKind.INTENT, CallKind.SPEECH}
    if not pinned:
        _model(model_choice)
    result = {key: value for key, value in body.items() if key in REQUEST_KEYS}
    result["model"] = SPARK if pinned else model_choice
    if kind == CallKind.COMPUTER:
        result["tools"] = [{"type": "computer"}]
    else:
        result.pop("tools", None)
    cap = OUTPUT_CAPS[kind]
    tokens = body.get("max_output_tokens", cap)
    _tokens(tokens)
    result["max_output_tokens"] = max(1, min(tokens, cap))
    reasoning = body.get("reasoning")
    effort = reasoning.get("effort", "minimal") if isinstance(reasoning, dict) else "minimal"
    result["reasoning"] = {
        "effort": effort if not pinned and effort in ("minimal", "low") else "minimal",
    }
    return result


def estimate_cost(model: str, input_tokens: int, max_output_tokens: int,
                  prices: Prices) -> float:
    """Estimate worst-case USD; callers must use input_token_bound(clamped_body).

    Do not supply an independently calculated input token count.
    """
    _model(model)
    _tokens(input_tokens)
    _tokens(max_output_tokens)
    if model == SPARK:
        input_price, output_price = prices.spark_input, prices.spark_output
    else:
        input_price, output_price = prices.wonder_input, prices.wonder_output
    return (input_tokens * input_price + max_output_tokens * output_price) / 1_000_000


def actual_cost(model: str, usage: Mapping[str, int], prices: Prices) -> float:
    """Price final usage; output_tokens already includes reasoning tokens."""
    return estimate_cost(model, usage["input_tokens"], usage["output_tokens"], prices)


@dataclass(frozen=True)
class InviteLimits:
    """Per-invite daily USD and Wonder allowances."""

    daily_usd: float
    wonder_calls: int
    wonder_usd: float

    def __post_init__(self) -> None:
        _nonnegative(self.daily_usd, "daily_usd")
        _tokens(self.wonder_calls)
        _nonnegative(self.wonder_usd, "wonder_usd")


# Initial policy defaults, not provider pricing or a promise of unlimited judging.
TESTER = InviteLimits(daily_usd=1.0, wonder_calls=5, wonder_usd=0.25)
JUDGE = InviteLimits(daily_usd=5.0, wonder_calls=25, wonder_usd=2.0)


@dataclass(frozen=True)
class Budget:
    daily_usd: float
    spark_reserve_fraction: float = 0.3

    def __post_init__(self) -> None:
        _nonnegative(self.daily_usd, "daily_usd")
        if not isfinite(self.spark_reserve_fraction) or not 0 <= self.spark_reserve_fraction <= 1:
            raise ValueError("spark_reserve_fraction must be between zero and one")


@dataclass
class Totals:
    calls: int = 0
    usd: float = 0.0


@dataclass
class DayTotals:
    overall: Totals = field(default_factory=Totals)
    invites: dict[str, Totals] = field(default_factory=dict)
    wonder: dict[str, Totals] = field(default_factory=dict)


@dataclass(frozen=True)
class Decision:
    """A None model refuses forwarding; a reason explains refusal or downgrade."""

    model: str | None
    reason: str | None = None


@dataclass(frozen=True)
class Reservation:
    day: date
    invite: str
    model: str
    estimated_cost: float
    reason: str | None = None


def body_for(reservation: Reservation, body: dict, kind: CallKind) -> dict:
    """Forward the reserved model, including any downgrade to Spark."""
    return clamp_request(body, kind, reservation.model)


@dataclass
class ResponseOwners:
    """Proxy must reject a previous_response_id that check refuses.

    In-memory ownership with the oldest recorded IDs dropped first; not thread-safe.
    """

    _owners: dict[str, str] = field(default_factory=dict, init=False, repr=False)

    def record(self, response_id: str, invite: str) -> None:
        self._owners[response_id] = invite
        if len(self._owners) > 10_000:
            del self._owners[next(iter(self._owners))]

    def check(self, previous_response_id: str, invite: str) -> bool:
        return (previous_response_id in self._owners
                and self._owners[previous_response_id] == invite)


@dataclass
class Ledger:
    """In-memory counters keyed by caller-supplied UTC dates; not thread-safe."""

    days: dict[date, DayTotals] = field(default_factory=dict)
    _pending: dict[int, Reservation] = field(default_factory=dict, init=False, repr=False)


def decide(ledger: Ledger, invite: str, limits: InviteLimits, budget: Budget,
           requested_model: str, estimated_cost: float, today: date) -> Decision:
    """Check projected spend; fallback uses the same conservative cost ceiling."""
    _model(requested_model)
    _nonnegative(estimated_cost, "estimated_cost")
    day = ledger.days.get(today, DayTotals())
    spent = day.invites.get(invite, Totals()).usd
    if spent >= limits.daily_usd or spent + estimated_cost > limits.daily_usd:
        return Decision(None, "invite_budget")
    if day.overall.usd >= budget.daily_usd or day.overall.usd + estimated_cost > budget.daily_usd:
        return Decision(None, "daily_budget")
    if requested_model == WONDER:
        wonder = day.wonder.get(invite, Totals())
        if (wonder.calls >= limits.wonder_calls or wonder.usd >= limits.wonder_usd
                or wonder.usd + estimated_cost > limits.wonder_usd):
            return Decision(SPARK, "wonder_allowance")
        available = budget.daily_usd * (1 - budget.spark_reserve_fraction)
        if day.overall.usd >= available or day.overall.usd + estimated_cost > available:
            return Decision(SPARK, "wonder_budget")
    return Decision(requested_model)


def reserve(ledger: Ledger, invite: str, limits: InviteLimits, budget: Budget,
            requested_model: str, estimated_cost: float, today: date) -> Reservation:
    """Decide and charge before forwarding; caller must serialize this operation.

    The estimate must bound the selected model and any Spark fallback. On refusal,
    raise ValueError with the decision reason. Forward the returned model only.
    """
    decision = decide(ledger, invite, limits, budget, requested_model, estimated_cost, today)
    if decision.model is None:
        raise ValueError(decision.reason)
    reservation = Reservation(today, invite, decision.model, estimated_cost, decision.reason)
    day = ledger.days.setdefault(today, DayTotals())
    counters = [day.overall, day.invites.setdefault(invite, Totals())]
    if decision.model == WONDER:
        counters.append(day.wonder.setdefault(invite, Totals()))
    for total in counters:
        total.calls += 1
        total.usd += estimated_cost
    ledger._pending[id(reservation)] = reservation
    return reservation


def reconcile(ledger: Ledger, reservation: Reservation,
              usage: Mapping[str, int] | None, prices: Prices,
              upstream_model: str | None = None) -> str | None:
    """Settle once on the original day; absent usage keeps the full reservation.

    Pass None for failures or cut streams, even if partial usage is available.
    Callers must serialize reconciliation with decisions and reservations.
    A different upstream model keeps the full reservation and returns model_mismatch.
    """
    if ledger._pending.get(id(reservation)) is not reservation:
        raise ValueError("unknown or already reconciled reservation")
    if upstream_model is not None and upstream_model != reservation.model:
        del ledger._pending[id(reservation)]
        return "model_mismatch"
    if usage is not None:
        cost = actual_cost(reservation.model, usage, prices)
        delta = cost - reservation.estimated_cost
        day = ledger.days[reservation.day]
        counters = [day.overall, day.invites[reservation.invite]]
        if reservation.model == WONDER:
            counters.append(day.wonder[reservation.invite])
        for total in counters:
            total.usd = max(0.0, total.usd + delta)
    del ledger._pending[id(reservation)]
