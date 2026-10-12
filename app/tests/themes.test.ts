// A4: every built-in theme meets WCAG 2.2 AA contrast for the pairs the interface uses.
import { test } from "node:test";
import assert from "node:assert/strict";
import { BUILT_IN, contrast, contrastProblems, customTheme, parseCustomTheme } from "../src/themes";

test("contrast ratios match WCAG examples", () => {
  assert.equal(Math.round(contrast("#000000", "#ffffff") * 10) / 10, 21);
  assert.equal(contrast("#777777", "#777777"), 1);
});

for (const theme of BUILT_IN) {
  test(`built-in theme ${theme.id} meets WCAG 2.2 AA`, () => {
    assert.deepEqual(contrastProblems(theme.tokens), []);
  });
}

test("custom themes start from a base and keep only valid colors", () => {
  const file = parseCustomTheme('{"name":"Mine","base":"dark","tokens":{"text":"#ffffff","bg":"red"}}');
  assert.ok(file);
  const t = customTheme(file!);
  assert.equal(t.tokens.text, "#ffffff");
  assert.equal(t.tokens.bg, BUILT_IN[1].tokens.bg);
  assert.equal(t.id, "custom:Mine");
  assert.equal(parseCustomTheme("not json"), null);
});

test("a low-contrast custom theme is reported", () => {
  const t = customTheme({ format: 1, name: "x", base: "light", tokens: { text: "#eeeeee" } });
  assert.ok(contrastProblems(t.tokens).some((p) => p.fg === "text"));
});
