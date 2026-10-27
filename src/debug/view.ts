// Pure formatting and wire validation for the development debug window (WP14).
// Listens to `DebugMsg` events inside Tauri and displays timeline items as MateScript-like
// chips, sentence text in a muted line, PCM arrivals per sentence with sample counts,
// timings as a horizontal bar chart against key-up, and look stats.
// Runs in Node tests directly without DOM or Tauri dependencies.

import type { AnswerState } from "../ipc/answer-stream.ts";
import { ANSWER_STATES } from "../ipc/answer-stream.ts";
import { parseTimelineItem } from "../ipc/timeline.ts";
import type { Bound, Level, Point, Say, Sound, Step, TimelineItem } from "../ipc/timeline.ts";

export const MAX_GENERATIONS = 20;

/** `DebugPcmMsg`: PCM payload carries sample count `samples` instead of base64 samplesB64. */
export interface DebugPcmMsg {
  readonly type: "pcm";
  readonly generation: number;
  readonly sentence: number;
  readonly sampleRate: number;
  readonly eos: boolean;
  readonly samples: number;
}

export type DebugAnswerMsg =
  | { readonly type: "state"; readonly generation: number; readonly state: AnswerState }
  | { readonly type: "item"; readonly generation: number; readonly item: TimelineItem }
  | DebugPcmMsg
  | { readonly type: "cancel"; readonly generation: number }
  | { readonly type: "line"; readonly generation: number; readonly key: string };

export interface DebugTimingMsg {
  readonly type: "timing";
  readonly generation: number;
  readonly name: string;
  readonly ms: number;
}

export interface DebugLookMsg {
  readonly type: "look";
  readonly generation: number;
  readonly elements: number;
  readonly listed: number;
  readonly painted: number;
  readonly ms: number;
}

export interface DebugAnswerWrapper {
  readonly type: "answer";
  readonly msg: DebugAnswerMsg;
}

export type DebugMsg = DebugAnswerWrapper | DebugTimingMsg | DebugLookMsg;

type JsonObject = Readonly<Record<string, unknown>>;

const ANSWER_WRAPPER_KEYS = ["type", "msg"] as const;
const TIMING_KEYS = ["type", "generation", "name", "ms"] as const;
const LOOK_KEYS = ["type", "generation", "elements", "listed", "painted", "ms"] as const;

const ANSWER_KEYS: Readonly<Record<DebugAnswerMsg["type"], readonly string[]>> = {
  state: ["type", "generation", "state"],
  item: ["type", "generation", "item"],
  pcm: ["type", "generation", "sentence", "sampleRate", "eos", "samples"],
  cancel: ["type", "generation"],
  line: ["type", "generation", "key"],
};

function isPlainObject(value: unknown): value is JsonObject {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function exact(value: unknown, keys: readonly string[]): JsonObject | null {
  if (!isPlainObject(value)) return null;
  const own = Object.keys(value);
  return own.length === keys.length && own.every((k) => keys.includes(k)) ? value : null;
}

function isId(value: unknown): value is number {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
}

function isPositiveFinite(value: unknown): value is number {
  return typeof value === "number" && Number.isFinite(value) && value >= 0;
}

function isSampleRate(value: unknown): value is number {
  return typeof value === "number" && Number.isInteger(value) && value >= 8000 && value <= 48000;
}

function isAnswerState(value: unknown): value is AnswerState {
  return typeof value === "string" && (ANSWER_STATES as readonly string[]).includes(value);
}

/**
 * Parses an untrusted `DebugAnswerMsg` (the inner payload of `{ type: "answer", msg: ... }`).
 * Never throws.
 */
export function parseDebugAnswerMsg(raw: unknown): DebugAnswerMsg | null {
  try {
    if (!isPlainObject(raw)) return null;
    const { type } = raw;
    if (typeof type !== "string" || !Object.hasOwn(ANSWER_KEYS, type)) return null;
    const msg = exact(raw, ANSWER_KEYS[type as DebugAnswerMsg["type"]]);
    if (!msg) return null;
    const { generation } = msg;
    if (!isId(generation)) return null;

    switch (type) {
      case "state": {
        const { state } = msg;
        return isAnswerState(state) ? { type, generation, state } : null;
      }
      case "item": {
        const item = parseTimelineItem(msg.item);
        return item ? { type, generation, item } : null;
      }
      case "pcm": {
        const { sentence, sampleRate, eos, samples } = msg;
        return isId(sentence) &&
          isSampleRate(sampleRate) &&
          typeof eos === "boolean" &&
          isId(samples)
          ? { type, generation, sentence, sampleRate, eos, samples }
          : null;
      }
      case "cancel":
        return { type, generation };
      case "line": {
        const { key } = msg;
        return typeof key === "string" && key.length > 0 ? { type, generation, key } : null;
      }
      default:
        return null;
    }
  } catch {
    return null;
  }
}

/**
 * Validates any untrusted wire payload against the `DebugMsg` contract.
 * Never throws.
 */
export function parseDebugMsg(raw: unknown): DebugMsg | null {
  try {
    if (!isPlainObject(raw)) return null;
    const { type } = raw;
    if (typeof type !== "string") return null;

    if (type === "answer") {
      const obj = exact(raw, ANSWER_WRAPPER_KEYS);
      if (!obj) return null;
      const msg = parseDebugAnswerMsg(obj.msg);
      return msg ? { type: "answer", msg } : null;
    }

    if (type === "timing") {
      const obj = exact(raw, TIMING_KEYS);
      if (!obj) return null;
      const { generation, name, ms } = obj;
      return isId(generation) && typeof name === "string" && name.length > 0 && isPositiveFinite(ms)
        ? { type: "timing", generation, name, ms }
        : null;
    }

    if (type === "look") {
      const obj = exact(raw, LOOK_KEYS);
      if (!obj) return null;
      const { generation, elements, listed, painted, ms } = obj;
      return isId(generation) &&
        isId(elements) &&
        isId(listed) &&
        isId(painted) &&
        isPositiveFinite(ms)
        ? { type: "look", generation, elements, listed, painted, ms }
        : null;
    }

    return null;
  } catch {
    return null;
  }
}

export function getDebugMsgGeneration(msg: DebugMsg): number {
  return msg.type === "answer" ? msg.msg.generation : msg.generation;
}

/** Chip representation for a single MateScript timeline element. */
export interface TimelineChip {
  readonly label: string;
  readonly kind: string;
}

/** Timeline row item: chips followed optionally by muted sentence text. */
export interface TimelineRowView {
  readonly chips: readonly TimelineChip[];
  readonly text?: string;
}

export function formatPoint(point: Point): string {
  if (point.kind === "id") return point.value;
  return `${point.value[0]},${point.value[1]}`;
}

export function formatSound(sound: Sound): string {
  const levelPart = sound.level !== "normal" ? ` ${sound.level}` : "";
  const soloPart = sound.solo ? " solo" : "";
  return `${sound.name}${levelPart}${soloPart}`;
}

export function formatBoundChip(bound: Bound): TimelineChip {
  switch (bound.kind) {
    case "point":
      return { label: `<point ${formatPoint(bound.value)}>`, kind: "point" };
    case "mood":
      return { label: `<mood ${bound.value}>`, kind: "mood" };
    case "sfx":
      return { label: `<sfx ${formatSound(bound.value)}>`, kind: "sfx" };
    case "step": {
      const { number, total, target } = bound.value;
      return { label: `<step ${number}/${total} ${formatPoint(target)}>`, kind: "step" };
    }
    case "show":
      return { label: `<show ${bound.value}>`, kind: "show" };
  }
}

/**
 * Formats a `TimelineItem` into a row of MateScript-like chips (`say #1`, `<point e12>`,
 * `<pause 400>`, `end`) and the sentence text when `item.kind === "say"`.
 */
export function formatTimelineItem(item: TimelineItem): TimelineRowView {
  switch (item.kind) {
    case "say": {
      const boundChips = item.value.bound.map(formatBoundChip);
      const sayChip: TimelineChip = { label: `say #${item.value.id}`, kind: "say" };
      return {
        chips: [...boundChips, sayChip],
        text: item.value.text,
      };
    }
    case "pause":
      return { chips: [{ label: `<pause ${item.value}>`, kind: "pause" }] };
    case "end":
      return { chips: [{ label: "end", kind: "end" }] };
    case "wait":
      return { chips: [{ label: "<wait click>", kind: "wait" }] };
    case "lang":
      return { chips: [{ label: `<lang ${item.value}>`, kind: "lang" }] };
    case "bound":
      return { chips: [formatBoundChip(item.value)] };
    case "sfx":
      return { chips: [{ label: `<sfx ${formatSound(item.value)}>`, kind: "sfx" }] };
    case "amb": {
      const ambText = item.value.name ? `<amb ${item.value.name} ${item.value.level}>` : "<amb off>";
      return { chips: [{ label: ambText, kind: "amb" }] };
    }
    case "point_rejected":
      return { chips: [{ label: "<point_rejected>", kind: "point_rejected" }] };
    case "bad_tag":
      return { chips: [{ label: "<bad_tag>", kind: "bad_tag" }] };
  }
}

/** Summary of PCM arrivals for a single sentence. */
export interface SentencePcmSummary {
  readonly sentence: number;
  readonly totalSamples: number;
  readonly sampleRate: number;
  readonly chunks: number;
  readonly eos: boolean;
  readonly arrivals: readonly number[];
}

/**
 * Groups PCM arrivals per sentence, computing sample totals and arrival counts.
 */
export function groupPcmBySentence(pcmList: readonly DebugPcmMsg[]): SentencePcmSummary[] {
  const map = new Map<
    number,
    { totalSamples: number; sampleRate: number; chunks: number; eos: boolean; arrivals: number[] }
  >();

  for (const pcm of pcmList) {
    let entry = map.get(pcm.sentence);
    if (!entry) {
      entry = {
        totalSamples: 0,
        sampleRate: pcm.sampleRate,
        chunks: 0,
        eos: false,
        arrivals: [],
      };
      map.set(pcm.sentence, entry);
    }
    entry.totalSamples += pcm.samples;
    entry.sampleRate = pcm.sampleRate;
    entry.chunks += 1;
    entry.arrivals.push(pcm.samples);
    if (pcm.eos) entry.eos = true;
  }

  return Array.from(map.entries())
    .sort(([sA], [sB]) => sA - sB)
    .map(([sentence, data]) => ({
      sentence,
      totalSamples: data.totalSamples,
      sampleRate: data.sampleRate,
      chunks: data.chunks,
      eos: data.eos,
      arrivals: data.arrivals,
    }));
}

/** Formats a sentence's PCM arrivals into a human-readable summary string. */
export function formatSentencePcm(summary: SentencePcmSummary): string {
  const label = summary.sentence === 0 ? "Sentence #0 (cached)" : `Sentence #${summary.sentence}`;
  const samplesFormatted = summary.totalSamples.toLocaleString("en-US");
  const khz = Math.round(summary.sampleRate / 100) / 10;
  const chunkText = summary.chunks === 1 ? "1 chunk" : `${summary.chunks} chunks`;
  const eosText = summary.eos ? ", eos" : "";
  return `${label}: ${samplesFormatted} samples (${chunkText} @ ${khz} kHz${eosText})`;
}

/** View model for a timing bar scaled against key-up (t = 0 ms). */
export interface TimingBarView {
  readonly name: string;
  readonly ms: number;
  readonly percentage: number;
}

/**
 * Computes horizontal bar chart widths against key-up (t = 0 ms).
 * Percentage is scaled to the maximum `ms` in the generation (or minimum 100 ms).
 */
export function computeTimingBars(timings: readonly DebugTimingMsg[]): TimingBarView[] {
  if (timings.length === 0) return [];
  const maxMs = Math.max(...timings.map((t) => t.ms), 100);

  return timings.map((timing) => {
    const rawPct = (timing.ms / maxMs) * 100;
    const percentage = Math.max(0, Math.min(100, Math.round(rawPct * 10) / 10));
    return {
      name: timing.name,
      ms: timing.ms,
      percentage,
    };
  });
}

/** Formats look stats into a summary string. */
export function formatLookStats(look: DebugLookMsg): string {
  return `${look.elements} elements, ${look.listed} listed, ${look.painted} painted in ${look.ms} ms`;
}

export function escapeHtml(str: string): string {
  return str
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}

/** The accumulated data for one generation. */
export interface GenerationRecord {
  readonly generation: number;
  readonly items: readonly TimelineItem[];
  readonly pcm: readonly DebugPcmMsg[];
  readonly timings: readonly DebugTimingMsg[];
  readonly look: DebugLookMsg | null;
  readonly state?: AnswerState;
  readonly cancelled?: boolean;
  readonly lines: readonly string[];
}

interface MutableGenerationRecord {
  generation: number;
  items: TimelineItem[];
  pcm: DebugPcmMsg[];
  timings: DebugTimingMsg[];
  look: DebugLookMsg | null;
  state?: AnswerState;
  cancelled?: boolean;
  lines: string[];
}

/**
 * Stores events per generation, keeping the newest on top, up to `MAX_GENERATIONS = 20`.
 */
export class DebugStore {
  private readonly generations = new Map<number, MutableGenerationRecord>();

  handleRaw(raw: unknown): boolean {
    const msg = parseDebugMsg(raw);
    if (!msg) return false;
    this.addMsg(msg);
    return true;
  }

  addMsg(msg: DebugMsg): void {
    const genId = getDebugMsgGeneration(msg);
    let record = this.generations.get(genId);
    if (!record) {
      record = {
        generation: genId,
        items: [],
        pcm: [],
        timings: [],
        look: null,
        lines: [],
      };
      this.generations.set(genId, record);
      this.prune();
    }

    if (msg.type === "timing") {
      const idx = record.timings.findIndex((t) => t.name === msg.name);
      if (idx >= 0) record.timings[idx] = msg;
      else record.timings.push(msg);
    } else if (msg.type === "look") {
      record.look = msg;
    } else if (msg.type === "answer") {
      const inner = msg.msg;
      switch (inner.type) {
        case "item":
          record.items.push(inner.item);
          break;
        case "pcm":
          record.pcm.push(inner);
          break;
        case "state":
          record.state = inner.state;
          break;
        case "cancel":
          record.cancelled = true;
          break;
        case "line":
          record.lines.push(inner.key);
          break;
      }
    }
  }

  /** Generations sorted descending by generation id (newest on top, at most 20). */
  getGenerations(): readonly GenerationRecord[] {
    return Array.from(this.generations.values()).sort((a, b) => b.generation - a.generation);
  }

  clear(): void {
    this.generations.clear();
  }

  private prune(): void {
    if (this.generations.size <= MAX_GENERATIONS) return;
    const sortedIds = Array.from(this.generations.keys()).sort((a, b) => b - a);
    const toRemove = sortedIds.slice(MAX_GENERATIONS);
    for (const id of toRemove) {
      this.generations.delete(id);
    }
  }
}

/**
 * Pure HTML rendering of a single generation card.
 */
export function renderGenerationHtml(record: GenerationRecord): string {
  const gen = record.generation;

  // Badges: state, cancelled, lines
  const badges: string[] = [];
  if (record.state) {
    badges.push(`<span class="badge badge-state">${escapeHtml(record.state)}</span>`);
  }
  if (record.cancelled) {
    badges.push(`<span class="badge badge-cancelled">cancelled</span>`);
  }
  for (const line of record.lines) {
    badges.push(`<span class="badge badge-line">${escapeHtml(line)}</span>`);
  }

  // Look stats
  let lookHtml = "";
  if (record.look) {
    lookHtml = `
      <section class="debug-section debug-look">
        <h3 class="section-title">Look</h3>
        <div class="look-box">${escapeHtml(formatLookStats(record.look))}</div>
      </section>`;
  }

  // Timings bar chart
  let timingsHtml = "";
  if (record.timings.length > 0) {
    const bars = computeTimingBars(record.timings);
    const rows = bars
      .map(
        (b) => `
        <div class="timing-row">
          <span class="timing-name">${escapeHtml(b.name)}</span>
          <div class="timing-track" aria-hidden="true">
            <div class="timing-bar" style="width: ${b.percentage}%"></div>
          </div>
          <span class="timing-val">${b.ms} ms</span>
        </div>`,
      )
      .join("");

    timingsHtml = `
      <section class="debug-section debug-timings">
        <h3 class="section-title">Timings (against key-up)</h3>
        <div class="timings-chart">${rows}</div>
      </section>`;
  }

  // Timeline items and muted sentence lines
  let timelineHtml = "";
  if (record.items.length > 0) {
    const rows = record.items
      .map((item) => {
        const view = formatTimelineItem(item);
        const chipsHtml = view.chips
          .map((c) => `<span class="chip chip-${escapeHtml(c.kind)}">${escapeHtml(c.label)}</span>`)
          .join(" ");
        const textHtml = view.text
          ? `<div class="sentence-text muted">${escapeHtml(view.text)}</div>`
          : "";
        return `<div class="timeline-entry"><div class="chips">${chipsHtml}</div>${textHtml}</div>`;
      })
      .join("");

    timelineHtml = `
      <section class="debug-section debug-timeline">
        <h3 class="section-title">Timeline</h3>
        <div class="timeline-entries">${rows}</div>
      </section>`;
  }

  // PCM arrivals per sentence
  let pcmHtml = "";
  if (record.pcm.length > 0) {
    const summaries = groupPcmBySentence(record.pcm);
    const rows = summaries
      .map((s) => `<li class="pcm-item">${escapeHtml(formatSentencePcm(s))}</li>`)
      .join("");

    pcmHtml = `
      <section class="debug-section debug-pcm">
        <h3 class="section-title">PCM Arrivals</h3>
        <ul class="pcm-list">${rows}</ul>
      </section>`;
  }

  const emptyNotice =
    !record.look && record.timings.length === 0 && record.items.length === 0 && record.pcm.length === 0
      ? `<div class="debug-empty-gen muted">No events recorded</div>`
      : "";

  return `
    <article class="generation-card" data-generation="${gen}">
      <header class="generation-header">
        <h2 class="generation-title">Generation #${gen}</h2>
        <div class="generation-badges">${badges.join(" ")}</div>
      </header>
      <div class="generation-body">
        ${lookHtml}
        ${timingsHtml}
        ${timelineHtml}
        ${pcmHtml}
        ${emptyNotice}
      </div>
    </article>`;
}

/**
 * Pure HTML rendering of the full debug view.
 */
export function renderDebugViewHtml(records: readonly GenerationRecord[]): string {
  if (records.length === 0) {
    return `<div class="debug-empty muted">Waiting for events from Tauri...</div>`;
  }
  return records.map(renderGenerationHtml).join("\n");
}
