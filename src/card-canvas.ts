// 1200x1600 comic-page keepsake renderer for the lesson card window (src/card.ts).
// Waits for document.fonts.ready and image load (with timeouts), then draws the
// picture panel (or Mate's bulb placeholder), bold title banner, speech-bubble
// steps, the family's note when there is one, local date, and "Made with Mate" mark
// using system fonts only.

import {
  KEEPSAKE_FONT_COMIC,
  KEEPSAKE_FONT_SANS,
  KEEPSAKE_HEIGHT,
  KEEPSAKE_WIDTH,
  MAX_KEEPSAKE_STEPS,
  computeKeepsakeLayout,
  containImageRect,
  fitTextBlock,
  formatKeepsakeDate,
  stripLeadingStepNumber,
  type Rect,
} from "./card-keepsake.ts";

const FONT_READY_TIMEOUT_MS = 2500;
const IMAGE_LOAD_TIMEOUT_MS = 5000;

const INK = "#1f2a44";
const MUTED = "#55607a";
const PAPER_BG = "#fcf9f1";
const SHEET_BG = "#ffffff";
const PANEL_WARM = "#fff8e2";
const ACCENT_YELLOW = "#ffcd3c";
const BUBBLE_ALT = "#faf6ea";

export interface KeepsakeRenderInput {
  readonly title: string;
  readonly steps: readonly string[];
  readonly imageUrl: string;
  readonly locale: string;
  readonly madeWithLabel: string;
  readonly date?: Date;
  /** The family's note (Settings), printed above the footer; empty or absent: none. */
  readonly familyNote?: string;
}

export interface KeepsakeRenderResult {
  readonly dataUrl: string;
  readonly hasPicture: boolean;
}

function create2dCanvas(
  width: number,
  height: number,
): { canvas: HTMLCanvasElement; ctx: CanvasRenderingContext2D } | null {
  if (typeof document === "undefined" || typeof document.createElement !== "function") {
    return null;
  }
  const el = document.createElement("canvas") as Partial<HTMLCanvasElement>;
  if (typeof el.getContext !== "function") {
    return null;
  }
  const canvas = el as HTMLCanvasElement;
  canvas.width = width;
  canvas.height = height;
  const ctx = canvas.getContext("2d");
  if (!ctx) {
    return null;
  }
  return { canvas, ctx };
}

export function canComposeKeepsake(): boolean {
  return create2dCanvas(1, 1) !== null;
}

async function waitWithTimeout<T>(
  promise: Promise<T>,
  timeoutMs: number,
  fallback: T,
): Promise<T> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    return await Promise.race([
      promise,
      new Promise<T>((resolve) => {
        timer = setTimeout(() => resolve(fallback), timeoutMs);
      }),
    ]);
  } catch {
    return fallback;
  } finally {
    if (timer !== undefined) {
      clearTimeout(timer);
    }
  }
}

async function waitForFontsReady(timeoutMs = FONT_READY_TIMEOUT_MS): Promise<void> {
  if (typeof document === "undefined" || !document.fonts?.ready) {
    return;
  }
  await waitWithTimeout(
    document.fonts.ready.then(() => undefined),
    timeoutMs,
    undefined,
  );
}

async function loadCardImage(
  imageUrl: string,
  timeoutMs = IMAGE_LOAD_TIMEOUT_MS,
): Promise<HTMLImageElement | null> {
  const trimmed = imageUrl.trim();
  if (!trimmed || typeof Image === "undefined") {
    return null;
  }

  const loadPromise = new Promise<HTMLImageElement | null>((resolve) => {
    const img = new Image();
    if (trimmed.startsWith("http://") || trimmed.startsWith("https://")) {
      img.crossOrigin = "anonymous";
    }
    const cleanup = (): void => {
      img.onload = null;
      img.onerror = null;
    };
    img.onload = () => {
      cleanup();
      resolve(img.naturalWidth > 0 && img.naturalHeight > 0 ? img : null);
    };
    img.onerror = () => {
      cleanup();
      resolve(null);
    };
    img.src = trimmed;
  });

  return waitWithTimeout(loadPromise, timeoutMs, null);
}

function traceRoundedRect(ctx: CanvasRenderingContext2D, rect: Rect, radius: number): void {
  const r = Math.max(0, Math.min(radius, rect.width / 2, rect.height / 2));
  const right = rect.x + rect.width;
  const bottom = rect.y + rect.height;
  ctx.beginPath();
  ctx.moveTo(rect.x + r, rect.y);
  ctx.lineTo(right - r, rect.y);
  ctx.quadraticCurveTo(right, rect.y, right, rect.y + r);
  ctx.lineTo(right, bottom - r);
  ctx.quadraticCurveTo(right, bottom, right - r, bottom);
  ctx.lineTo(rect.x + r, bottom);
  ctx.quadraticCurveTo(rect.x, bottom, rect.x, bottom - r);
  ctx.lineTo(rect.x, rect.y + r);
  ctx.quadraticCurveTo(rect.x, rect.y, rect.x + r, rect.y);
  ctx.closePath();
}

function traceSpeechBubble(
  ctx: CanvasRenderingContext2D,
  bubble: Rect,
  radius: number,
  tailTargetY: number,
): void {
  const r = Math.max(0, Math.min(radius, bubble.width / 2, bubble.height / 2));
  const left = bubble.x;
  const right = bubble.x + bubble.width;
  const top = bubble.y;
  const bottom = bubble.y + bubble.height;
  const tailHalf = 11;
  const tailLen = 18;
  const tipY = Math.max(top + r + tailHalf + 2, Math.min(bottom - r - tailHalf - 2, tailTargetY));

  ctx.beginPath();
  ctx.moveTo(left + r, top);
  ctx.lineTo(right - r, top);
  ctx.quadraticCurveTo(right, top, right, top + r);
  ctx.lineTo(right, bottom - r);
  ctx.quadraticCurveTo(right, bottom, right - r, bottom);
  ctx.lineTo(left + r, bottom);
  ctx.quadraticCurveTo(left, bottom, left, bottom - r);
  ctx.lineTo(left, tipY + tailHalf);
  ctx.lineTo(left - tailLen, tipY);
  ctx.lineTo(left, tipY - tailHalf);
  ctx.lineTo(left, top + r);
  ctx.quadraticCurveTo(left, top, left + r, top);
  ctx.closePath();
}

function fillHalftone(
  ctx: CanvasRenderingContext2D,
  rect: Rect,
  dotColor: string,
  spacing = 20,
  dotRadius = 2.1,
): void {
  const tile = create2dCanvas(spacing, spacing);
  if (tile) {
    tile.ctx.fillStyle = dotColor;
    tile.ctx.beginPath();
    tile.ctx.arc(spacing / 4, spacing / 4, dotRadius, 0, Math.PI * 2);
    tile.ctx.arc((spacing * 3) / 4, (spacing * 3) / 4, dotRadius, 0, Math.PI * 2);
    tile.ctx.fill();
    const pattern = ctx.createPattern(tile.canvas, "repeat");
    if (pattern) {
      ctx.save();
      ctx.fillStyle = pattern;
      ctx.fillRect(rect.x, rect.y, rect.width, rect.height);
      ctx.restore();
      return;
    }
  }
}

function drawSunburstBackdrop(ctx: CanvasRenderingContext2D, panel: Rect): void {
  const cx = panel.x + panel.width / 2;
  const cy = panel.y + panel.height * 0.52;
  const maxR = Math.hypot(panel.width, panel.height);

  ctx.fillStyle = PANEL_WARM;
  ctx.fillRect(panel.x, panel.y, panel.width, panel.height);

  // Subtle comic sunburst wedges
  ctx.fillStyle = "rgba(255, 205, 60, 0.22)";
  const rayCount = 16;
  for (let i = 0; i < rayCount; i += 1) {
    const a0 = (i * Math.PI * 2) / rayCount;
    const a1 = a0 + Math.PI / rayCount;
    ctx.beginPath();
    ctx.moveTo(cx, cy);
    ctx.lineTo(cx + Math.cos(a0) * maxR, cy + Math.sin(a0) * maxR);
    ctx.lineTo(cx + Math.cos(a1) * maxR, cy + Math.sin(a1) * maxR);
    ctx.closePath();
    ctx.fill();
  }

  const glow = ctx.createRadialGradient(cx, cy, 24, cx, cy, panel.height * 0.65);
  glow.addColorStop(0, "rgba(255, 251, 235, 0.95)");
  glow.addColorStop(0.55, "rgba(255, 238, 170, 0.55)");
  glow.addColorStop(1, "rgba(255, 248, 226, 0)");
  ctx.fillStyle = glow;
  ctx.fillRect(panel.x, panel.y, panel.width, panel.height);

  fillHalftone(ctx, panel, "rgba(31, 42, 68, 0.055)", 22, 2);
}

function drawComicPlaceholderBulb(ctx: CanvasRenderingContext2D, panel: Rect): void {
  const cx = panel.x + panel.width / 2;
  const cy = panel.y + panel.height * 0.48;
  const scale = Math.min(1, panel.height / 470);

  ctx.save();
  ctx.translate(cx, cy);
  ctx.scale(scale, scale);

  // Warm comic halo
  const halo = ctx.createRadialGradient(0, -16, 20, 0, -16, 195);
  halo.addColorStop(0, "rgba(255, 205, 60, 0.72)");
  halo.addColorStop(0.6, "rgba(255, 205, 60, 0.2)");
  halo.addColorStop(1, "rgba(255, 205, 60, 0)");
  ctx.fillStyle = halo;
  ctx.beginPath();
  ctx.arc(0, -16, 195, 0, Math.PI * 2);
  ctx.fill();

  // Cheerful spark lines around Mate's bulb
  ctx.strokeStyle = INK;
  ctx.lineWidth = 5;
  ctx.lineCap = "round";
  const rayAngles = [-2.55, -2.05, -1.57, -1.09, -0.59, -0.12, 3.26];
  for (const angle of rayAngles) {
    const r0 = 132;
    const r1 = 158;
    ctx.beginPath();
    ctx.moveTo(Math.cos(angle) * r0, -24 + Math.sin(angle) * r0);
    ctx.lineTo(Math.cos(angle) * r1, -24 + Math.sin(angle) * r1);
    ctx.stroke();
  }

  // Pear-shaped bulb glass
  const glassGrad = ctx.createLinearGradient(-70, -125, 70, 65);
  glassGrad.addColorStop(0, "#fffdf2");
  glassGrad.addColorStop(0.55, "#ffe588");
  glassGrad.addColorStop(1, ACCENT_YELLOW);

  ctx.beginPath();
  ctx.moveTo(-48, 66);
  ctx.bezierCurveTo(-56, 36, -94, 10, -94, -36);
  ctx.bezierCurveTo(-94, -92, -52, -132, 0, -132);
  ctx.bezierCurveTo(52, -132, 94, -92, 94, -36);
  ctx.bezierCurveTo(94, 10, 56, 36, 48, 66);
  ctx.closePath();
  ctx.fillStyle = glassGrad;
  ctx.fill();
  ctx.lineWidth = 6;
  ctx.strokeStyle = INK;
  ctx.lineJoin = "round";
  ctx.stroke();

  // Glass highlight
  ctx.beginPath();
  ctx.arc(-36, -68, 22, Math.PI * 0.95, Math.PI * 1.55);
  ctx.strokeStyle = "#ffffff";
  ctx.lineWidth = 7;
  ctx.stroke();

  // Warm filament behind face
  ctx.beginPath();
  ctx.moveTo(-22, 22);
  ctx.lineTo(-12, -10);
  ctx.lineTo(12, -10);
  ctx.lineTo(22, 22);
  ctx.strokeStyle = "#c27803";
  ctx.lineWidth = 4.5;
  ctx.stroke();

  // Mate's friendly eyes, cheeks and smile
  ctx.fillStyle = INK;
  ctx.beginPath();
  ctx.arc(-28, -32, 8.5, 0, Math.PI * 2);
  ctx.arc(28, -32, 8.5, 0, Math.PI * 2);
  ctx.fill();

  ctx.fillStyle = "#ffffff";
  ctx.beginPath();
  ctx.arc(-31, -35, 3, 0, Math.PI * 2);
  ctx.arc(25, -35, 3, 0, Math.PI * 2);
  ctx.fill();

  ctx.fillStyle = "rgba(245, 158, 11, 0.38)";
  ctx.beginPath();
  ctx.ellipse(-46, -16, 12, 7, 0, 0, Math.PI * 2);
  ctx.ellipse(46, -16, 12, 7, 0, 0, Math.PI * 2);
  ctx.fill();

  ctx.beginPath();
  ctx.arc(0, -20, 20, 0.2 * Math.PI, 0.8 * Math.PI);
  ctx.strokeStyle = INK;
  ctx.lineWidth = 5;
  ctx.stroke();

  // Screw base treads
  const treads: readonly Rect[] = [
    { x: -46, y: 66, width: 92, height: 18 },
    { x: -40, y: 86, width: 80, height: 16 },
    { x: -32, y: 104, width: 64, height: 15 },
  ];
  for (const tread of treads) {
    traceRoundedRect(ctx, tread, 7);
    ctx.fillStyle = "#cbd5e1";
    ctx.fill();
    ctx.lineWidth = 4.5;
    ctx.strokeStyle = INK;
    ctx.stroke();
  }

  // Contact tip
  ctx.beginPath();
  ctx.moveTo(-20, 120);
  ctx.lineTo(20, 120);
  ctx.lineTo(12, 134);
  ctx.lineTo(-12, 134);
  ctx.closePath();
  ctx.fillStyle = INK;
  ctx.fill();

  ctx.restore();
}

function drawMiniBulbMark(ctx: CanvasRenderingContext2D, cx: number, cy: number): void {
  ctx.save();
  ctx.translate(cx, cy);

  ctx.fillStyle = "rgba(255, 205, 60, 0.4)";
  ctx.beginPath();
  ctx.arc(0, -2, 18, 0, Math.PI * 2);
  ctx.fill();

  ctx.beginPath();
  ctx.arc(0, -4, 11, Math.PI * 0.78, Math.PI * 2.22);
  ctx.lineTo(6, 8);
  ctx.lineTo(-6, 8);
  ctx.closePath();
  ctx.fillStyle = ACCENT_YELLOW;
  ctx.fill();
  ctx.lineWidth = 2.5;
  ctx.strokeStyle = INK;
  ctx.lineJoin = "round";
  ctx.stroke();

  ctx.beginPath();
  ctx.moveTo(-5, 12);
  ctx.lineTo(5, 12);
  ctx.moveTo(-3.5, 15.5);
  ctx.lineTo(3.5, 15.5);
  ctx.lineCap = "round";
  ctx.lineWidth = 2.5;
  ctx.strokeStyle = INK;
  ctx.stroke();

  ctx.restore();
}

function drawKeepsakeScene(
  ctx: CanvasRenderingContext2D,
  input: KeepsakeRenderInput,
  loadedImage: HTMLImageElement | null,
): void {
  const visibleSteps = input.steps.slice(0, MAX_KEEPSAKE_STEPS);
  const familyNote = input.familyNote?.trim() ?? "";
  const layout = computeKeepsakeLayout(visibleSteps.length, familyNote.length > 0);

  // 1. Outer comic paper + halftone margin
  ctx.fillStyle = PAPER_BG;
  ctx.fillRect(0, 0, layout.width, layout.height);
  fillHalftone(
    ctx,
    { x: 0, y: 0, width: layout.width, height: layout.height },
    "rgba(31, 42, 68, 0.08)",
    18,
    2,
  );

  // Comic page sheet + hard offset shadow
  traceRoundedRect(
    ctx,
    {
      x: layout.pageFrame.x + 8,
      y: layout.pageFrame.y + 8,
      width: layout.pageFrame.width,
      height: layout.pageFrame.height,
    },
    18,
  );
  ctx.fillStyle = INK;
  ctx.fill();

  traceRoundedRect(ctx, layout.pageFrame, 18);
  ctx.fillStyle = SHEET_BG;
  ctx.fill();
  ctx.lineWidth = 5;
  ctx.strokeStyle = INK;
  ctx.stroke();

  // 2. Top picture panel with thick ink border
  traceRoundedRect(
    ctx,
    {
      x: layout.picturePanel.x + 7,
      y: layout.picturePanel.y + 7,
      width: layout.picturePanel.width,
      height: layout.picturePanel.height,
    },
    14,
  );
  ctx.fillStyle = INK;
  ctx.fill();

  ctx.save();
  traceRoundedRect(ctx, layout.picturePanel, 14);
  ctx.clip();
  drawSunburstBackdrop(ctx, layout.picturePanel);

  if (loadedImage) {
    const dest = containImageRect(
      loadedImage.naturalWidth,
      loadedImage.naturalHeight,
      layout.pictureInner,
    );
    if (dest.width > 0 && dest.height > 0) {
      ctx.fillStyle = SHEET_BG;
      ctx.fillRect(dest.x - 4, dest.y - 4, dest.width + 8, dest.height + 8);
      ctx.drawImage(loadedImage, dest.x, dest.y, dest.width, dest.height);
      ctx.strokeStyle = INK;
      ctx.lineWidth = 4;
      ctx.strokeRect(dest.x, dest.y, dest.width, dest.height);
    }
  } else {
    drawComicPlaceholderBulb(ctx, layout.picturePanel);
  }
  ctx.restore();

  traceRoundedRect(ctx, layout.picturePanel, 14);
  ctx.lineWidth = 6;
  ctx.strokeStyle = INK;
  ctx.stroke();

  // 3. Bold comic title banner
  traceRoundedRect(
    ctx,
    {
      x: layout.titleBanner.x + 6,
      y: layout.titleBanner.y + 6,
      width: layout.titleBanner.width,
      height: layout.titleBanner.height,
    },
    14,
  );
  ctx.fillStyle = INK;
  ctx.fill();

  traceRoundedRect(ctx, layout.titleBanner, 14);
  ctx.fillStyle = ACCENT_YELLOW;
  ctx.fill();
  ctx.lineWidth = 5.5;
  ctx.strokeStyle = INK;
  ctx.stroke();

  const fittedTitle = fitTextBlock(
    input.title,
    {
      maxWidth: layout.titleTextBox.width,
      maxHeight: layout.titleTextBox.height,
      maxFontSize: 42,
      minFontSize: 24,
      fontStep: 2,
      lineHeightRatio: 1.2,
      maxLines: 3,
    },
    (sample, fontSize) => {
      ctx.font = `800 ${fontSize}px ${KEEPSAKE_FONT_SANS}`;
      return ctx.measureText(sample).width;
    },
  );

  ctx.font = `800 ${fittedTitle.fontSize}px ${KEEPSAKE_FONT_SANS}`;
  ctx.fillStyle = INK;
  ctx.textAlign = "center";
  ctx.textBaseline = "middle";
  const titleBlockHeight = fittedTitle.lines.length * fittedTitle.lineHeight;
  const titleStartY =
    layout.titleTextBox.y +
    (layout.titleTextBox.height - titleBlockHeight) / 2 +
    fittedTitle.lineHeight / 2;
  const titleCenterX = layout.titleTextBox.x + layout.titleTextBox.width / 2;
  for (let i = 0; i < fittedTitle.lines.length; i += 1) {
    ctx.fillText(fittedTitle.lines[i], titleCenterX, titleStartY + i * fittedTitle.lineHeight);
  }

  // 4. Numbered steps in comic speech-bubble boxes
  for (let i = 0; i < layout.steps.length; i += 1) {
    const slot = layout.steps[i];
    const stepNumber = i + 1;
    const stepText = stripLeadingStepNumber(visibleSteps[i] ?? "", stepNumber);

    // Speech bubble shadow + fill + stroke
    ctx.save();
    ctx.translate(5, 5);
    traceSpeechBubble(ctx, slot.bubble, 16, slot.badgeCenter.y);
    ctx.fillStyle = INK;
    ctx.fill();
    ctx.restore();

    traceSpeechBubble(ctx, slot.bubble, 16, slot.badgeCenter.y);
    ctx.fillStyle = i % 2 === 0 ? BUBBLE_ALT : SHEET_BG;
    ctx.fill();
    ctx.lineWidth = 4.5;
    ctx.lineJoin = "round";
    ctx.strokeStyle = INK;
    ctx.stroke();

    // Circular comic number badge
    ctx.beginPath();
    ctx.arc(slot.badgeCenter.x + 3, slot.badgeCenter.y + 3, slot.badgeRadius, 0, Math.PI * 2);
    ctx.fillStyle = INK;
    ctx.fill();

    ctx.beginPath();
    ctx.arc(slot.badgeCenter.x, slot.badgeCenter.y, slot.badgeRadius, 0, Math.PI * 2);
    ctx.fillStyle = ACCENT_YELLOW;
    ctx.fill();
    ctx.lineWidth = 4.5;
    ctx.strokeStyle = INK;
    ctx.stroke();

    ctx.font = `800 28px ${KEEPSAKE_FONT_COMIC}`;
    ctx.fillStyle = INK;
    ctx.textAlign = "center";
    ctx.textBaseline = "middle";
    ctx.fillText(String(stepNumber), slot.badgeCenter.x, slot.badgeCenter.y + 1);

    // Wrapped & fitted step text
    const fittedStep = fitTextBlock(
      stepText,
      {
        maxWidth: slot.textBox.width,
        maxHeight: slot.textBox.height,
        maxFontSize: 30,
        minFontSize: 20,
        fontStep: 1,
        lineHeightRatio: 1.28,
      },
      (sample, fontSize) => {
        ctx.font = `600 ${fontSize}px ${KEEPSAKE_FONT_SANS}`;
        return ctx.measureText(sample).width;
      },
    );

    ctx.font = `600 ${fittedStep.fontSize}px ${KEEPSAKE_FONT_SANS}`;
    ctx.fillStyle = INK;
    ctx.textAlign = "left";
    ctx.textBaseline = "middle";
    const stepBlockHeight = fittedStep.lines.length * fittedStep.lineHeight;
    const stepStartY =
      slot.textBox.y +
      (slot.textBox.height - stepBlockHeight) / 2 +
      fittedStep.lineHeight / 2;
    for (let lineIdx = 0; lineIdx < fittedStep.lines.length; lineIdx += 1) {
      ctx.fillText(
        fittedStep.lines[lineIdx],
        slot.textBox.x,
        stepStartY + lineIdx * fittedStep.lineHeight,
      );
    }
  }

  // 5. The family's note in the handwritten comic face, on a warm panel above the footer
  if (layout.note) {
    const box = layout.note;
    traceRoundedRect(ctx, box, 14);
    ctx.fillStyle = PANEL_WARM;
    ctx.fill();
    ctx.lineWidth = 3.5;
    ctx.strokeStyle = INK;
    ctx.stroke();

    const fittedNote = fitTextBlock(
      familyNote,
      {
        maxWidth: box.width - 56,
        maxHeight: box.height - 20,
        maxFontSize: 32,
        minFontSize: 22,
        fontStep: 2,
        lineHeightRatio: 1.25,
        maxLines: 2,
      },
      (sample, fontSize) => {
        ctx.font = `700 ${fontSize}px ${KEEPSAKE_FONT_COMIC}`;
        return ctx.measureText(sample).width;
      },
    );
    ctx.font = `700 ${fittedNote.fontSize}px ${KEEPSAKE_FONT_COMIC}`;
    ctx.fillStyle = INK;
    ctx.textAlign = "center";
    ctx.textBaseline = "middle";
    const noteBlockHeight = fittedNote.lines.length * fittedNote.lineHeight;
    const noteStartY = box.y + (box.height - noteBlockHeight) / 2 + fittedNote.lineHeight / 2;
    for (let i = 0; i < fittedNote.lines.length; i += 1) {
      ctx.fillText(fittedNote.lines[i], box.x + box.width / 2, noteStartY + i * fittedNote.lineHeight);
    }
  }

  // 6. Footer: local date on the left, "Made with Mate" bulb mark on the right
  ctx.beginPath();
  ctx.moveTo(layout.footer.x, layout.footer.y - 6);
  ctx.lineTo(layout.footer.x + layout.footer.width, layout.footer.y - 6);
  ctx.lineWidth = 2.5;
  ctx.strokeStyle = "#dfd6bf";
  ctx.stroke();

  const footerMidY = layout.footer.y + layout.footer.height / 2 + 2;
  const dateText = formatKeepsakeDate(input.date ?? new Date(), input.locale);
  ctx.font = `600 22px ${KEEPSAKE_FONT_SANS}`;
  ctx.fillStyle = MUTED;
  ctx.textAlign = "left";
  ctx.textBaseline = "middle";
  ctx.fillText(dateText, layout.footer.x + 4, footerMidY);

  ctx.font = `700 22px ${KEEPSAKE_FONT_COMIC}`;
  ctx.fillStyle = INK;
  ctx.textAlign = "right";
  const rightEdge = layout.footer.x + layout.footer.width - 4;
  ctx.fillText(input.madeWithLabel, rightEdge, footerMidY);
  const labelWidth = ctx.measureText(input.madeWithLabel).width;
  drawMiniBulbMark(ctx, rightEdge - labelWidth - 22, footerMidY - 2);
}

/**
 * Waits for fonts and optional card picture, composes the 1200x1600 comic
 * keepsake onto an offscreen canvas, and exports a PNG data URL.
 */
export async function composeCaptionedKeepsake(
  input: KeepsakeRenderInput,
): Promise<KeepsakeRenderResult | null> {
  const surface = create2dCanvas(KEEPSAKE_WIDTH, KEEPSAKE_HEIGHT);
  if (!surface) {
    return null;
  }

  const [, loadedImage] = await Promise.all([
    waitForFontsReady(),
    loadCardImage(input.imageUrl),
  ]);

  drawKeepsakeScene(surface.ctx, input, loadedImage);

  try {
    return {
      dataUrl: surface.canvas.toDataURL("image/png"),
      hasPicture: loadedImage !== null,
    };
  } catch (error) {
    // If a cross-origin image ever tainted the canvas, redraw with the bulb placeholder.
    console.warn("[card] picture tainted canvas; using placeholder panel:", error);
    const cleanSurface = create2dCanvas(KEEPSAKE_WIDTH, KEEPSAKE_HEIGHT);
    if (!cleanSurface) {
      return null;
    }
    drawKeepsakeScene(cleanSurface.ctx, input, null);
    try {
      return {
        dataUrl: cleanSurface.canvas.toDataURL("image/png"),
        hasPicture: false,
      };
    } catch (innerError) {
      console.warn("[card] failed to export keepsake PNG:", innerError);
      return null;
    }
  }
}
