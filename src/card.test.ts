// Run with `npm test` (Node's own test runner; Node 24 strips the types).
// Unit tests for the lesson card parser and model (src/card.ts, WP62).

import assert from "node:assert/strict";
import { test } from "node:test";
import { inspect } from "node:util";

import {
  KEEPSAKE_HEIGHT,
  KEEPSAKE_WIDTH,
  LESSON_CARD_EVENT,
  MAX_CAPTIONED_PNG_BYTES,
  MAX_KEEPSAKE_STEPS,
  PLACEHOLDER_MATE_SVG,
  computeKeepsakeLayout,
  containImageRect,
  creditsKey,
  decodedBase64ByteLength,
  fitTextBlock,
  formatKeepsakeDate,
  getPlaceholderCard,
  mountLessonCard,
  parseLessonCard,
  prepareCaptionedSavePayload,
  resolveCardImageUrl,
  sendCaptionedCardOnce,
  splitPngDataUrl,
  stripLeadingStepNumber,
  wrapTextLines,
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
  wordsBy: "glimmer",
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
    // A payload from before WP71 has no wordsBy: the lesson's own words.
    wordsBy: "lesson",
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
    wordsBy: "glimmer",
  };
  const parsed = parseLessonCard(raw);
  assert.deepEqual(parsed, raw);
});

test("parseLessonCard keeps wordsBy glimmer or lesson", () => {
  for (const wordsBy of ["glimmer", "lesson"]) {
    const parsed = parseLessonCard({ ...VALID_CARD, wordsBy });
    assert.equal(parsed?.wordsBy, wordsBy);
  }
});

test("parseLessonCard reads a missing or unknown wordsBy as lesson, never as a bad card", () => {
  const { wordsBy: _, ...withoutWordsBy } = VALID_CARD;
  assert.equal(parseLessonCard(withoutWordsBy)?.wordsBy, "lesson");

  const unknown = [
    undefined,
    null,
    "",
    "Glimmer",
    " glimmer ",
    "muse",
    "image",
    1,
    true,
    {},
    ["glimmer"],
  ];
  for (const wordsBy of unknown) {
    const parsed = parseLessonCard({ ...VALID_CARD, wordsBy });
    assert.ok(parsed !== null, inspect(wordsBy));
    assert.equal(parsed.wordsBy, "lesson", inspect(wordsBy));
  }
});

test("creditsKey credits Muse Glimmer only for its own words", () => {
  assert.equal(creditsKey("glimmer"), "card.credits_glimmer");
  assert.equal(creditsKey("lesson"), "card.credits_image");
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

test("parseLessonCard accepts missing images and rejects malformed imagePath", () => {
  for (const imagePath of [undefined, null, "", "   ", "\t\n"]) {
    assert.deepEqual(parseLessonCard({ ...VALID_CARD, imagePath }), {
      ...VALID_CARD, imagePath: "",
    });
  }
  const badPaths = [
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
    {
      title: "Title",
      steps: ["Step 1"],
      imagePath: "path.png",
      get wordsBy() {
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
  assert.equal(placeholder.wordsBy, "lesson");
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

test("PLACEHOLDER_MATE_SVG contains valid SVG markup", () => {
  assert.ok(PLACEHOLDER_MATE_SVG.startsWith("<svg"));
  assert.ok(PLACEHOLDER_MATE_SVG.endsWith("</svg>"));
  assert.ok(PLACEHOLDER_MATE_SVG.includes("bulb-glow"));
  assert.ok(PLACEHOLDER_MATE_SVG.includes("placeholder-bulb"));
});

test("text-only and failed images keep steps and Print usable", async () => {
  class Element {
    id = "";
    className = "";
    textContent = "";
    innerHTML = "";
    hidden = false;
    style = { display: "" };
    children: Element[] = [];
    attributes = new Map<string, string>();
    listeners = new Map<string, () => void>();
    append(...nodes: Element[]): void { this.children.push(...nodes); }
    replaceChildren(...nodes: Element[]): void { this.children = nodes; }
    setAttribute(key: string, value: string): void { this.attributes.set(key, value); }
    getAttribute(key: string): string | null { return this.attributes.get(key) ?? null; }
    removeAttribute(key: string): void { this.attributes.delete(key); }
    set src(value: string) { this.setAttribute("src", value); }
    addEventListener(event: string, listener: () => void): void { this.listeners.set(event, listener); }
  }
  const globals = globalThis as { document?: unknown; window?: unknown };
  const oldDocument = globals.document;
  const oldWindow = globals.window;
  const nodes: Element[] = [];
  let prints = 0;
  globals.document = {
    documentElement: { lang: "en" },
    createElement: () => { const node = new Element(); nodes.push(node); return node; },
  };
  globals.window = { print: () => { prints += 1; } };
  const root = new Element();
  try {
    const controller = mountLessonCard(root as unknown as HTMLElement);
    const byId = (id: string): Element => {
      const node = nodes.find((node) => node.id === id);
      assert.ok(node, id);
      return node;
    };
    for (const imagePath of [undefined, null, ""]) {
      assert.equal(controller.handleRaw({ ...VALID_CARD, imagePath }), true);
      assert.equal(byId("card-image").getAttribute("src"), null);
      assert.equal(byId("card-image").style.display, "none");
      assert.equal(byId("card-placeholder-art").style.display, "flex");
      assert.equal(byId("card-steps").children.length, VALID_CARD.steps.length);
      byId("card-print-btn").listeners.get("click")?.();
    }
    await controller.setCard({ ...VALID_CARD, imagePath: "data:image/png;base64,bad" });
    byId("card-image").listeners.get("error")?.();
    assert.equal(byId("card-image").style.display, "none");
    assert.equal(byId("card-placeholder-art").style.display, "flex");
    assert.equal(byId("card-credits").hidden, true);
    assert.equal(byId("card-steps").children[0].children[1].textContent, VALID_CARD.steps[0]);
    byId("card-print-btn").listeners.get("click")?.();
    assert.equal(prints, 4);
    controller.destroy();
  } finally {
    globals.document = oldDocument;
    globals.window = oldWindow;
  }
});

test("parseLessonCard treats id as optional, trims it, and rejects malformed id", () => {
  const withId = parseLessonCard({ ...VALID_CARD, id: "  card-2026-11-01  " });
  assert.equal(withId?.id, "card-2026-11-01");

  for (const emptyId of [undefined, null, "", "   ", "\t\n"]) {
    const parsed = parseLessonCard({ ...VALID_CARD, id: emptyId });
    assert.ok(parsed !== null, inspect(emptyId));
    assert.equal("id" in parsed, false, inspect(emptyId));
  }

  for (const badId of [42, false, {}, ["id"]]) {
    assert.equal(parseLessonCard({ ...VALID_CARD, id: badId }), null, inspect(badId));
  }

  assert.equal(
    parseLessonCard({
      ...VALID_CARD,
      get id(): string {
        throw new Error("boom");
      },
    }),
    null,
  );
});

test("stripLeadingStepNumber removes matching step prefixes without touching content numbers", () => {
  assert.equal(stripLeadingStepNumber("1. Open Settings", 1), "Open Settings");
  assert.equal(stripLeadingStepNumber("  2) Выберите «Размер текста» ", 2), "Выберите «Размер текста»");
  assert.equal(stripLeadingStepNumber("100% zoom is the default", 1), "100% zoom is the default");
  assert.equal(stripLeadingStepNumber("3. Open Settings", 1), "3. Open Settings");
  assert.equal(stripLeadingStepNumber("1.", 1), "1.");
});

test("wrapTextLines wraps Latin and Cyrillic text and splits overlong tokens", () => {
  const monoMeasure = (sample: string): number => Array.from(sample).length * 10;

  assert.deepEqual(wrapTextLines("   ", 100, monoMeasure), []);
  assert.deepEqual(wrapTextLines("Hello", 0, monoMeasure), []);

  assert.deepEqual(
    wrapTextLines("Open   Windows   Settings now", 140, monoMeasure),
    ["Open Windows", "Settings now"],
  );

  assert.deepEqual(
    wrapTextLines("Откройте параметры экрана сегодня", 190, monoMeasure),
    ["Откройте параметры", "экрана сегодня"],
  );

  const longPath = "C:\\Users\\Senior\\Pictures\\MetaMate\\keepsake.png";
  const wrappedPath = wrapTextLines(longPath, 120, monoMeasure);
  assert.ok(wrappedPath.length >= 4);
  for (const line of wrappedPath) {
    assert.ok(monoMeasure(line) <= 120, line);
  }
  assert.equal(wrappedPath.join(""), longPath);
});

test("fitTextBlock shrinks font down to minimum before truncating with an ellipsis", () => {
  const scaledMeasure = (sample: string, fontSize: number): number =>
    Array.from(sample).length * fontSize * 0.6;

  // Short text stays at maxFontSize.
  const comfortable = fitTextBlock(
    "Open Settings",
    { maxWidth: 400, maxHeight: 90, maxFontSize: 30, minFontSize: 20, fontStep: 2 },
    scaledMeasure,
  );
  assert.equal(comfortable.fontSize, 30);
  assert.equal(comfortable.truncated, false);
  assert.deepEqual(comfortable.lines, ["Open Settings"]);

  // Longer text shrinks font size rather than truncating early.
  const shrunk = fitTextBlock(
    "Выберите раздел «Специальные возможности» и передвиньте ползунок масштаба",
    { maxWidth: 420, maxHeight: 95, maxFontSize: 30, minFontSize: 20, fontStep: 3 },
    scaledMeasure,
  );
  assert.ok(shrunk.fontSize < 30 && shrunk.fontSize >= 20);
  assert.equal(shrunk.truncated, false);
  assert.ok(shrunk.lines.length * shrunk.lineHeight <= 95);

  // Very long text hits minFontSize first, then truncates with an ellipsis inside maxWidth.
  const overflowing = fitTextBlock(
    "Open Windows Settings, choose Accessibility and Text size, drag the slider slowly to the right until every word is comfortable to read, and click Apply to keep the change",
    { maxWidth: 300, maxHeight: 64, maxFontSize: 30, minFontSize: 20, fontStep: 3 },
    scaledMeasure,
  );
  assert.equal(overflowing.fontSize, 20);
  assert.equal(overflowing.truncated, true);
  assert.equal(overflowing.lines.length, 2);
  assert.ok(overflowing.lines[overflowing.lines.length - 1].endsWith("…"));
  for (const line of overflowing.lines) {
    assert.ok(scaledMeasure(line, overflowing.fontSize) <= 300, line);
  }
});

test("computeKeepsakeLayout fits 1..6 steps inside 1200x1600 without overlap and clamps >6", () => {
  for (const requested of [1, 2, 3, 4, 5, 6, 8]) {
    const layout = computeKeepsakeLayout(requested);
    assert.equal(layout.width, KEEPSAKE_WIDTH);
    assert.equal(layout.height, KEEPSAKE_HEIGHT);
    assert.equal(layout.steps.length, Math.min(requested, MAX_KEEPSAKE_STEPS));

    assert.ok(layout.picturePanel.y >= layout.pageFrame.y);
    assert.ok(
      layout.picturePanel.y + layout.picturePanel.height < layout.titleBanner.y,
      `panel overlaps title for ${requested}`,
    );
    assert.ok(
      layout.titleBanner.y + layout.titleBanner.height < layout.steps[0].bubble.y,
      `title overlaps first step for ${requested}`,
    );

    for (let i = 0; i < layout.steps.length; i += 1) {
      const slot = layout.steps[i];
      assert.ok(slot.textBox.width > 0 && slot.textBox.height > 0);
      assert.ok(slot.badgeCenter.x - slot.badgeRadius >= layout.pageFrame.x);
      assert.ok(slot.bubble.x + slot.bubble.width <= layout.pageFrame.x + layout.pageFrame.width);
      if (i + 1 < layout.steps.length) {
        assert.ok(slot.bubble.y + slot.bubble.height < layout.steps[i + 1].bubble.y);
      }
    }

    const lastBubble = layout.steps[layout.steps.length - 1].bubble;
    assert.ok(lastBubble.y + lastBubble.height < layout.footer.y);
    assert.ok(
      layout.footer.y + layout.footer.height <= layout.pageFrame.y + layout.pageFrame.height,
    );
  }

  assert.equal(computeKeepsakeLayout(0).steps.length, 0);
});

test("computeKeepsakeLayout reserves the family note's band only when there is a note", () => {
  for (const requested of [1, 2, 3, 4, 5, 6, 8]) {
    // No note: no band and not a pixel taken from the steps.
    const plain = computeKeepsakeLayout(requested);
    assert.equal(plain.note, null);
    assert.deepEqual(computeKeepsakeLayout(requested, false), plain);

    const noted = computeKeepsakeLayout(requested, true);
    const note = noted.note;
    assert.ok(note, `no note band for ${requested}`);
    assert.deepEqual(noted.picturePanel, plain.picturePanel);
    assert.deepEqual(noted.titleBanner, plain.titleBanner);
    assert.deepEqual(noted.footer, plain.footer);
    assert.equal(noted.steps.length, plain.steps.length);
    const lastBubble = noted.steps[noted.steps.length - 1].bubble;
    assert.ok(lastBubble.y + lastBubble.height < note.y, `step overlaps note for ${requested}`);
    assert.ok(note.y + note.height < noted.footer.y, `note overlaps footer for ${requested}`);
    assert.ok(note.x >= noted.pageFrame.x && note.x + note.width <= noted.pageFrame.x + noted.pageFrame.width);
    for (let i = 0; i < noted.steps.length; i += 1) {
      assert.ok(noted.steps[i].bubble.height <= plain.steps[i].bubble.height);
      assert.ok(noted.steps[i].textBox.height > 0);
    }
  }
});

test("parseLessonCard keeps a family note on one line and drops an empty or malformed one", () => {
  assert.equal(
    parseLessonCard({ ...VALID_CARD, familyNote: "  We are\n proud of you!  " })?.familyNote,
    "We are proud of you!",
  );
  for (const familyNote of [undefined, null, "", "   ", 42, {}, ["Hi"]]) {
    const parsed = parseLessonCard({ ...VALID_CARD, familyNote });
    assert.ok(parsed !== null, inspect(familyNote));
    assert.equal("familyNote" in parsed, false, inspect(familyNote));
    assert.deepEqual(parsed, VALID_CARD);
  }
});

test("containImageRect preserves aspect ratio inside the panel without cropping", () => {
  const box = { x: 78, y: 78, width: 1044, height: 514 };
  const square = containImageRect(1024, 1024, box);
  assert.equal(square.width, 514);
  assert.equal(square.height, 514);
  assert.equal(square.x, 78 + Math.round((1044 - 514) / 2));
  assert.equal(square.y, 78);

  const wide = containImageRect(2088, 514, box);
  assert.equal(wide.width, 1044);
  assert.equal(wide.height, 257);

  assert.deepEqual(containImageRect(0, 500, box), { x: 78, y: 78, width: 0, height: 0 });
});

test("splitPngDataUrl and prepareCaptionedSavePayload enforce base64 format and 8 MiB cap", () => {
  const sampleBase64 = "iVBORw0KGgoAAAANSUhEUg==";
  const sampleUrl = `data:image/png;base64,${sampleBase64}`;
  assert.equal(splitPngDataUrl(sampleUrl), sampleBase64);
  assert.equal(splitPngDataUrl(`DATA:image/png;charset=utf-8;base64, ${sampleBase64} `), sampleBase64);
  assert.equal(splitPngDataUrl("data:image/jpeg;base64,/9j/4AAQ"), null);
  assert.equal(splitPngDataUrl("data:image/png;base64,"), null);
  assert.equal(splitPngDataUrl("data:image/png;base64,not-base64!"), null);
  assert.equal(decodedBase64ByteLength("AQ=="), 1);
  assert.equal(decodedBase64ByteLength("AQI="), 2);
  assert.equal(decodedBase64ByteLength("AQID"), 3);

  assert.deepEqual(prepareCaptionedSavePayload(undefined, sampleUrl), {
    ok: false,
    reason: "missing-id",
  });
  assert.deepEqual(prepareCaptionedSavePayload("   ", sampleUrl), {
    ok: false,
    reason: "missing-id",
  });
  assert.deepEqual(prepareCaptionedSavePayload("card-1", "not-a-data-url"), {
    ok: false,
    reason: "invalid-data-url",
  });

  // Exact 8 MiB (8,388,608 bytes = 2,796,202 * 3 + 2 bytes) passes; +1 byte skips.
  const exact8MiBBase64 = `${"AAAA".repeat(2_796_202)}AAA=`;
  assert.equal(decodedBase64ByteLength(exact8MiBBase64), MAX_CAPTIONED_PNG_BYTES);
  const okAtLimit = prepareCaptionedSavePayload(
    " card-limit ",
    `data:image/png;base64,${exact8MiBBase64}`,
  );
  assert.equal(okAtLimit.ok, true);
  if (okAtLimit.ok) {
    assert.equal(okAtLimit.id, "card-limit");
    assert.equal(okAtLimit.byteLength, MAX_CAPTIONED_PNG_BYTES);
  }

  const over8MiBBase64 = "AAAA".repeat(2_796_203);
  assert.equal(decodedBase64ByteLength(over8MiBBase64), MAX_CAPTIONED_PNG_BYTES + 1);
  assert.deepEqual(
    prepareCaptionedSavePayload("card-over", `data:image/png;base64,${over8MiBBase64}`),
    { ok: false, reason: "too-large" },
  );
});

test("sendCaptionedCardOnce saves once per card id, strips prefix, and warns on failure", async () => {
  const calls: Array<{ id: string; pngBase64: string }> = [];
  const warnings: unknown[][] = [];
  const origWarn = console.warn;
  console.warn = (...args: unknown[]) => {
    warnings.push(args);
  };

  try {
    const savedIds = new Set<string>();
    const url = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUg==";
    const saver = async (args: { readonly id: string; readonly pngBase64: string }) => {
      calls.push({ id: args.id, pngBase64: args.pngBase64 });
    };

    // Absent id is skipped silently.
    assert.equal(await sendCaptionedCardOnce(undefined, url, savedIds, saver), false);
    assert.equal(calls.length, 0);
    assert.equal(warnings.length, 0);

    // First send for "card-a" succeeds without data: prefix; duplicate is skipped.
    assert.equal(await sendCaptionedCardOnce("card-a", url, savedIds, saver), true);
    assert.equal(await sendCaptionedCardOnce("card-a", url, savedIds, saver), false);
    assert.deepEqual(calls, [{ id: "card-a", pngBase64: "iVBORw0KGgoAAAANSUhEUg==" }]);

    // Failures log console.warn only and do not throw or retry.
    const failingSaver = async (): Promise<void> => {
      throw new Error("disk full");
    };
    assert.equal(await sendCaptionedCardOnce("card-b", url, savedIds, failingSaver), false);
    assert.equal(warnings.length, 1);
    assert.equal(await sendCaptionedCardOnce("card-b", url, savedIds, saver), false);
  } finally {
    console.warn = origWarn;
  }
});

test("formatKeepsakeDate formats dates in English and Russian", () => {
  const sampleDate = new Date("2026-11-01T12:00:00Z");
  const en = formatKeepsakeDate(sampleDate, "en");
  const ru = formatKeepsakeDate(sampleDate, "ru");
  assert.ok(en.includes("2026"), en);
  assert.ok(ru.includes("2026"), ru);
});
