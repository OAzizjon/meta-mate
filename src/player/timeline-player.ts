// Plays an answer's MateScript timeline (docs/matescript.md, "Timeline") on Mate and the audio
// engine. Items run strictly in order: a sentence, a solo sound, a pause, a wait and the "can't
// find" line are barriers; bound actions start together with their sentence. Speech PCM reaches
// the engine separately (same generation and sentence id): the player only waits for the
// engine's end of that sentence. Spoken and shown text never goes to the log.

import type { AudioEngine } from "../audio/index.ts";
import { bubbleReadMs } from "../bubble-timing.ts";
import type { LocaleCode, TeacherLineKey } from "../i18n/index.ts";
import type { Bound, Point, Say, TimelineItem } from "../ipc/timeline.ts";
import type { CssRect, RobotController } from "../robot/index.ts";
import { isRobotMood } from "../robot/types.ts";

/** The `<pause>` range of `crates/mm-core/src/matescript/tags.rs` (Rust clamps already). */
const PAUSE_MIN_MS = 100;
const PAUSE_MAX_MS = 3000;

export type PlayerRobot = Pick<RobotController, "setState" | "setMood" | "aimAt">;
export type PlayerAudio = Pick<AudioEngine, "cancel" | "waitForSentenceEnd" | "playSfx" | "setAmbience">;

export interface TimelinePlayerOptions {
  /** Consumes Rust's stored target once; returns a localized status bubble. */
  readonly openTarget?: (generation: number) => Promise<string | null>;
  readonly robot: PlayerRobot;
  readonly audio: PlayerAudio;
  /** `setLocale` from src/i18n; called only for an English or Russian answer. */
  readonly setLocale: (locale: LocaleCode) => unknown;
  /**
   * Shows `point` of answer `generation` (with its guide step, if any) and returns it in this
   * window's CSS pixels, or null when it can't be shown (then no beam). Rust resolves the
   * point against its own look at the screen (src-tauri/src/pointing.rs).
   */
  readonly resolveTarget: (
    point: Point,
    generation: number,
    step: { readonly number: number; readonly total: number } | null,
  ) => CssRect | null | Promise<CssRect | null>;
  /** Shows a `<show>` bubble or silent sentence; null hides it. */
  readonly showBubble: (text: string | null) => void;
  /** A teacher line for the caller to speak; the timeline goes on once its promise settles. */
  readonly onLine: (line: TeacherLineKey) => void | Promise<void>;
  /** `End` was played. A cancelled generation never reports it. */
  readonly onDone: (generation: number) => void;
  /** The timeline stopped at `Wait`; `resolveWait()` releases it. */
  readonly onWaitReached?: (generation: number) => void;
  /** Resolves after `ms`, or early on abort (tests pass a fake). */
  readonly sleep?: (ms: number, signal: AbortSignal) => Promise<void>;
}

interface Run {
  readonly generation: number;
  readonly queue: TimelineItem[];
  readonly abort: AbortController;
  /** `End` is queued: later items of this generation are dropped. */
  closed: boolean;
  pumping: boolean;
  /** What a cancel has to take down. */
  aimed: boolean;
  shown: boolean;
  fallbackShown: boolean;
  releaseWait: (() => void) | null;
  announced: boolean;
  opened: boolean;
}

export class TimelinePlayer {
  private readonly options: TimelinePlayerOptions;
  private readonly sleep: (ms: number, signal: AbortSignal) => Promise<void>;
  private run: Run | null = null;
  private canceledThrough = -1;

  constructor(options: TimelinePlayerOptions) {
    this.options = options;
    this.sleep = options.sleep ?? sleep;
  }

  /**
   * Queues the next item of `generation` (items arrive in timeline order). A newer generation
   * cancels the current one first; items of an older or cancelled generation are dropped.
   */
  push(generation: number, item: TimelineItem): void {
    if (!Number.isSafeInteger(generation) || generation <= this.canceledThrough) return;
    let run = this.run;
    if (run && generation < run.generation) return;
    if (!run || generation > run.generation) {
      if (run) this.cancel(run.generation);
      run = {
        generation,
        queue: [],
        abort: new AbortController(),
        closed: false,
        pumping: false,
        aimed: false,
        shown: false,
        fallbackShown: false,
        releaseWait: null,
        announced: false,
        opened: false,
      };
      this.run = run;
    }
    if (run.closed) return;
    if (item.kind === "end") run.closed = true;
    run.queue.push(item);
    if (!run.pumping) void this.pump(run);
  }

  /**
   * Barge-in: stops every generation up to and including `generation`, like
   * `AudioEngine.cancel` (which it calls). Pending waits are abandoned and this answer's beam
   * and bubble are cleared; the robot's state is left to the caller.
   */
  cancel(generation: number): void {
    if (!Number.isSafeInteger(generation)) return;
    this.canceledThrough = Math.max(this.canceledThrough, generation);
    this.options.audio.cancel(generation);
    const run = this.run;
    if (!run || run.generation > generation) return;
    this.run = null;
    run.queue.length = 0;
    run.releaseWait = null;
    run.abort.abort();
    if (run.aimed) this.options.robot.aimAt(null);
    if (run.shown) this.options.showBubble(null);
  }

  /** Releases the current `Wait` (click or "next" confirmed). False when nothing waits. */
  resolveWait(): boolean {
    const run = this.run;
    const release = run?.releaseWait;
    if (!run || !release) return false;
    run.releaseWait = null;
    release();
    return true;
  }

  private async pump(run: Run): Promise<void> {
    run.pumping = true;
    while (!run.abort.signal.aborted) {
      const item = run.queue.shift();
      if (!item) break;
      try {
        await this.play(run, item);
      } catch (error) {
        report(item.kind, error);
      }
    }
    run.pumping = false;
  }

  private async play(run: Run, item: TimelineItem): Promise<void> {
    const { robot, audio } = this.options;
    const { signal } = run.abort;
    switch (item.kind) {
      case "open": {
        // Cancelled or interrupted after the announcement: nothing opens (Command Code #1449).
        if (signal.aborted || !run.announced || run.opened) return;
        run.opened = true;
        const text = await this.options.openTarget?.(run.generation);
        if (!signal.aborted && text) {
          this.options.showBubble(text);
          run.shown = true;
          run.fallbackShown = false;
          // Leave the line up long enough to read before the answer ends (Gemini #1462).
          await untilAborted(signal, this.sleep(bubbleReadMs(text), signal));
        }
        return;
      }
      case "lang": {
        const locale = localeOf(item.value);
        if (locale) this.options.setLocale(locale);
        return;
      }
      case "say":
        return this.say(run, item.value);
      case "bound":
        // An action with no sentence after it (answer end): it plays alone, no speech wait.
        if ((await this.apply(run, item.value)) && !signal.aborted) robot.setState("pointing");
        return;
      case "sfx":
        await untilAborted(signal, audio.playSfx(item.value, run.generation).finished);
        return;
      case "amb":
        audio.setAmbience(item.value.name, item.value.level);
        return;
      case "pause":
        await untilAborted(signal, this.sleep(clampPause(item.value), signal));
        return;
      case "wait": {
        robot.setState("waiting_for_click");
        const released = new Promise<void>((resolve) => {
          run.releaseWait = resolve;
        });
        this.options.onWaitReached?.(run.generation);
        await untilAborted(signal, released);
        return;
      }
      case "point_rejected":
        // "I can't find it" beats pointing at a guess (invariant 5); the caller speaks the line.
        robot.aimAt(null);
        run.aimed = false;
        robot.setState("cant_find");
        await untilAborted(signal, Promise.resolve(this.options.onLine("CantFind")));
        return;
      case "end":
        this.options.onDone(run.generation);
        return;
      case "bad_tag":
        return;
    }
  }

  private async say(run: Run, say: Say): Promise<void> {
    if (run.fallbackShown) {
      this.options.showBubble(null);
      run.shown = false;
      run.fallbackShown = false;
    }
    let pointing = false;
    for (const bound of say.bound) {
      // A failed action must not cost the sentence its speech wait.
      try {
        if (await this.apply(run, bound)) pointing = true;
      } catch (error) {
        report(bound.kind, error);
      }
      if (run.abort.signal.aborted) return;
    }
    this.options.robot.setState(pointing ? "pointing" : "speaking");
    run.announced = true;
    let hadSamples = true;
    await untilAborted(
      run.abort.signal,
      this.options.audio.waitForSentenceEnd(run.generation, say.id).then(
        (played) => {
          hadSamples = played;
        },
        (error: unknown) => {
          hadSamples = false;
          report("say", error);
        },
      ),
    );
    if (run.abort.signal.aborted || hadSamples || say.id === 0) return;
    this.options.showBubble(say.text);
    run.shown = true;
    run.fallbackShown = true;
    await untilAborted(run.abort.signal, this.sleep(bubbleReadMs(say.text), run.abort.signal));
  }

  /** Starts one bound action; true when the beam now shines on a target. */
  private async apply(run: Run, bound: Bound): Promise<boolean> {
    switch (bound.kind) {
      case "sfx":
        this.options.audio.playSfx(bound.value, run.generation);
        return false;
      case "mood":
        if (isRobotMood(bound.value)) this.options.robot.setMood(bound.value);
        return false;
      case "point":
        return this.aim(run, bound.value, null);
      case "step":
        return this.aim(run, bound.value.target, {
          number: bound.value.number,
          total: bound.value.total,
        });
      case "show":
        this.options.showBubble(bound.value);
        run.shown = true;
        run.fallbackShown = false;
        return false;
    }
  }

  private async aim(
    run: Run,
    point: Point,
    step: { readonly number: number; readonly total: number } | null,
  ): Promise<boolean> {
    const rect = await this.options.resolveTarget(point, run.generation, step);
    if (run.abort.signal.aborted) {
      if (rect !== null) this.options.robot.aimAt(null);
      return false;
    }
    this.options.robot.aimAt(rect);
    run.aimed = rect !== null;
    return rect !== null;
  }
}

/** The UI locale for an answer's `<lang>` (ISO 639-1 or 639-3); null keeps the current one. */
function localeOf(code: string): LocaleCode | null {
  switch (code) {
    case "en":
    case "eng":
      return "en";
    case "ru":
    case "rus":
      return "ru";
    default:
      return null;
  }
}

function clampPause(ms: number): number {
  return Math.min(PAUSE_MAX_MS, Math.max(PAUSE_MIN_MS, ms));
}

/** True once `promise` settles (fulfilled or rejected), false if the run is aborted first. */
function untilAborted(signal: AbortSignal, promise: Promise<unknown>): Promise<boolean> {
  if (signal.aborted) return Promise.resolve(false);
  return new Promise<boolean>((resolve) => {
    const onAbort = (): void => resolve(false);
    const onSettled = (): void => {
      signal.removeEventListener("abort", onAbort);
      resolve(true);
    };
    signal.addEventListener("abort", onAbort, { once: true });
    promise.then(onSettled, onSettled);
  });
}

function sleep(ms: number, signal: AbortSignal): Promise<void> {
  return new Promise<void>((resolve) => {
    if (signal.aborted) {
      resolve();
      return;
    }
    const done = (): void => {
      clearTimeout(timer);
      signal.removeEventListener("abort", done);
      resolve();
    };
    const timer = setTimeout(done, ms);
    signal.addEventListener("abort", done, { once: true });
  });
}

/** Names the item kind only: its text stays out of the log. */
function report(kind: string, error: unknown): void {
  console.error(`[player] ${kind} failed`, error);
}
