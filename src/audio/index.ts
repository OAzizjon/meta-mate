// Public facade for MetaMate's WebView audio engine.

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
  pushPcm(chunk: PcmChunk): void;
  cancel(generationId: number): void;
  waitForSentenceEnd(generationId: number, sentenceId: number): Promise<void>;
  playCue(name: CueName, opts?: CueOptions): PlaybackHandle;
  playSfx(sound: Sound | string): PlaybackHandle;
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
  const mixer = new AudioMixer(ctx);
  if (options.settings) {
    mixer.setSettings(options.settings);
  }

  const soundBank = new SoundBank(ctx, mixer, {
    basePath: options.basePath,
    fetcher: options.fetcher,
  });

  let resumeState: AudioResumeState = "fallback";
  const stateListeners = new Set<(state: AudioResumeState) => void>();
  const workletListeners = new Set<(msg: WorkletOutboundMsg) => void>();
  const sentenceWaiters = new Map<string, Set<() => void>>();

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
        const waiters = sentenceWaiters.get(key);
        if (waiters) {
          sentenceWaiters.delete(key);
          for (const resolve of waiters) {
            resolve();
          }
        }
      }
      for (const listener of workletListeners) {
        listener(msg);
      }
    };
  }

  const resumed = await ensureResumed(ctx);
  resumeState = toResumeState(resumed);
  options.onFallbackStateChange?.(resumeState === "fallback");

  const unbindAutoResume = bindAutoResume(ctx, updateResumeState);

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
    pushPcm(chunk: PcmChunk): void {
      if (!workletNode) {
        return;
      }
      const pcm = toDetachedArrayBuffer(chunk.samples);
      const msg: WorkletInboundMsg = {
        type: "push",
        generationId: chunk.generationId,
        sentenceId: chunk.sentenceId,
        sampleRate: chunk.sampleRate,
        eos: chunk.eos,
        pcm,
      };
      workletNode.port.postMessage(msg, [pcm]);
    },
    cancel(generationId: number): void {
      if (workletNode) {
        const msg: WorkletInboundMsg = {
          type: "cancel",
          generationId,
        };
        workletNode.port.postMessage(msg);
      }
      mixer.setSpeechActive(false);
      options.onVoiceLevel?.(0);
      soundBank.stopAllSfxAndCues();

      // Resolve any pending sentence waiters for older or equal generations.
      for (const [key, waiters] of sentenceWaiters.entries()) {
        const genPart = Number(key.split(":", 1)[0]);
        if (Number.isFinite(genPart) && genPart <= generationId) {
          sentenceWaiters.delete(key);
          for (const resolve of waiters) {
            resolve();
          }
        }
      }
    },
    waitForSentenceEnd(
      generationId: number,
      sentenceId: number,
    ): Promise<void> {
      const key = `${generationId}:${sentenceId}`;
      return new Promise<void>((resolve) => {
        let set = sentenceWaiters.get(key);
        if (!set) {
          set = new Set();
          sentenceWaiters.set(key, set);
        }
        set.add(resolve);
      });
    },
    playCue(name: CueName, opts?: CueOptions): PlaybackHandle {
      return soundBank.playCue(name, opts);
    },
    playSfx(sound: Sound | string): PlaybackHandle {
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
      unbindAutoResume();
      soundBank.stopAllSfxAndCues();
      soundBank.stopAmbienceImmediately();
      for (const waiters of sentenceWaiters.values()) {
        for (const resolve of waiters) {
          resolve();
        }
      }
      sentenceWaiters.clear();
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
