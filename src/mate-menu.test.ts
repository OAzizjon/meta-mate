import assert from "node:assert/strict";
import { test } from "node:test";
import { guestUsageOf, hideReturnTime, MATE_MENU_ITEMS } from "./mate-menu.ts";

test("menu dispatches existing commands with separators around temporary hide", () => {
  assert.deepEqual(MATE_MENU_ITEMS.map((item) => item?.command ?? null), [
    "open_chat", "open_home", "open_settings_window", null, "hide_mate", null, "quit_app",
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

test("guest usage: only a well-formed count shows", () => {
  assert.deepEqual(guestUsageOf({ left: 9, limit: 12, reset: 100 }), { left: 9, limit: 12 });
  assert.deepEqual(guestUsageOf({ left: 20, limit: 12 }), { left: 12, limit: 12 });
  assert.equal(guestUsageOf(null), null);
  assert.equal(guestUsageOf({ left: "9", limit: 12 }), null);
  assert.equal(guestUsageOf({ left: 1, limit: 0 }), null);
});
