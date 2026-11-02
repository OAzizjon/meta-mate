// The Settings window's form controls (src/settings.ts): a radio group in a fieldset and a
// checkbox, each choice a whole clickable option. Builds the DOM only; src/settings.ts writes
// the localized text (invariant 7) and the selection.

import type { LocaleKey } from "../i18n/index.ts";

export interface RadioSpec<V extends string> {
  readonly id: string;
  readonly value: V;
  readonly labelKey?: LocaleKey;
  readonly literalLabel?: string;
  readonly hintKey?: LocaleKey;
}

/**
 * `cards`: each choice a whole card, its hint inside it. `segments`: short choices side by
 * side on one row that wraps; a hint goes on one line under the row, shown for the chosen one.
 */
export type RadioLayout = "cards" | "segments";

export interface RadioGroupRefs<V extends string> {
  readonly fieldset: HTMLFieldSetElement;
  readonly legend: HTMLLegendElement;
  readonly legendKey: LocaleKey;
  readonly layout: RadioLayout;
  readonly items: ReadonlyArray<{
    readonly value: V;
    readonly wrapper: HTMLLabelElement;
    readonly input: HTMLInputElement;
    readonly titleEl: HTMLSpanElement;
    readonly labelKey?: LocaleKey;
    readonly literalLabel?: string;
    readonly hintEl?: HTMLSpanElement;
    readonly hintKey?: LocaleKey;
  }>;
}

export function buildRadioGroup<V extends string>(
  name: string,
  legendKey: LocaleKey,
  specs: readonly RadioSpec<V>[],
  layout: RadioLayout,
  onSelect: (value: V) => void,
): RadioGroupRefs<V> {
  const segmented = layout === "segments";
  const fieldset = document.createElement("fieldset");
  fieldset.className = "settings-group";

  const legend = document.createElement("legend");
  legend.className = "settings-legend";
  fieldset.append(legend);

  const list = document.createElement("div");
  list.className = segmented ? "settings-segments" : "settings-options";
  fieldset.append(list);

  // Under the row, so every segment stays one short name.
  let hints: HTMLDivElement | undefined;
  if (segmented && specs.some((spec) => spec.hintKey)) {
    hints = document.createElement("div");
    hints.className = "settings-segment-hints";
    fieldset.append(hints);
  }

  const items = specs.map((spec) => {
    const wrapper = document.createElement("label");
    wrapper.className = segmented ? "settings-segment" : "settings-radio-option";
    wrapper.htmlFor = spec.id;

    const input = document.createElement("input");
    input.type = "radio";
    input.id = spec.id;
    input.name = name;
    input.value = spec.value;

    const textWrap = document.createElement("span");
    textWrap.className = "option-text";

    const titleEl = document.createElement("span");
    titleEl.className = "option-title";
    if (!segmented) {
      titleEl.id = `${spec.id}-title`;
      input.setAttribute("aria-labelledby", titleEl.id);
    }
    textWrap.append(titleEl);

    let hintEl: HTMLSpanElement | undefined;
    if (spec.hintKey) {
      hintEl = document.createElement("span");
      hintEl.className = "option-hint";
      hintEl.id = `${spec.id}-hint`;
      // A hidden hint still describes its radio: it is referenced directly.
      input.setAttribute("aria-describedby", hintEl.id);
      (hints ?? textWrap).append(hintEl);
    }

    input.addEventListener("change", () => {
      if (input.checked) {
        // The mark and the hint follow the click at once, not after the save.
        syncRadioSelection(group, spec.value);
        onSelect(spec.value);
      }
    });

    wrapper.append(input, textWrap);
    list.append(wrapper);

    return {
      value: spec.value,
      wrapper,
      input,
      titleEl,
      labelKey: spec.labelKey,
      literalLabel: spec.literalLabel,
      hintEl,
      hintKey: spec.hintKey,
    };
  });

  const group: RadioGroupRefs<V> = { fieldset, legend, legendKey, layout, items };
  return group;
}

/** Checks the `selected` radio and marks its whole option; segments show only its hint. */
export function syncRadioSelection<V extends string>(
  group: RadioGroupRefs<V>,
  selected: V,
): void {
  for (const item of group.items) {
    const checked = item.value === selected;
    item.input.checked = checked;
    item.wrapper.classList.toggle("is-selected", checked);
    if (item.hintEl && group.layout === "segments") {
      item.hintEl.hidden = !checked;
    }
  }
}

export interface CheckboxRefs {
  readonly wrapper: HTMLLabelElement;
  readonly input: HTMLInputElement;
  readonly titleEl: HTMLSpanElement;
  readonly labelKey: LocaleKey;
  readonly hintEl?: HTMLSpanElement;
  readonly hintKey?: LocaleKey;
}

export function buildCheckbox(
  id: string,
  labelKey: LocaleKey,
  onToggle: (checked: boolean) => void,
  hintKey?: LocaleKey,
): CheckboxRefs {
  const wrapper = document.createElement("label");
  wrapper.className = "settings-checkbox-option";
  wrapper.htmlFor = id;

  // Still a checkbox, drawn as an on/off switch (src/theme.css).
  const input = document.createElement("input");
  input.type = "checkbox";
  input.className = "switch";
  input.id = id;

  const textWrap = document.createElement("span");
  textWrap.className = "option-text";

  const titleEl = document.createElement("span");
  titleEl.className = "option-title";
  textWrap.append(titleEl);

  let hintEl: HTMLSpanElement | undefined;
  if (hintKey) {
    hintEl = document.createElement("span");
    hintEl.className = "option-hint";
    hintEl.id = `${id}-hint`;
    input.setAttribute("aria-describedby", hintEl.id);
    textWrap.append(hintEl);
  }

  input.addEventListener("change", () => {
    onToggle(input.checked);
  });

  wrapper.append(input, textWrap);
  return { wrapper, input, titleEl, labelKey, hintEl, hintKey };
}

export function syncCheckbox(box: CheckboxRefs, checked: boolean): void {
  box.input.checked = checked;
  box.wrapper.classList.toggle("is-selected", checked);
}
