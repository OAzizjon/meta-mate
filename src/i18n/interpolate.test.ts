// Run with `npm test` (Node's own test runner; Node 24 strips the types).
import assert from "node:assert/strict";
import { test } from "node:test";

import { interpolate } from "./interpolate.ts";

test("{name} defaults to the robot name passed in", () => {
  assert.equal(interpolate("Hello! I'm {name}.", "Bulb"), "Hello! I'm Bulb.");
  assert.equal(interpolate("I'm {name}.", "Bulb", { name: "Other" }), "I'm Other.");
});

test("step placeholders are filled and unknown ones stay visible", () => {
  assert.equal(
    interpolate("Step {n} of {m}: {instruction}", "Bulb", { n: 2, m: 3, instruction: "Click" }),
    "Step 2 of 3: Click",
  );
  assert.equal(interpolate("Step {n} of {m}", "Bulb", { n: 1 }), "Step 1 of {m}");
});

test("a value is inserted once, never re-expanded", () => {
  assert.equal(interpolate("{instruction}", "Bulb", { instruction: "{name}" }), "{name}");
});
