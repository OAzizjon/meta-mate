// AudioWorkletProcessor for streamed 16-bit PCM speech in a bounded ring buffer.

import type { WorkletInboundMsg, WorkletOutboundMsg } from "./types.ts";

export const TTS_WORKLET_NAME = "tts-ring-processor";
export const DEFAULT_WORKLET_SAMPLE_RATE = 48_000;
export const RING_BUFFER_SECONDS = 30;
export const RING_BUFFER_CAPACITY =
  DEFAULT_WORKLET_SAMPLE_RATE * RING_BUFFER_SECONDS;
const RMS_EMIT_HZ = 30;
const INT16_SCALE = 1 / 32768;

export interface WorkletPortLike {
  onmessage: ((event: MessageEvent<WorkletInboundMsg>) => void) | null;
  postMessage(message: WorkletOutboundMsg): void;
}

export interface AudioWorkletProcessorLike {
  readonly port: WorkletPortLike;
}

type AudioWorkletProcessorCtor = new (
  options?: AudioWorkletNodeOptions,
) => AudioWorkletProcessorLike;

declare global {
  var AudioWorkletProcessor: AudioWorkletProcessorCtor | undefined;
  var registerProcessor:
    | ((name: string, processorCtor: unknown) => void)
    | undefined;
}

class FallbackAudioWorkletProcessor implements AudioWorkletProcessorLike {
  readonly port: WorkletPortLike = {
    onmessage: null,
    postMessage: () => {
      // No-op when instantiated outside AudioWorkletGlobalScope in unit tests.
    },
  };
}

const BaseWorkletProcessor: AudioWorkletProcessorCtor =
  typeof globalThis.AudioWorkletProcessor === "function"
    ? globalThis.AudioWorkletProcessor
    : FallbackAudioWorkletProcessor;

interface SentenceSpan {
  generationId: number;
  sentenceId: number;
  startSample: number;
  endSample: number | null;
  started: boolean;
}

function getWorkletSampleRate(): number {
  const globalScope = globalThis as { sampleRate?: unknown };
  return typeof globalScope.sampleRate === "number" &&
    globalScope.sampleRate > 0
    ? globalScope.sampleRate
    : DEFAULT_WORKLET_SAMPLE_RATE;
}

export class TtsRingProcessor extends BaseWorkletProcessor {
  private readonly ring: Float32Array;
  private readonly capacity: number;
  private readonly targetSampleRate: number;
  private readonly rmsIntervalFrames: number;
  private readonly sentences: SentenceSpan[] = [];

  private readPos = 0;
  private writePos = 0;
  private activeGenerationId = 0;
  private canceledThrough = -1;
  private resampleKey = "";
  private sourceRate = 0;
  private sourceFrames = 0;
  private nextSourcePosition = 0;
  private boundarySample = 0;
  private chunkStart = 0;
  private speaking = false;
  private framesSinceRms = 0;

  constructor(options?: AudioWorkletNodeOptions) {
    super(options);
    const paramRate =
      options?.processorOptions &&
      typeof (options.processorOptions as { sampleRate?: unknown })
        .sampleRate === "number"
        ? ((options.processorOptions as { sampleRate: number }).sampleRate ??
          getWorkletSampleRate())
        : getWorkletSampleRate();
    this.targetSampleRate =
      paramRate > 0 ? paramRate : DEFAULT_WORKLET_SAMPLE_RATE;
    this.capacity = Math.max(
      128,
      Math.round(this.targetSampleRate * RING_BUFFER_SECONDS),
    );
    this.ring = new Float32Array(this.capacity);
    this.rmsIntervalFrames = Math.max(
      128,
      Math.round(this.targetSampleRate / RMS_EMIT_HZ),
    );

    this.port.onmessage = (event: MessageEvent<WorkletInboundMsg>) => {
      this.handleMessage(event.data);
    };
  }

  get bufferedFrames(): number {
    return this.writePos - this.readPos;
  }

  get currentGenerationId(): number {
    return this.activeGenerationId;
  }

  handleMessage(msg: WorkletInboundMsg): void {
    if (msg.type === "cancel") {
      this.cancel(msg.generationId);
      return;
    }
    this.pushPcm(
      msg.generationId,
      msg.sentenceId,
      msg.sampleRate,
      msg.eos,
      new Int16Array(msg.pcm),
    );
  }

  cancel(generationId: number): void {
    this.canceledThrough = Math.max(this.canceledThrough, generationId);
    if (generationId < this.activeGenerationId) {
      return;
    }
    this.activeGenerationId = generationId;
    this.resetStream();
  }

  private resetStream(): void {
    this.readPos = 0;
    this.writePos = 0;
    this.sentences.length = 0;
    this.framesSinceRms = 0;
    this.resampleKey = "";
    if (this.speaking) {
      this.speaking = false;
      this.emit({ type: "speech-active", active: false, rms: 0 });
    }
  }

  pushPcm(
    generationId: number,
    sentenceId: number,
    srcSampleRate: number,
    eos: boolean,
    pcm: Int16Array,
  ): void {
    if (generationId <= this.canceledThrough || generationId < this.activeGenerationId) {
      return;
    }
    if (generationId > this.activeGenerationId) {
      this.resetStream();
      this.activeGenerationId = generationId;
    }

    if (!this.writeResampled(pcm, srcSampleRate, eos, generationId, sentenceId)) {
      return;
    }

    let span = this.findOpenSentence(generationId, sentenceId);
    if (!span) {
      span = {
        generationId,
        sentenceId,
        startSample: this.chunkStart,
        endSample: null,
        started: false,
      };
      this.sentences.push(span);
    }

    if (eos) {
      span.endSample = this.writePos;
    }
  }

  process(
    _inputs: Float32Array[][],
    outputs: Float32Array[][],
    _parameters?: Record<string, Float32Array>,
  ): boolean {
    const outputChannels = outputs[0];
    if (!outputChannels || outputChannels.length === 0) {
      return true;
    }
    const primary = outputChannels[0];
    if (!primary) {
      return true;
    }

    const blockSize = primary.length;
    let sumSquares = 0;
    let framesRead = 0;

    this.emitCompletedSentencesAt(this.readPos);

    for (let i = 0; i < blockSize; i++) {
      this.emitSentenceStartsAt(this.readPos);

      if (this.readPos < this.writePos) {
        const sample = this.ring[this.readPos % this.capacity] ?? 0;
        primary[i] = sample;
        sumSquares += sample * sample;
        this.readPos += 1;
        framesRead += 1;
        this.emitCompletedSentencesAt(this.readPos);
      } else {
        primary[i] = 0;
      }
    }

    // Mirror mono speech onto any additional output channels.
    for (let ch = 1; ch < outputChannels.length; ch++) {
      const channel = outputChannels[ch];
      if (channel) {
        channel.set(primary);
      }
    }

    const rms = blockSize > 0 ? Math.sqrt(sumSquares / blockSize) : 0;

    if (framesRead > 0) {
      this.framesSinceRms += framesRead;
      if (!this.speaking) {
        this.speaking = true;
        this.framesSinceRms = 0;
        this.emit({ type: "speech-active", active: true, rms });
      } else if (this.framesSinceRms >= this.rmsIntervalFrames) {
        this.framesSinceRms = 0;
        this.emit({ type: "speech-active", active: true, rms });
      }
    } else if (this.speaking) {
      this.speaking = false;
      this.framesSinceRms = 0;
      this.emit({ type: "speech-active", active: false, rms: 0 });
      if (this.hasOpenSentence()) {
        this.emit({
          type: "underrun",
          generationId: this.activeGenerationId,
        });
      }
    }

    if (this.readPos === this.writePos && this.sentences.length === 0) {
      this.readPos = 0;
      this.writePos = 0;
    }

    return true;
  }

  private writeResampled(
    pcm: Int16Array,
    srcSampleRate: number,
    eos: boolean,
    generationId: number,
    sentenceId: number,
  ): boolean {
    const rate = Number.isFinite(srcSampleRate) && srcSampleRate > 0
      ? srcSampleRate : this.targetSampleRate;
    const key = `${generationId}:${sentenceId}`;
    const continuing = this.resampleKey === key && this.sourceRate === rate;
    const offset = continuing ? this.sourceFrames : 0;
    let position = continuing ? this.nextSourcePosition : 0;
    const end = offset + pcm.length;
    const ratio = rate / this.targetSampleRate;
    // Without EOS, defer interpolation that needs the next chunk's first sample.
    const limit = eos || rate === this.targetSampleRate
      ? end : Math.max(0, end - 1);
    const count = Math.max(0, Math.ceil((limit - position) / ratio));
    const availableFrames = this.capacity - this.bufferedFrames;
    if (count > availableFrames) {
      // Reject atomically: the producer can retry after the consumer drains.
      this.emit({
        type: "overflow",
        generationId,
        sentenceId,
        requiredFrames: count,
        availableFrames,
      });
      return false;
    }
    this.chunkStart = this.writePos;
    const sampleAt = (index: number): number => index < offset
      ? this.boundarySample
      : (pcm[Math.min(pcm.length - 1, index - offset)] ?? this.boundarySample);
    for (let i = 0; i < count; i++) {
      const index = Math.floor(position);
      const fraction = position - index;
      const first = sampleAt(index);
      const second = sampleAt(Math.min(end - 1, index + 1));
      this.writeSample((first + (second - first) * fraction) * INT16_SCALE);
      position += ratio;
    }
    this.resampleKey = eos ? "" : key;
    this.sourceRate = rate;
    this.sourceFrames = end;
    this.nextSourcePosition = position;
    if (pcm.length > 0) this.boundarySample = pcm[pcm.length - 1] ?? 0;
    return true;
  }

  private writeSample(sample: number): void {
    const clamped = Math.max(-1, Math.min(1, sample));
    this.ring[this.writePos % this.capacity] = clamped;
    this.writePos += 1;
  }

  private findOpenSentence(
    generationId: number,
    sentenceId: number,
  ): SentenceSpan | undefined {
    for (let i = this.sentences.length - 1; i >= 0; i--) {
      const span = this.sentences[i];
      if (
        span &&
        span.generationId === generationId &&
        span.sentenceId === sentenceId &&
        span.endSample === null
      ) {
        return span;
      }
    }
    return undefined;
  }

  private hasOpenSentence(): boolean {
    return this.sentences.some((span) => span.endSample === null);
  }

  private emitSentenceStartsAt(sampleIndex: number): void {
    for (const span of this.sentences) {
      if (!span.started && sampleIndex >= span.startSample) {
        span.started = true;
        this.emit({
          type: "sentence-start",
          generationId: span.generationId,
          sentenceId: span.sentenceId,
        });
      } else if (span.startSample > sampleIndex) {
        break;
      }
    }
  }

  private emitCompletedSentencesAt(sampleIndex: number): void {
    while (this.sentences.length > 0) {
      const first = this.sentences[0];
      if (first && first.endSample !== null && sampleIndex >= first.endSample) {
        this.sentences.shift();
        if (!first.started) {
          first.started = true;
          this.emit({
            type: "sentence-start",
            generationId: first.generationId,
            sentenceId: first.sentenceId,
          });
        }
        this.emit({
          type: "sentence-end",
          generationId: first.generationId,
          sentenceId: first.sentenceId,
        });
      } else {
        break;
      }
    }
  }

  private emit(msg: WorkletOutboundMsg): void {
    this.port.postMessage(msg);
  }
}

if (typeof globalThis.registerProcessor === "function") {
  globalThis.registerProcessor(TTS_WORKLET_NAME, TtsRingProcessor);
}
