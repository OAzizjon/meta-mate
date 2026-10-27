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
    if (generationId > this.activeGenerationId) {
      this.activeGenerationId = generationId;
    }
    this.readPos = 0;
    this.writePos = 0;
    this.sentences.length = 0;
    this.framesSinceRms = 0;
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
    if (generationId < this.activeGenerationId) {
      return;
    }
    if (generationId > this.activeGenerationId) {
      this.cancel(generationId);
    }

    let span = this.findOpenSentence(generationId, sentenceId);
    if (!span) {
      span = {
        generationId,
        sentenceId,
        startSample: this.writePos,
        endSample: null,
        started: false,
      };
      this.sentences.push(span);
    }

    if (pcm.length > 0) {
      this.writeResampled(pcm, srcSampleRate);
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

    this.flushZeroLengthSentences();

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

  private writeResampled(pcm: Int16Array, srcSampleRate: number): void {
    const validSrcRate =
      srcSampleRate > 0 ? srcSampleRate : this.targetSampleRate;
    if (validSrcRate === this.targetSampleRate) {
      for (let i = 0; i < pcm.length; i++) {
        this.writeSample((pcm[i] ?? 0) * INT16_SCALE);
      }
      return;
    }

    const outLength = Math.max(
      1,
      Math.round((pcm.length * this.targetSampleRate) / validSrcRate),
    );
    const ratio = validSrcRate / this.targetSampleRate;
    const lastIndex = pcm.length - 1;

    for (let i = 0; i < outLength; i++) {
      const srcPos = i * ratio;
      const index0 = Math.min(lastIndex, Math.floor(srcPos));
      const index1 = Math.min(lastIndex, index0 + 1);
      const frac = srcPos - index0;
      const s0 = (pcm[index0] ?? 0) * INT16_SCALE;
      const s1 = (pcm[index1] ?? 0) * INT16_SCALE;
      this.writeSample(s0 + (s1 - s0) * frac);
    }
  }

  private writeSample(sample: number): void {
    const clamped = Math.max(-1, Math.min(1, sample));
    if (this.writePos - this.readPos >= this.capacity) {
      // Bounded ring buffer: advance readPos if producer outruns consumer.
      this.readPos = this.writePos - this.capacity + 1;
      this.emitCompletedSentencesAt(this.readPos);
    }
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

  private flushZeroLengthSentences(): void {
    while (this.sentences.length > 0) {
      const first = this.sentences[0];
      if (
        first &&
        first.endSample !== null &&
        first.startSample === first.endSample &&
        this.readPos >= first.endSample
      ) {
        this.sentences.shift();
        if (!first.started) {
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
