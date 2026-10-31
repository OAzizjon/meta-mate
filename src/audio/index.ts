// Public facade for MetaMate's WebView audio engine.

import { SentenceCompletions } from "./sentence-completions.ts";
import { AudioIdleController } from "./idle.ts";
import type { PlaybackLease } from "./idle.ts";

import {
  AUDIO_SAMPLE_RATE,
  bindAutoResume,
  ensureResumed,
  initAudioContext,
  toResumeState,
} from "./context.ts";
import {
  AudioMixer,
  BUTTERWORTH_Q,
  clampCuePan,
  dbToLinear,
  DUCK_AMB_DB,
  DUCK_AMB_GAIN,
  DUCK_ATTACK_SECONDS,
  DUCK_ATTACK_TIME_CONSTANT,
  DUCK_RELEASE_SECONDS,
  DUCK_RELEASE_TIME_CONSTANT,
  DUCK_SFX_DB,
  DUCK_SFX_GAIN,
  LEVEL_DB_TABLE,
  LEVEL_GAIN_TABLE,
  levelGain,
  LIMITER_ATTACK_SECONDS,
  LIMITER_KNEE_DB,
  LIMITER_RATIO,
  LIMITER_RELEASE_SECONDS,
  LIMITER_THRESHOLD_DB,
  MAX_CUE_PAN,
  panFromViewportX,
  resolveEffectiveLevel,
  SFX_BANDPASS_HIGH_HZ,
  SFX_BANDPASS_LOW_HZ,
} from "./mixer.ts";
import {
  AMBIENCE_CROSSFADE_SECONDS,
  buildSoundUrl,
  FIRST_12_SOUNDS,
  isAmbienceName,
  isCueName,
  isSoundAllowedInMode,
  MAGIC_FADE_DURATION_SECONDS,
  MAGIC_FADE_START_SECONDS,
  SOUND_CATALOG,
  SoundBank,
  type SoundBankOptions,
  type SoundCatalogEntry,
  SOUNDS_BASE_PATH,
} from "./sound-bank.ts";
import {
  DEFAULT_WORKLET_SAMPLE_RATE,
  RING_BUFFER_CAPACITY,
  RING_BUFFER_SECONDS,
  TTS_WORKLET_NAME,
  TtsRingProcessor,
} from "./tts-worklet.processor.ts";
import type {
  AudioResumeState,
  AudioSettings,
  CueName,
  CueOptions,
  PcmChunk,
  PlaybackHandle,
  PlayerCallbacks,
  Sound,
  SoundLevel,
  WorkletInboundMsg,
  WorkletOutboundMsg,
} from "./types.ts";

export {
  AMBIENCE_CROSSFADE_SECONDS,
  AUDIO_SAMPLE_RATE,
  AudioMixer,
  bindAutoResume,
  buildSoundUrl,
  BUTTERWORTH_Q,
  clampCuePan,
  dbToLinear,
  DEFAULT_WORKLET_SAMPLE_RATE,
  DUCK_AMB_DB,
  DUCK_AMB_GAIN,
  DUCK_ATTACK_SECONDS,
  DUCK_ATTACK_TIME_CONSTANT,
  DUCK_RELEASE_SECONDS,
  DUCK_RELEASE_TIME_CONSTANT,
  DUCK_SFX_DB,
  DUCK_SFX_GAIN,
  ensureResumed,
  FIRST_12_SOUNDS,
  initAudioContext,
  isAmbienceName,
  isCueName,
  isSoundAllowedInMode,
  LEVEL_DB_TABLE,
  LEVEL_GAIN_TABLE,
  levelGain,
  LIMITER_ATTACK_SECONDS,
  LIMITER_KNEE_DB,
  LIMITER_RATIO,
  LIMITER_RELEASE_SECONDS,
  LIMITER_THRESHOLD_DB,
  MAGIC_FADE_DURATION_SECONDS,
  MAGIC_FADE_START_SECONDS,
  MAX_CUE_PAN,
  panFromViewportX,
  resolveEffectiveLevel,
  RING_BUFFER_CAPACITY,
  RING_BUFFER_SECONDS,
  SFX_BANDPASS_HIGH_HZ,
  SFX_BANDPASS_LOW_HZ,
  SOUND_CATALOG,
  SoundBank,
  type SoundBankOptions,
  type SoundCatalogEntry,
  SOUNDS_BASE_PATH,
  toResumeState,
  TTS_WORKLET_NAME,
  TtsRingProcessor,
};

export * from "./types.ts";

export interface AudioEngineOptions extends PlayerCallbacks, SoundBankOptions {
  readonly settings?: Partial<AudioSettings>;
  readonly preload?: boolean;
}

export interface AudioEngine {
  readonly ctx: AudioContext;
  readonly mixer: AudioMixer;
  readonly soundBank: SoundBank;
  readonly workletNode: AudioWorkletNode | null;
  readonly state: AudioResumeState;
  readonly fallbackNeeded: boolean;
  ensureResumed(): Promise<boolean>;
  setIdle(idle: boolean): void;
  setInactive(inactive: boolean): void;
  pushPcm(chunk: PcmChunk): void;
  interrupt(generationId: number): void;
  cancel(generationId: number): void;
  /** Resolves with whether the completed sentence had PCM samples. */
  waitForSentenceEnd(generationId: number, sentenceId: number): Promise<boolean>;
  playCue(name: CueName, opts?: CueOptions): PlaybackHandle;
  // Pass the answer generation when scheduling effects before its first PCM chunk.
  playSfx(sound: Sound | string, generationId?: number): PlaybackHandle;
  setAmbience(name: string | null, level?: SoundLevel): void;
  setSettings(settings: Partial<AudioSettings>): void;
  onWorkletMessage(listener: (msg: WorkletOutboundMsg) => void): () => void;
  onStateChange(listener: (state: AudioResumeState) => void): () => void;
  dispose(): Promise<void>;
}

function toDetachedArrayBuffer(samples: Int16Array): ArrayBuffer {
  if (
    samples.byteOffset === 0 &&
    samples.byteLength === samples.buffer.byteLength &&
    samples.buffer instanceof ArrayBuffer
  ) {
    return samples.buffer.slice(0);
  }
  const copy = new Int16Array(samples.length);
  copy.set(samples);
  return copy.buffer;
}

export async function createAudioEngine(
  options: AudioEngineOptions = {},
): Promise<AudioEngine> {
  const ctx = await initAudioContext();
  const idleClock = new AudioIdleController(ctx);
  const speech = new Map<string, { generation: number; lease: PlaybackLease; hadSamples: boolean }>();
  const releaseSpeech = (through: number): void => {
    for (const [key, entry] of speech) {
      if (entry.generation > through) continue;
      entry.lease.release();
      speech.delete(key);
    }
  };
  const mixer = new AudioMixer(ctx);
  if (options.settings) {
    mixer.setSettings(options.settings);
  }

  const soundBank = new SoundBank(ctx, mixer, {
    basePath: options.basePath,
    fetcher: options.fetcher,
    beginPlayback: () => idleClock.begin(),
  });

  let resumeState: AudioResumeState = "fallback";
  const stateListeners = new Set<(state: AudioResumeState) => void>();
  const workletListeners = new Set<(msg: WorkletOutboundMsg) => void>();
  const sentenceCompletions = new SentenceCompletions();
  let canceledThrough = -1;
  let activeGeneration = 0;
  let pcmQueue: Promise<void> = Promise.resolve();
  let disposed = false;

  const updateResumeState = (next: AudioResumeState): void => {
    if (resumeState === next) {
      return;
    }
    resumeState = next;
    options.onFallbackStateChange?.(next === "fallback");
    for (const listener of stateListeners) {
      listener(next);
    }
  };

  let workletNode: AudioWorkletNode | null = null;
  if (typeof AudioWorkletNode === "function") {
    workletNode = new AudioWorkletNode(ctx, TTS_WORKLET_NAME, {
      numberOfInputs: 0,
      numberOfOutputs: 1,
      outputChannelCount: [1],
      processorOptions: { sampleRate: ctx.sampleRate },
    });
    workletNode.connect(mixer.speechInput);

    workletNode.port.onmessage = (
      event: MessageEvent<WorkletOutboundMsg>,
    ): void => {
      const msg = event.data;
      if (msg.type === "speech-active") {
        mixer.setSpeechActive(msg.active);
        options.onVoiceLevel?.(msg.rms);
      } else if (msg.type === "sentence-end") {
        const key = `${msg.generationId}:${msg.sentenceId}`;
        // Interrupts release the entry before the worklet reports its end; no fallback then.
        const hadSamples = speech.get(key)?.hadSamples ?? true;
        speech.get(key)?.lease.release();
        speech.delete(key);
        sentenceCompletions.complete(msg.generationId, msg.sentenceId, hadSamples);
      }
      for (const listener of workletListeners) {
        listener(msg);
      }
    };
  }

  const resumed = await ensureResumed(ctx);
  resumeState = toResumeState(resumed);
  options.onFallbackStateChange?.(resumeState === "fallback");

  const unbindAutoResume = bindAutoResume(ctx, updateResumeState, () => idleClock.wantsRunning);

  if (options.preload) {
    void soundBank.preload();
  }

  return {
    ctx,
    mixer,
    soundBank,
    workletNode,
    get state(): AudioResumeState {
      return resumeState;
    },
    get fallbackNeeded(): boolean {
      return resumeState === "fallback";
    },
    async ensureResumed(): Promise<boolean> {
      const ok = await ensureResumed(ctx);
      updateResumeState(toResumeState(ok));
      return ok;
    },
    setIdle: (idle) => idleClock.setIdle(idle),
    setInactive: (inactive) => idleClock.setInactive(inactive),
    pushPcm(chunk: PcmChunk): void {
      if (disposed || chunk.generationId <= canceledThrough || chunk.generationId < activeGeneration) return;
      if (chunk.generationId > activeGeneration) {
        releaseSpeech(chunk.generationId - 1);
        activeGeneration = chunk.generationId;
        soundBank.beginGeneration(activeGeneration);
      }
      if (!workletNode) {
        return;
      }
      const key = `${chunk.generationId}:${chunk.sentenceId}`;
      let entry = speech.get(key);
      if (!entry) {
        entry = { generation: chunk.generationId, lease: idleClock.begin(), hadSamples: false };
        speech.set(key, entry);
      }
      const pending = entry;
      // All chunks of a sentence share the resume barrier; callbacks retain wire order.
      pcmQueue = pcmQueue.then(async () => {
        await pending.lease.ready;
        if (disposed || speech.get(key) !== pending || chunk.generationId <= canceledThrough || chunk.generationId < activeGeneration) return;
        const pcm = toDetachedArrayBuffer(chunk.samples);
        const msg: WorkletInboundMsg = {
          type: "push",
          generationId: chunk.generationId,
          sentenceId: chunk.sentenceId,
          sampleRate: chunk.sampleRate,
          eos: chunk.eos,
          pcm,
        };
        workletNode!.port.postMessage(msg, [pcm]);
        pending.hadSamples ||= chunk.samples.length > 0;
      }).catch(() => {
        if (speech.get(key) === pending) speech.delete(key);
        pending.lease.release();
        updateResumeState("fallback");
      });
    },
    interrupt(generationId: number): void {
      if (disposed || generationId <= canceledThrough) return;
      releaseSpeech(generationId);
      if (workletNode) {
        const msg: WorkletInboundMsg = {
          type: "interrupt",
          generationId,
        };
        workletNode.port.postMessage(msg);
      }
      if (generationId === activeGeneration) {
        mixer.setSpeechActive(false);
        options.onVoiceLevel?.(0);
      }
    },
    cancel(generationId: number): void {
      releaseSpeech(generationId);
      canceledThrough = Math.max(canceledThrough, generationId);
      if (workletNode) {
        const msg: WorkletInboundMsg = {
          type: "cancel",
          generationId,
        };
        workletNode.port.postMessage(msg);
      }
      if (generationId >= activeGeneration) {
        activeGeneration = generationId + 1;
        mixer.setSpeechActive(false);
        options.onVoiceLevel?.(0);
        soundBank.beginGeneration(activeGeneration);
      }

      sentenceCompletions.cancel(generationId);
    },
    waitForSentenceEnd(
      generationId: number,
      sentenceId: number,
    ): Promise<boolean> {
      return sentenceCompletions.wait(generationId, sentenceId);
    },
    playCue(name: CueName, opts?: CueOptions): PlaybackHandle {
      return soundBank.playCue(name, opts);
    },
    playSfx(sound: Sound | string, generationId = activeGeneration): PlaybackHandle {
      if (disposed || generationId <= canceledThrough || generationId < activeGeneration) {
        return { stop: () => {}, finished: Promise.resolve() };
      }
      if (generationId > activeGeneration) {
        releaseSpeech(generationId - 1);
        activeGeneration = generationId;
        soundBank.beginGeneration(generationId);
      }
      return soundBank.playSfx(sound);
    },
    setAmbience(name: string | null, level?: SoundLevel): void {
      soundBank.setAmbience(name, level);
    },
    setSettings(settings: Partial<AudioSettings>): void {
      mixer.setSettings(settings);
    },
    onWorkletMessage(listener: (msg: WorkletOutboundMsg) => void): () => void {
      workletListeners.add(listener);
      return () => {
        workletListeners.delete(listener);
      };
    },
    onStateChange(listener: (state: AudioResumeState) => void): () => void {
      stateListeners.add(listener);
      return () => {
        stateListeners.delete(listener);
      };
    },
    async dispose(): Promise<void> {
      disposed = true;
      idleClock.dispose();
      releaseSpeech(Infinity);
      unbindAutoResume();
      soundBank.stopAllSfxAndCues();
      soundBank.stopAmbienceImmediately();
      sentenceCompletions.dispose();
      workletListeners.clear();
      stateListeners.clear();
      if (workletNode) {
        workletNode.port.onmessage = null;
        workletNode.disconnect();
      }
      if (ctx.state !== "closed") {
        await ctx.close();
      }
    },
  };
}
