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

export interface RadioGroupRefs<V extends string> {
  readonly fieldset: HTMLFieldSetElement;
  readonly legend: HTMLLegendElement;
  readonly legendKey: LocaleKey;
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
  optionsClassName: string,
  optionClassName: string,
  onSelect: (value: V) => void,
): RadioGroupRefs<V> {
  const fieldset = document.createElement("fieldset");
  fieldset.className = "settings-group";

  const legend = document.createElement("legend");
  legend.className = "settings-legend";
  fieldset.append(legend);

  const list = document.createElement("div");
  list.className = optionsClassName;
  fieldset.append(list);

  const items = specs.map((spec) => {
    const wrapper = document.createElement("label");
    wrapper.className = optionClassName;
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
    textWrap.append(titleEl);

    let hintEl: HTMLSpanElement | undefined;
    if (spec.hintKey) {
      hintEl = document.createElement("span");
      hintEl.className = "option-hint";
      hintEl.id = `${spec.id}-hint`;
      input.setAttribute("aria-describedby", hintEl.id);
      textWrap.append(hintEl);
    }

    input.addEventListener("change", () => {
      if (input.checked) {
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

  return { fieldset, legend, legendKey, items };
}

/** Checks the `selected` radio and marks its whole option. */
export function syncRadioSelection<V extends string>(
  group: RadioGroupRefs<V>,
  selected: V,
): void {
  for (const item of group.items) {
    const checked = item.value === selected;
    item.input.checked = checked;
    item.wrapper.classList.toggle("is-selected", checked);
  }
}

export interface CheckboxRefs {
  readonly wrapper: HTMLLabelElement;
  readonly input: HTMLInputElement;
  readonly titleEl: HTMLSpanElement;
  readonly labelKey: LocaleKey;
}

export function buildCheckbox(
  id: string,
  labelKey: LocaleKey,
  onToggle: (checked: boolean) => void,
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

  input.addEventListener("change", () => {
    onToggle(input.checked);
  });

  wrapper.append(input, textWrap);
  return { wrapper, input, titleEl, labelKey };
}
