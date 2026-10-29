// OGG preloader/decoder cache (/sounds/<kind>/<name>.ogg), panned cue player, SFX player, and crossfading ambience loop.

import { clampCuePan, type AudioMixer } from "./mixer.ts";
import type { PlaybackLease } from "./idle.ts";
import {
  AMBIENCE_NAMES,
  CUE_NAMES,
  type AmbienceName,
  type CueName,
  type CueOptions,
  type PlaybackHandle,
  type Sound,
  type SoundKind,
  type SoundLevel,
  type SoundMode,
} from "./types.ts";

export const SOUNDS_BASE_PATH = "/sounds";
export const AMBIENCE_CROSSFADE_SECONDS = 1.5;
export const MAGIC_FADE_START_SECONDS = 2.5;
export const MAGIC_FADE_DURATION_SECONDS = 2.5;

export interface SoundCatalogEntry {
  readonly kind: "cues" | "sfx";
  readonly defaultLevel: SoundLevel;
  readonly calm: boolean;
}

// Canonical sound table matching crates/mm-core/src/matescript/vocabulary.rs and docs/sounds.md.
export const SOUND_CATALOG: Readonly<Record<string, SoundCatalogEntry>> = {
  ping: { kind: "cues", defaultLevel: "soft", calm: true },
  success: { kind: "cues", defaultLevel: "soft", calm: true },
  oops: { kind: "cues", defaultLevel: "soft", calm: true },
  think: { kind: "cues", defaultLevel: "soft", calm: true },
  listen: { kind: "cues", defaultLevel: "soft", calm: true },
  done: { kind: "cues", defaultLevel: "soft", calm: true },
  whoosh: { kind: "cues", defaultLevel: "soft", calm: true },
  pop: { kind: "cues", defaultLevel: "soft", calm: true },
  sleep: { kind: "cues", defaultLevel: "soft", calm: true },
  wake: { kind: "cues", defaultLevel: "soft", calm: true },
  birds: { kind: "sfx", defaultLevel: "soft", calm: true },
  rooster: { kind: "sfx", defaultLevel: "normal", calm: false },
  cat: { kind: "sfx", defaultLevel: "normal", calm: true },
  dog: { kind: "sfx", defaultLevel: "normal", calm: false },
  owl: { kind: "sfx", defaultLevel: "soft", calm: true },
  horse: { kind: "sfx", defaultLevel: "normal", calm: false },
  knock: { kind: "sfx", defaultLevel: "normal", calm: true },
  clock: { kind: "sfx", defaultLevel: "soft", calm: true },
  bell: { kind: "sfx", defaultLevel: "normal", calm: true },
  rimshot: { kind: "sfx", defaultLevel: "normal", calm: false },
  drumroll: { kind: "sfx", defaultLevel: "normal", calm: false },
  applause: { kind: "sfx", defaultLevel: "normal", calm: true },
  cheer: { kind: "sfx", defaultLevel: "normal", calm: false },
  magic: { kind: "sfx", defaultLevel: "soft", calm: true },
  fanfare: { kind: "sfx", defaultLevel: "loud", calm: false },
  thunder: { kind: "sfx", defaultLevel: "loud", calm: false },
  wind: { kind: "sfx", defaultLevel: "normal", calm: true },
  splash: { kind: "sfx", defaultLevel: "normal", calm: true },
  boom: { kind: "sfx", defaultLevel: "loud", calm: false },
  laser: { kind: "sfx", defaultLevel: "normal", calm: false },
  boing: { kind: "sfx", defaultLevel: "normal", calm: true },
  slip: { kind: "sfx", defaultLevel: "normal", calm: false },
  giggle: { kind: "sfx", defaultLevel: "soft", calm: true },
  footsteps: { kind: "sfx", defaultLevel: "soft", calm: true },
  door: { kind: "sfx", defaultLevel: "normal", calm: false },
  phone: { kind: "sfx", defaultLevel: "normal", calm: false },
  train: { kind: "sfx", defaultLevel: "normal", calm: false },
  car: { kind: "sfx", defaultLevel: "loud", calm: false },
  heart: { kind: "sfx", defaultLevel: "soft", calm: false },
  tada: { kind: "sfx", defaultLevel: "normal", calm: true },
  sad: { kind: "sfx", defaultLevel: "normal", calm: false },
};

export const FIRST_12_SOUNDS: ReadonlyArray<{
  readonly kind: SoundKind;
  readonly name: string;
}> = [
  { kind: "cues", name: "ping" },
  { kind: "cues", name: "success" },
  { kind: "cues", name: "oops" },
  { kind: "cues", name: "think" },
  { kind: "cues", name: "whoosh" },
  { kind: "cues", name: "done" },
  { kind: "amb", name: "forest" },
  { kind: "amb", name: "rain" },
  { kind: "sfx", name: "birds" },
  { kind: "sfx", name: "thunder" },
  { kind: "sfx", name: "rimshot" },
  { kind: "sfx", name: "applause" },
];

export function buildSoundUrl(
  kind: SoundKind,
  name: string,
  basePath: string = SOUNDS_BASE_PATH,
): string {
  const cleanBase = basePath.endsWith("/") ? basePath.slice(0, -1) : basePath;
  return `${cleanBase}/${kind}/${name}.ogg`;
}

export function isCueName(name: string): name is CueName {
  return (CUE_NAMES as readonly string[]).includes(name);
}

export function isAmbienceName(name: string): name is AmbienceName {
  return (AMBIENCE_NAMES as readonly string[]).includes(name);
}

export function isSoundAllowedInMode(name: string, mode: SoundMode): boolean {
  const entry = SOUND_CATALOG[name];
  if (!entry) {
    return false;
  }
  return mode === "fun" || entry.calm;
}

interface ActiveAmbienceBed {
  readonly release: () => void;
  readonly name: string;
  readonly source: AudioBufferSourceNode;
  readonly gainNode: GainNode;
  stopTimer: ReturnType<typeof setTimeout> | null;
}

export interface SoundBankOptions {
  readonly basePath?: string;
  readonly fetcher?: typeof fetch;
  readonly beginPlayback?: () => PlaybackLease;
}

export class SoundBank {
  private readonly ctx: AudioContext;
  private readonly mixer: AudioMixer;
  private readonly basePath: string;
  private readonly fetcher: typeof fetch;
  private readonly beginPlayback: () => PlaybackLease;

  private readonly cache = new Map<string, AudioBuffer>();
  private readonly inflight = new Map<string, Promise<AudioBuffer>>();
  private readonly activeStops = new Set<() => void>();
  private activeThinkStop: (() => void) | null = null;
  private currentAmbience: ActiveAmbienceBed | null = null;
  private readonly fadingAmbience = new Set<ActiveAmbienceBed>();
  private sfxGeneration = 0;
  private sfxCount = 0;
  private ambienceRequestToken = 0;

  constructor(
    ctx: AudioContext,
    mixer: AudioMixer,
    options?: SoundBankOptions,
  ) {
    this.ctx = ctx;
    this.mixer = mixer;
    this.basePath = options?.basePath ?? SOUNDS_BASE_PATH;
    this.fetcher = options?.fetcher ?? globalThis.fetch.bind(globalThis);
    this.beginPlayback = options?.beginPlayback ?? (() => ({ ready: Promise.resolve(), release() {} }));
    this.mixer.onModeChange(() => {
      // Stop old policy playback, including pending one-shots and fading beds.
      this.stopAllSfxAndCues();
      this.stopAmbienceImmediately();
    });
  }

  beginGeneration(generationId: number): void {
    if (generationId <= this.sfxGeneration) return;
    this.sfxGeneration = generationId;
    this.sfxCount = 0;
    this.stopAllSfxAndCues();
  }

  getCachedBuffer(kind: SoundKind, name: string): AudioBuffer | undefined {
    return this.cache.get(buildSoundUrl(kind, name, this.basePath));
  }

  /**
   * Loads and decodes `/sounds/<kind>/<name>.ogg` with fetch + decodeAudioData, caching the AudioBuffer.
   */
  async load(kind: SoundKind, name: string): Promise<AudioBuffer> {
    const url = buildSoundUrl(kind, name, this.basePath);
    const cached = this.cache.get(url);
    if (cached) {
      return cached;
    }

    const existing = this.inflight.get(url);
    if (existing) {
      return existing;
    }

    const promise = (async (): Promise<AudioBuffer> => {
      try {
        const response = await this.fetcher(url);
        if (!response.ok) {
          throw new Error(`Failed to fetch sound ${url}: ${response.status}`);
        }
        const arrayBuffer = await response.arrayBuffer();
        // Slice buffer so decodeAudioData detaching the ArrayBuffer never mutates caller copies.
        const decoded = await this.ctx.decodeAudioData(arrayBuffer.slice(0));
        this.cache.set(url, decoded);
        return decoded;
      } finally {
        this.inflight.delete(url);
      }
    })();

    this.inflight.set(url, promise);
    return promise;
  }

  /**
   * Preloads a batch of sounds (defaults to the core cues and first-12 library set).
   */
  async preload(
    entries: ReadonlyArray<{
      readonly kind: SoundKind;
      readonly name: string;
    }> = FIRST_12_SOUNDS,
  ): Promise<void> {
    await Promise.allSettled(
      entries.map((entry) => this.load(entry.kind, entry.name)),
    );
  }

  /**
   * Plays a synthesized cue on the cues bus with stereo panning clamped to [-0.3, +0.3].
   * `think` loops seamlessly until stopped.
   */
  playCue(name: CueName, opts?: CueOptions): PlaybackHandle {
    if (!isCueName(name)) {
      return { stop: () => {}, finished: Promise.resolve() };
    }

    if (name === "think" && this.activeThinkStop) {
      this.activeThinkStop();
      this.activeThinkStop = null;
    }

    const shouldLoop = opts?.loop ?? name === "think";
    const playback = this.beginPlayback();
    const panValue = clampCuePan(opts?.pan ?? 0);

    let cancelled = false;
    let activeSource: AudioBufferSourceNode | null = null;
    let resolveFinished: (() => void) | null = null;

    const finished = new Promise<void>((resolve) => {
      resolveFinished = resolve;
    });

    const cleanup = (): void => {
      playback.release();
      this.activeStops.delete(stop);
      if (name === "think" && this.activeThinkStop === stop) {
        this.activeThinkStop = null;
      }
      if (resolveFinished) {
        const r = resolveFinished;
        resolveFinished = null;
        r();
      }
    };

    const stop = (): void => {
      if (cancelled) {
        return;
      }
      cancelled = true;
      if (activeSource) {
        try {
          activeSource.stop();
        } catch {
          // Ignore if source already ended.
        }
        activeSource = null;
      }
      cleanup();
    };

    this.activeStops.add(stop);
    if (name === "think") {
      this.activeThinkStop = stop;
    }

    void Promise.all([this.load("cues", name), playback.ready])
      .then(([buffer]) => {
        if (cancelled) {
          cleanup();
          return;
        }
        const source = this.ctx.createBufferSource();
        source.buffer = buffer;
        source.loop = shouldLoop;

        const panner = this.ctx.createStereoPanner();
        panner.pan.value = panValue;

        source.connect(panner);
        panner.connect(this.mixer.cuesInput);

        source.onended = () => {
          source.disconnect();
          panner.disconnect();
          cleanup();
        };

        activeSource = source;
        source.start(0);
      })
      .catch(() => {
        cleanup();
      });

    return { stop, finished };
  }

  /**
   * Plays an SFX one-shot through the level gain table and 300 Hz-3.2 kHz bandpass bus.
   * Blocks `calm: no` sounds in calm mode and fades out `magic` after 2.5 s.
   */
  playSfx(soundInput: Sound | string): PlaybackHandle {
    const rawName =
      typeof soundInput === "string" ? soundInput : soundInput.name;
    const entry = SOUND_CATALOG[rawName];
    if (!entry) {
      return { stop: () => {}, finished: Promise.resolve() };
    }

    const mode = this.mixer.getMode();
    if (mode === "calm" && !entry.calm) {
      return { stop: () => {}, finished: Promise.resolve() };
    }

    const requestedLevel: SoundLevel =
      typeof soundInput === "string" ? entry.defaultLevel : soundInput.level;
    if (this.sfxCount >= (mode === "calm" ? 2 : 4)) {
      return { stop: () => {}, finished: Promise.resolve() };
    }
    // Reserve before loading so concurrent requests share the same budget.
    this.sfxCount += 1;
    const playback = this.beginPlayback();

    let cancelled = false;
    let activeSource: AudioBufferSourceNode | null = null;
    let resolveFinished: (() => void) | null = null;

    const finished = new Promise<void>((resolve) => {
      resolveFinished = resolve;
    });

    const cleanup = (): void => {
      playback.release();
      this.activeStops.delete(stop);
      if (resolveFinished) {
        const r = resolveFinished;
        resolveFinished = null;
        r();
      }
    };

    const stop = (): void => {
      if (cancelled) {
        return;
      }
      cancelled = true;
      if (activeSource) {
        try {
          activeSource.stop();
        } catch {
          // Ignore if source already stopped.
        }
        activeSource = null;
      }
      cleanup();
    };

    this.activeStops.add(stop);

    const loadBuffer = async (): Promise<AudioBuffer> => {
      try {
        return await this.load(entry.kind, rawName);
      } catch (err) {
        const fallbackKind: SoundKind = entry.kind === "cues" ? "sfx" : "cues";
        if (this.cache.has(buildSoundUrl(fallbackKind, rawName, this.basePath))) {
          return this.load(fallbackKind, rawName);
        }
        throw err;
      }
    };

    void Promise.all([loadBuffer(), playback.ready])
      .then(([buffer]) => {
        const currentMode = this.mixer.getMode();
        if (cancelled || !isSoundAllowedInMode(rawName, currentMode)) {
          cleanup();
          return;
        }
        const source = this.ctx.createBufferSource();
        source.buffer = buffer;

        const gainNode = this.ctx.createGain();
        const now = this.ctx.currentTime;
        const targetGain = this.mixer.levelGain(requestedLevel, currentMode);
        gainNode.gain.setValueAtTime(targetGain, now);

        // `magic` (AsGliss.ogg, 5.1 s): fade out after ~2.5 s in the mixer (docs/sounds.md:94).
        if (rawName === "magic") {
          const fadeStart = now + MAGIC_FADE_START_SECONDS;
          const fadeEnd = fadeStart + MAGIC_FADE_DURATION_SECONDS;
          gainNode.gain.setValueAtTime(targetGain, fadeStart);
          gainNode.gain.linearRampToValueAtTime(0, fadeEnd);
        }

        source.connect(gainNode);
        gainNode.connect(this.mixer.sfxInput);

        source.onended = () => {
          source.disconnect();
          gainNode.disconnect();
          cleanup();
        };

        activeSource = source;
        source.start(0);
      })
      .catch(() => {
        cleanup();
      });

    return { stop, finished };
  }

  /**
   * Starts, switches, or stops (`null` / `"off"`) the looping ambience bed with a 1.5 s linear crossfade.
   */
  setAmbience(
    name: string | null,
    level: SoundLevel = "soft",
  ): void {
    const token = ++this.ambienceRequestToken;
    const normalized =
      name === null || name === "off" || name === "" ? null : name;

    if (normalized === null || !isAmbienceName(normalized)) {
      if (this.currentAmbience) {
        this.fadeOutAmbienceBed(this.currentAmbience);
        this.currentAmbience = null;
      }
      return;
    }

    const targetGain = this.mixer.levelGain(level);

    if (this.currentAmbience && this.currentAmbience.name === normalized) {
      const now = this.ctx.currentTime;
      this.currentAmbience.gainNode.gain.setValueAtTime(
        this.currentAmbience.gainNode.gain.value,
        now,
      );
      this.currentAmbience.gainNode.gain.linearRampToValueAtTime(
        targetGain,
        now + AMBIENCE_CROSSFADE_SECONDS,
      );
      return;
    }

    const previous = this.currentAmbience;
    if (previous) {
      this.fadeOutAmbienceBed(previous);
      this.currentAmbience = null;
    }

    const playback = this.beginPlayback();
    let handedOff = false;
    void Promise.all([this.load("amb", normalized), playback.ready])
      .then(([buffer]) => {
        if (token !== this.ambienceRequestToken) {
          return;
        }
        const source = this.ctx.createBufferSource();
        source.buffer = buffer;
        source.loop = true;

        const gainNode = this.ctx.createGain();
        const now = this.ctx.currentTime;
        gainNode.gain.setValueAtTime(0, now);
        gainNode.gain.linearRampToValueAtTime(
          this.mixer.levelGain(level),
          now + AMBIENCE_CROSSFADE_SECONDS,
        );

        source.connect(gainNode);
        gainNode.connect(this.mixer.ambInput);
        source.start(0);

        this.currentAmbience = {
          release: playback.release,
          name: normalized,
          source,
          gainNode,
          stopTimer: null,
        };
        handedOff = true;
      })
      .catch(() => {
        // Ignore missing ambience file in offline/test environments.
      }).finally(() => {
        if (!handedOff) playback.release();
      });
  }

  /**
   * Immediately stops all active one-shot SFX and cues (including the `think` loop) on barge-in.
   */
  stopAllSfxAndCues(): void {
    const stops = Array.from(this.activeStops);
    this.activeStops.clear();
    this.activeThinkStop = null;
    for (const stop of stops) {
      stop();
    }
  }

  stopAmbienceImmediately(): void {
    this.ambienceRequestToken += 1;
    const beds = new Set(this.fadingAmbience);
    if (this.currentAmbience) beds.add(this.currentAmbience);
    this.currentAmbience = null;
    this.fadingAmbience.clear();
    for (const bed of beds) this.stopAmbienceBed(bed);
  }

  private stopAmbienceBed(bed: ActiveAmbienceBed): void {
    bed.release();
    if (bed.stopTimer !== null) {
      clearTimeout(bed.stopTimer);
    }
    try {
      bed.source.stop();
    } catch {
      // Ignore if already stopped.
    }
    bed.source.disconnect();
    bed.gainNode.disconnect();
  }

  private fadeOutAmbienceBed(bed: ActiveAmbienceBed): void {
    this.fadingAmbience.add(bed);
    const now = this.ctx.currentTime;
    bed.gainNode.gain.setValueAtTime(bed.gainNode.gain.value, now);
    bed.gainNode.gain.linearRampToValueAtTime(
      0,
      now + AMBIENCE_CROSSFADE_SECONDS,
    );
    bed.stopTimer = setTimeout(() => {
      this.fadingAmbience.delete(bed);
      this.stopAmbienceBed(bed);
    }, Math.round(AMBIENCE_CROSSFADE_SECONDS * 1000) + 50);
  }
}
