// The small DOM helpers Home's sections share (src/home/*.ts). Text always goes in as
// textContent: names, about lines and card titles are the person's or a model's words.

import type { LocaleKey } from "../i18n/index.ts";

/** The page's locale lookup, passed in by src/home.ts (src/i18n cannot load in Node). */
export type Translate = (key: LocaleKey) => string;

export function el<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  className: string,
  text?: string,
): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  node.className = className;
  if (text !== undefined) {
    node.textContent = text;
  }
  return node;
}

/** A plain button that never submits a form. */
export function button(className: string, text: string, id?: string): HTMLButtonElement {
  const node = el("button", className, text);
  node.type = "button";
  if (id !== undefined) {
    node.id = id;
  }
  return node;
}
