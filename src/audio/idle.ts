// Audio clock policy, independent of the DOM and the worklet for Node tests.
export const AUDIO_IDLE_MS = 3000;

export interface PlaybackLease {
  readonly ready: Promise<void>;
  release(): void;
}

type AudioClock = Pick<AudioContext, "state" | "resume" | "suspend">;

export class AudioIdleController {
  private readonly ctx: AudioClock;
  private idle = true;
  private inactive = false;
  private active = 0;
  private disposed = false;
  private timer: ReturnType<typeof setTimeout> | undefined;
  private transition: Promise<void> = Promise.resolve();

  constructor(ctx: AudioClock) {
    this.ctx = ctx;
    this.schedule();
  }

  /** Intentional suspension must never be undone by the gesture/statechange handler. */
  get wantsRunning(): boolean {
    return !this.disposed && this.active > 0;
  }

  setIdle(idle: boolean): void {
    if (this.idle === idle) return;
    this.idle = idle;
    this.schedule();
  }

  setInactive(inactive: boolean): void {
    if (this.inactive === inactive) return;
    this.inactive = inactive;
    this.schedule();
  }

  begin(): PlaybackLease {
    this.active++;
    clearTimeout(this.timer);
    let released = false;
    const ready = this.enqueue(async () => {
      if (this.disposed) throw new Error("Audio engine disposed");
      if (this.ctx.state !== "running") await this.ctx.resume();
      if (this.ctx.state !== "running") throw new Error("Audio resume blocked");
    });
    return {
      ready,
      release: () => {
        if (released) return;
        released = true;
        this.active--;
        this.schedule();
      },
    };
  }

  dispose(): void {
    this.disposed = true;
    clearTimeout(this.timer);
  }

  private enqueue(action: () => Promise<void>): Promise<void> {
    const next = this.transition.then(action);
    this.transition = next.catch(() => {});
    return next;
  }

  private schedule(): void {
    clearTimeout(this.timer);
    if (this.disposed || this.active > 0 || (!this.idle && !this.inactive)) return;
    // Keep playing answers/cues audible when hidden; stop the clock as soon as they end.
    const suspend = (): void => {
      void this.enqueue(async () => {
        if (this.disposed || this.active > 0 || (!this.idle && !this.inactive)) return;
        if (this.ctx.state === "running") await this.ctx.suspend();
      }).catch(() => {});
    };
    if (this.inactive) suspend();
    else this.timer = setTimeout(suspend, AUDIO_IDLE_MS);
  }
}
