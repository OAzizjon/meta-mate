// Pure formatting and wire validation for the "Under the hood" debug window (WP14, D78).
// Listens to `DebugMsg` events inside Tauri and displays the pipeline strip, timeline items
// as MateScript-like chips, sentence text in a muted line, PCM arrivals per sentence with
// sample counts, timings as a horizontal bar chart against key-up, and look stats.
// Runs in Node tests directly without DOM or Tauri dependencies.

import type { AnswerModel, AnswerMsg, AnswerState } from "../ipc/answer-stream.ts";
import { ANSWER_MODELS, ANSWER_STATES, parseAnswerMsg } from "../ipc/answer-stream.ts";
import { parseTimelineItem } from "../ipc/timeline.ts";
import type { Bound, Point, Sound, TimelineItem } from "../ipc/timeline.ts";

export const MAX_GENERATIONS = 20;

export const PRIVACY_NOTE =
  "Shown here: timings, models, Mate's own words and counts. Never your words or your screenshot.";

export const DEBUG_ANSWER_MODELS = [
  "muse-spark-1.3",
  "muse-wonder-1.0",
  "muse-glimmer-30b",
] as const satisfies readonly AnswerModel[];

export type DebugAnswerModel = (typeof DEBUG_ANSWER_MODELS)[number];

export const MODEL_FULL_NAMES: Readonly<Record<AnswerModel, string>> = {
  "muse-spark-1.3": "Muse Spark 1.3",
  "muse-wonder-1.0": "Muse Wonder 1.0",
  "muse-glimmer-30b": "Muse Glimmer 30B",
  "sam-3.1": "SAM 3.1",
  "muse-voice-transcribe-1.0": "Muse Voice Transcribe 1.0",
};

export function isDebugAnswerModel(value: unknown): value is DebugAnswerModel {
  return typeof value === "string" && (DEBUG_ANSWER_MODELS as readonly string[]).includes(value);
}

export function isAnswerModel(value: unknown): value is AnswerModel {
  return typeof value === "string" && (ANSWER_MODELS as readonly string[]).includes(value);
}

export function formatModelName(model: string): string {
  return isAnswerModel(model) ? MODEL_FULL_NAMES[model] : model;
}

export interface PipelineStepView {
  readonly id: string;
  readonly label: string;
  readonly active: boolean;
}

export const PIPELINE_STEPS: readonly Omit<PipelineStepView, "active">[] = [
  { id: "voice", label: "Your voice" },
  { id: "asr", label: "Muse Voice Transcribe" },
  { id: "model", label: "Muse Spark / Wonder / Glimmer" },
  { id: "timeline", label: "MateScript timeline" },
  { id: "tts", label: "MMS-TTS voice" },
];

export const PIPELINE_MODEL_LABELS: Readonly<Record<DebugAnswerModel, string>> = {
  "muse-spark-1.3": "Muse Spark",
  "muse-wonder-1.0": "Wonder",
  "muse-glimmer-30b": "Glimmer",
};

/** The answer model of the newest generation, or null when unset or outside the answer set. */
export function activePipelineModel(records: readonly GenerationRecord[]): DebugAnswerModel | null {
  if (records.length === 0) return null;
  const newest = records[0];
  if (newest.model !== undefined) {
    return isDebugAnswerModel(newest.model) ? newest.model : null;
  }
  for (let i = newest.lines.length - 1; i >= 0; i--) {
    const candidate = newest.lines[i];
    if (isDebugAnswerModel(candidate)) return candidate;
  }
  return null;
}

/** Pure HTML rendering of the pipeline strip, lighting up the newest generation's model chip. */
export function renderPipelineStripHtml(activeModel: string | null = null): string {
  const lit = isDebugAnswerModel(activeModel) ? activeModel : null;
  const modelSubchips = DEBUG_ANSWER_MODELS.map((id) => {
    const isLit = lit === id;
    const cls = isLit ? "pipeline-chip pipeline-model is-active" : "pipeline-chip pipeline-model";
    const current = isLit ? ` aria-current="true"` : "";
    return `<span class="${cls}" data-model="${id}"${current}>${escapeHtml(PIPELINE_MODEL_LABELS[id])}</span>`;
  }).join(` <span class="pipeline-slash" aria-hidden="true">/</span> `);

  const chips = PIPELINE_STEPS.map((step) => {
    if (step.id === "model") {
      const cls = lit ? "pipeline-models is-active" : "pipeline-models";
      return `<span class="${cls}" data-step="model" data-active-model="${lit ?? ""}">${modelSubchips}</span>`;
    }
    return `<span class="pipeline-chip" data-step="${step.id}">${escapeHtml(step.label)}</span>`;
  });

  const joined = chips.join(` <span class="pipeline-arrow" aria-hidden="true">-&gt;</span> `);
  return `<div class="pipeline-strip" role="region" aria-label="Meta AI pipeline">${joined}</div>`;
}

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
  | Extract<AnswerMsg, { readonly type: "model" }>
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
  model: ["type", "generation", "model"],
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
      case "model": {
        const model = parseAnswerMsg(msg);
        return model?.type === "model" ? model : null;
      }
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
    case "open":
      return { chips: [{ label: "<open>", kind: "open" }] };
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

export const TIMING_LABELS: Readonly<Record<string, string>> = {
  transcript: "heard you",
  asr_final: "heard you",
  look: "checked screen",
  first_item: "first words",
  first_text: "first words",
  first_sentence: "first sentence",
  first_audio: "first sound",
  done: "finished",
};

export function formatTimingLabel(name: string): string | null {
  return Object.hasOwn(TIMING_LABELS, name) ? TIMING_LABELS[name] : null;
}

/** View model for a timing bar scaled against key-up (t = 0 ms). */
export interface TimingBarView {
  readonly name: string;
  readonly label: string | null;
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
      label: formatTimingLabel(timing.name),
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
  readonly model?: AnswerModel | null;
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
  model?: AnswerModel | null;
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
        case "model":
          record.model = inner.model;
          if (inner.model !== null && !record.lines.includes(inner.model)) {
            record.lines.push(inner.model);
          }
          break;
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

  /** The answer model of the newest generation, or null. */
  getActiveModel(): DebugAnswerModel | null {
    return activePipelineModel(this.getGenerations());
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

function resolveRecordModel(record: GenerationRecord): AnswerModel | null {
  if (record.model !== undefined) return record.model;
  for (let i = record.lines.length - 1; i >= 0; i--) {
    const line = record.lines[i];
    if (isAnswerModel(line)) return line;
  }
  return null;
}

/**
 * Pure HTML rendering of a single generation card.
 */
export function renderGenerationHtml(record: GenerationRecord): string {
  const gen = record.generation;
  const model = resolveRecordModel(record);

  // Badges: model (with full name), state, cancelled, lines
  const badges: string[] = [];
  if (model) {
    badges.push(
      `<span class="badge badge-model" data-model="${escapeHtml(model)}">${escapeHtml(formatModelName(model))}</span>`,
    );
  }
  if (record.state) {
    badges.push(`<span class="badge badge-state">${escapeHtml(record.state)}</span>`);
  }
  if (record.cancelled) {
    badges.push(`<span class="badge badge-cancelled">cancelled</span>`);
  }
  for (const line of record.lines) {
    if (isAnswerModel(line) && model === line) continue;
    if (isAnswerModel(line)) {
      badges.push(
        `<span class="badge badge-model" data-model="${escapeHtml(line)}">${escapeHtml(formatModelName(line))}</span>`,
      );
    } else {
      badges.push(`<span class="badge badge-line">${escapeHtml(line)}</span>`);
    }
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
      .map((b) => {
        const plainHtml = b.label
          ? `<span class="timing-plain">${escapeHtml(b.label)}</span> `
          : "";
        return `
        <div class="timing-row">
          <span class="timing-label">${plainHtml}<span class="timing-name">${escapeHtml(b.name)}</span></span>
          <div class="timing-track" aria-hidden="true">
            <div class="timing-bar" style="width: ${b.percentage}%"></div>
          </div>
          <span class="timing-val">${b.ms} ms</span>
        </div>`;
      })
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
