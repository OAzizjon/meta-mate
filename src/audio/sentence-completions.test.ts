import assert from "node:assert/strict";
import test from "node:test";
import { SentenceCompletions } from "./sentence-completions.ts";

async function resolvesImmediately(promise: Promise<void>) {
  let resolved = false;
  void promise.then(() => { resolved = true; });
  await Promise.resolve();
  assert.equal(resolved, true);
}

test("completion before a wait is remembered, including repeated waits", async () => {
  const completions = new SentenceCompletions();
  completions.complete(1, 7);
  await resolvesImmediately(completions.wait(1, 7));
  await resolvesImmediately(completions.wait(1, 7));
});

test("completion releases all existing waiters and leaves other sentences pending", async () => {
  const completions = new SentenceCompletions();
  const first = completions.wait(1, 1);
  const second = completions.wait(1, 1);
  let otherResolved = false;
  void completions.wait(1, 2).then(() => { otherResolved = true; });
  completions.complete(1, 1);
  await resolvesImmediately(first);
  await resolvesImmediately(second);
  assert.equal(otherResolved, false);
  completions.dispose();
});

test("cancellation is remembered without resolving newer generations", async () => {
  const completions = new SentenceCompletions();
  const pending = completions.wait(1, 1);
  let newerResolved = false;
  void completions.wait(2, 1).then(() => { newerResolved = true; });
  completions.cancel(1);
  completions.cancel(0);
  await resolvesImmediately(pending);
  await resolvesImmediately(completions.wait(1, 99));
  await resolvesImmediately(completions.wait(0, 99));
  assert.equal(newerResolved, false);
  completions.dispose();
  await resolvesImmediately(completions.wait(3, 1));
});
