import type { AnswerMsg } from "./answer-stream.ts";

/** Recordings can interrupt a lesson without replacing its answer generation. */
export class AnswerGenerations {
  answer = -1;
  newest = -1;
  listening: number | null = null;
  private recording = -1;
  private cancelledThrough = -1;

  accept(msg: AnswerMsg): boolean {
    if (msg.type === "cancel") {
      this.cancelledThrough = Math.max(this.cancelledThrough, msg.generation);
      return msg.generation >= this.answer;
    }
    if (msg.generation <= this.cancelledThrough) return false;
    if (msg.generation < this.answer) return false;
    if (msg.type === "state") {
      // Input state does not retire the lesson that is waiting for these words.
      if (msg.generation < this.recording && msg.generation !== this.answer) return false;
      this.recording = Math.max(this.recording, msg.generation);
      if (msg.generation === this.recording) {
        this.listening = msg.state === "listening" ? msg.generation : null;
      }
    } else if (
      msg.type === "level" || msg.type === "heard" ||
      (msg.type === "line" && msg.key === "robot.click_stop")
    ) {
      if (msg.generation !== this.recording) return false;
    } else {
      this.answer = msg.generation;
    }
    this.newest = Math.max(this.newest, msg.generation);
    return true;
  }

  canIdle(generation: number): boolean {
    return generation >= this.newest;
  }

  get holdPresentation(): boolean {
    return this.listening !== null && this.listening > this.answer;
  }
}

/** Keep the latest visual change of each kind while the microphone owns the UI. */
export class HeldPresentation {
  private pending = new Map<string, () => void>();

  apply(key: string, held: boolean, update: () => void): void {
    if (held) this.pending.set(key, update);
    else update();
  }

  release(): void {
    const updates = [...this.pending.values()];
    this.pending.clear();
    for (const update of updates) update();
  }

  clear(): void {
    this.pending.clear();
  }
}
