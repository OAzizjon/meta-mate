import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { PTT_KEYS } from "./model.ts";
import { pttKeyLabel, withPttKey } from "./ptt-key.ts";

test("both locales render every chosen key in guidance without leftover placeholders", () => {
  for (const locale of ["en", "ru"]) {
    const strings = JSON.parse(readFileSync(new URL(`../../locales/${locale}.json`, import.meta.url), "utf8"));
    const translate = (key: string): string => strings[key];
    for (const key of PTT_KEYS) {
      const label = pttKeyLabel(key, translate);
      assert.ok(label);
      for (const text of ["robot.first_launch", "tray.tooltip", "onboarding.key_right_ctrl",
        "home.talk_title", "home.talk_hold", "home.talk_release", "home.talk_where"]) {
        const rendered = withPttKey(strings[text], key, translate);
        assert.ok(rendered.includes(label), `${locale} ${text} ${key}`);
        assert.ok(!rendered.includes("{key}"));
      }
    }
  }
});
