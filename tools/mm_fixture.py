"""Pure fixture loading and geometry for recorded UIA probes."""

from __future__ import annotations

from dataclasses import dataclass
import json
import math
from pathlib import Path
import re


INTERACTIVE_TYPES = {
    "Button", "Hyperlink", "MenuItem", "ListItem", "TabItem", "CheckBox",
    "RadioButton", "ComboBox", "Edit", "TreeItem", "SplitButton", "Slider",
    "Spinner", "DataItem",
}
Rect = tuple[float, float, float, float]


@dataclass
class Fixture:
    name: str
    window: dict
    nodes: list[dict]
    json_path: Path
    png_path: Path | None
    shot_frame: Rect


def _rect(value: object) -> Rect | None:
    if not isinstance(value, (list, tuple)) or len(value) != 4:
        return None
    if any(type(n) not in (int, float) or not math.isfinite(n) for n in value):
        return None
    if value[2] <= 0 or value[3] <= 0:
        return None
    return tuple(value)


def load_fixture(root: Path, name: str) -> Fixture:
    if not re.fullmatch(r"[A-Za-z0-9_-]{1,60}", name):
        raise ValueError("Fixture name must contain 1-60 letters, digits, - or _")
    path = root / "fixtures" / "uia" / f"{name}.json"
    try:
        data = json.loads(path.read_text(encoding="utf-8-sig"))
    except (OSError, UnicodeError, ValueError) as exc:
        raise ValueError(f"Cannot load fixture {path}: {exc}") from exc
    if not isinstance(data, dict) or not isinstance(data.get("window"), dict):
        raise ValueError(f"Invalid fixture {path}: expected a window object")
    window = data["window"]
    shot_frame = _rect(window.get("shot_frame", window.get("frame")))
    if shot_frame is None:
        raise ValueError(f"Invalid fixture {path}: expected positive shot_frame/frame bounds")
    nodes = data.get("nodes")
    if not isinstance(nodes, list) or any(not isinstance(node, dict) for node in nodes):
        raise ValueError(f"Invalid fixture {path}: expected a list of node objects")
    for node in nodes:
        if not isinstance(node.get("id"), str) or not isinstance(node.get("type"), str):
            raise ValueError(f"Invalid fixture {path}: node needs string id and type")
        rect = node.get("rect")
        # Empty UIA rectangles cannot be targets; malformed geometry is an error.
        if rect is not None and _rect(rect) is None:
            if not isinstance(rect, list) or len(rect) != 4 or any(
                type(n) not in (int, float) or not math.isfinite(n) for n in rect
            ) or rect[2] < 0 or rect[3] < 0:
                raise ValueError(f"Invalid fixture {path}: invalid node rectangle")
    png_path = path.with_suffix(".png")
    return Fixture(name, window, nodes, path, png_path if png_path.is_file() else None, shot_frame)


def unsafe_reason(fx: Fixture) -> str | None:
    for node in fx.nodes:
        if node.get("password") is not False:
            return "Password flag is true or unknown"
        if node.get("password_like") is not False:
            return "Password-like flag is true or unknown"
    if fx.png_path is None or not fx.png_path.is_file():
        return "Missing PNG screenshot"
    if fx.window.get("truncated") is not False:
        return "UIA tree is truncated or completeness is unknown"
    # Without per-monitor DPI awareness the rects and the pixels may not line up.
    if fx.window.get("dpi_awareness") != "per-monitor":
        return "Not recorded per-monitor DPI aware"
    return None


def _round(value: float) -> int:
    """Round half up everywhere, so a point maps the same way on a monitor at negative X."""
    return math.floor(value + 0.5)


@dataclass
class FrameMap:
    shot_frame: Rect
    sent_size: tuple[int, int]

    def __post_init__(self) -> None:
        if _rect(self.shot_frame) is None:
            raise ValueError("shot_frame must have finite, positive dimensions")
        if len(self.sent_size) != 2 or any(type(n) is not int or n <= 0 for n in self.sent_size):
            raise ValueError("sent_size must contain two positive integers")

    def to_sent(self, px: float, py: float) -> tuple[int, int]:
        x, y, w, h = self.shot_frame
        sw, sh = self.sent_size
        return _round((px - x) * sw / w), _round((py - y) * sh / h)

    def to_phys(self, sx: float, sy: float) -> tuple[int, int]:
        x, y, w, h = self.shot_frame
        sw, sh = self.sent_size
        return _round(x + sx * w / sw), _round(y + sy * h / sh)

    def fit(self, max_long_edge: int) -> tuple[int, int]:
        """Fit the original capture, preserving aspect ratio up to pixel rounding."""
        if type(max_long_edge) is not int or max_long_edge <= 0:
            raise ValueError("max_long_edge must be a positive integer")
        w, h = self.shot_frame[2:]
        scale = min(1.0, max_long_edge / max(w, h))
        return max(1, _round(w * scale)), max(1, _round(h * scale))


def _visible_nodes(fx: Fixture) -> list[tuple[dict, Rect]]:
    x, y, w, h = fx.shot_frame
    result = []
    for node in fx.nodes:
        rect = _rect(node.get("rect"))
        if node.get("enabled") is not True or node.get("offscreen") is not False or rect is None:
            continue
        nx, ny, nw, nh = rect
        if nx < x + w and ny < y + h and nx + nw > x and ny + nh > y:
            result.append((node, rect))
    return result


def _label(node: dict) -> str:
    # JSON quoting keeps names containing newlines or quotes on one terminal line.
    return f'{node["id"]} {node["type"]} {json.dumps(node.get("name", ""), ensure_ascii=True)}'


def element_list(fx: Fixture, fmap: FrameMap, limit: int = 150) -> list[str]:
    if limit <= 0:
        return []
    nodes = _visible_nodes(fx)
    # Stable partition: keep tree order within both groups.
    nodes.sort(key=lambda item: item[0]["type"] not in INTERACTIVE_TYPES)
    x, y, w, h = fx.shot_frame
    lines = []
    for node, (nx, ny, nw, nh) in nodes[:limit]:
        left, top = fmap.to_sent(max(x, nx), max(y, ny))
        right, bottom = fmap.to_sent(min(x + w, nx + nw), min(y + h, ny + nh))
        lines.append(f"{_label(node)} [{left},{top},{right - left},{bottom - top}]")
    return lines


@dataclass
class Hit:
    node: dict | None
    interactive: bool
    distance_px: float | None


def hit_test(fx: Fixture, px: float, py: float) -> Hit:
    containing = []
    distances = []
    for node, (x, y, w, h) in _visible_nodes(fx):
        interactive = node["type"] in INTERACTIVE_TYPES
        # Half-open bounds match screenshot pixel coordinates.
        if x <= px < x + w and y <= py < y + h:
            containing.append((not interactive, w * h, node))
        if interactive:
            distances.append(math.hypot(max(x - px, 0, px - x - w),
                                        max(y - py, 0, py - y - h)))
    node = min(containing, key=lambda item: item[:2])[2] if containing else None
    interactive = node is not None and node["type"] in INTERACTIVE_TYPES
    return Hit(node, interactive, None if interactive or not distances else min(distances))


def describe(hit: Hit) -> str:
    label = _label(hit.node) if hit.node is not None else "miss"
    kind = "interactive" if hit.interactive else "noninteractive" if hit.node is not None else ""
    distance = (f"; nearest interactive edge {hit.distance_px:.1f} px"
                if hit.distance_px is not None else "")
    return f"{label}{' (' + kind + ')' if kind else ''}{distance}"
