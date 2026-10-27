"""Pure request and spending policy; callers must serialize ledger operations."""

import base64
import binascii
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
# The floor for a continuation's billed history when the earlier turn reported no usage.
PREVIOUS_TURN_TOKENS = 8000
# Role markers and item framing around the text, per request (generous).
FRAMING_TOKENS = 64
# A first turn sends input_image; a computer_call_output sends computer_screenshot.
IMAGE_TYPES = {"input_image", "computer_screenshot"}
# Images arrive as PNG data URLs made by the app (crates/mm-core/src/shot.rs): no remote
# URLs (they would escape the body limit), at most 2 MB and 1280 px on the long edge.
MAX_IMAGE_BYTES = 2_000_000
MAX_IMAGE_EDGE = 1280
PNG_PREFIX = "data:image/png;base64,"
# A teacher task is at most 12 model turns (docs/architecture.md section 3): the history a
# continuation is billed for grows with the chain, so the chain is capped and priced.
MAX_CHAIN_DEPTH = 12
MAX_JSON_DEPTH = 32
MAX_ID_CHARS = 128
# pending_safety_checks ids are opaque tokens: 573 characters in the S4 recordings.
MAX_SAFETY_ID_CHARS = 4096
TTS_LANGUAGES = {"en", "ru"}
# One spoken sentence (docs/prompts.md: Mate speaks short lines).
TTS_MAX_CHARS = 300


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


def _is_image(part) -> bool:
    return isinstance(part, dict) and isinstance(part.get("type"), str) and part["type"] in IMAGE_TYPES


def _image_count(body: dict) -> int:
    return sum(_is_image(part) for part in _input_parts(body.get("input")))


def _depth(value, limit: int = MAX_JSON_DEPTH) -> bool:
    """Whether `value` nests at most `limit` levels deep (iteratively: no recursion)."""
    pending = [(value, 1)]
    while pending:
        part, level = pending.pop()
        if level > limit:
            return False
        if isinstance(part, dict):
            pending.extend((child, level + 1) for child in part.values())
        elif isinstance(part, list):
            pending.extend((child, level + 1) for child in part)
    return True


def _keys(item: dict, allowed: set[str]) -> None:
    if not set(item) <= allowed:
        raise ValueError("unexpected field in input")


def _short_text(value) -> bool:
    return isinstance(value, str) and 0 < len(value) <= MAX_ID_CHARS


def _check_message(item: dict) -> None:
    _keys(item, {"type", "role", "content"})
    if item.get("role") != "user":
        raise ValueError("only user messages are accepted")
    content = item.get("content")
    if isinstance(content, str):
        return
    if not isinstance(content, list):
        raise ValueError("message content must be text or a list")
    for part in content:
        if not isinstance(part, dict):
            raise ValueError("message parts must be objects")
        if part.get("type") == "input_text":
            _keys(part, {"type", "text"})
            if not isinstance(part.get("text"), str):
                raise ValueError("input_text needs text")
        elif part.get("type") == "input_image":
            _keys(part, {"type", "image_url"})
        else:
            raise ValueError("message part type not allowed")


def _check_call_output(item: dict) -> None:
    _keys(item, {"type", "call_id", "output", "acknowledged_safety_checks"})
    if not _short_text(item.get("call_id")):
        raise ValueError("computer_call_output needs a call_id")
    output = item.get("output")
    if not isinstance(output, dict) or output.get("type") != "computer_screenshot":
        raise ValueError("computer_call_output needs a computer_screenshot")
    _keys(output, {"type", "image_url"})
    checks = item.get("acknowledged_safety_checks", [])
    if not isinstance(checks, list) or len(checks) > 8:
        raise ValueError("acknowledged_safety_checks must be a short list")
    for check in checks:
        check_id = check.get("id") if isinstance(check, dict) else None
        if not isinstance(check_id, str) or not 0 < len(check_id) <= MAX_SAFETY_ID_CHARS:
            raise ValueError("an acknowledged safety check needs an id")
        _keys(check, {"id", "code", "message"})
        if any(not isinstance(check[key], str) or len(check[key]) > 500
               for key in ("code", "message") if key in check):
            raise ValueError("safety check fields must be short text")


def check_input(body: dict) -> None:
    """Only the shapes the app sends (crates/mm-core/src/api/request.rs), so nothing else
    reaches Meta unpriced: `instructions` is text; `input` is text or a list of user
    messages (input_text, input_image) and computer_call_outputs (computer_screenshot)."""
    instructions = body.get("instructions")
    if instructions is not None and not isinstance(instructions, str):
        raise ValueError("instructions must be text")
    items = body.get("input")
    if items is None or isinstance(items, str):
        return
    if not isinstance(items, list):
        raise ValueError("input must be text or a list of items")
    for item in items:
        if not isinstance(item, dict):
            raise ValueError("input items must be objects")
        kind = item.get("type", "message")
        if kind == "message":
            _check_message(item)
        elif kind == "computer_call_output":
            _check_call_output(item)
        else:
            raise ValueError("input item type not allowed")


def check_images(body: dict) -> None:
    """Every image is an embedded PNG within the size and edge limits (ValueError otherwise)."""
    for part in _input_parts(body.get("input")):
        if not _is_image(part):
            continue
        url = part.get("image_url")
        if not isinstance(url, str) or not url.startswith(PNG_PREFIX):
            raise ValueError("images must be embedded PNG data URLs")
        encoded = url[len(PNG_PREFIX):]
        if len(encoded) > MAX_IMAGE_BYTES * 4 // 3 + 4:
            raise ValueError("image too large")
        try:
            png = base64.b64decode(encoded, validate=True)
        except (binascii.Error, ValueError):
            raise ValueError("image is not valid base64") from None
        if len(png) > MAX_IMAGE_BYTES or len(png) < 24 or png[:8] != b"\x89PNG\r\n\x1a\n" or png[12:16] != b"IHDR":
            raise ValueError("image is not a PNG within the size limit")
        width, height = int.from_bytes(png[16:20], "big"), int.from_bytes(png[20:24], "big")
        if not 0 < width <= MAX_IMAGE_EDGE or not 0 < height <= MAX_IMAGE_EDGE:
            raise ValueError("image larger than 1280 px on an edge")


def input_token_bound(body: dict, chain_depth: int = 0, previous_tokens: int = 0) -> int:
    """Bound clamped input/instructions text, images and billed previous context: the
    history of a `previous_response_id` chain is billed again. `previous_tokens` is the
    earlier turn's reported input + output (ResponseOwners.chain); text counts one token
    per UTF-8 byte, since a byte-level tokenizer never makes more tokens than bytes."""
    text_bytes = 0
    pending = [body.get("input"), body.get("instructions")]
    while pending:
        part = pending.pop()
        if isinstance(part, str):
            text_bytes += len(part.encode("utf-8"))
        elif isinstance(part, dict):
            if not _is_image(part):
                pending.extend(part.values())
        elif isinstance(part, list):
            pending.extend(part)
    previous = 0
    if "previous_response_id" in body:
        previous = max(PREVIOUS_TURN_TOKENS * max(1, chain_depth), previous_tokens)
    return FRAMING_TOKENS + text_bytes + _image_count(body) * IMAGE_TOKENS + previous


def clamp_request(body: dict, kind: CallKind, model_choice: str) -> dict:
    """Return an allowlisted request without modifying the client's body."""
    if not _depth(body):
        raise ValueError("request nests too deeply")
    if len(json.dumps(body).encode("utf-8")) > MAX_BODY_BYTES:
        raise ValueError("request body too large")
    if _image_count(body) > MAX_IMAGES:
        raise ValueError("too many input images")
    check_input(body)
    check_images(body)
    if "stream" in body and not isinstance(body["stream"], bool):
        raise ValueError("stream must be true or false")
    if "previous_response_id" in body and not _short_text(body["previous_response_id"]):
        raise ValueError("previous_response_id must be a response id")
    if "truncation" in body and body["truncation"] not in ("auto", "disabled"):
        raise ValueError("truncation must be auto or disabled")
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
    """Price final usage; output_tokens already includes reasoning tokens. Malformed
    usage raises ValueError (the caller then keeps the whole reservation)."""
    if not isinstance(usage, Mapping):
        raise ValueError("usage must be an object")
    return estimate_cost(model, usage.get("input_tokens"), usage.get("output_tokens"), prices)


@dataclass(frozen=True)
class InviteLimits:
    """Per-invite daily USD, Wonder, call and speech-recognition allowances."""

    daily_usd: float
    wonder_calls: int
    wonder_usd: float
    daily_calls: int = 300
    asr_seconds: int = 1200
    tts_chars: int = 20_000

    def __post_init__(self) -> None:
        _nonnegative(self.daily_usd, "daily_usd")
        _tokens(self.wonder_calls)
        _nonnegative(self.wonder_usd, "wonder_usd")
        _tokens(self.daily_calls)
        _tokens(self.asr_seconds)
        _tokens(self.tts_chars)


# Initial policy defaults, not provider pricing or a promise of unlimited judging.
TESTER = InviteLimits(daily_usd=1.0, wonder_calls=5, wonder_usd=0.25, daily_calls=150,
                      asr_seconds=600, tts_chars=10_000)
JUDGE = InviteLimits(daily_usd=5.0, wonder_calls=25, wonder_usd=2.0, daily_calls=400,
                     asr_seconds=1800, tts_chars=30_000)


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
    asr_seconds: dict[str, float] = field(default_factory=dict)
    tts_chars: dict[str, int] = field(default_factory=dict)


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

    _owners: dict[str, tuple[str, int, int]] = field(default_factory=dict, init=False, repr=False)

    def record(self, response_id: str, invite: str, depth: int = 1,
               usage: Mapping | None = None, bound_tokens: int = 0) -> None:
        """`depth`: how many turns the chain ending in this response has; `usage`: the
        response's reported usage, whose input + output a continuation is billed again;
        `bound_tokens`: the request's input bound + output cap, the history to assume
        when the usage is missing or malformed."""
        tokens = context_tokens(usage)
        self._owners[response_id] = (invite, depth, bound_tokens if tokens is None else tokens)
        if len(self._owners) > 10_000:
            del self._owners[next(iter(self._owners))]

    def check(self, previous_response_id: str, invite: str) -> bool:
        return self.depth(previous_response_id, invite) is not None

    def depth(self, previous_response_id: str, invite: str) -> int | None:
        """The chain depth of an own, known response; None for unknown or foreign ids."""
        chain = self.chain(previous_response_id, invite)
        return chain[0] if chain is not None else None

    def chain(self, previous_response_id: str, invite: str) -> tuple[int, int] | None:
        """(depth, history tokens) of an own, known response; None otherwise."""
        owner = self._owners.get(previous_response_id)
        return (owner[1], owner[2]) if owner is not None and owner[0] == invite else None


def context_tokens(usage: Mapping | None) -> int | None:
    """input + output tokens of a reported usage; None when it is missing or malformed."""
    if not isinstance(usage, Mapping):
        return None
    counts = (usage.get("input_tokens"), usage.get("output_tokens"))
    if any(type(count) is not int or count < 0 for count in counts):
        return None
    return counts[0] + counts[1]


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
    own = day.invites.get(invite, Totals())
    if own.calls >= limits.daily_calls:
        return Decision(None, "invite_calls")
    spent = own.usd
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
        try:
            cost = actual_cost(reservation.model, usage, prices)
        except ValueError:
            # Malformed usage: keep the whole reservation, but settle it.
            del ledger._pending[id(reservation)]
            return "bad_usage"
        delta = cost - reservation.estimated_cost
        day = ledger.days[reservation.day]
        counters = [day.overall, day.invites[reservation.invite]]
        if reservation.model == WONDER:
            counters.append(day.wonder[reservation.invite])
        for total in counters:
            total.usd = max(0.0, total.usd + delta)
    del ledger._pending[id(reservation)]


def asr_allowance(ledger: Ledger, invite: str, limits: InviteLimits, today: date) -> float:
    """Seconds of speech recognition this invite may still use today."""
    used = ledger.days.get(today, DayTotals()).asr_seconds.get(invite, 0.0)
    return max(0.0, limits.asr_seconds - used)


def charge_asr(ledger: Ledger, invite: str, seconds: float, today: date) -> None:
    """Add the audio a recognition session sent (callers serialize, like reserve)."""
    _nonnegative(seconds, "seconds")
    day = ledger.days.setdefault(today, DayTotals())
    day.asr_seconds[invite] = day.asr_seconds.get(invite, 0.0) + seconds


def reserve_asr(ledger: Ledger, invite: str, limits: InviteLimits, wanted: float,
                today: date) -> float:
    """Charge up to `wanted` seconds now, before the session connects, so parallel sessions
    cannot each see the same remainder; returns the seconds granted (0: refuse)."""
    _nonnegative(wanted, "wanted")
    granted = min(wanted, asr_allowance(ledger, invite, limits, today))
    charge_asr(ledger, invite, granted, today)
    return granted


def settle_asr(ledger: Ledger, invite: str, reserved: float, used: float, day: date) -> None:
    """Give back the reserved seconds a session did not send (on the day it reserved)."""
    _nonnegative(reserved, "reserved")
    _nonnegative(used, "used")
    totals = ledger.days.setdefault(day, DayTotals())
    unused = max(0.0, reserved - used)
    totals.asr_seconds[invite] = max(0.0, totals.asr_seconds.get(invite, 0.0) - unused)


def tts_allowance(ledger: Ledger, invite: str, limits: InviteLimits, today: date) -> int:
    """Characters of speech synthesis this invite may still use today."""
    used = ledger.days.get(today, DayTotals()).tts_chars.get(invite, 0)
    return max(0, limits.tts_chars - used)


def charge_tts(ledger: Ledger, invite: str, chars: int, today: date) -> None:
    """Charge synthesized text before the call (callers serialize, like reserve)."""
    _tokens(chars)
    day = ledger.days.setdefault(today, DayTotals())
    day.tts_chars[invite] = day.tts_chars.get(invite, 0) + chars
