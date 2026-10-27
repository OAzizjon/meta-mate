// Shared audio, bus, cue, worklet, and timeline types for the WebView audio engine.

export type SoundMode = "calm" | "fun";

export type SoundLevel = "soft" | "normal" | "loud";

export type SoundKind = "cues" | "sfx" | "amb";

export const CUE_NAMES = [
  "ping",
  "success",
  "oops",
  "think",
  "listen",
  "done",
  "whoosh",
  "pop",
  "sleep",
  "wake",
] as const;

export type CueName = (typeof CUE_NAMES)[number];

export const AMBIENCE_NAMES = [
  "forest",
  "rain",
  "sea",
  "fire",
  "night",
  "park",
  "cafe",
] as const;

export type AmbienceName = (typeof AMBIENCE_NAMES)[number];

export const MOOD_NAMES = [
  "calm",
  "happy",
  "thinking",
  "surprised",
  "sad",
  "proud",
  "sleepy",
] as const;

export type MoodName = (typeof MOOD_NAMES)[number];

export type RobotState =
  | "Idle"
  | "Listening"
  | "Thinking"
  | "Speaking"
  | "Flying"
  | "Pointing"
  | "WaitingForClick"
  | "Success"
  | "Oops"
  | "CantFind"
  | "Sleeping";

export type AudioResumeState = "running" | "fallback";

export interface AudioSettings {
  readonly volume: number;
  readonly voiceSpeed: number;
  readonly mode: SoundMode;
  readonly reduceMotion?: boolean;
}

export interface Sound {
  readonly name: string;
  readonly level: SoundLevel;
  readonly solo?: boolean;
}

export type Point =
  | { readonly kind: "id"; readonly id: string }
  | { readonly kind: "pixel"; readonly x: number; readonly y: number }
  | { readonly Id: string }
  | { readonly Pixel: readonly [number, number] };

export interface Step {
  readonly number: number;
  readonly total: number;
  readonly target: Point;
}

export type Bound =
  | { readonly kind: "sfx"; readonly sound: Sound }
  | { readonly kind: "mood"; readonly mood: MoodName }
  | { readonly kind: "point"; readonly point: Point }
  | { readonly kind: "step"; readonly step: Step }
  | { readonly kind: "show"; readonly text: string };

export interface Say {
  readonly id: number;
  readonly text: string;
  readonly bound: readonly Bound[];
}

export type Item =
  | { readonly kind: "lang"; readonly code: string }
  | { readonly kind: "say"; readonly say: Say }
  | { readonly kind: "bound"; readonly bound: Bound }
  | { readonly kind: "sfx"; readonly sound: Sound }
  | {
      readonly kind: "amb";
      readonly name: string | null;
      readonly level: SoundLevel;
    }
  | { readonly kind: "pause"; readonly ms: number }
  | { readonly kind: "wait" }
  | { readonly kind: "point_rejected" }
  | { readonly kind: "end" }
  | { readonly kind: "bad_tag" };

export interface PcmChunk {
  readonly generationId: number;
  readonly sentenceId: number;
  readonly sampleRate: number;
  readonly eos: boolean;
  readonly samples: Int16Array;
}

export type WorkletInboundMsg =
  | {
      readonly type: "push";
      readonly generationId: number;
      readonly sentenceId: number;
      readonly sampleRate: number;
      readonly eos: boolean;
      readonly pcm: ArrayBuffer;
    }
  | {
      readonly type: "cancel";
      readonly generationId: number;
    };

export type WorkletOutboundMsg =
  | {
      readonly type: "overflow";
      readonly generationId: number;
      readonly sentenceId: number;
      readonly requiredFrames: number;
      readonly availableFrames: number;
    }
  | {
      readonly type: "sentence-start" | "sentence-end";
      readonly generationId: number;
      readonly sentenceId: number;
    }
  | {
      readonly type: "speech-active";
      readonly active: boolean;
      readonly rms: number;
    }
  | {
      readonly type: "underrun";
      readonly generationId: number;
    };

export interface CueOptions {
  readonly pan?: number;
  readonly loop?: boolean;
}

export interface PlaybackHandle {
  readonly stop: () => void;
  readonly finished: Promise<void>;
}

export interface PlayerCallbacks {
  onMood?: (mood: MoodName) => void;
  onState?: (state: RobotState) => void;
  onVoiceLevel?: (rms: number) => void;
  onPoint?: (
    point: Point | null,
    step?: Step,
  ) => Promise<{ panX: number; valid: boolean }>;
  onShow?: (text: string | null) => void;
  onRefusal?: (key: "robot.cant_find") => void;
  onBadTag?: () => void;
  onFallbackStateChange?: (fallbackNeeded: boolean) => void;
}
