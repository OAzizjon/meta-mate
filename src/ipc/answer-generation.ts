import type { AnswerMsg } from "./answer-stream.ts";

/** Recordings can interrupt a lesson without replacing its answer generation. */
export class AnswerGenerations {
  answer = -1;
  newest = -1;
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
    } else if (msg.type === "level" || msg.type === "heard") {
      if (msg.generation !== this.recording) return false;
    } else {
      this.answer = msg.generation;
    }
    this.newest = Math.max(this.newest, msg.generation);
    return true;
  }
}
