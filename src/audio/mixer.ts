// Master Web Audio graph: speech, sfx (300 Hz-3.2 kHz bandpass), cues, and ambience buses with ducking and brickwall limiter.

import type { AudioSettings, SoundLevel, SoundMode } from "./types.ts";

export const SFX_BANDPASS_LOW_HZ = 300;
export const SFX_BANDPASS_HIGH_HZ = 3200;
export const BUTTERWORTH_Q = 0.7071;

export const DUCK_AMB_DB = -12;
export const DUCK_SFX_DB = -6;
export const DUCK_AMB_GAIN = dbToLinear(DUCK_AMB_DB); // ~0.2512
export const DUCK_SFX_GAIN = dbToLinear(DUCK_SFX_DB); // ~0.5012

// 3 time constants (~95% settled) correspond to 300 ms attack and 600 ms release.
export const DUCK_ATTACK_SECONDS = 0.3;
export const DUCK_RELEASE_SECONDS = 0.6;
export const DUCK_ATTACK_TIME_CONSTANT = 0.1;
export const DUCK_RELEASE_TIME_CONSTANT = 0.2;

export const MAX_CUE_PAN = 0.3;

export const LIMITER_THRESHOLD_DB = -3;
export const LIMITER_KNEE_DB = 0;
export const LIMITER_RATIO = 20;
export const LIMITER_ATTACK_SECONDS = 0.001;
export const LIMITER_RELEASE_SECONDS = 0.08;

export const LEVEL_DB_TABLE: Readonly<Record<SoundLevel, number>> = {
  soft: -6,
  normal: 0,
  loud: 3,
};

export const LEVEL_GAIN_TABLE: Readonly<Record<SoundLevel, number>> = {
  soft: dbToLinear(LEVEL_DB_TABLE.soft),
  normal: dbToLinear(LEVEL_DB_TABLE.normal),
  loud: dbToLinear(LEVEL_DB_TABLE.loud),
};

export function dbToLinear(db: number): number {
  return Math.pow(10, db / 20);
}

/**
 * Resolves the effective SoundLevel under the active user policy (`calm` caps all sounds at `soft`).
 */
export function resolveEffectiveLevel(
  level: SoundLevel,
  mode: SoundMode,
): SoundLevel {
  return mode === "calm" ? "soft" : level;
}

/**
 * Maps `soft` (-6 dB), `normal` (0 dB), `loud` (+3 dB) to linear gain; in `calm` mode caps at `soft`.
 */
export function levelGain(
  level: SoundLevel,
  mode: SoundMode = "calm",
): number {
  const effective = resolveEffectiveLevel(level, mode);
  return LEVEL_GAIN_TABLE[effective];
}

/**
 * Hard-clamps stereo pan to [-0.3, +0.3] for older listeners and asymmetric hearing loss.
 */
export function clampCuePan(pan: number): number {
  if (!Number.isFinite(pan)) {
    return 0;
  }
  return Math.max(-MAX_CUE_PAN, Math.min(MAX_CUE_PAN, pan));
}

/**
 * Computes cue pan in [-0.3, +0.3] from a target CSS X coordinate and viewport CSS width.
 */
export function panFromViewportX(
  targetCssX: number,
  viewportCssWidth: number,
): number {
  if (
    !Number.isFinite(targetCssX) ||
    !Number.isFinite(viewportCssWidth) ||
    viewportCssWidth <= 0
  ) {
    return 0;
  }
  const normalized = (targetCssX / viewportCssWidth) * 2 - 1;
  return clampCuePan(normalized * MAX_CUE_PAN);
}

export class AudioMixer {
  readonly ctx: AudioContext;
  readonly speechInput: GainNode;
  readonly sfxInput: GainNode;
  readonly cuesInput: GainNode;
  readonly ambInput: GainNode;

  readonly sfxHighpass: BiquadFilterNode;
  readonly sfxLowpass: BiquadFilterNode;
  readonly sfxDuckGain: GainNode;
  readonly ambDuckGain: GainNode;
  readonly masterSum: GainNode;
  readonly masterVolumeGain: GainNode;
  readonly limiter: DynamicsCompressorNode;

  private mode: SoundMode = "calm";
  private volume = 1.0;
  private voiceSpeed = 1.0;
  private reduceMotion = false;
  private speechActive = false;

  constructor(ctx: AudioContext) {
    this.ctx = ctx;

    this.speechInput = ctx.createGain();
    this.sfxInput = ctx.createGain();
    this.cuesInput = ctx.createGain();
    this.ambInput = ctx.createGain();

    // 300 Hz - 3.2 kHz 2nd-order Butterworth bandpass for SFX (laptop speakers & hearing aids).
    this.sfxHighpass = ctx.createBiquadFilter();
    this.sfxHighpass.type = "highpass";
    this.sfxHighpass.frequency.value = SFX_BANDPASS_LOW_HZ;
    this.sfxHighpass.Q.value = BUTTERWORTH_Q;

    this.sfxLowpass = ctx.createBiquadFilter();
    this.sfxLowpass.type = "lowpass";
    this.sfxLowpass.frequency.value = SFX_BANDPASS_HIGH_HZ;
    this.sfxLowpass.Q.value = BUTTERWORTH_Q;

    this.sfxDuckGain = ctx.createGain();
    this.ambDuckGain = ctx.createGain();
    this.masterSum = ctx.createGain();
    this.masterVolumeGain = ctx.createGain();

    // Brickwall limiter at -3 dBFS.
    this.limiter = ctx.createDynamicsCompressor();
    this.limiter.threshold.value = LIMITER_THRESHOLD_DB;
    this.limiter.knee.value = LIMITER_KNEE_DB;
    this.limiter.ratio.value = LIMITER_RATIO;
    this.limiter.attack.value = LIMITER_ATTACK_SECONDS;
    this.limiter.release.value = LIMITER_RELEASE_SECONDS;

    // Wire speech bus: speechInput -> masterSum
    this.speechInput.connect(this.masterSum);

    // Wire sfx bus: sfxInput -> highpass(300Hz) -> lowpass(3200Hz) -> sfxDuckGain -> masterSum
    this.sfxInput.connect(this.sfxHighpass);
    this.sfxHighpass.connect(this.sfxLowpass);
    this.sfxLowpass.connect(this.sfxDuckGain);

    // Wire cues bus: cuesInput -> sfxDuckGain -> masterSum (cues are already band-limited in synth_cues.py)
    this.cuesInput.connect(this.sfxDuckGain);
    this.sfxDuckGain.connect(this.masterSum);

    // Wire ambience bus: ambInput -> ambDuckGain -> masterSum
    this.ambInput.connect(this.ambDuckGain);
    this.ambDuckGain.connect(this.masterSum);

    // Wire master chain: masterSum -> masterVolumeGain -> limiter -> destination
    this.masterSum.connect(this.masterVolumeGain);
    this.masterVolumeGain.connect(this.limiter);
    this.limiter.connect(ctx.destination);
  }

  getMode(): SoundMode {
    return this.mode;
  }

  getSettings(): Required<AudioSettings> {
    return {
      volume: this.volume,
      voiceSpeed: this.voiceSpeed,
      mode: this.mode,
      reduceMotion: this.reduceMotion,
    };
  }

  isSpeechActive(): boolean {
    return this.speechActive;
  }

  /**
   * Schedules smooth ducking of ambience (-12 dB) and sfx/cues (-6 dB) while speech is active.
   * Attack: 300 ms (timeConstant 0.1 s); Release: 600 ms (timeConstant 0.2 s).
   */
  setSpeechActive(active: boolean): void {
    this.speechActive = active;
    const now = this.ctx.currentTime;
    if (active) {
      this.ambDuckGain.gain.setTargetAtTime(
        DUCK_AMB_GAIN,
        now,
        DUCK_ATTACK_TIME_CONSTANT,
      );
      this.sfxDuckGain.gain.setTargetAtTime(
        DUCK_SFX_GAIN,
        now,
        DUCK_ATTACK_TIME_CONSTANT,
      );
    } else {
      this.ambDuckGain.gain.setTargetAtTime(
        1.0,
        now,
        DUCK_RELEASE_TIME_CONSTANT,
      );
      this.sfxDuckGain.gain.setTargetAtTime(
        1.0,
        now,
        DUCK_RELEASE_TIME_CONSTANT,
      );
    }
  }

  setSettings(opts: Partial<AudioSettings>): void {
    if (typeof opts.volume === "number" && Number.isFinite(opts.volume)) {
      this.volume = Math.max(0, Math.min(1, opts.volume));
      this.masterVolumeGain.gain.setTargetAtTime(
        this.volume,
        this.ctx.currentTime,
        0.02,
      );
    }
    if (
      typeof opts.voiceSpeed === "number" &&
      Number.isFinite(opts.voiceSpeed)
    ) {
      this.voiceSpeed = Math.max(0.5, Math.min(2.0, opts.voiceSpeed));
    }
    if (opts.mode === "calm" || opts.mode === "fun") {
      this.mode = opts.mode;
    }
    if (typeof opts.reduceMotion === "boolean") {
      this.reduceMotion = opts.reduceMotion;
    }
  }

  levelGain(level: SoundLevel, mode: SoundMode = this.mode): number {
    return levelGain(level, mode);
  }
}
