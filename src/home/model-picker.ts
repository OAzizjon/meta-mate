// Home window's "Mate's brain" model picker: pure choice navigation, confirmation line,
// and the three comic pill radio cards (quick = Muse Spark 1.3, thorough = Muse Wonder 1.0,
// open = Muse Glimmer 30B).

import type { LocaleKey } from "../i18n/index.ts";
import { DEFAULT_SETTINGS, MODEL_CHOICES, isModelChoice } from "../settings/model.ts";
import type { ModelChoice } from "../settings/model.ts";

export const HOME_MODEL_CHOICES: readonly ModelChoice[] = MODEL_CHOICES;

/** How long the "Mate now thinks with ..." confirmation stays visible after a selection. */
export const HOME_MODEL_SAVED_MS = 4_000;

export interface ModelCardSpec {
  readonly value: ModelChoice;
  readonly labelKey: LocaleKey;
  readonly hintKey: LocaleKey;
}

export const HOME_MODEL_CARDS: readonly ModelCardSpec[] = [
  {
    value: "quick",
    labelKey: "home.model_quick",
    hintKey: "home.model_quick_hint",
  },
  {
    value: "thorough",
    labelKey: "home.model_thorough",
    hintKey: "home.model_thorough_hint",
  },
  {
    value: "open",
    labelKey: "home.model_open",
    hintKey: "home.model_open_hint",
  },
] as const;

export const HOME_MODEL_SPECS: Readonly<Record<ModelChoice, ModelCardSpec>> = {
  quick: HOME_MODEL_CARDS[0],
  thorough: HOME_MODEL_CARDS[1],
  open: HOME_MODEL_CARDS[2],
};

/** Steps forward (`1`) or backward (`-1`) around `HOME_MODEL_CHOICES`, wrapping at the ends. */
export function stepModelChoice(current: ModelChoice, direction: 1 | -1): ModelChoice {
  const index = HOME_MODEL_CHOICES.indexOf(current);
  const start = index === -1 ? 0 : index;
  const next = (start + direction + HOME_MODEL_CHOICES.length) % HOME_MODEL_CHOICES.length;
  return HOME_MODEL_CHOICES[next];
}

export function nextModelChoice(current: ModelChoice): ModelChoice {
  return stepModelChoice(current, 1);
}

export function prevModelChoice(current: ModelChoice): ModelChoice {
  return stepModelChoice(current, -1);
}

/** Returns the adjacent choice for an arrow key, or `null` for any other key. */
export function arrowModelChoice(current: ModelChoice, key: string): ModelChoice | null {
  if (key === "ArrowRight" || key === "ArrowDown") {
    return stepModelChoice(current, 1);
  }
  if (key === "ArrowLeft" || key === "ArrowUp") {
    return stepModelChoice(current, -1);
  }
  return null;
}

export function isSelectKey(key: string): boolean {
  return key === "Enter" || key === " " || key === "Spacebar";
}

/** Formats the confirmation line ("Mate now thinks with {model}.") for `choice`. */
export function modelSavedText(
  choice: ModelChoice,
  translate: (key: LocaleKey) => string,
): string {
  const spec = HOME_MODEL_SPECS[choice] ?? HOME_MODEL_SPECS.quick;
  const modelName = translate(spec.labelKey);
  return translate("home.model_saved").replace("{model}", () => modelName);
}

const SVG_NS = "http://www.w3.org/2000/svg";

function svgEl(tag: string, attrs: Readonly<Record<string, string>>): SVGElement {
  const node = document.createElementNS(SVG_NS, tag);
  for (const [name, value] of Object.entries(attrs)) {
    node.setAttribute(name, value);
  }
  return node;
}

function buildModelIcon(choice: ModelChoice): HTMLElement {
  const wrap = document.createElement("span");
  wrap.className = "home-icon home-model-icon";
  wrap.setAttribute("aria-hidden", "true");

  const svg = svgEl("svg", { viewBox: "0 0 48 48", "aria-hidden": "true" });
  if (choice === "quick") {
    // Lightning spark for Muse Spark 1.3.
    svg.append(
      svgEl("circle", { class: "i-glow", cx: "24", cy: "24", r: "18" }),
      svgEl("polygon", { class: "i-fill", points: "26,6 12,26 23,26 20,42 36,20 25,20" }),
    );
  } else if (choice === "thorough") {
    // Wonder star lens for Muse Wonder 1.0.
    svg.append(
      svgEl("circle", { class: "i-glow", cx: "22", cy: "22", r: "17" }),
      svgEl("circle", { class: "i-paper", cx: "21", cy: "21", r: "13" }),
      svgEl("path", {
        class: "i-fill",
        d: "M21 12 L23.5 18.5 L30 21 L23.5 23.5 L21 30 L18.5 23.5 L12 21 L18.5 18.5 Z",
      }),
      svgEl("path", { class: "i-line", d: "M31 31 L41 41" }),
    );
  } else {
    // Open book and beacon for Muse Glimmer 30B (open weights).
    svg.append(
      svgEl("circle", { class: "i-glow", cx: "24", cy: "24", r: "18" }),
      svgEl("path", {
        class: "i-paper",
        d: "M8 16 C14 16 20 18 24 22 C28 18 34 16 40 16 L40 36 C34 36 28 38 24 42 C20 38 14 36 8 36 Z",
      }),
      svgEl("path", { class: "i-line", d: "M24 22 L24 42" }),
      svgEl("circle", { class: "i-fill", cx: "24", cy: "11", r: "4" }),
    );
  }
  wrap.append(svg);
  return wrap;
}

export interface MountModelPickerOptions {
  readonly t: (key: LocaleKey) => string;
  readonly initialModel?: ModelChoice;
  readonly onSelect: (model: ModelChoice) => Promise<unknown> | unknown;
  readonly savedMs?: number;
}

export interface ModelPickerController {
  readonly element: HTMLElement;
  getSelected(): ModelChoice;
  setSelected(model: ModelChoice): void;
  sync(model: ModelChoice): void;
  destroy(): void;
}

function isMountOptions(
  value: MountModelPickerOptions | HTMLElement,
): value is MountModelPickerOptions {
  return typeof (value as MountModelPickerOptions).t === "function";
}

export function mountModelPicker(
  optionsOrContainer: MountModelPickerOptions | HTMLElement,
  maybeOptions?: MountModelPickerOptions,
): ModelPickerController {
  const container = isMountOptions(optionsOrContainer) ? null : optionsOrContainer;
  const options = isMountOptions(optionsOrContainer) ? optionsOrContainer : maybeOptions!;
  const savedMs = options.savedMs ?? HOME_MODEL_SAVED_MS;

  let selected: ModelChoice = isModelChoice(options.initialModel)
    ? options.initialModel
    : DEFAULT_SETTINGS.model;
  let focusedChoice: ModelChoice = selected;
  let destroyed = false;
  let statusTimer: ReturnType<typeof setTimeout> | undefined;

  const root = document.createElement("section");
  root.className = "home-section home-model comic-panel";
  root.setAttribute("aria-labelledby", "home-model-title");

  const head = document.createElement("div");
  head.className = "home-section-head";
  const title = document.createElement("h2");
  title.className = "home-section-title";
  title.id = "home-model-title";
  title.textContent = options.t("home.model_title");
  const intro = document.createElement("p");
  intro.className = "home-section-intro";
  intro.id = "home-model-intro";
  intro.textContent = options.t("home.model_intro");
  head.append(title, intro);

  const list = document.createElement("div");
  list.className = "home-model-list";
  list.setAttribute("role", "radiogroup");
  list.setAttribute("aria-labelledby", "home-model-title");
  list.setAttribute("aria-describedby", "home-model-intro");

  const status = document.createElement("p");
  status.className = "home-try-status home-model-status";
  status.id = "home-model-status";
  status.setAttribute("role", "status");
  status.setAttribute("aria-live", "polite");
  status.hidden = true;

  const buttons = {} as Record<ModelChoice, HTMLButtonElement>;

  const syncRadios = (): void => {
    for (const spec of HOME_MODEL_CARDS) {
      const btn = buttons[spec.value];
      const isChecked = spec.value === selected;
      const isFocusable = spec.value === focusedChoice;
      btn.setAttribute("aria-checked", isChecked ? "true" : "false");
      btn.setAttribute("tabindex", isFocusable ? "0" : "-1");
      btn.classList.toggle("is-selected", isChecked);
    }
  };

  const showStatus = (text: string, failed: boolean): void => {
    if (destroyed) {
      return;
    }
    clearTimeout(statusTimer);
    status.textContent = text;
    status.classList.toggle("is-error", failed);
    status.hidden = false;
    statusTimer = setTimeout(() => {
      status.textContent = "";
      status.classList.remove("is-error");
      status.hidden = true;
    }, savedMs);
  };

  // Only the newest click reports; a failed save puts the earlier choice back.
  let requests = 0;
  const selectChoice = async (choice: ModelChoice): Promise<void> => {
    if (destroyed) {
      return;
    }
    const previous = selected;
    const request = ++requests;
    selected = choice;
    focusedChoice = choice;
    syncRadios();
    try {
      await options.onSelect(choice);
      if (request === requests) showStatus(modelSavedText(choice, options.t), false);
    } catch {
      if (request !== requests) return;
      if (selected === choice) {
        selected = previous;
        focusedChoice = previous;
        syncRadios();
      }
      showStatus(options.t("home.model_failed"), true);
    }
  };

  for (const spec of HOME_MODEL_CARDS) {
    const btn = document.createElement("button");
    btn.type = "button";
    btn.className = "home-model-card";
    btn.id = `home-model-${spec.value}`;
    btn.setAttribute("role", "radio");
    btn.setAttribute("aria-labelledby", `home-model-${spec.value}-name`);
    btn.setAttribute("aria-describedby", `home-model-${spec.value}-hint`);

    const icon = buildModelIcon(spec.value);
    const name = document.createElement("span");
    name.className = "home-model-name";
    name.id = `home-model-${spec.value}-name`;
    name.textContent = options.t(spec.labelKey);

    const hint = document.createElement("span");
    hint.className = "home-model-hint";
    hint.id = `home-model-${spec.value}-hint`;
    hint.textContent = options.t(spec.hintKey);

    btn.append(icon, name, hint);
    buttons[spec.value] = btn;

    btn.addEventListener("click", () => {
      void selectChoice(spec.value);
    });

    btn.addEventListener("keydown", (event: KeyboardEvent) => {
      const next = arrowModelChoice(spec.value, event.key);
      if (next !== null) {
        event.preventDefault();
        focusedChoice = next;
        syncRadios();
        buttons[next].focus();
        return;
      }
      if (isSelectKey(event.key)) {
        event.preventDefault();
        void selectChoice(spec.value);
      }
    });

    list.append(btn);
  }

  syncRadios();
  root.append(head, list, status);
  if (container) {
    container.append(root);
  }

  const setSelected = (model: ModelChoice): void => {
    if (!isModelChoice(model) || destroyed) {
      return;
    }
    selected = model;
    focusedChoice = model;
    syncRadios();
  };

  return {
    element: root,
    getSelected: () => selected,
    setSelected,
    sync: setSelected,
    destroy(): void {
      destroyed = true;
      clearTimeout(statusTimer);
    },
  };
}

export const createModelPicker = mountModelPicker;
