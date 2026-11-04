// Pure layout, text fitting, and PNG payload helpers for the 1200x1600 lesson
// card keepsake canvas (src/card.ts, src/card-canvas.ts). No DOM dependencies.

export const KEEPSAKE_WIDTH = 1200;
export const KEEPSAKE_HEIGHT = 1600;
export const MAX_KEEPSAKE_STEPS = 6;
export const MAX_CAPTIONED_PNG_BYTES = 8 * 1024 * 1024;
export const CARD_SAVE_CAPTIONED_COMMAND = "card_save_captioned";

/** System font stacks only; both ship with Windows and cover Latin + Cyrillic. */
export const KEEPSAKE_FONT_SANS = '"Segoe UI", system-ui, -apple-system, sans-serif';
export const KEEPSAKE_FONT_COMIC = '"Segoe Print", "Segoe UI", system-ui, sans-serif';

export interface Rect {
  readonly x: number;
  readonly y: number;
  readonly width: number;
  readonly height: number;
}

export interface KeepsakeStepSlot {
  readonly bubble: Rect;
  readonly badgeCenter: { readonly x: number; readonly y: number };
  readonly badgeRadius: number;
  readonly textBox: Rect;
}

export interface KeepsakeLayout {
  readonly width: number;
  readonly height: number;
  readonly pageFrame: Rect;
  readonly picturePanel: Rect;
  readonly pictureInner: Rect;
  readonly titleBanner: Rect;
  readonly titleTextBox: Rect;
  readonly steps: readonly KeepsakeStepSlot[];
  /** The family's note between the steps and the footer; `null` without a note. */
  readonly note: Rect | null;
  readonly footer: Rect;
}

export type MeasureTextFn = (text: string) => number;
export type MeasureScaledTextFn = (text: string, fontSize: number) => number;

export interface FitTextOptions {
  readonly maxWidth: number;
  readonly maxHeight: number;
  readonly maxFontSize: number;
  readonly minFontSize: number;
  readonly fontStep?: number;
  readonly lineHeightRatio?: number;
  readonly maxLines?: number;
}

export interface FittedText {
  readonly lines: readonly string[];
  readonly fontSize: number;
  readonly lineHeight: number;
  readonly truncated: boolean;
}

export type CaptionedSaveDecision =
  | { readonly ok: true; readonly id: string; readonly pngBase64: string; readonly byteLength: number }
  | { readonly ok: false; readonly reason: "missing-id" | "invalid-data-url" | "too-large" };

const ELLIPSIS = "\u2026";
const PNG_DATA_URL_PREFIX = /^data:image\/png(?:;[^,]*)?;base64,/i;
const BASE64_BODY = /^[A-Za-z0-9+/]+={0,2}$/;

/**
 * Removes a leading `"1. "` or `"1) "` when it duplicates `stepNumber` on a
 * numbered comic step badge. Preserves numbers that are part of the sentence.
 */
export function stripLeadingStepNumber(step: string, stepNumber: number): string {
  const trimmed = step.trim();
  const match = /^(\d+)[.)]\s+(.*)$/su.exec(trimmed);
  if (match && Number(match[1]) === stepNumber) {
    const rest = match[2].trim();
    if (rest.length > 0) {
      return rest;
    }
  }
  return trimmed;
}

function breakLongWord(word: string, maxWidth: number, measure: MeasureTextFn): string[] {
  const chars = Array.from(word);
  const chunks: string[] = [];
  let current = "";
  for (const ch of chars) {
    const next = current + ch;
    if (current.length > 0 && measure(next) > maxWidth) {
      chunks.push(current);
      current = ch;
    } else {
      current = next;
    }
  }
  if (current.length > 0) {
    chunks.push(current);
  }
  return chunks;
}

/**
 * Wraps `text` into lines no wider than `maxWidth` according to `measure`.
 * Collapses runs of whitespace and breaks single overlong tokens by code point.
 */
export function wrapTextLines(
  text: string,
  maxWidth: number,
  measure: MeasureTextFn,
): string[] {
  const normalized = text.replace(/\s+/g, " ").trim();
  if (!normalized || !(maxWidth > 0)) {
    return [];
  }

  const words = normalized.split(" ");
  const lines: string[] = [];
  let current = "";

  for (const word of words) {
    if (!current) {
      if (measure(word) <= maxWidth) {
        current = word;
      } else {
        const pieces = breakLongWord(word, maxWidth, measure);
        for (let i = 0; i < pieces.length - 1; i += 1) {
          lines.push(pieces[i]);
        }
        current = pieces[pieces.length - 1] ?? "";
      }
      continue;
    }

    const candidate = `${current} ${word}`;
    if (measure(candidate) <= maxWidth) {
      current = candidate;
    } else {
      lines.push(current);
      if (measure(word) <= maxWidth) {
        current = word;
      } else {
        const pieces = breakLongWord(word, maxWidth, measure);
        for (let i = 0; i < pieces.length - 1; i += 1) {
          lines.push(pieces[i]);
        }
        current = pieces[pieces.length - 1] ?? "";
      }
    }
  }

  if (current) {
    lines.push(current);
  }
  return lines;
}

function truncateLineWithEllipsis(
  line: string,
  maxWidth: number,
  measure: MeasureTextFn,
): string {
  const clean = line.replace(/[.,;:!?\-\s]+$/u, "");
  if (clean.length > 0 && measure(clean + ELLIPSIS) <= maxWidth) {
    return clean + ELLIPSIS;
  }

  const chars = Array.from(clean);
  for (let len = chars.length - 1; len > 0; len -= 1) {
    const stem = chars
      .slice(0, len)
      .join("")
      .replace(/[.,;:!?\-\s]+$/u, "");
    if (stem.length > 0 && measure(stem + ELLIPSIS) <= maxWidth) {
      return stem + ELLIPSIS;
    }
  }
  return ELLIPSIS;
}

/**
 * Wraps `text` inside `maxWidth` x `maxHeight`, shrinking the font from
 * `maxFontSize` down to `minFontSize` before truncating the last line with `…`.
 */
export function fitTextBlock(
  text: string,
  options: FitTextOptions,
  measure: MeasureScaledTextFn,
): FittedText {
  const maxFont = Math.max(1, Math.floor(options.maxFontSize));
  const minFont = Math.max(1, Math.min(maxFont, Math.floor(options.minFontSize)));
  const step = Math.max(1, Math.floor(options.fontStep ?? 2));
  const ratio = options.lineHeightRatio ?? 1.28;
  const normalized = text.replace(/\s+/g, " ").trim();

  if (!normalized || !(options.maxWidth > 0) || !(options.maxHeight > 0)) {
    return {
      lines: [],
      fontSize: minFont,
      lineHeight: Math.max(1, Math.round(minFont * ratio)),
      truncated: false,
    };
  }

  const sizes: number[] = [];
  for (let size = maxFont; size > minFont; size -= step) {
    sizes.push(size);
  }
  sizes.push(minFont);

  let minLines: string[] = [];
  let minLineHeight = Math.max(1, Math.round(minFont * ratio));
  let minAllowedLines = 1;

  for (const fontSize of sizes) {
    const lineHeight = Math.max(1, Math.round(fontSize * ratio));
    const byHeight = Math.max(1, Math.floor(options.maxHeight / lineHeight));
    const allowedLines =
      options.maxLines !== undefined
        ? Math.max(1, Math.min(options.maxLines, byHeight))
        : byHeight;

    const wrapped = wrapTextLines(normalized, options.maxWidth, (sample) =>
      measure(sample, fontSize),
    );
    if (wrapped.length <= allowedLines) {
      return {
        lines: wrapped,
        fontSize,
        lineHeight,
        truncated: false,
      };
    }

    if (fontSize === minFont) {
      minLines = wrapped;
      minLineHeight = lineHeight;
      minAllowedLines = allowedLines;
    }
  }

  const kept = minLines.slice(0, minAllowedLines - 1);
  const lastRaw = minLines[minAllowedLines - 1] ?? "";
  const lastTruncated = truncateLineWithEllipsis(lastRaw, options.maxWidth, (sample) =>
    measure(sample, minFont),
  );
  return {
    lines: [...kept, lastTruncated],
    fontSize: minFont,
    lineHeight: minLineHeight,
    truncated: true,
  };
}

/**
 * Fits an image of `srcWidth` x `srcHeight` inside `box` without cropping.
 */
export function containImageRect(srcWidth: number, srcHeight: number, box: Rect): Rect {
  if (!(srcWidth > 0) || !(srcHeight > 0) || !(box.width > 0) || !(box.height > 0)) {
    return { x: box.x, y: box.y, width: 0, height: 0 };
  }
  const scale = Math.min(box.width / srcWidth, box.height / srcHeight);
  const width = Math.round(srcWidth * scale);
  const height = Math.round(srcHeight * scale);
  const x = box.x + Math.round((box.width - width) / 2);
  const y = box.y + Math.round((box.height - height) / 2);
  return { x, y, width, height };
}

/** The family note's band above the footer, and its gap to the last step. */
const NOTE_HEIGHT = 96;
const NOTE_GAP = 18;

/**
 * Computes the 1200x1600 comic-page layout rectangles for up to 6 steps. With `hasNote`, the
 * steps make room for the family's note above the footer; without one, nothing is reserved.
 */
export function computeKeepsakeLayout(stepCount: number, hasNote = false): KeepsakeLayout {
  const clampedCount =
    stepCount <= 0
      ? 0
      : Math.min(MAX_KEEPSAKE_STEPS, Math.floor(stepCount));

  const pageFrame: Rect = { x: 28, y: 28, width: 1144, height: 1544 };
  const contentX = 60;
  const contentWidth = 1080;

  let panelHeight = 550;
  if (clampedCount === 4) {
    panelHeight = 510;
  } else if (clampedCount === 5) {
    panelHeight = 465;
  } else if (clampedCount >= 6) {
    panelHeight = 425;
  }

  const picturePanel: Rect = {
    x: contentX,
    y: 60,
    width: contentWidth,
    height: panelHeight,
  };
  const pictureInner: Rect = {
    x: picturePanel.x + 18,
    y: picturePanel.y + 18,
    width: picturePanel.width - 36,
    height: picturePanel.height - 36,
  };

  const titleHeight = clampedCount <= 4 ? 124 : 112;
  const titleBanner: Rect = {
    x: contentX,
    y: picturePanel.y + picturePanel.height + 22,
    width: contentWidth,
    height: titleHeight,
  };
  const titleTextBox: Rect = {
    x: titleBanner.x + 32,
    y: titleBanner.y + 14,
    width: titleBanner.width - 64,
    height: titleBanner.height - 28,
  };

  const footer: Rect = {
    x: contentX,
    y: 1490,
    width: contentWidth,
    height: 54,
  };

  const note: Rect | null = hasNote
    ? { x: contentX, y: footer.y - 22 - NOTE_HEIGHT, width: contentWidth, height: NOTE_HEIGHT }
    : null;

  const steps: KeepsakeStepSlot[] = [];
  if (clampedCount > 0) {
    const stepsTop = titleBanner.y + titleBanner.height + 24;
    const stepsBottom = note ? note.y - NOTE_GAP : footer.y - 22;
    const availableHeight = stepsBottom - stepsTop;
    const gap = clampedCount <= 4 ? 18 : 14;
    const rawBubbleHeight = Math.floor(
      (availableHeight - gap * (clampedCount - 1)) / clampedCount,
    );
    const bubbleHeight = Math.max(84, Math.min(164, rawBubbleHeight));
    const badgeRadius = 28;
    const bubbleX = contentX + 76;
    const bubbleWidth = contentWidth - 76;

    for (let i = 0; i < clampedCount; i += 1) {
      const rowY = stepsTop + i * (bubbleHeight + gap);
      const bubble: Rect = {
        x: bubbleX,
        y: rowY,
        width: bubbleWidth,
        height: bubbleHeight,
      };
      steps.push({
        bubble,
        badgeRadius,
        badgeCenter: {
          x: contentX + badgeRadius,
          y: rowY + Math.min(44, Math.floor(bubbleHeight / 2)),
        },
        textBox: {
          x: bubble.x + 28,
          y: bubble.y + 14,
          width: bubble.width - 56,
          height: bubble.height - 28,
        },
      });
    }
  }

  return {
    width: KEEPSAKE_WIDTH,
    height: KEEPSAKE_HEIGHT,
    pageFrame,
    picturePanel,
    pictureInner,
    titleBanner,
    titleTextBox,
    steps,
    note,
    footer,
  };
}

/**
 * Strips the `data:image/png;base64,` prefix and validates the base64 body.
 */
export function splitPngDataUrl(dataUrl: string): string | null {
  if (typeof dataUrl !== "string") {
    return null;
  }
  const trimmed = dataUrl.trim();
  const match = PNG_DATA_URL_PREFIX.exec(trimmed);
  if (!match) {
    return null;
  }
  const body = trimmed.slice(match[0].length).replace(/\s+/g, "");
  if (body.length === 0 || body.length % 4 !== 0 || !BASE64_BODY.test(body)) {
    return null;
  }
  return body;
}

/**
 * Exact decoded byte length of a valid RFC 4648 padded base64 string.
 */
export function decodedBase64ByteLength(base64: string): number {
  if (base64.length === 0) {
    return 0;
  }
  const padding = base64.endsWith("==") ? 2 : base64.endsWith("=") ? 1 : 0;
  return Math.floor((base64.length * 3) / 4) - padding;
}

/**
 * Validates `id` and a PNG data URL against the 8 MiB limit before saving.
 */
export function prepareCaptionedSavePayload(
  id: string | undefined,
  dataUrl: string,
  maxBytes: number = MAX_CAPTIONED_PNG_BYTES,
): CaptionedSaveDecision {
  const cleanId = typeof id === "string" ? id.trim() : "";
  if (!cleanId) {
    return { ok: false, reason: "missing-id" };
  }
  const pngBase64 = splitPngDataUrl(dataUrl);
  if (!pngBase64) {
    return { ok: false, reason: "invalid-data-url" };
  }
  const byteLength = decodedBase64ByteLength(pngBase64);
  if (byteLength > maxBytes) {
    return { ok: false, reason: "too-large" };
  }
  return { ok: true, id: cleanId, pngBase64, byteLength };
}

/**
 * Formats `date` for the keepsake footer in the active locale (`en` or `ru`).
 */
export function formatKeepsakeDate(date: Date, locale: string): string {
  const tag = locale.trim().toLowerCase().startsWith("ru") ? "ru-RU" : "en-US";
  try {
    return new Intl.DateTimeFormat(tag, {
      year: "numeric",
      month: "long",
      day: "numeric",
    }).format(date);
  } catch {
    return date.toISOString().slice(0, 10);
  }
}
