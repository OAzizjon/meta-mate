// Keep sentence completion durable when worklet events precede timeline waits.
export class SentenceCompletions {
  private readonly completed = new Set<string>();
  private readonly waiters = new Map<string, Set<() => void>>();
  private canceledThrough = -1;
  private disposed = false;

  wait(generationId: number, sentenceId: number): Promise<void> {
    const key = `${generationId}:${sentenceId}`;
    if (this.disposed || generationId <= this.canceledThrough || this.completed.has(key)) {
      return Promise.resolve();
    }
    return new Promise<void>((resolve) => {
      let set = this.waiters.get(key);
      if (!set) {
        set = new Set();
        this.waiters.set(key, set);
      }
      set.add(resolve);
    });
  }

  complete(generationId: number, sentenceId: number): void {
    const key = `${generationId}:${sentenceId}`;
    this.completed.add(key);
    this.resolve(key);
  }

  cancel(generationId: number): void {
    this.canceledThrough = Math.max(this.canceledThrough, generationId);
    for (const key of this.waiters.keys()) {
      if (Number(key.split(":", 1)[0]) <= this.canceledThrough) this.resolve(key);
    }
    for (const key of this.completed) {
      if (Number(key.split(":", 1)[0]) <= this.canceledThrough) this.completed.delete(key);
    }
  }

  dispose(): void {
    this.disposed = true;
    for (const key of this.waiters.keys()) this.resolve(key);
    this.completed.clear();
  }

  private resolve(key: string): void {
    const waiters = this.waiters.get(key);
    this.waiters.delete(key);
    if (waiters) for (const resolve of waiters) resolve();
  }
}
