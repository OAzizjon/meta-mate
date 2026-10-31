// The MateScript timeline as it arrives from Rust: one `{"kind", "value"}` object per item
// (`crates/mm-core/src/matescript/timeline.rs`, serde adjacent tagging). The reference is
// `crates/mm-core/tests/snapshots/timeline_wire__wire_format.snap`: change both together.

/** `Level`: playback volume of a sound or an ambience bed. */
export type Level = "soft" | "normal" | "loud";

/** `Sound`: a validated sound effect; `solo` sounds are timeline barriers. */
export interface Sound {
  readonly name: string;
  readonly level: Level;
  readonly solo: boolean;
}

/** `Point`: a candidate element id, or a pixel `[x, y]` of the screenshot sent (u32). */
export type Point =
  | { readonly kind: "id"; readonly value: string }
  | { readonly kind: "pixel"; readonly value: readonly [number, number] };

/** `Step`: step `number` of `total` (1 <= number <= total) of a guide. */
export interface Step {
  readonly number: number;
  readonly total: number;
  readonly target: Point;
}

/** `Bound`: an action that starts with its sentence (or alone at the answer's end). */
export type Bound =
  | { readonly kind: "sfx"; readonly value: Sound }
  | { readonly kind: "mood"; readonly value: string }
  | { readonly kind: "point"; readonly value: Point }
  | { readonly kind: "step"; readonly value: Step }
  | { readonly kind: "show"; readonly value: string };

/** `Say`: one sentence; `id` starts at 1 within a generation (0 is for pre-cached phrases). */
export interface Say {
  readonly id: number;
  readonly text: string;
  readonly bound: readonly Bound[];
}

/** `Item`: one timeline entry. Unit variants carry no `value`. */
export type TimelineItem =
  | { readonly kind: "open"; readonly value: { readonly kind: "site" | "app"; readonly value: string } }
  | { readonly kind: "lang"; readonly value: string }
  | { readonly kind: "say"; readonly value: Say }
  | { readonly kind: "bound"; readonly value: Bound }
  | { readonly kind: "sfx"; readonly value: Sound }
  | { readonly kind: "amb"; readonly value: { readonly name: string | null; readonly level: Level } }
  | { readonly kind: "pause"; readonly value: number }
  | { readonly kind: "wait" }
  | { readonly kind: "point_rejected" }
  | { readonly kind: "end" }
  | { readonly kind: "bad_tag" };

type JsonObject = Readonly<Record<string, unknown>>;

const LEVELS: readonly string[] = ["soft", "normal", "loud"];
const U32_MAX = 0xffff_ffff;

/**
 * Checks one parsed JSON value against the wire format and returns a fresh copy, or null when
 * anything is off (unknown kind, missing or extra key, wrong type, out-of-range number). Never
 * throws.
 */
export function parseTimelineItem(json: unknown): TimelineItem | null {
  try {
    return parseItem(json);
  } catch {
    // A throwing getter or proxy is malformed input too.
    return null;
  }
}

function parseItem(json: unknown): TimelineItem | null {
  const unit = exact(json, ["kind"]);
  if (unit) {
    switch (unit.kind) {
      case "wait":
        return { kind: "wait" };
      case "point_rejected":
        return { kind: "point_rejected" };
      case "end":
        return { kind: "end" };
      case "bad_tag":
        return { kind: "bad_tag" };
      default:
        return null;
    }
  }
  const tag = tagged(json);
  if (!tag) return null;
  const { value } = tag;
  switch (tag.kind) {
    case "open": {
      const target = tagged(value);
      return target && (target.kind === "site" || target.kind === "app") && typeof target.value === "string"
        ? { kind: "open", value: { kind: target.kind, value: target.value } }
        : null;
    }
    case "lang":
      return typeof value === "string" ? { kind: "lang", value } : null;
    case "say": {
      const say = parseSay(value);
      return say ? { kind: "say", value: say } : null;
    }
    case "bound": {
      const bound = parseBound(value);
      return bound ? { kind: "bound", value: bound } : null;
    }
    case "sfx": {
      const sound = parseSound(value);
      return sound ? { kind: "sfx", value: sound } : null;
    }
    case "amb": {
      const amb = exact(value, ["name", "level"]);
      if (!amb) return null;
      const { name, level } = amb;
      return (name === null || typeof name === "string") && isLevel(level)
        ? { kind: "amb", value: { name, level } }
        : null;
    }
    case "pause":
      return isU32(value) ? { kind: "pause", value } : null;
    default:
      return null;
  }
}

function parseSay(json: unknown): Say | null {
  const obj = exact(json, ["id", "text", "bound"]);
  if (!obj) return null;
  const { id, text, bound } = obj;
  if (!isU64(id) || id < 1 || typeof text !== "string" || !Array.isArray(bound)) return null;
  const actions: Bound[] = [];
  for (const entry of bound as readonly unknown[]) {
    const action = parseBound(entry);
    if (!action) return null;
    actions.push(action);
  }
  return { id, text, bound: actions };
}

function parseBound(json: unknown): Bound | null {
  const tag = tagged(json);
  if (!tag) return null;
  const { value } = tag;
  switch (tag.kind) {
    case "sfx": {
      const sound = parseSound(value);
      return sound ? { kind: "sfx", value: sound } : null;
    }
    case "mood":
      return typeof value === "string" ? { kind: "mood", value } : null;
    case "point": {
      const point = parsePoint(value);
      return point ? { kind: "point", value: point } : null;
    }
    case "step": {
      const step = parseStep(value);
      return step ? { kind: "step", value: step } : null;
    }
    case "show":
      return typeof value === "string" ? { kind: "show", value } : null;
    default:
      return null;
  }
}

function parseSound(json: unknown): Sound | null {
  const obj = exact(json, ["name", "level", "solo"]);
  if (!obj) return null;
  const { name, level, solo } = obj;
  return typeof name === "string" && isLevel(level) && typeof solo === "boolean"
    ? { name, level, solo }
    : null;
}

function parseStep(json: unknown): Step | null {
  const obj = exact(json, ["number", "total", "target"]);
  if (!obj) return null;
  const { number, total } = obj;
  const target = parsePoint(obj.target);
  if (!isU32(number) || !isU32(total) || number < 1 || number > total || !target) return null;
  return { number, total, target };
}

function parsePoint(json: unknown): Point | null {
  const tag = tagged(json);
  if (!tag) return null;
  const { value } = tag;
  if (tag.kind === "id") return typeof value === "string" ? { kind: "id", value } : null;
  if (tag.kind !== "pixel" || !Array.isArray(value) || value.length !== 2) return null;
  const [x, y] = value as readonly unknown[];
  return isU32(x) && isU32(y) ? { kind: "pixel", value: [x, y] } : null;
}

/** `{"kind": string, "value": ...}` with no other keys. */
function tagged(json: unknown): { readonly kind: string; readonly value: unknown } | null {
  const obj = exact(json, ["kind", "value"]);
  if (!obj) return null;
  const { kind, value } = obj;
  return typeof kind === "string" ? { kind, value } : null;
}

/** A plain JSON object whose own keys are exactly `keys` (in any order). */
function exact(json: unknown, keys: readonly string[]): JsonObject | null {
  if (typeof json !== "object" || json === null || Array.isArray(json)) return null;
  const own = Object.keys(json);
  return own.length === keys.length && own.every((key) => keys.includes(key))
    ? (json as JsonObject)
    : null;
}

function isLevel(value: unknown): value is Level {
  return typeof value === "string" && LEVELS.includes(value);
}

function isU32(value: unknown): value is number {
  return typeof value === "number" && Number.isInteger(value) && value >= 0 && value <= U32_MAX;
}

/** u64 as a JSON number: only the exactly representable range. */
function isU64(value: unknown): value is number {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
}
