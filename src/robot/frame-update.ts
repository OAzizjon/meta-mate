// Keep the latest value and flush once per frame, without a continuously running loop.
export function createFrameUpdate(
  render: () => void,
  request: (callback: FrameRequestCallback) => number = (callback) => requestAnimationFrame(callback),
  cancel: (frame: number) => void = (frame) => cancelAnimationFrame(frame),
): { schedule(): void; cancel(): void } {
  let frame: number | undefined;
  return {
    schedule() {
      if (frame !== undefined) return;
      frame = request(() => {
        frame = undefined;
        render();
      });
    },
    cancel() {
      if (frame !== undefined) cancel(frame);
      frame = undefined;
    },
  };
}
