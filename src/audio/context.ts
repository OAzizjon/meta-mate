// AudioContext lifecycle and gesture-free unlock for the audio-owner WebView.

import type { AudioResumeState } from "./types.ts";
// Vite compiles the processor into its own script and gives its URL: a plain
// `new URL("./x.ts", import.meta.url)` would ship untranspiled TypeScript.
import workletUrl from "./tts-worklet.processor.ts?worker&url";

export const AUDIO_SAMPLE_RATE = 48_000;
const CLOCK_CHECK_DELAY_MS = 50;
const CLOCK_START_TIMEOUT_MS = 2000;

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => {
    setTimeout(resolve, ms);
  });
}

/**
 * Creates the single 48 kHz interactive AudioContext and loads the TTS ring-buffer AudioWorklet.
 */
export async function initAudioContext(): Promise<AudioContext> {
  const ctx = new AudioContext({
    sampleRate: AUDIO_SAMPLE_RATE,
    latencyHint: "interactive",
  });
  await ctx.audioWorklet.addModule(workletUrl);
  return ctx;
}

/**
 * Attempts to resume `ctx` without a DOM user gesture (WebView2 `--autoplay-policy=no-user-gesture-required`)
 * and verifies that `ctx.currentTime` advances after 50 ms.
 * Returns `false` when blocked so the caller can show a fallback state or switch to Rust audio.
 */
export async function ensureResumed(ctx: AudioContext): Promise<boolean> {
  try {
    if (ctx.state !== "running") {
      await ctx.resume();
    }
  } catch {
    // Autoplay policy blocked resume without a user gesture.
    return false;
  }

  // The output device can take over a second to start: a running context whose clock has
  // not moved yet is not blocked (measured 2026-10-27: t=0 at 50 ms, t=1.45 at 1.5 s).
  const startClock = ctx.currentTime;
  for (let waited = 0; waited < CLOCK_START_TIMEOUT_MS; waited += CLOCK_CHECK_DELAY_MS) {
    await sleep(CLOCK_CHECK_DELAY_MS);
    if (ctx.state !== "running") {
      try {
        await ctx.resume();
      } catch {
        return false;
      }
    }
    if (ctx.state === "running" && (ctx.currentTime > startClock || ctx.currentTime > 0)) {
      return true;
    }
  }
  return false;
}

export function toResumeState(resumed: boolean): AudioResumeState {
  return resumed ? "running" : "fallback";
}

/**
 * Keeps `ctx` resumed across `statechange` events and opportunistic DOM gestures,
 * notifying `onStateChange` whenever the engine transitions between `"running"` and `"fallback"`.
 */
export function bindAutoResume(
  ctx: AudioContext,
  onStateChange?: (state: AudioResumeState) => void,
): () => void {
  let disposed = false;

  const tryResume = (): void => {
    if (disposed || ctx.state === "closed") {
      return;
    }
    if (ctx.state === "running") {
      onStateChange?.("running");
      return;
    }
    void ctx
      .resume()
      .then(() => {
        if (!disposed) {
          onStateChange?.(ctx.state === "running" ? "running" : "fallback");
        }
      })
      .catch(() => {
        if (!disposed) {
          onStateChange?.("fallback");
        }
      });
  };

  const handleStateChange = (): void => {
    if (disposed) {
      return;
    }
    if (ctx.state === "running") {
      onStateChange?.("running");
    } else if (ctx.state === "suspended") {
      tryResume();
    }
  };

  ctx.addEventListener("statechange", handleStateChange);
  if (typeof window !== "undefined") {
    window.addEventListener("pointerdown", tryResume, { passive: true });
    window.addEventListener("keydown", tryResume, { passive: true });
  }

  return () => {
    disposed = true;
    ctx.removeEventListener("statechange", handleStateChange);
    if (typeof window !== "undefined") {
      window.removeEventListener("pointerdown", tryResume);
      window.removeEventListener("keydown", tryResume);
    }
  };
}
