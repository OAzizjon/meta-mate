// Run with `npm test` (Node's own test runner; Node 24 strips the types).
// Unit tests for the lesson card parser and model (src/card.ts, WP62).

import assert from "node:assert/strict";
import { test } from "node:test";
import { inspect } from "node:util";

import {
  LESSON_CARD_EVENT,
  PLACEHOLDER_MATE_SVG,
  escapeHtml,
  getPlaceholderCard,
  parseLessonCard,
  resolveCardImageUrl,
} from "./card.ts";
import type { LessonCard } from "./card.ts";

const VALID_CARD: LessonCard = {
  title: "How to make the text bigger",
  steps: [
    "1. Open Settings",
    "2. Choose Accessibility and Text size",
    "3. Move the slider until comfortable",
  ],
  imagePath: "C:\\Users\\Senior\\Pictures\\MetaMate\\lesson-text-size.png",
};

test("LESSON_CARD_EVENT constant matches the Tauri contract", () => {
  assert.equal(LESSON_CARD_EVENT, "lesson-card");
});

test("parseLessonCard parses valid payload into a fresh copy with trimmed fields", () => {
  const raw = {
    title: "  How to make the text bigger  ",
    steps: [
      "  1. Open Settings  ",
      "2. Choose Accessibility and Text size",
      " 3. Move the slider until comfortable ",
    ],
    imagePath: " C:\\Users\\Senior\\Pictures\\MetaMate\\card.png ",
  };

  const parsed = parseLessonCard(raw);
  assert.ok(parsed !== null, "valid card should parse successfully");
  assert.deepEqual(parsed, {
    title: "How to make the text bigger",
    steps: [
      "1. Open Settings",
      "2. Choose Accessibility and Text size",
      "3. Move the slider until comfortable",
    ],
    imagePath: "C:\\Users\\Senior\\Pictures\\MetaMate\\card.png",
  });

  // Verify immutability: modifying the original steps array doesn't affect parsed card
  raw.steps.push("4. Extra step");
  assert.equal(parsed.steps.length, 3);
});

test("parseLessonCard accepts valid single-step card", () => {
  const raw = {
    title: "Print this page",
    steps: ["Press Ctrl and P together on your keyboard"],
    imagePath: "/pictures/metamate/print.png",
  };
  const parsed = parseLessonCard(raw);
  assert.deepEqual(parsed, raw);
});

test("parseLessonCard rejects non-objects and primitives", () => {
  const badInputs = [
    null,
    undefined,
    true,
    false,
    42,
    0,
    -1,
    Number.NaN,
    Infinity,
    "lesson-card",
    [],
    [VALID_CARD],
  ];

  for (const input of badInputs) {
    assert.equal(parseLessonCard(input), null, inspect(input));
  }
});

test("parseLessonCard rejects missing or malformed title", () => {
  const badTitles = [
    undefined,
    null,
    "",
    "   ",
    "\n\t",
    123,
    true,
    {},
    [],
    ["A title"],
  ];

  for (const title of badTitles) {
    const raw = {
      title,
      steps: ["Step 1"],
      imagePath: "C:\\path\\card.png",
    };
    assert.equal(parseLessonCard(raw), null, inspect(title));
  }
});

test("parseLessonCard rejects missing, non-array, empty, or invalid steps", () => {
  const badSteps = [
    undefined,
    null,
    "",
    123,
    true,
    {},
    [], // empty steps not allowed
    [""], // empty step string
    ["   "], // whitespace only step
    [123], // non-string item
    [null],
    [undefined],
    ["Valid step", ""], // partial invalid step
    ["Valid step", 42],
    ["Valid step", null],
  ];

  for (const steps of badSteps) {
    const raw = {
      title: "How to do something",
      steps,
      imagePath: "C:\\path\\card.png",
    };
    assert.equal(parseLessonCard(raw), null, inspect(steps));
  }
});

test("parseLessonCard rejects missing or malformed imagePath", () => {
  const badPaths = [
    undefined,
    null,
    "",
    "   ",
    "\t\n",
    123,
    false,
    {},
    [],
  ];

  for (const imagePath of badPaths) {
    const raw = {
      title: "How to do something",
      steps: ["Step 1"],
      imagePath,
    };
    assert.equal(parseLessonCard(raw), null, inspect(imagePath));
  }
});

test("parseLessonCard rejects hostile objects and throwing getters without throwing", () => {
  const boom = (): never => {
    throw new Error("boom");
  };

  const hostileList: unknown[] = [
    new Proxy({}, { ownKeys: boom }),
    new Proxy({ ...VALID_CARD }, { get: boom }),
    {
      get title() {
        return boom();
      },
      steps: ["Step 1"],
      imagePath: "path.png",
    },
    {
      title: "Title",
      get steps() {
        return boom();
      },
      imagePath: "path.png",
    },
    {
      title: "Title",
      steps: ["Step 1"],
      get imagePath() {
        return boom();
      },
    },
  ];

  for (const hostile of hostileList) {
    assert.equal(parseLessonCard(hostile), null);
  }
});

test("getPlaceholderCard returns valid LessonCard with localized strings", () => {
  const placeholder = getPlaceholderCard();
  assert.ok(typeof placeholder.title === "string" && placeholder.title.length > 0);
  assert.ok(Array.isArray(placeholder.steps) && placeholder.steps.length > 0);
  for (const step of placeholder.steps) {
    assert.ok(typeof step === "string" && step.length > 0);
  }
  assert.equal(typeof placeholder.imagePath, "string");
});

test("resolveCardImageUrl handles web, data, asset, and empty URLs correctly", async () => {
  assert.equal(await resolveCardImageUrl(""), "");
  assert.equal(await resolveCardImageUrl("   "), "");

  const webUrl = "https://example.com/pictures/card.png";
  assert.equal(await resolveCardImageUrl(webUrl), webUrl);

  const dataUrl = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUg==";
  assert.equal(await resolveCardImageUrl(dataUrl), dataUrl);

  const assetUrl = "asset://localhost/pictures/card.png";
  assert.equal(await resolveCardImageUrl(assetUrl), assetUrl);

  const slashPath = "/assets/art/mate.png";
  assert.equal(await resolveCardImageUrl(slashPath), slashPath);
});

test("escapeHtml escapes HTML metacharacters", () => {
  assert.equal(escapeHtml("Mate & Friends"), "Mate &amp; Friends");
  assert.equal(escapeHtml("<script>alert('xss')</script>"), "&lt;script&gt;alert(&#39;xss&#39;)&lt;/script&gt;");
  assert.equal(escapeHtml('Click "Save"'), "Click &quot;Save&quot;");
});

test("PLACEHOLDER_MATE_SVG contains valid SVG markup", () => {
  assert.ok(PLACEHOLDER_MATE_SVG.startsWith("<svg"));
  assert.ok(PLACEHOLDER_MATE_SVG.endsWith("</svg>"));
  assert.ok(PLACEHOLDER_MATE_SVG.includes("bulb-glow"));
  assert.ok(PLACEHOLDER_MATE_SVG.includes("placeholder-bulb"));
});
