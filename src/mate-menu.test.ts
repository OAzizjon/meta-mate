import assert from "node:assert/strict";
import { test } from "node:test";
import { hideReturnTime, MATE_MENU_ITEMS } from "./mate-menu.ts";

test("menu dispatches existing commands with separators around temporary hide", () => {
  assert.deepEqual(MATE_MENU_ITEMS.map((item) => item?.command ?? null), [
    "open_home", "open_settings_window", null, "hide_mate", null, "quit_app",
  ]);
});

test("return time adds the full hour from the click and crosses midnight", () => {
  const now = new Date(2026, 9, 29, 23, 30).getTime();
  const expected = new Date(2026, 9, 30, 0, 30);
  for (const locale of ["en", "ru"]) {
    assert.equal(hideReturnTime(60, now, locale), expected.toLocaleTimeString(locale, {
      hour: "2-digit", minute: "2-digit",
    }));
  }
});
